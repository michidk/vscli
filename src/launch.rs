use std::ffi::OsString;

use color_eyre::eyre::{eyre, Result};
use log::info;

use crate::{opts::LaunchBehaviour, workspace::Workspace};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchConfig<'a> {
    workspace: Workspace<'a>,
    behaviour: LaunchBehaviour,
    insiders: bool,
    args: Vec<OsString>,
}

impl<'a> LaunchConfig<'a> {
    pub fn new(
        workspace: Workspace<'a>,
        behaviour: LaunchBehaviour,
        insiders: bool,
        args: Vec<OsString>,
    ) -> Self {
        Self {
            workspace,
            behaviour,
            insiders,
            args,
        }
    }

    pub fn launch(&self) -> Result<()> {
        match self.behaviour {
            LaunchBehaviour::Detect => {
                if self.workspace.has_devcontainer() {
                    info!("Opening devcontainer...");
                    self.workspace.open(&self.args, self.insiders)?;
                } else {
                    info!("Devcontainer not found, opening the classic way...");
                    self.workspace.open_classic(&self.args, self.insiders)?;
                }
            }
            LaunchBehaviour::ForceContainer => {
                if self.workspace.has_devcontainer() {
                    info!("Opening devcontainer...");
                    self.workspace.open(&self.args, self.insiders)?;
                } else {
                    return Err(eyre!("Devcontainer not found, but was forced to open it."));
                }
            }
            LaunchBehaviour::ForceClassic => {
                info!("Opening vscode the classic way...");
                self.workspace.open_classic(&self.args, self.insiders)?;
            }
        }

        Ok(())
    }
}
