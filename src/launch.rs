use std::{ffi::OsString, fmt::Display, path::PathBuf, str::FromStr};

use clap::ValueEnum;
use color_eyre::eyre::{self, bail, eyre, Result};
use inquire::Select;
use log::{info, trace};
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
            Self::Detect => f.write_str(LAUNCH_DETECT),
            Self::ForceContainer => f.write_str(LAUNCH_FORCE_CONTAINER),
            Self::ForceClassic => f.write_str(LAUNCH_FORCE_CLASSIC),
        }
    }
}

/// The launch behavior that is used to start vscode.
/// Is saved in the history file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Behavior {
    pub strategy: ContainerStrategy,
    pub insiders: bool,
    pub args: Vec<OsString>,
}

/// The configuration for the launch behavior of vscode.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Setup {
    workspace: Workspace,
    behavior: Behavior,
    dry_run: bool,
}

impl Setup {
    pub fn new(workspace: Workspace, behavior: Behavior, dry_run: bool) -> Self {
        Self {
            workspace,
            behavior,
            dry_run,
        }
    }

    /// Selects the dev container that should be used.
    fn detect(&self, config: Option<PathBuf>) -> Result<Option<DevContainer>> {
        let name = self.workspace.name.clone();

        if let Some(config) = config {
            trace!("Dev container set by path: {config:?}");
            Ok(Some(DevContainer::from_config(config.as_path(), &name)?))
        } else {
            let configs = self.workspace.find_dev_container_configs();
            let dev_containers = self.workspace.load_dev_containers(&configs)?;

            match configs.len() {
                0 => {
                    trace!("No dev container specified.");
                    Ok(None)
                }
                1 => {
                    trace!("Select only dev container");
                    Ok(dev_containers.into_iter().next())
                }
                _ => Ok(Some(
                    Select::new(
                        "Multiple dev containers found! Please select one:",
                        dev_containers,
                    )
                    .prompt()?,
                )),
            }
        }
    }

    /// Launches vscode with the given configuration.
    /// Returns the dev container that was used, if any.
    pub fn launch(self, config: Option<PathBuf>) -> Result<Option<DevContainer>> {
        match self.behavior.strategy {
            ContainerStrategy::Detect => {
                let dev_container = self.detect(config)?;

                if let Some(ref dev_container) = dev_container {
                    info!("Opening dev container...");
                    self.workspace.open(
                        self.behavior.args,
                        self.behavior.insiders,
                        self.dry_run,
                        dev_container,
                    )?;
                } else {
                    info!("Dev containers not found, opening without containers...");
                    self.workspace.open_classic(
                        self.behavior.args,
                        self.behavior.insiders,
                        self.dry_run,
                    )?;
                }
                Ok(dev_container)
            }
            ContainerStrategy::ForceContainer => {
                let dev_container = self.detect(config)?;

                if let Some(ref dev_container) = dev_container {
                    info!("Force opening dev container...");
                    self.workspace.open(
                        self.behavior.args,
                        self.behavior.insiders,
                        self.dry_run,
                        dev_container,
                    )?;
                } else {
                    bail!("Dev container not found, but was forced to open it.");
                }
                Ok(dev_container)
            }
            ContainerStrategy::ForceClassic => {
                info!("Opening vscode without dev containers...");
                self.workspace.open_classic(
                    self.behavior.args,
                    self.behavior.insiders,
                    self.dry_run,
                )?;
                Ok(None)
            }
        }
    }
}
