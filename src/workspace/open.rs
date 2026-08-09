use super::{DevContainer, Workspace, exec_code};
use crate::uri::{DevcontainerUriJson, FileUriJson};
#[cfg(unix)]
use color_eyre::eyre::eyre;
use color_eyre::eyre::{Result, WrapErr, bail};
#[cfg(unix)]
use log::debug;
use log::trace;
use std::ffi::OsString;
use std::path::Path;
#[cfg(unix)]
use std::process::Command;

impl Workspace {
    pub fn open(
        &self,
        mut args: Vec<OsString>,
        dry_run: bool,
        dev_container: &DevContainer,
        command: &str,
        subfolder: Option<&Path>,
    ) -> Result<()> {
        if args.iter().any(|arg| arg == "--folder-uri") {
            bail!("Specifying `--folder-uri` is not possible while using vscli.");
        }

        let container_folder = container_folder(dev_container, subfolder);
        let mut workspace_path = self.path.to_string_lossy().into_owned();
        let mut config_path = dev_container.config_path.to_string_lossy().into_owned();

        #[cfg(unix)]
        if is_wsl()? {
            debug!("WSL detected");
            workspace_path = wslpath2::convert(
                workspace_path.as_str(),
                None,
                wslpath2::Conversion::WslToWindows,
                true,
            )
            .map_err(|error| {
                eyre!("Error while getting wslpath: {error} (path: {workspace_path:?})")
            })?;
            config_path = wslpath2::convert(
                config_path.as_str(),
                None,
                wslpath2::Conversion::WslToWindows,
                true,
            )
            .map_err(|error| {
                eyre!("Error while getting wslpath: {error} (path: {config_path:?})")
            })?;
        }

        #[cfg(windows)]
        {
            workspace_path = workspace_path.replace("\\\\?\\", "");
            config_path = config_path.replace("\\\\?\\", "");
        }

        let folder_uri = DevcontainerUriJson {
            host_path: workspace_path,
            config_file: FileUriJson::new(config_path.as_str()),
        };
        let json = serde_json::to_string(&folder_uri)?;
        trace!("Folder uri JSON: {json}");

        let uri = format!(
            "vscode-remote://dev-container+{}{container_folder}",
            hex::encode(json.as_bytes())
        );
        args.push(OsString::from("--folder-uri"));
        args.push(OsString::from(uri));

        exec_code(args, dry_run, command)
            .wrap_err_with(|| "Error opening vscode using dev container...")
    }
}

fn container_folder(dev_container: &DevContainer, subfolder: Option<&Path>) -> String {
    let mut folder = dev_container.workspace_path_in_container.clone();
    if let Some(subfolder) = subfolder {
        let subfolder = subfolder.to_string_lossy().replace('\\', "/");
        if !subfolder.is_empty() && subfolder != "." {
            if !folder.ends_with('/') {
                folder.push('/');
            }
            folder.push_str(&subfolder);
        }
    }
    folder
}

#[cfg(unix)]
fn is_wsl() -> Result<bool> {
    let output = Command::new("uname")
        .arg("-a")
        .output()
        .wrap_err("Failed to execute uname")?;
    let uname_output = String::from_utf8(output.stdout)?;
    Ok(
        (uname_output.contains("Microsoft") || uname_output.contains("WSL"))
            && std::env::var("WSLENV").is_ok(),
    )
}
