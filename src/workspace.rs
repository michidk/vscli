use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use log::{debug, trace};
use std::ffi::OsString;
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

impl DevContainer {
    /// Creates a new `DevContainer` from a dev container config file and fallback workspace name.
    pub fn from_config(path: &Path, workspace_name: &str) -> Result<DevContainer> {
        let dev_container = Self::parse_dev_container_config(path)?;
        trace!("dev container config: {dev_container:?}");

        let folder: String = if let Some(folder) = dev_container["workspaceFolder"].as_str() {
            debug!("Read workspace folder from config: {folder}");
            // Substitute variables in the workspace folder path
            Self::substitute_variables(folder, workspace_name)
        } else {
            debug!("Could not read workspace folder from config -> using default folder");
            format!("/workspaces/{workspace_name}")
        };
        trace!("Workspace folder: {folder}");

        let name = if let Some(name) = dev_container["name"].as_str() {
            debug!("Read workspace name from config: {name}");
            Some(name.to_owned())
        } else {
            debug!("Could not read workspace name from config");
            None
        };
        trace!("Workspace name: {name:?}");

        Ok(DevContainer {
            config_path: path.to_owned(),
            workspace_path_in_container: folder,
            name,
        })
    }

    /// Parses the dev container config file.
    /// `https://code.visualstudio.com/remote/advancedcontainers/change-default-source-mount`
    fn parse_dev_container_config(path: &Path) -> Result<serde_json::Value> {
        let path_log = path.display();

        let content = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("Failed to read dev container config file: {path_log}"))?;

        let config: serde_json::Value = json5::from_str(&content)
            .wrap_err_with(|| format!("Failed to parse json file: {path_log}"))?;

        debug!("Parsed dev container config: {path_log}");
        Ok(config)
    }

    /// Substitutes variables in the workspace folder path.
    /// Supports the following variables:
    /// - ${localWorkspaceFolderBasename} - The name of the workspace folder
    /// - ${localWorkspaceFolder} - The full path to the workspace folder (defaults to /workspaces/<name>)
    fn substitute_variables(folder: &str, workspace_name: &str) -> String {
        let mut result = folder.to_owned();

        // Replace ${localWorkspaceFolderBasename} with the workspace name
        result = result.replace("${localWorkspaceFolderBasename}", workspace_name);

        // Replace ${localWorkspaceFolder} with the full workspace path
        // This defaults to /workspaces/<workspace_name> for consistency
        let default_workspace = format!("/workspaces/{workspace_name}");
        result = result.replace("${localWorkspaceFolder}", &default_workspace);

        result
    }
}

/// A workspace is a folder which contains a vscode project.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Workspace {
    /// The path of the workspace.
    pub path: PathBuf,
    /// The name of the workspace.
    pub name: String,
    /// The remote SSH host alias, if this workspace is remote.
    pub remote_host: Option<String>,
}

impl Workspace {
    /// Creates a new `Workspace` from the given path to a folder.
    pub fn from_path(path: &Path) -> Result<Workspace> {
        // check for valid path
        if !path.exists() {
            bail!("Path {} does not exist", path.display());
        }

        // canonicalize path
        let path_log = path.display();
        let path = std::fs::canonicalize(path)
            .wrap_err_with(|| format!("Error canonicalizing path: {path_log}"))?;
        trace!("Canonicalized path: {path_log}");

        // get workspace name (either directory or file name)
        let workspace_name = workspace_name(&path)?;
        trace!("Workspace name: {workspace_name}");

        let ws = Workspace {
            path,
            name: workspace_name,
            remote_host: None,
        };
        trace!("{ws:?}");
        Ok(ws)
    }

