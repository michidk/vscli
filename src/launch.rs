use std::{ffi::OsString, str::FromStr};

use clap::ValueEnum;
use color_eyre::eyre::{self, eyre, Result};
use log::info;
use serde::{Deserialize, Serialize};

use crate::workspace::Workspace;

pub const LAUNCH_DETECT: &str = "detect";
pub const LAUNCH_FORCE_CONTAINER: &str = "force-container";
pub const LAUNCH_FORCE_CLASSIC: &str = "force-classic";

/// Set the devcontainer launch strategy of vscode.
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
    /// Use devcontainer if it was detected
    #[default]
    Detect,
    /// Force open with devcontainer, even if no config was found
    ForceContainer,
    /// Ignore devcontainers
    ForceClassic,
}

impl FromStr for ContainerStrategy {
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

/// The launch behaviour that is used to start vscode.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Behaviour {
    pub container: ContainerStrategy,
    pub insiders: bool,
    pub args: Vec<OsString>,
}

/// The configuration for the launch behaviour of vscode.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Config {
    workspace: Workspace,
    behaviour: Behaviour,
    dry_run: bool,
}

impl Config {
    pub fn new(workspace: Workspace, behaviour: Behaviour, dry_run: bool) -> Self {
        Self {
            workspace,
            behaviour,
            dry_run,
        }
    }

    /// Launches vscode with the given configuration.
    pub fn launch(&self) -> Result<()> {
        match self.behaviour.container {
            ContainerStrategy::Detect => {
                if self.workspace.has_devcontainer() {
                    info!("Opening devcontainer...");
                    self.workspace.open(
                        &self.behaviour.args,
                        self.behaviour.insiders,
                        self.dry_run,
                    )?;
                } else {
                    info!("Devcontainer not found, opening the classic way...");
                    self.workspace.open_classic(
                        &self.behaviour.args,
                        self.behaviour.insiders,
                        self.dry_run,
                    )?;
                }
            }
            ContainerStrategy::ForceContainer => {
                if self.workspace.has_devcontainer() {
                    info!("Opening devcontainer...");
                    self.workspace.open(
                        &self.behaviour.args,
                        self.behaviour.insiders,
                        self.dry_run,
                    )?;
                } else {
                    return Err(eyre!("Devcontainer not found, but was forced to open it."));
                }
            }
            ContainerStrategy::ForceClassic => {
                info!("Opening vscode the classic way...");
                self.workspace.open_classic(
                    &self.behaviour.args,
                    self.behaviour.insiders,
                    self.dry_run,
                )?;
            }
        }

        Ok(())
    }
}
