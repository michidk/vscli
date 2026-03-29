use std::{ffi::OsString, path::PathBuf};

use clap::{Args, Parser, Subcommand};

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

    /// Overwrite the default path to the config directory
    #[arg(long, env = "VSCLI_CONFIG_DIR", global = true)]
    pub config_dir: Option<PathBuf>,

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
    /// The editor command to use (e.g. "code", "code-insiders", "cursor")
    #[arg(short, long, env)]
    pub command: Option<String>,

    /// Launch behavior
    #[arg(short, long, ignore_case = true)]
    pub behavior: Option<ContainerStrategy>,

    /// Overwrites the path to the dev container config file (accepts a path or a config name)
    #[arg(long, env)]
    pub config: Option<PathBuf>,

    /// Additional arguments to pass to the editor
    #[arg(value_parser, env)]
    pub args: Vec<OsString>,
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
        /// Hide the instruction message in the UI
        #[arg(long)]
        hide_instructions: bool,

        /// Hide additional information like strategy, command, args and dev container path in the UI
        #[arg(long)]
        hide_info: bool,

        #[command(flatten)]
        launch: LaunchArgs,
    },
    /// Manage external devcontainer configurations.
    #[clap(alias = "cfg")]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Manage running devcontainers.
    #[clap(alias = "ct")]
    Container {
        #[command(subcommand)]
        action: ContainerAction,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ConfigAction {
    /// Open interactive config picker.
    Ui,
    /// List available configs.
    #[clap(alias = "ls")]
    List {
        /// Show full paths and descriptions.
        #[arg(short, long)]
        long: bool,
    },
    /// Print the config directory path.
    Dir,
    /// Create a new minimal devcontainer config.
    Add {
        /// Name for the new config.
        name: String,
    },
    /// Copy a stored config into a target directory.
    #[clap(alias = "cp")]
    Copy {
        /// Name of the stored config to copy.
        name: String,
        /// Directory to copy the config into.
        #[arg(value_parser, default_value = ".")]
        path: PathBuf,
    },
    /// Remove a config by name.
    Rm {
        /// Name of the config to remove.
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ContainerAction {
    /// Open interactive container picker.
    Ui,
    /// List devcontainers.
    #[clap(alias = "ls")]
    List {
        /// Include stopped containers.
        #[arg(short, long)]
        all: bool,
    },
    /// Show detailed information about a devcontainer.
    Info {
        /// Container ID or ID prefix.
        id: String,
    },
    /// Stop a running devcontainer.
    Stop {
        /// Container ID or ID prefix.
        id: String,
    },
}