    /// Creates a new remote `Workspace` from the given absolute path and SSH host alias.
    pub fn from_remote_path(path: &Path, remote_host: String) -> Result<Workspace> {
        if !path.is_absolute() {
            bail!("Remote path must be absolute: {}", path.display());
        }

        let workspace_name = workspace_name(path)?;
        let ws = Workspace {
            path: path.to_path_buf(),
            name: workspace_name,
            remote_host: Some(remote_host),
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
    ///
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
        dry_run: bool,
        dev_container: &DevContainer,
        command: &str,
        subfolder: Option<&Path>,
    ) -> Result<()> {
        if args.iter().any(|arg| arg == "--folder-uri") {
            bail!("Specifying `--folder-uri` is not possible while using vscli.");
        }

        if self.remote_host.is_some() {
            let uri = self.remote_folder_uri(subfolder);
            args.push(OsString::from("--folder-uri"));
            args.push(OsString::from(uri));
            return exec_code(args, dry_run, command)
                .wrap_err_with(|| "Error opening vscode using remote SSH...");
        }

        let mut container_folder: String = dev_container.workspace_path_in_container.clone();
        if let Some(sub) = subfolder {
            let sub_str = sub.to_string_lossy().replace('\\', "/");
            if !sub_str.is_empty() && sub_str != "." {
                if !container_folder.ends_with('/') {
                    container_folder.push('/');
                }
                container_folder.push_str(&sub_str);
            }
        }

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

            ws_path = wslpath2::convert(
                ws_path.as_str(),
                None,
                wslpath2::Conversion::WslToWindows,
                true,
            )
            .map_err(|e| eyre!("Error while getting wslpath: {} (path: {ws_path:?})", e))?;
            dc_path = wslpath2::convert(
                dc_path.as_str(),
                None,
                wslpath2::Conversion::WslToWindows,
                true,
            )
            .map_err(|e| eyre!("Error while getting wslpath: {} (path: {dc_path:?})", e))?;
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

        exec_code(args, dry_run, command)
            .wrap_err_with(|| "Error opening vscode using dev container...")
    }

    /// Open vscode like with the `code` command
    pub fn open_classic(
        &self,
        mut args: Vec<OsString>,
        dry_run: bool,
        command: &str,
    ) -> Result<()> {
        trace!("path: {}", self.path.display());
        trace!("args: {args:?}");

        if self.remote_host.is_some() {
            let uri = self.remote_folder_uri(None);
            args.push(OsString::from("--folder-uri"));
            args.push(OsString::from(uri));
            return exec_code(args, dry_run, command)
                .wrap_err_with(|| "Error opening vscode using remote SSH...");
        }

        args.insert(0, self.path.as_os_str().to_owned());
        exec_code(args, dry_run, command)
            .wrap_err_with(|| "Error opening vscode the classic way...")
    }

    fn remote_folder_uri(&self, subfolder: Option<&Path>) -> String {
        let host = self
            .remote_host
            .as_deref()
            .expect("remote folder URI requires remote host");
        let remote_path = remote_workspace_path(&self.path, subfolder);
        format!("vscode-remote://ssh-remote+{host}{remote_path}")
    }
}

fn workspace_name(path: &Path) -> Result<String> {
    if let Some(name) = path.file_name() {
        return Ok(name.to_string_lossy().into_owned());
    }

    let display = path.display().to_string();
    if display.is_empty() {
        Err(eyre!("Error getting workspace from path"))
    } else {
        Ok(display)
    }
}

fn remote_workspace_path(path: &Path, subfolder: Option<&Path>) -> String {
    let mut remote_path = path.to_string_lossy().replace('\\', "/");
    if !remote_path.starts_with('/') {
        remote_path.insert(0, '/');
    }

    if let Some(subfolder) = subfolder {
        let subfolder = subfolder.to_string_lossy().replace('\\', "/");
        if !subfolder.is_empty() && subfolder != "." {
            if !remote_path.ends_with('/') {
                remote_path.push('/');
            }
            remote_path.push_str(subfolder.trim_start_matches('/'));
        }
    }

    remote_path
}

/// Executes the vscode executable with the given arguments on Unix.
#[cfg(unix)]
fn exec_code(args: Vec<OsString>, dry_run: bool, command: &str) -> Result<()> {
    // test if cmd exists
    Command::new(command)
        .arg("-v")
        .output()
        .wrap_err_with(|| format!("`{command}` does not exists."))?;

    run(command, args, dry_run)
}

/// Executes the vscode executable with the given arguments on Windows.
#[cfg(windows)]
fn exec_code(mut args: Vec<OsString>, dry_run: bool, command: &str) -> Result<()> {
    let cmd = "cmd";
    args.insert(0, OsString::from("/c"));
    args.insert(1, OsString::from(command));

    // test if cmd exists
    Command::new(cmd)
        .arg("-v")
        .output()
        .wrap_err_with(|| format!("`{cmd}` does not exists."))?;

    run(cmd, args, dry_run)
}

/// Executes a command with given arguments and debug outputs, with an option for dry run
fn run(cmd: &str, args: Vec<OsString>, dry_run: bool) -> Result<()> {
    debug!("executable: {cmd}");
    debug!("final args: {args:?}");

    if !dry_run {
        let output = Command::new(cmd).args(args).output()?;
        debug!("Command output: {output:?}");
    }

    Ok(())
}

/// Converts a Docker label path (which may be a Windows/WSL path) to a local filesystem path.
#[cfg(unix)]
pub fn resolve_local_path(path: &str) -> String {
    if (path.starts_with("\\\\wsl") || path.starts_with("//wsl"))
        && let Ok(converted) =
            wslpath2::convert(path, None, wslpath2::Conversion::WindowsToWsl, true)
    {
        return converted;
    }
    path.to_string()
}

/// Converts a Docker label path (which may be a Windows/WSL path) to a local filesystem path.
#[cfg(windows)]
pub fn resolve_local_path(path: &str) -> String {
    path.to_string()
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

    #[test]
    fn test_substitute_variables() {
        // Test ${localWorkspaceFolderBasename} substitution
        let folder = "/workspaces/${localWorkspaceFolderBasename}";
        let result = DevContainer::substitute_variables(folder, "my-project");
        assert_eq!(result, "/workspaces/my-project");

        // Test ${localWorkspaceFolder} substitution
        let folder = "${localWorkspaceFolder}/src";
        let result = DevContainer::substitute_variables(folder, "my-project");
        assert_eq!(result, "/workspaces/my-project/src");

        // Test combination of variables
        let folder = "${localWorkspaceFolder}/${localWorkspaceFolderBasename}-test";
        let result = DevContainer::substitute_variables(folder, "my-project");
        assert_eq!(result, "/workspaces/my-project/my-project-test");

        // Test no variables
        let folder = "/custom/path";
        let result = DevContainer::substitute_variables(folder, "my-project");
        assert_eq!(result, "/custom/path");
    }

    #[test]
    fn test_remote_workspace_path_appends_subfolder() {
        let path = remote_workspace_path(
            Path::new("/home/dev/workspace"),
            Some(Path::new("apps/api")),
        );
        assert_eq!(path, "/home/dev/workspace/apps/api");
    }

    #[test]
    fn test_remote_workspace_path_normalizes_windows_separators() {
        let path = remote_workspace_path(
            Path::new("/home/dev/workspace"),
            Some(Path::new("apps\\api")),
        );
        assert_eq!(path, "/home/dev/workspace/apps/api");
    }

    #[test]
    fn test_remote_workspace_uri_uses_ssh_remote_scheme() {
        let ws = Workspace::from_remote_path(
            Path::new("/home/dev/workspace"),
            "remote-test".to_string(),
        )
        .unwrap();
        assert_eq!(
            ws.remote_folder_uri(Some(Path::new("packages/api"))),
            "vscode-remote://ssh-remote+remote-test/home/dev/workspace/packages/api"
        );
    }
}
