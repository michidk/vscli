use std::{ffi::OsString, path::PathBuf};

use clap::{command, Parser, Subcommand};

use crate::{launch::ContainerStrategy, ui::Focus};

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

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Opens a dev container.
    #[clap(alias = "o")]
    Open {
        /// The path of the vscode project to open
        #[arg(value_parser, default_value = ".")]
        path: PathBuf,

        /// Additional arguments to pass to vscode
        #[arg(value_parser, env)]
        args: Vec<OsString>,

        /// Launch behavior
        #[arg(short, long, default_value_t = ContainerStrategy::Detect, ignore_case = true)]
        behavior: ContainerStrategy,

        /// Overwrites the path to the dev container config file
        #[arg(short, long, env)]
        config: Option<PathBuf>,

        /// Whether to launch the insider's version of vscode
        #[arg(short = 'n', long, env)]
        insiders: bool,
    },
    /// Opens an interactive list of recently used workspaces.
    #[clap(alias = "ui")]
    Recent {
        #[arg(value_enum, short, long, default_value_t = Focus::Search, ignore_case = true)]
        focus: Focus,
    },
}
