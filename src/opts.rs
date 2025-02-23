use std::{ffi::OsString, path::PathBuf};

use clap::{command, Args, Parser, Subcommand};

use crate::launch::ContainerStrategy;

/// Main CLI arguments
#[derive(Parser, Debug)]
#[command(
    name = "vscli",
    about = "A CLI tool to launch vscode projects, which supports dev containers.",
    author,
    version,
    about
)]
pub(crate) struct Opts {
    /// Overwrite the default path to the history file
    #[arg(short = 's', long, env, global = true)]
    pub history_path: Option<PathBuf>,

    /// Whether to launch in dry-run mode (not actually open vscode)
    #[arg(short, long, alias = "dry", env, global = true)]
    pub dry_run: bool,

    /// The verbosity of the output
    #[command(flatten)]
    pub verbose: clap_verbosity_flag::Verbosity<clap_verbosity_flag::InfoLevel>,

    /// The command to run
    #[command(subcommand)]
    pub command: Commands,
}

/// Arguments for launching an editor
#[derive(Args, Debug, Clone)]
pub(crate) struct LaunchArgs {
    /// Additional arguments to pass to the editor
    #[arg(value_parser, env)]
    pub args: Vec<OsString>,

    /// Launch behavior
    #[arg(short, long, ignore_case = true)]
    pub behavior: Option<ContainerStrategy>,

    /// Overwrites the path to the dev container config file
    #[arg(short, long, env)]
    pub config: Option<PathBuf>,

    /// The editor command to use (e.g. "code", "code-insiders", "cursor")
    #[arg(long, env)]
    pub command: Option<String>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Opens a dev container.
    #[clap(alias = "o")]
    Open {
        /// The path of the vscode project to open
        #[arg(value_parser, default_value = ".")]
        path: PathBuf,

        #[command(flatten)]
        launch: LaunchArgs,
    },
    /// Opens an interactive list of recently used workspaces.
    #[clap(alias = "ui")]
    Recent {
        #[command(flatten)]
        launch: LaunchArgs,
    },
}
