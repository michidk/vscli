use std::{ffi::OsString, path::PathBuf, str::FromStr};

use clap::{command, Parser, ValueEnum};
use color_eyre::eyre::{self, eyre};

const LAUNCH_DETECT: &str = "detect";
const LAUNCH_FORCE_CONTAINER: &str = "force-container";
const LAUNCH_FORCE_CLASSIC: &str = "force-classic";

/// Main CLI arguments
#[derive(Parser, Debug)]
#[command(
    name = "vscli",
    about = "A CLI tool to launch vscode projects, which supports devcontainers."
)]
pub struct Opts {
    /// The path of the vscode project to open
    #[arg(value_parser, default_value = ".")]
    pub path: PathBuf,

    /// Aditional arguments to pass to vscode
    #[arg(value_parser)]
    pub args: Vec<OsString>,

    /// Launch behaviour
    #[arg(short, long, default_value = LAUNCH_DETECT, ignore_case = true)]
    pub behaviour: LaunchBehaviour,

    /// Whether to launch the insiders version of vscode
    #[arg(short, long)]
    pub insiders: bool,

    /// The verbosity of the output
    #[arg(short, long, global = true, default_value = "info", ignore_case = true)]
    pub verbosity: log::LevelFilter,
}

/// Set the launch bevaiour of vscode.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LaunchBehaviour {
    /// Use devcontainer if it was detected
    #[default]
    Detect,
    /// Force open with devcontainer, even if no config was found
    ForceContainer,
    /// Ignore devcontainers
    ForceClassic,
}

impl FromStr for LaunchBehaviour {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            LAUNCH_DETECT => Ok(Self::Detect),
            LAUNCH_FORCE_CONTAINER => Ok(Self::ForceContainer),
            LAUNCH_FORCE_CLASSIC => Ok(Self::ForceClassic),
            _ => Err(eyre!("Invalid launch behaviour: {}", s)),
        }
    }
}
