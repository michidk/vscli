use std::{ffi::OsString, fmt::Display, num::NonZeroUsize, path::PathBuf, str::FromStr};

use clap::ValueEnum;
use color_eyre::eyre::{self, eyre, Result};
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
            Self::Detect => write!(f, "{LAUNCH_DETECT}"),
            Self::ForceContainer => write!(f, "{LAUNCH_FORCE_CONTAINER}"),
            Self::ForceClassic => write!(f, "{LAUNCH_FORCE_CLASSIC}"),
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
    index: Option<NonZeroUsize>,
    config: Option<PathBuf>,
}

impl Setup {
    pub fn new(
        workspace: Workspace,
        behavior: Behavior,
        dry_run: bool,
        index: Option<NonZeroUsize>,
        config: Option<PathBuf>,
    ) -> Self {
        Self {
            workspace,
            behavior,
            dry_run,
            index,
            config,
        }
    }

    fn detect(&self) -> Result<Option<DevContainer>> {
        let name = self.workspace.name.clone();
        let configs = self.workspace.find_dev_container_configs();

        // either use the dev container selected by index ...
        if let Some(nz_index) = self.index {
            trace!("Dev container set by index: {nz_index}");
            let index = nz_index.get() - 1;
            if index >= configs.len() {
                return Err(eyre!("No dev container on position {nz_index} found."));
            }
            let dev_containers = self.workspace.load_dev_containers(&configs)?;
            return Ok(Some(
                dev_containers
                    .get(index)
                    .expect("Index out of bounds")
                    .clone(),
            ));
        }

        // ... or use the dev container specified by path
        if let Some(config) = &self.config {
            trace!("Dev container set by path: {config:?}");
            Ok(Some(DevContainer::from_config(config, &name)?))
        } else {
            // ... or use the first dev container found
            let mut dev_containers = self.workspace.load_dev_containers(&configs)?;
            // but check if multiple are defined first
            match configs.len() {
                0 => {
                    trace!("No dev container specified.");
                    Ok(None)
                }
                1 => {
                    trace!("Select only dev container");
                    Ok(Some(dev_containers.remove(0)))
                }
                _ => {
                    let mut list = String::new();
                    for (i, dev_container) in dev_containers.iter().enumerate() {
                        let i = i + 1;
                        let path = dev_container.path.to_string_lossy();
                        let display = if let Some(name) = dev_container.name.clone() {
                            format!("\n- [{i}] {name}: {path}")
                        } else {
                            format!("\n- [{i}] {path}")
                        };
                        list.push_str(&display);
                    }
                    info!("Several dev container configurations found. Please use the --index flag to specify which one should be used:{list}");
                    Err(eyre!("Several dev container configurations found."))
                }
            }
        }
    }

    /// Launches vscode with the given configuration.
    pub fn launch(&self) -> Result<()> {
        match self.behavior.strategy {
            ContainerStrategy::Detect => {
                let dev_container = self.detect()?;
                if let Some(dev_container) = dev_container {
                    info!("Opening dev container...");
                    self.workspace.open(
                        &self.behavior.args,
                        self.behavior.insiders,
                        self.dry_run,
                        &dev_container,
                    )?;
                } else {
                    info!("Dev containers not found, opening without containers...");
                    self.workspace.open_classic(
                        &self.behavior.args,
                        self.behavior.insiders,
                        self.dry_run,
                    )?;
                }
            }
            ContainerStrategy::ForceContainer => {
                let dev_container = self.detect()?;

                if let Some(dev_container) = dev_container {
                    info!("Force opening dev container...");
                    self.workspace.open(
                        &self.behavior.args,
                        self.behavior.insiders,
                        self.dry_run,
                        &dev_container,
                    )?;
                } else {
                    return Err(eyre!("Dev container not found, but was forced to open it."));
                }
            }
            ContainerStrategy::ForceClassic => {
                info!("Opening vscode without dev containers...");
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
