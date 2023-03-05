use color_eyre::eyre::{eyre, Result, WrapErr};
use log::debug;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A workspace is a folder which contains a vscode project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// The path of the workspace.
    path: PathBuf,
    /// The name of the workspace.
    workspace_name: String,
    /// The folder of the workspace in the container.
    workspace_folder: String,
}

impl Workspace {
    /// Creates a new workspace from the given path.
    pub fn from_path(path: &Path) -> Result<Workspace> {
        // check for valid path
        if !path.exists() {
            return Err(eyre!("Path {} does not exist", path.display()));
        }

        if !path.is_dir() {
            return Err(eyre!("Path {} is not a folder", path.display()));
        }

        // canonicalize path
        let path = std::fs::canonicalize(path).wrap_err_with(|| "Error canonicalizing path")?;

        // get workspace name
        let workspace_name = path
            .file_name()
            .ok_or_else(|| eyre!("Error getting workspace from path"))?
            .to_string_lossy()
            .into_owned();

        // default workspace folder
        let mut workspace_folder = format!("/workspaces/{workspace_name}");

        // check for devcontainer config to read custom workspace folder
        let dc_folder = path.join(".devcontainer");
        if dc_folder.exists() && dc_folder.is_dir() {
            debug!("Found devcontainer folder: {}", dc_folder.display());
            let dc_config = dc_folder.join("devcontainer.json");
            if dc_config.exists() && dc_config.is_file() {
                debug!("Found devcontainer config: {}", dc_config.display());
                if let Ok(folder) = parse_workspace_folder_from_config(&dc_config) {
                    debug!("Read workspace folder from config: {}", workspace_folder);
                    workspace_folder = folder;
                }
            }
        }

        Ok(Workspace {
            path,
            workspace_name,
            workspace_folder,
        })
    }

    /// Checks if the workspace has a devcontainer.
    pub fn has_devcontainer(&self) -> bool {
        let path = self.path.join(".devcontainer");
        path.exists() && path.is_dir()
    }

    /// Open vscode using the devcontainer extension.
    pub fn open(&self, args: &[OsString], insiders: bool) -> Result<()> {
        let mut path: String = self.path.to_string_lossy().into_owned();

        // detect WSL
        if std::env::var("WSLENV").is_ok() {
            debug!("WSL detected");
            path = wslpath2::convert(
                path.as_str(),
                None,
                wslpath2::Conversion::WslToWindows,
                true,
            )
            .map_err(|e| eyre!("Error while getting wslpath: {}", e))?;
        }

        debug!("encode: {path}");
        let hex = hex::encode(path);
        let uri = format!(
            "vscode-remote://dev-container%2B{hex}{}",
            self.workspace_folder
        );

        let mut args = args.to_owned();
        args.push(OsStr::new("--folder-uri").to_owned());
        args.push(OsStr::new(uri.as_str()).to_owned());

        exec_code(&args, insiders).wrap_err_with(|| "Error opening vscode using devcontainers...")
    }

    /// Open vscode like with the `code` command
    pub fn open_classic(&self, args: &Vec<OsString>, insiders: bool) -> Result<()> {
        debug!("path: {}", self.path.display());
        debug!("args: {:?}", args);

        let mut args = args.clone();
        args.insert(0, self.path.as_os_str().to_owned());
        exec_code(&args, insiders).wrap_err_with(|| "Error opening vscode the classic way...")
    }
}

/// Executes the vscode executable with the given arguments on unix.
#[cfg(unix)]
fn exec_code(args: &Vec<OsString>, insiders: bool) -> Result<()> {
    let cmd = if insiders { "code-insiders" } else { "code" };

    debug!("executable: {cmd}");
    debug!("final args: {:?}", args);
    Command::new(cmd).args(args).output()?;
    Ok(())
}

/// Executes the vscode executable with the given arguments on Windows.
#[cfg(windows)]
fn exec_code(args: &Vec<OsString>, insiders: bool) -> Result<()> {
    let cmd = "cmd";
    let mut args = args.clone();
    args.insert(0, OsString::from("/c"));
    args.insert(
        1,
        if insiders {
            OsString::from("code-insiders")
        } else {
            OsString::from("code")
        },
    );

    debug!("executable: {cmd}");
    debug!("final args: {:?}", args);
    Command::new(cmd).args(args).output()?;
    Ok(())
}

/// Parses the workspace folder from the given devcontainer config file.
/// `https://code.visualstudio.com/remote/advancedcontainers/change-default-source-mount`
fn parse_workspace_folder_from_config(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let config: serde_json::Value = serde_json::from_reader(reader)?;
    let workspace_folder = config["workspaceFolder"]
        .as_str()
        .ok_or_else(|| eyre!("Error parsing workspace config file: workspaceFolder not found"))?;
    Ok(workspace_folder.to_owned())
}
