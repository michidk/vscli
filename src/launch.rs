use std::{ffi::OsString, fmt::Display, str::FromStr};

use clap::ValueEnum;
use color_eyre::eyre::{self, eyre, Result};
use log::info;
use serde::{Deserialize, Serialize};

use crate::workspace::{DevContainer, Workspace};

pub const LAUNCH_DETECT: &str = "detect";
pub const LAUNCH_FORCE_CONTAINER: &str = "force-container";
pub const LAUNCH_FORCE_CLASSIC: &str = "force-classic";

/// Set the dev container launch strategy of vscode.
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    ValueEnum,
    Serialize,
    Deserialize,
)]
pub enum ContainerStrategy {
    /// Use dev container if it was detected
    #[default]
    Detect,
    /// Force open with dev container, even if no config was found
    ForceContainer,
    /// Ignore dev container
    ForceClassic,
}

impl FromStr for ContainerStrategy {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            LAUNCH_DETECT => Ok(Self::Detect),
            LAUNCH_FORCE_CONTAINER => Ok(Self::ForceContainer),
            LAUNCH_FORCE_CLASSIC => Ok(Self::ForceClassic),
            _ => Err(eyre!("Invalid launch behavior: {}", s)),
        }
    }
}

impl Display for ContainerStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Detect => write!(f, "{LAUNCH_DETECT}"),
            Self::ForceContainer => write!(f, "{LAUNCH_FORCE_CONTAINER}"),
            Self::ForceClassic => write!(f, "{LAUNCH_FORCE_CLASSIC}"),
        }
    }
}

/// The launch behavior that is used to start vscode.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Behavior {
    pub strategy: ContainerStrategy,
    pub insiders: bool,
    pub args: Vec<OsString>,
}

/// The configuration for the launch behavior of vscode.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Config {
    workspace: Workspace,
    behavior: Behavior,
    dev_container: Option<DevContainer>,
    dry_run: bool,
}

impl Config {
    pub fn new(
        workspace: Workspace,
        behavior: Behavior,
        dev_container: Option<DevContainer>,
        dry_run: bool,
    ) -> Self {
        Self {
            workspace,
            behavior,
            dev_container,
            dry_run,
        }
    }

    /// Launches vscode with the given configuration.
    pub fn launch(&self) -> Result<()> {
        match self.behavior.strategy {
            ContainerStrategy::Detect => {
                if self.workspace.dev_containers.is_empty() {
                    info!("Dev container not found, opening the classic way...");
                    self.workspace.open_classic(
                        &self.behavior.args,
                        self.behavior.insiders,
                        self.dry_run,
                    )?;
                } else {
                    info!("Opening dev container...");
                    self.workspace.open(
                        &self.behavior.args,
                        self.behavior.insiders,
                        self.dry_run,
                        self.dev_container.clone(),
                    )?;
                }
            }
            ContainerStrategy::ForceContainer => {
                if self.workspace.dev_containers.is_empty() {
                    return Err(eyre!("Dev container not found, but was forced to open it."));
                }

                info!("Opening dev container...");
                self.workspace.open(
                    &self.behavior.args,
                    self.behavior.insiders,
                    self.dry_run,
                    self.dev_container.clone(),
                )?;
            }
            ContainerStrategy::ForceClassic => {
                info!("Opening vscode the classic way...");
                self.workspace.open_classic(
                    &self.behavior.args,
                    self.behavior.insiders,
                    self.dry_run,
                )?;
            }
        }

        Ok(())
    }
}
