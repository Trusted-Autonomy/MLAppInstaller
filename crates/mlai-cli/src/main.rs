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
        /// Backend option to pass to a component's setup (key=value, repeatable).
        /// Only valid for components with supports_options_protocol = true.
        #[arg(long = "set", value_parser = parse_set_option)]
        set: Vec<(String, String)>,
    },
    /// Manage stored credentials (hosted-model API keys, etc.)
    Credential {
        #[command(subcommand)]
        action: CredentialAction,
    },
}

#[derive(Subcommand)]
enum CredentialAction {
    /// Store a secret value (read from stdin) under the given key
    Set {
        key: String,
        #[arg(long)]
        vault_dir: Option<PathBuf>,
    },
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
        } => commands::install::run(&manifest, &install_root, component.as_deref(), &set),
        Commands::Credential { action } => match action {
            CredentialAction::Set { key, vault_dir } => commands::credential::set(&key, vault_dir),
        },
    }
}
