use color_eyre::eyre::{eyre, Result, WrapErr};
use log::{debug, info, trace};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

use crate::uri::{DevcontainerUriJson, FileUriJson};

/// A workspace is a folder which contains a vscode project.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Workspace {
    /// The path of the workspace.
    pub path: PathBuf,
    /// The name of the workspace.
    pub workspace_name: String,
    /// The devcontainer configurations
    pub devcontainers: Vec<Devcontainer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Devcontainer {
    pub path: PathBuf,
    pub name: Option<String>,
    pub path_in_container: String,
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

        // parse devcontainers and their properties
        let mut devcontainers = Vec::<Devcontainer>::new();
        let configs = find_devcontainer_configs(&path);
        for config_path in &configs {
            let devcontainer = parse_devcontainer_config(config_path)?;

            debug!("Parsed devcontainer config: {:?}", config_path);
            trace!("devcontainer config: {:?}", devcontainer);
            let folder: String = if let Some(folder) = devcontainer["workspaceFolder"].as_str() {
                debug!("Read workspace folder from config: {}", folder);
                folder.to_owned()
            } else {
                debug!("Could not read workspace folder from config -> using default folder");
                format!("/workspaces/{workspace_name}")
            };

            let name = if let Some(name) = devcontainer["name"].as_str() {
                trace!("Read workspace name from config: {}", name);
                Some(name.to_owned())
            } else {
                trace!("Could not read workspace name from config");
                None
            };

            devcontainers.push(Devcontainer {
                path: config_path.clone(),
                path_in_container: folder,
                name,
            });
        }

        let ws = Workspace {
            path,
            workspace_name,
            devcontainers,
        };
        trace!("{ws:?}");
        Ok(ws)
    }

    /// Open vscode using the devcontainer extension.
    pub fn open(
        &self,
        args: &[OsString],
        insiders: bool,
        dry_run: bool,
        devcontainer: Option<Devcontainer>,
    ) -> Result<()> {
        // get the folder path from the selected devcontainer
        let devcontainer: Devcontainer = if let Some(devcontainer) = devcontainer {
            devcontainer
        } else if self.devcontainers.len() == 1 {
            let devcontainer = self.devcontainers.get(0).expect("Index out of bounds");
            devcontainer.clone()
        } else {
            let mut list = String::new();
            for (i, devcontainer) in self.devcontainers.iter().enumerate() {
                let path = devcontainer.path.to_string_lossy();
                let display = if let Some(name) = devcontainer.name.clone() {
                    format!("- [{i}] {name}: {path}\n")
                } else {
                    format!("- [{i}] {path}\n")
                };
                list.push_str(&display);
            }
            info!("Multiple devcontainer configs found. Please specify which one to use with the --index flag:\n{list}");
            return Ok(());
        };

        let container_folder: String = devcontainer.path_in_container.clone();

        let mut ws_path: String = self.path.to_string_lossy().into_owned();
        let mut dc_path: String = devcontainer.path.to_string_lossy().into_owned();
        trace!("ws_path: {ws_path}");
        trace!("dc_path: {dc_path}");

        // detect WSL
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
                uname_output.contains("Microsoft") || uname_output.contains("WSL")
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
            .wrap_err_with(|| "Error opening vscode using devcontainer...")
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

/// Finds all devcontainer configs in the workspace.
///
/// # Note
/// This searches in the following locations:
/// - A `.devcontainer.json` defined directly in the workspace folder.
/// - A `.devcontainer/devcontainer.json` defined in the `.devcontainer/` folder.
/// - Any `.devcontainer/**/devcontainer.json` file in any `.devcontainer/` subfolder (only one level deep).
/// This should results in a devcontainer detection algorithm similar to the one vscode uses.
pub fn find_devcontainer_configs(path: &Path) -> Vec<PathBuf> {
    let mut configs = Vec::new();

    // check if we have a `devcontainer.json` directly in the workspace
    let direct_config = path.join(".devcontainer.json");
    if direct_config.is_file() {
        trace!("Found devcontainer config: {}", direct_config.display());
        configs.push(direct_config);
    }

    // check configs one level deep in `.devcontainer/`
    let devcontainer_dir = path.join(".devcontainer");
    for entry in WalkDir::new(devcontainer_dir)
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
        debug!("Found devcontainer config: {}", path.display());
        configs.push(path);
    }

    debug!(
        "Found {} devcontainer configs: {:?}",
        configs.len(),
        configs
    );

    configs
}

/// Parses the devcontainer config file.
/// `https://code.visualstudio.com/remote/advancedcontainers/change-default-source-mount`
fn parse_devcontainer_config(path: &Path) -> Result<serde_jsonc::Value> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let config: serde_jsonc::Value = serde_jsonc::from_reader(reader)
        .wrap_err_with(|| format!("Failed to parse json file: {path:?}"))?;
    Ok(config)
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
