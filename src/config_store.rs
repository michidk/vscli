use color_eyre::eyre::{Result, WrapErr, bail};
use log::{debug, trace, warn};
use std::path::{Path, PathBuf};

mod commands;
mod copy;
mod path;

use copy::CopyPlan;

pub use commands::run_command;
pub use path::config_name_from_path;

const MINIMAL_DEVCONTAINER: &str = r#"{
    "name": "{name}",
    "image": "mcr.microsoft.com/devcontainers/base:ubuntu"
}
"#;

/// Represents a discovered config entry in the config directory.
#[derive(Debug, Clone)]
pub struct ConfigEntry {
    /// The short name of the config (directory name).
    pub name: String,
    /// The full path to the config root directory.
    pub root: PathBuf,
    /// The description from the devcontainer.json "name" field, if available.
    pub description: Option<String>,
}

/// Manages external devcontainer configs stored in a central directory.
///
/// Configs are directories containing `.devcontainer/devcontainer.json`,
/// stored under a configurable root directory.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    dir: PathBuf,
}

impl ConfigStore {
    /// Creates a new `ConfigStore` with the given directory, or the default.
    ///
    /// Default: `$XDG_DATA_HOME/vscli/configs` (typically `~/.local/share/vscli/configs`).
    pub fn new(dir: Option<PathBuf>) -> Self {
        let dir = dir.unwrap_or_else(|| {
            let mut path = dirs::data_local_dir().expect("Local data dir not found");
            path.push("vscli");
            path.push("configs");
            path
        });
        Self { dir }
    }

    /// Returns the config directory path.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Resolves a config name or path to a `devcontainer.json` path.
    ///
    /// Resolution order:
    /// 1. If the value is an existing filesystem path, use it directly.
    /// 2. Otherwise, try to resolve it as a name in the config directory.
    pub fn resolve(&self, name_or_path: &Path) -> Option<PathBuf> {
        if let Some(path) = Self::resolve_as_path(name_or_path) {
            trace!("Resolved config as direct path: {}", name_or_path.display());
            return Some(path);
        }

        if let Some(name) = name_or_path.to_str()
            && let Some(path) = self.resolve_name(name)
        {
            debug!("Resolved config name '{}' to: {}", name, path.display());
            return Some(path);
        }

        None
    }

    /// Lists all valid configs in the config directory.
    pub fn list(&self) -> Vec<ConfigEntry> {
        let mut entries = Vec::new();

        if !self.dir.is_dir() {
            debug!("Config directory does not exist: {}", self.dir.display());
            return entries;
        }

        let Ok(read_dir) = std::fs::read_dir(&self.dir) else {
            warn!("Failed to read config directory: {}", self.dir.display());
            return entries;
        };

        for dir_entry in read_dir.flatten() {
            let path = dir_entry.path();
            if !path.is_dir() {
                continue;
            }

            let config_path = path.join(".devcontainer").join("devcontainer.json");
            if !config_path.is_file() {
                trace!("Skipping invalid config directory: {}", path.display());
                continue;
            }

            let name = dir_entry.file_name().to_string_lossy().into_owned();

            let description = Self::read_config_name(&config_path);

            entries.push(ConfigEntry {
                name,
                root: path,
                description,
            });
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    /// Creates a minimal config with the given name.
    pub fn add(&self, name: &str) -> Result<PathBuf> {
        Self::validate_name(name)?;
        let root = self.dir.join(name);
        if root.exists() {
            bail!("Config '{}' already exists at: {}", name, root.display());
        }

        let devcontainer_dir = root.join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).wrap_err_with(|| {
            format!(
                "Failed to create config directory: {}",
                devcontainer_dir.display()
            )
        })?;

        let config_path = devcontainer_dir.join("devcontainer.json");
        let content = MINIMAL_DEVCONTAINER.replace("{name}", name);
        std::fs::write(&config_path, content)
            .wrap_err_with(|| format!("Failed to write config: {}", config_path.display()))?;

        Ok(root)
    }

    /// Removes a config by name.
    pub fn rm(&self, name: &str) -> Result<()> {
        Self::validate_name(name)?;
        let root = self.dir.join(name);
        if !root.exists() {
            bail!("Config '{}' not found", name);
        }

        let canonical_root = root
            .canonicalize()
            .wrap_err("Failed to canonicalize config path")?;
        let canonical_dir = self
            .dir
            .canonicalize()
            .wrap_err("Failed to canonicalize config dir")?;
        if !canonical_root.starts_with(&canonical_dir) {
            bail!(
                "Refusing to remove path outside config directory: {}",
                root.display()
            );
        }

        std::fs::remove_dir_all(&root)
            .wrap_err_with(|| format!("Failed to remove config: {}", root.display()))?;

        Ok(())
    }

    /// Copies a stored config into the given target directory.
    pub fn copy_into(&self, name: &str, target_dir: &Path) -> Result<PathBuf> {
        Self::validate_name(name)?;

        let source_root = self.dir.join(name);
        if !source_root.is_dir() {
            bail!("Config '{}' not found", name);
        }

        if !target_dir.exists() {
            bail!("Target directory does not exist: {}", target_dir.display());
        }
        if !target_dir.is_dir() {
            bail!("Target path is not a directory: {}", target_dir.display());
        }

        let plans = CopyPlan::children(&source_root, target_dir)?;
        for plan in plans {
            plan.execute()?;
        }

        Ok(target_dir.join(".devcontainer"))
    }

    fn validate_name(name: &str) -> Result<()> {
        if name.is_empty()
            || name.contains('/')
            || (cfg!(windows) && name.contains('\\'))
            || name.contains('\0')
            || name == "."
            || name == ".."
        {
            bail!("Invalid config name: '{name}'");
        }
        Ok(())
    }

    /// Tries to resolve a plain name to a devcontainer.json path within the config directory.
    fn resolve_name(&self, name: &str) -> Option<PathBuf> {
        let candidate = self
            .dir
            .join(name)
            .join(".devcontainer")
            .join("devcontainer.json");
        candidate.is_file().then_some(candidate)
    }

    /// Tries to resolve an arbitrary path to a devcontainer.json file.
    fn resolve_as_path(path: &Path) -> Option<PathBuf> {
        if path.is_file() {
            if path.file_name().and_then(|f| f.to_str()) == Some("devcontainer.json") {
                return Some(path.to_owned());
            }
            return None;
        }

        if path.is_dir() {
            let candidate = path.join(".devcontainer").join("devcontainer.json");
            if candidate.is_file() {
                return Some(candidate);
            }
            let candidate = path.join("devcontainer.json");
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        None
    }

    /// Reads the "name" field from a devcontainer.json, if present.
    fn read_config_name(config_path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(config_path).ok()?;
        let parsed: serde_json::Value = json5::from_str(&content).ok()?;
        parsed["name"].as_str().map(String::from)
    }
}

#[cfg(test)]
mod tests;
