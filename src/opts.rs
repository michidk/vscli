use std::{ffi::OsString, path::PathBuf};

use clap::{command, Parser, Subcommand};

use crate::launch::{ContainerStrategy, LAUNCH_DETECT};

/// Main CLI arguments
#[derive(Parser, Debug)]
#[command(
    name = "vscli",
    about = "A CLI tool to launch vscode projects, which supports devcontainers.",
    author,
    version,
    about
)]
pub(crate) struct Opts {
    /// The path of the vscode project to open
    #[arg(value_parser, default_value = ".")]
    pub path: PathBuf,

    /// Aditional arguments to pass to vscode
    #[arg(value_parser)]
    pub args: Vec<OsString>,

    /// Launch behaviour
    #[arg(short, long, default_value = LAUNCH_DETECT, ignore_case = true)]
    pub behaviour: ContainerStrategy,

    /// Whether to launch the insiders version of vscode
    #[arg(short, long)]
    pub insiders: bool,

    /// The verbosity of the output
    #[arg(short, long, global = true, default_value = "info", ignore_case = true)]
    pub verbosity: log::LevelFilter,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// The recent UI command
    #[clap(alias = "ui")]
    Recent,
}
