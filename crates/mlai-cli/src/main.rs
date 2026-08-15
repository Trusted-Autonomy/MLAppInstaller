use clap::{Parser, Subcommand};
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
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Install {
            manifest,
            install_root,
            component,
        } => commands::install::run(&manifest, &install_root, component.as_deref()),
    }
}
