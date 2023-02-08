#![forbid(unsafe_code)]

use std::path::PathBuf;

use cargo_semver_checks::GlobalConfig;
use cargo_semver_checks::PackageSelection;
use cargo_semver_checks::Rustdoc;
use cargo_semver_checks::ScopeSelection;
use cargo_semver_checks::SemverQuery;
use clap::{Args, Parser, Subcommand};

fn main() -> anyhow::Result<()> {
    human_panic::setup_panic!();

    let Cargo::SemverChecks(args) = Cargo::parse();
    if args.bugreport {
        use bugreport::{bugreport, collector::*, format::Markdown};
        bugreport!()
            .info(SoftwareVersion::default())
            .info(OperatingSystem::default())
            .info(CommandLine::default())
            .info(CommandOutput::new("cargo version", "cargo", &["-V"]))
            .info(CompileTimeInformation::default())
            .print::<Markdown>();
        std::process::exit(0);
    } else if args.list {
        let queries = SemverQuery::all_queries();
        let mut rows = vec![["id", "type", "description"], ["==", "====", "==========="]];
        for query in queries.values() {
            rows.push([
                query.id.as_str(),
                query.required_update.as_str(),
                query.description.as_str(),
            ]);
        }
        let mut widths = [0; 3];
        for row in &rows {
            widths[0] = widths[0].max(row[0].len());
            widths[1] = widths[1].max(row[1].len());
            widths[2] = widths[2].max(row[2].len());
        }
        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();
        for row in rows {
            use std::io::Write;
            writeln!(
                stdout,
                "{0:<1$} {2:<3$} {4:<5$}",
                row[0], widths[0], row[1], widths[1], row[2], widths[2]
            )?;
        }

        let mut config = GlobalConfig::new().set_level(args.verbosity.log_level());
        config.shell_note("Use `--explain <id>` to see more details")?;
        std::process::exit(0);
    } else if let Some(id) = args.explain.as_deref() {
        let queries = SemverQuery::all_queries();
        let query = queries.get(id).ok_or_else(|| {
            let ids = queries.keys().cloned().collect::<Vec<_>>();
            anyhow::format_err!(
                "Unknown id `{}`, available id's:\n  {}",
                id,
                ids.join("\n  ")
            )
        })?;
        println!(
            "{}",
            query
                .reference
                .as_deref()
                .unwrap_or(query.description.as_str())
        );
        if let Some(link) = &query.reference_link {
            println!();
            println!("See also {link}");
        }
        std::process::exit(0);
    }

    match args.command {
        Some(SemverChecksCommands::CheckRelease(args)) => {
            let check: cargo_semver_checks::Check = args.into();
            let report = check.check_release()?;
            if report.success() {
                std::process::exit(0)
            } else {
                std::process::exit(1);
            }
        }
        None => {
            anyhow::bail!("subcommand required");
        }
    }
}

#[derive(Parser)]
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
#[command(version, propagate_version = true)]
enum Cargo {
    SemverChecks(SemverChecks),
}

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct SemverChecks {
    #[arg(long, global = true, exclusive = true)]
    bugreport: bool,

    #[arg(long, global = true, exclusive = true)]
    explain: Option<String>,

    #[arg(long, global = true, exclusive = true)]
    list: bool,

    #[command(flatten)]
    verbosity: clap_verbosity_flag::Verbosity<clap_verbosity_flag::InfoLevel>,

    #[command(subcommand)]
    command: Option<SemverChecksCommands>,
}

/// Check your crate for semver violations.
#[derive(Subcommand)]
enum SemverChecksCommands {
    #[command(alias = "diff-files")]
    CheckRelease(CheckRelease),
}

#[derive(Args)]
struct CheckRelease {
    #[command(flatten, next_help_heading = "Current")]
    pub manifest: clap_cargo::Manifest,

    #[command(flatten, next_help_heading = "Current")]
    pub workspace: clap_cargo::Workspace,

    /// The current rustdoc json output to test for semver violations.
    #[arg(
        long,
        short_alias = 'c',
        alias = "current",
        value_name = "JSON_PATH",
        help_heading = "Current",
        requires = "baseline_rustdoc"
    )]
    current_rustdoc: Option<PathBuf>,

    /// Version from registry to lookup for a baseline
    #[arg(
        long,
        value_name = "X.Y.Z",
        help_heading = "Baseline",
        group = "baseline"
    )]
    baseline_version: Option<String>,

    /// Git revision to lookup for a baseline
    #[arg(
        long,
        value_name = "REV",
        help_heading = "Baseline",
        group = "baseline"
    )]
    baseline_rev: Option<String>,

    /// Directory containing baseline crate source
    #[arg(
        long,
        value_name = "MANIFEST_ROOT",
        help_heading = "Baseline",
        group = "baseline"
    )]
    baseline_root: Option<PathBuf>,

    /// The rustdoc json file to use as a semver baseline.
    #[arg(
        long,
        short_alias = 'b',
        alias = "baseline",
        value_name = "JSON_PATH",
        help_heading = "Baseline",
        group = "baseline"
    )]
    baseline_rustdoc: Option<PathBuf>,

    #[command(flatten)]
    verbosity: clap_verbosity_flag::Verbosity<clap_verbosity_flag::InfoLevel>,
}

impl From<CheckRelease> for cargo_semver_checks::Check {
    fn from(value: CheckRelease) -> Self {
        let (current, current_project_root) = if let Some(current_rustdoc) = value.current_rustdoc {
            (Rustdoc::from_path(current_rustdoc), None)
        } else if let Some(manifest) = value.manifest.manifest_path {
            let project_root = if manifest.is_dir() {
                manifest
            } else {
                manifest
                    .parent()
                    .expect("manifest path doesn't have a parent")
                    .to_path_buf()
            };
            (Rustdoc::from_root(&project_root), Some(project_root))
        } else {
            let project_root = std::env::current_dir().expect("can't determine current directory");
            (Rustdoc::from_root(&project_root), Some(project_root))
        };
        let mut check = Self::new(current);
        if value.workspace.all || value.workspace.workspace {
            let mut selection = PackageSelection::new(ScopeSelection::Workspace);
            selection.with_excluded_packages(value.workspace.exclude);
            check.with_package_selection(selection);
        } else if !value.workspace.package.is_empty() {
            check.with_packages(value.workspace.package);
        }
        let baseline = {
            if let Some(baseline_version) = value.baseline_version {
                Rustdoc::from_version(baseline_version)
            } else if let Some(baseline_rev) = value.baseline_rev {
                let root = if let Some(baseline_root) = value.baseline_root {
                    baseline_root
                } else if let Some(current_root) = current_project_root {
                    current_root
                } else {
                    std::env::current_dir().expect("can't determine current directory")
                };
                Rustdoc::from_git_revision(root, baseline_rev)
            } else if let Some(baseline_rustdoc) = value.baseline_rustdoc {
                Rustdoc::from_path(baseline_rustdoc)
            } else {
                let root = if let Some(baseline_root) = value.baseline_root {
                    baseline_root
                } else {
                    std::env::current_dir().expect("can't determine current directory")
                };
                Rustdoc::from_root(root)
            }
        };
        check.with_baseline(baseline);
        if let Some(log_level) = value.verbosity.log_level() {
            check.with_log_level(log_level);
        }
        check
    }
}

#[test]
fn verify_cli() {
    use clap::CommandFactory;
    Cargo::command().debug_assert()
}
