use color_eyre::eyre::{eyre, Result, WrapErr};
use log::{debug, trace};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

use crate::uri::{DevcontainerUriJson, FileUriJson};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DevContainer {
    pub path: PathBuf,
    pub name: Option<String>,
    pub path_in_container: String,
}

impl DevContainer {
    /// Creates a new `DevContainer` from a dev container config file and fallback workspace name.
    pub fn from_config(path: &Path, workspace_name: &str) -> Result<DevContainer> {
        let dev_container = Self::parse_dev_container_config(path)?;
        trace!("dev container config: {:?}", dev_container);

        let folder: String = if let Some(folder) = dev_container["workspaceFolder"].as_str() {
            debug!("Read workspace folder from config: {}", folder);
            folder.to_owned()
        } else {
            debug!("Could not read workspace folder from config -> using default folder");
            format!("/workspaces/{workspace_name}")
        };

        let name = if let Some(name) = dev_container["name"].as_str() {
            trace!("Read workspace name from config: {}", name);
            Some(name.to_owned())
        } else {
            trace!("Could not read workspace name from config");
            None
        };

        Ok(DevContainer {
            path: path.to_owned(),
            path_in_container: folder,
            name,
        })
    }

    /// Parses the dev container config file.
    /// `https://code.visualstudio.com/remote/advancedcontainers/change-default-source-mount`
    fn parse_dev_container_config(path: &Path) -> Result<serde_jsonc::Value> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let config: serde_jsonc::Value = serde_jsonc::from_reader(reader)
            .wrap_err_with(|| format!("Failed to parse json file: {path:?}"))?;

        debug!("Parsed dev container config: {:?}", path);
        Ok(config)
    }
}

/// A workspace is a folder which contains a vscode project.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Workspace {
    /// The path of the workspace.
    pub path: PathBuf,
    /// The name of the workspace.
    pub name: String,
}

impl Workspace {
    /// Creates a new `Workspace` from the given path to a folder.
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

        let ws = Workspace {
            path,
            name: workspace_name,
        };
        trace!("{ws:?}");
        Ok(ws)
    }

    /// Finds all dev container configs in the workspace.
    ///
    /// # Note
    /// This searches in the following locations:
    /// - A `.devcontainer.json` defined directly in the workspace folder.
    /// - A `.devcontainer/devcontainer.json` defined in the `.devcontainer/` folder.
    /// - Any `.devcontainer/**/devcontainer.json` file in any `.devcontainer/` subfolder (only one level deep).
    /// This should results in a dev container detection algorithm similar to the one vscode uses.
    pub fn find_dev_container_configs(&self) -> Vec<PathBuf> {
        let mut configs = Vec::new();

        // check if we have a `devcontainer.json` directly in the workspace
        let direct_config = self.path.join(".devcontainer.json");
        if direct_config.is_file() {
            trace!("Found dev container config: {}", direct_config.display());
            configs.push(direct_config);
        }

        // check configs one level deep in `.devcontainer/`
        let dev_container_dir = self.path.join(".devcontainer");
        for entry in WalkDir::new(dev_container_dir)
            .max_depth(2)
            .sort_by_file_name()
            .into_iter()
            .filter(|e| {
                e.as_ref()
                    .is_ok_and(|e| e.file_type().is_file() && e.file_name() == "devcontainer.json")
            })
            .flatten()
        {
            let path = entry.into_path();
            trace!(
                "Found dev container config in .devcontainer folder: {}",
                path.display()
            );
            configs.push(path);
        }

        debug!(
            "Found {} dev container configs: {:?}",
            configs.len(),
            configs
        );

        configs
    }

    pub fn load_dev_containers(&self, paths: &Vec<PathBuf>) -> Result<Vec<DevContainer>> {
        // parse dev containers and their properties
        let mut dev_containers = Vec::<DevContainer>::new();
        for config_path in paths {
            dev_containers.push(DevContainer::from_config(config_path, &self.name)?);
        }

        Ok(dev_containers)
    }

    /// Open vscode using the specified dev container.
    pub fn open(
        &self,
        args: &[OsString],
        insiders: bool,
        dry_run: bool,
        dev_container: &DevContainer,
    ) -> Result<()> {
        // get the folder path from the selected dev container
        let container_folder: String = dev_container.path_in_container.clone();

        let mut ws_path: String = self.path.to_string_lossy().into_owned();
        let mut dc_path: String = dev_container.path.to_string_lossy().into_owned();
        trace!("ws_path: {ws_path}");
        trace!("dc_path: {dc_path}");

        // detect WSL (excluding Docker containers)
        let is_wsl: bool = {
            #[cfg(unix)]
            {
                // Execute `uname -a` and capture the output
                let output = Command::new("uname")
                    .arg("-a")
                    .output()
                    .expect("Failed to execute command");

                // Convert the output to a UTF-8 string
                let uname_output = String::from_utf8(output.stdout)?;

                // Check if the output contains "Microsoft" or "WSL" which are indicators of WSL environment
                // Also we want to check for the WSLENV variable, which is not available in Docker containers
                (uname_output.contains("Microsoft") || uname_output.contains("WSL"))
                    && std::env::var("WSLENV").is_ok()
            }
            #[cfg(windows)]
            {
                false
            }
        };

        if is_wsl {
            debug!("WSL detected");
            ws_path = wslpath2::convert(
                ws_path.as_str(),
                None,
                wslpath2::Conversion::WslToWindows,
                true,
            )
            .map_err(|e| eyre!("Error while getting wslpath: {}", e))?;
            dc_path = wslpath2::convert(
                dc_path.as_str(),
                None,
                wslpath2::Conversion::WslToWindows,
                true,
            )
            .map_err(|e| eyre!("Error while getting wslpath: {}", e))?;
        }

        #[cfg(windows)]
        {
            ws_path = ws_path.replace("\\\\?\\", "");
            dc_path = dc_path.replace("\\\\?\\", "");
        }

        // note: gotta run on windows, linux and wsl. currently tested: only wsl

        trace!("ws_path: {ws_path}");
        trace!("dc_path: {dc_path}");
        let folder_uri = DevcontainerUriJson {
            host_path: ws_path,
            config_file: FileUriJson::new(dc_path.as_str()),
        };
        let json = serde_jsonc::to_string(&folder_uri)?;

        // let json = json!({
        //     "hostPath": ws_path,
        // }).to_string();
        trace!("text: {json}");

        let hex = hex::encode(json.as_bytes());
        let uri = format!("vscode-remote://dev-container+{hex}{container_folder}");

        let mut args = args.to_owned();
        args.push(OsStr::new("--folder-uri").to_owned());
        args.push(OsStr::new(uri.as_str()).to_owned());

        exec_code(&args, insiders, dry_run)
            .wrap_err_with(|| "Error opening vscode using dev container...")
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
