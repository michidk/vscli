use color_eyre::eyre::{bail, eyre, Result, WrapErr};
use log::{debug, trace};
use std::ffi::OsString;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

use crate::uri::{DevcontainerUriJson, FileUriJson};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DevContainer {
    pub config_path: PathBuf,
    pub name: Option<String>,
    pub workspace_path_in_container: String,
}

// Used in the inquire select prompt
impl Display for DevContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = self.config_path.display();
        if let Some(name) = &self.name {
            write!(f, "{name} ({path})")
        } else {
            write!(f, "{path}")
        }
    }
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
            config_path: path.to_owned(),
            workspace_path_in_container: folder,
            name,
        })
    }

    /// Parses the dev container config file.
    /// `https://code.visualstudio.com/remote/advancedcontainers/change-default-source-mount`
    fn parse_dev_container_config(path: &Path) -> Result<serde_json::Value> {
        let content = std::fs::read_to_string(path)?;

        let config: serde_json::Value = json5::from_str(&content)
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
            bail!("Path {} does not exist", path.display());
        }

        // canonicalize path
        let path = std::fs::canonicalize(path).wrap_err_with(|| "Error canonicalizing path")?;
        trace!("Canonicalized path: {}", path.display());

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
            .filter(|e| matches!(e, Ok(x) if x.file_type().is_file() && x.file_name() == "devcontainer.json"))
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

    pub fn load_dev_containers(&self, paths: &[PathBuf]) -> Result<Vec<DevContainer>> {
        // parse dev containers and their properties
        paths
            .iter()
            .map(|config_path| DevContainer::from_config(config_path, &self.name))
            .collect::<Result<Vec<_>, _>>()
    }

    /// Open vscode using the specified dev container.
    pub fn open(
        &self,
        mut args: Vec<OsString>,
        insiders: bool,
        dry_run: bool,
        dev_container: &DevContainer,
    ) -> Result<()> {
        // Checking if '--folder-uri' is present in the arguments
        if args.iter().any(|arg| arg == "--folder-uri") {
            bail!("Specifying `--folder-uri` is not possible while using vscli.");
        }

        // get the folder path from the selected dev container
        let container_folder: String = dev_container.workspace_path_in_container.clone();

        let mut ws_path: String = self.path.to_string_lossy().into_owned();
        let mut dc_path: String = dev_container.config_path.to_string_lossy().into_owned();

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
            // CHECK: Not so nice to work on paths in "string" fashion
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

        let folder_uri = DevcontainerUriJson {
            host_path: ws_path,
            config_file: FileUriJson::new(dc_path.as_str()),
        };
        let json = serde_json::to_string(&folder_uri)?;

        trace!("Folder uri JSON: {json}");

        let hex = hex::encode(json.as_bytes());
        let uri = format!("vscode-remote://dev-container+{hex}{container_folder}");

        args.push(OsString::from("--folder-uri"));
        args.push(OsString::from(uri.as_str()));

        exec_code(args, insiders, dry_run)
            .wrap_err_with(|| "Error opening vscode using dev container...")
    }

    /// Open vscode like with the `code` command
    pub fn open_classic(
        &self,
        mut args: Vec<OsString>,
        insiders: bool,
        dry_run: bool,
    ) -> Result<()> {
        trace!("path: {}", self.path.display());
        trace!("args: {:?}", args);

        args.insert(0, self.path.as_os_str().to_owned());
        exec_code(args, insiders, dry_run)
            .wrap_err_with(|| "Error opening vscode the classic way...")
    }
}

/// Executes the vscode executable with the given arguments on Unix.
#[cfg(unix)]
fn exec_code(args: Vec<OsString>, insiders: bool, dry_run: bool) -> Result<()> {
    let cmd = if insiders { "code-insiders" } else { "code" };
    // test if cmd exists
    Command::new(cmd)
        .arg("-v")
        .output()
        .wrap_err_with(|| format!("`{cmd}` does not exists."))?;

    run(cmd, args, dry_run)
}

/// Executes the vscode executable with the given arguments on Windows.
#[cfg(windows)]
fn exec_code(mut args: Vec<OsString>, insiders: bool, dry_run: bool) -> Result<()> {
    let cmd = "cmd";
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

    run(cmd, args, dry_run)
}

/// Executes a command with given arguments and debug outputs, with an option for dry run
fn run(cmd: &str, args: Vec<OsString>, dry_run: bool) -> Result<()> {
    debug!("executable: {}", cmd);
    debug!("final args: {:?}", args);

    if !dry_run {
        let output = Command::new(cmd).args(args).output()?;
        debug!("Command output: {:?}", output);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_devcontainer() {
        let path = PathBuf::from("tests/fixtures/devcontainer.json");
        let result = DevContainer::from_config(&path, "test");
        assert!(result.is_ok());
        let dev_container = result.unwrap();

        assert_eq!(dev_container.config_path, path);
        assert_eq!(dev_container.name, Some(String::from("Rust")));
        assert_eq!(
            dev_container.workspace_path_in_container,
            "/workspaces/test"
        );
    }
}
