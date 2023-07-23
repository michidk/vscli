use color_eyre::eyre::{eyre, Result, WrapErr};
use log::{debug, trace};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A workspace is a folder which contains a vscode project.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Workspace {
    /// The path of the workspace.
    path: PathBuf,
    /// The name of the workspace.
    pub workspace_name: String,
    /// The folder of the workspace in the container.
    workspace_folder: Option<String>,
}

impl Workspace {
    /// Creates a new workspace from the given path.
    pub fn from_path(path: &Path) -> Result<Workspace> {
        // check for valid path
        if !path.exists() {
            return Err(eyre!("Path {} does not exist", path.display()));
        }

        // canonicalize path
        let path = std::fs::canonicalize(path).wrap_err_with(|| "Error canonicalizing path")?;
        trace!("Canonicalized path: {}", path.to_string_lossy());

        // get workspace name (either directory or file name)
        let workspace_name = path
            .file_name()
            .ok_or_else(|| eyre!("Error getting workspace from path"))?
            .to_string_lossy()
            .into_owned();
        trace!("Workspace name: {workspace_name}");

        let mut config_path: Option<PathBuf> = None;
        let mut workspace_folder = None;

        // find config; either `.devcontainer.json` or `.devcontainer/devcontainer.json`
        let dc_config = path.join(".devcontainer.json");
        if dc_config.is_file() {
            debug!("Found devcontainer config: {}", dc_config.display());
            config_path = Some(dc_config);
        } else {
            let dc_folder = path.join(".devcontainer");
            if dc_folder.is_dir() {
                debug!("Found devcontainer folder: {}", dc_folder.display());
                let dc_config = dc_folder.join("devcontainer.json");
                if dc_config.is_file() {
                    debug!("Found devcontainer config: {}", dc_config.display());
                    config_path = Some(dc_config);
                } else {
                    debug!("No devcontainer config found in `.devcontainer` folder");
                }
            }
        }

        // parse workspace folder from .devcontainer config (if it exists)
        if let Some(path) = config_path {
            if let Ok(folder) = parse_workspace_folder_from_config(&path) {
                debug!("Read workspace folder from config: {}", folder);
                workspace_folder = Some(folder);
            } else {
                debug!("Could not read workspace folder from config -> using default folder");
                workspace_folder = Some(format!("/workspaces/{workspace_name}"));
            }
        } else {
            trace!("Devcontainer config not found, no workspace folder");
        }

        let ws = Workspace {
            path,
            workspace_name,
            workspace_folder,
        };
        trace!("Workspace: {ws:?}");
        Ok(ws)
    }

    /// Checks if the workspace has a devcontainer.
    pub fn has_devcontainer(&self) -> bool {
        self.path.join(".devcontainer").is_dir() || self.path.join(".devcontainer.json").is_file()
    }

    /// Open vscode using the devcontainer extension.
    pub fn open(&self, args: &[OsString], insiders: bool, dry_run: bool) -> Result<()> {
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

        trace!("encode: {path}");
        let hex = hex::encode(path);
        let uri = format!(
            "vscode-remote://dev-container%2B{hex}{}",
            self.workspace_folder.as_ref().expect("open() cannot be called without setting a workspace folder; use open_classic when no devcontainer config is found.")
        );

        let mut args = args.to_owned();
        args.push(OsStr::new("--folder-uri").to_owned());
        args.push(OsStr::new(uri.as_str()).to_owned());

        exec_code(&args, insiders, dry_run)
            .wrap_err_with(|| "Error opening vscode using devcontainers...")
    }

    /// Open vscode like with the `code` command
    pub fn open_classic(&self, args: &Vec<OsString>, insiders: bool, dry_run: bool) -> Result<()> {
        trace!("path: {}", self.path.display());
        trace!("args: {:?}", args);

        let mut args = args.clone();
        args.insert(0, self.path.as_os_str().to_owned());
        exec_code(&args, insiders, dry_run)
            .wrap_err_with(|| "Error opening vscode the classic way...")
    }
}

/// Executes the vscode executable with the given arguments on unix.
#[cfg(unix)]
fn exec_code(args: &Vec<OsString>, insiders: bool, dry_run: bool) -> Result<()> {
    let cmd = if insiders { "code-insiders" } else { "code" };
    // test if cmd exists
    Command::new(cmd)
        .arg("-v")
        .output()
        .wrap_err_with(|| format!("`{cmd}` does not exists."))?;

    debug!("executable: {cmd}");
    debug!("final args: {:?}", args);

    if !dry_run {
        Command::new(cmd).args(args).output()?;
    }

    Ok(())
}

/// Executes the vscode executable with the given arguments on Windows.
#[cfg(windows)]
fn exec_code(args: &Vec<OsString>, insiders: bool, dry_run: bool) -> Result<()> {
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

    // test if cmd exists
    Command::new(cmd)
        .arg("-v")
        .output()
        .wrap_err_with(|| format!("`{cmd}` does not exists."))?;

    debug!("executable: {cmd}");
    debug!("final args: {:?}", args);

    if !dry_run {
        Command::new(cmd).args(args).output()?;
    }

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
