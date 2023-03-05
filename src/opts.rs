use std::{ffi::OsString, path::PathBuf, str::FromStr};

use color_eyre::eyre::{self, eyre};
use structopt::{clap, StructOpt};

const LAUNCH_DETECT: &str = "detect";
const LAUNCH_FORCE_CONTAINER: &str = "force-container";
const LAUNCH_FORCE_CLASSIC: &str = "force-classic";

/// Set the launch bevaiour of vscode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchBehaviour {
    /// Use devcontainer if it was detected
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

impl LaunchBehaviour {
    /// Returns all possible variants of the launch behaviour.
    fn variants() -> &'static [&'static str] {
        &[LAUNCH_DETECT, LAUNCH_FORCE_CONTAINER, LAUNCH_FORCE_CLASSIC]
    }
}

/// Main CLI arguments
#[derive(StructOpt, Debug)]
#[structopt(
    name = "vscli",
    about = "A CLI tool to launch vscode projects, which supports devcontainers.",
    setting = clap::AppSettings::TrailingVarArg,
    setting = clap::AppSettings::AllowLeadingHyphen
)]
pub struct Opts {
    /// The path of the vscode project to open
    #[structopt(parse(from_os_str))]
    pub path: PathBuf,

    /// Input arguments to pass to vscode
    #[structopt(parse(from_os_str))]
    pub args: Vec<OsString>,

    /// Launch behaviour
    #[structopt(short, long, possible_values = &LaunchBehaviour::variants(), default_value = LAUNCH_DETECT, case_insensitive = true)]
    pub behaviour: LaunchBehaviour,

    /// Whether to launch the insiders version of vscode
    #[structopt(short, long)]
    pub insiders: bool,

    /// The verbosity of the output
    #[structopt(short, long, global = true, default_value = "info")]
    pub verbosity: log::LevelFilter,
}
