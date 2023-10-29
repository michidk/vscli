use std::{ffi::OsString, path::PathBuf};

use clap::{command, Parser, Subcommand};

use crate::launch::{ContainerStrategy, LAUNCH_DETECT};

/// Main CLI arguments
#[derive(Parser, Debug)]
#[command(
    name = "vscli",
    about = "A CLI tool to launch vscode projects, which supports devcontainer.",
    author,
    version,
    about
)]
pub(crate) struct Opts {
    /// The path of the vscode project to open
    #[arg(value_parser, default_value = ".")]
    pub path: PathBuf,

    /// Additional arguments to pass to vscode
    #[arg(value_parser, env)]
    pub args: Vec<OsString>,

    /// Launch behavior
    #[arg(short, long, default_value = LAUNCH_DETECT, ignore_case = true)]
    pub behavior: ContainerStrategy,

    /// Whether to launch the insider's version of vscode
    #[arg(short, long, env)]
    pub insiders: bool,

    /// Overwrite the default path to the history file
    #[arg(short = 's', long, env)]
    pub history_path: Option<PathBuf>,

    /// Whether to launch in dry-run mode (not actually open vscode)
    #[arg(short, long, alias = "dry")]
    pub dry_run: bool,

    /// The verbosity of the output
    #[arg(
        short,
        long,
        global = true,
        default_value = "info",
        ignore_case = true,
        env
    )]
    pub verbosity: log::LevelFilter,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Opens an interactive list of recently used workspaces
    #[clap(alias = "ui")]
    Recent,
}
