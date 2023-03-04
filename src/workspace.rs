use color_eyre::eyre::{eyre, Result, WrapErr};
use log::debug;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Workspace<'a> {
    path: &'a Path,
}

impl<'a> Workspace<'a> {
    pub fn from_path(path: &'a Path) -> Result<Workspace<'a>> {
        // check for valid path
        if !path.exists() {
            return Err(eyre!("Path {} does not exist", path.display()));
        }

        if !path.is_dir() {
            return Err(eyre!("Path {} is not a folder", path.display()));
        }

        Ok(Workspace { path })
    }

    pub fn has_devcontainer(&self) -> bool {
        self.path.join(".devcontainer").exists()
    }

    pub fn open(&self, args: &[OsString], insiders: bool) -> Result<()> {
        let path =
            std::fs::canonicalize(self.path).wrap_err_with(|| "Error canonicalizing path")?;
        let workspace_name: &str = &path
            .file_name()
            .ok_or_else(|| eyre!("Error getting workspace from path"))?
            .to_string_lossy();
        let workspace = format!("workspaces/{}", workspace_name); // TODO: read from devcontainers file in future (https://github.com/microsoft/vscode-remote-release/issues/2133#issuecomment-1430651840) using our custom devcontainer crate (push that to crates.io)
        let mut path: String = path.to_string_lossy().into_owned();

        // detect WSL
        if std::env::var("WSLENV").is_ok() {
            debug!("WSL detected");
            path =
                wslpath::convert_path(path.as_str(), None, wslpath::Settings::WslToWindows, true)
                    .map_err(|e| eyre!("Error while getting wslpath: {}", e))?;
        }

        debug!("encode: {path}");
        let hex = hex::encode(path);
        let uri = format!("vscode-remote://dev-container%2B{hex}/{workspace}");

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

fn exec_code(args: &Vec<OsString>, insiders: bool) -> Result<()> {
    let cmd = if insiders { "code-insiders" } else { "code" };
    debug!("executable: {cmd}");
    debug!("final args: {:?}", args);
    Command::new(cmd).args(args).output()?;
    Ok(())
}
