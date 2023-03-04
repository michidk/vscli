use std::{ffi::OsString, path::PathBuf, str::FromStr};

use color_eyre::eyre::{self, eyre};
use structopt::{clap, StructOpt};

const LAUNCH_DETECT: &str = "detect";
const LAUNCH_FORCE_CONTAINER: &str = "force-container";
const LAUNCH_FORCE_CLASSIC: &str = "force-classic";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchBehaviour {
    /// use devcontainer if it was detected
    Detect,
    /// force open with devcontainer, even if no config was found
    ForceContainer,
    /// ignore devcontainers
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
    fn variants() -> &'static [&'static str] {
        &[LAUNCH_DETECT, LAUNCH_FORCE_CONTAINER, LAUNCH_FORCE_CLASSIC]
    }
}

/// Main CLI arguments
#[derive(StructOpt, Debug)]
#[structopt(
    name = "vscli",
    about = "Shorthand for opening vscode with devcontainer support.",
    setting = clap::AppSettings::TrailingVarArg,
    setting = clap::AppSettings::AllowLeadingHyphen
)]
pub struct Opts {
    /// Input path
    #[structopt(parse(from_os_str))]
    pub path: PathBuf,

    /// Input args
    #[structopt(parse(from_os_str))]
    pub args: Vec<OsString>,

    /// Open behaviour
    #[structopt(short, long, possible_values = &LaunchBehaviour::variants(), default_value = LAUNCH_DETECT, case_insensitive = true)]
    pub behaviour: LaunchBehaviour,

    /// Use vscode-insiders
    #[structopt(short, long)]
    pub insiders: bool,

    /// Output debug info
    #[structopt(short, long, global = true, default_value = "info")]
    pub verbosity: log::LevelFilter,
}
