use std::{ffi::OsString, fmt::Display, path::PathBuf, str::FromStr};

use clap::ValueEnum;
use color_eyre::eyre::{self, Result, bail, eyre};
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

/// The launch behavior that is used to start vscode (saved in the history file)
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Behavior {
    /// The strategy to use for launching the container.
    pub strategy: ContainerStrategy,
    /// Additional arguments to pass to the editor.
    pub args: Vec<OsString>,
    /// The editor command to use (e.g. "code", "code-insiders", "cursor")
    #[serde(default = "default_editor_command")]
    pub command: String,
}

fn default_editor_command() -> String {
    "code".to_string()
}

/// Formats the editor name based on the command for display in messages.
fn format_editor_name(command: &str) -> String {
    match command.to_lowercase().as_str() {
        "code" => "Visual Studio Code".to_string(),
        "code-insiders" => "Visual Studio Code Insiders".to_string(),
        "cursor" => "Cursor".to_string(),
        "codium" => "VSCodium".to_string(),
        "positron" => "Positron".to_string(),
        _ => format!("'{command}'"),
    }
}

/// The configuration for the launch behavior
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Setup {
    /// The workspace configuration.
    workspace: Workspace,
    /// The behavior configuration.
    behavior: Behavior,
    /// Whether to perform a dry run, not actually launching the editor.
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
                    trace!("Selected the only existing dev container.");
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
        let editor_name = format_editor_name(&self.behavior.command);

        match self.behavior.strategy {
            ContainerStrategy::Detect => {
                let dev_container = self.detect(config)?;

                if let Some(ref dev_container) = dev_container {
                    info!("Opening dev container with {}...", editor_name);
                    self.workspace.open(
                        self.behavior.args,
                        self.dry_run,
                        dev_container,
                        &self.behavior.command,
                    )?;
                } else {
                    info!(
                        "No dev container found, opening on host system with {}...",
                        editor_name
                    );
                    self.workspace.open_classic(
                        self.behavior.args,
                        self.dry_run,
                        &self.behavior.command,
                    )?;
                }
                Ok(dev_container)
            }
            ContainerStrategy::ForceContainer => {
                let dev_container = self.detect(config)?;

                if let Some(ref dev_container) = dev_container {
                    info!("Force opening dev container with {}...", editor_name);
                    self.workspace.open(
                        self.behavior.args,
                        self.dry_run,
                        dev_container,
                        &self.behavior.command,
                    )?;
                } else {
                    bail!(
                        "No dev container found, but was forced to open it using dev containers."
                    );
                }
                Ok(dev_container)
            }
            ContainerStrategy::ForceClassic => {
                info!("Opening without dev containers using {}...", editor_name);
                self.workspace.open_classic(
                    self.behavior.args,
                    self.dry_run,
                    &self.behavior.command,
                )?;
                Ok(None)
            }
        }
    }
}
