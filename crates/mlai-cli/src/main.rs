use clap::{Parser, Subcommand};
use mlai_core::catalog::{GpuVendor, Os};
use std::path::PathBuf;

mod commands;

#[derive(Parser)]
#[command(
    name = "mlai",
    version,
    about = "MLAppInstaller: cross-platform installer engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install components from a manifest
    Install {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        install_root: PathBuf,
        #[arg(long)]
        component: Option<String>,
        /// Backend option to pass to a component's setup (key=value, repeatable).
        /// Only valid for components with supports_options_protocol = true.
        #[arg(long = "set", value_parser = parse_set_option)]
        set: Vec<(String, String)>,
        /// Reinstall even components already healthy at their current version
        #[arg(long)]
        force: bool,
    },
    /// Re-verify installed components against disk and fix any that are broken
    Repair {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        install_root: PathBuf,
        #[arg(long)]
        component: Option<String>,
    },
    /// Remove all installed components
    Uninstall {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        install_root: PathBuf,
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
        /// Report what would be removed without deleting anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Resolve the best-fit model for a purpose against a hardware profile
    Catalog {
        #[command(subcommand)]
        action: CatalogAction,
    },
}

#[derive(Subcommand)]
enum CatalogAction {
    Resolve {
        #[arg(long)]
        purpose: String,
        #[arg(long = "catalog")]
        catalog_paths: Vec<PathBuf>,
        #[arg(long, value_enum)]
        os: CliOs,
        #[arg(long = "gpu-vendor", value_enum)]
        gpu_vendor: CliGpuVendor,
        #[arg(long)]
        vram_gb: f64,
        #[arg(long)]
        effective_vram_gb: f64,
        #[arg(long)]
        disk_free_gb: f64,
        #[arg(long, default_value_t = 0.0)]
        reserve_vram_gb: f64,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum CliOs {
    Windows,
    Macos,
    Linux,
}

impl From<CliOs> for Os {
    fn from(v: CliOs) -> Os {
        match v {
            CliOs::Windows => Os::Windows,
            CliOs::Macos => Os::Macos,
            CliOs::Linux => Os::Linux,
        }
    }
}

#[derive(Clone, clap::ValueEnum)]
enum CliGpuVendor {
    Nvidia,
    Amd,
    Apple,
    Intel,
    None,
}

impl From<CliGpuVendor> for GpuVendor {
    fn from(v: CliGpuVendor) -> GpuVendor {
        match v {
            CliGpuVendor::Nvidia => GpuVendor::Nvidia,
            CliGpuVendor::Amd => GpuVendor::Amd,
            CliGpuVendor::Apple => GpuVendor::Apple,
            CliGpuVendor::Intel => GpuVendor::Intel,
            CliGpuVendor::None => GpuVendor::None,
        }
    }
}

fn parse_set_option(s: &str) -> Result<(String, String), String> {
    match s.split_once('=') {
        Some((k, v)) => Ok((k.to_string(), v.to_string())),
        None => Err(format!("invalid --set value '{s}': expected key=value")),
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Install {
            manifest,
            install_root,
            component,
            set,
            force,
        } => commands::install::run(&manifest, &install_root, component.as_deref(), &set, force),
        Commands::Repair {
            manifest,
            install_root,
            component,
        } => commands::repair::run(&manifest, &install_root, component.as_deref()),
        Commands::Uninstall {
            manifest,
            install_root,
            yes,
            dry_run,
        } => commands::uninstall::run(&manifest, &install_root, yes, dry_run),
        Commands::Catalog { action } => match action {
            CatalogAction::Resolve {
                purpose,
                catalog_paths,
                os,
                gpu_vendor,
                vram_gb,
                effective_vram_gb,
                disk_free_gb,
                reserve_vram_gb,
            } => commands::catalog::resolve(
                &purpose,
                &catalog_paths,
                os.into(),
                gpu_vendor.into(),
                vram_gb,
                effective_vram_gb,
                disk_free_gb,
                reserve_vram_gb,
            ),
        },
    }
}
