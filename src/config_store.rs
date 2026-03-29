use color_eyre::eyre::{Result, WrapErr, bail};
use log::{debug, info, trace, warn};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::opts::ConfigAction;
use crate::ui;

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

        for entry in std::fs::read_dir(&source_root).wrap_err_with(|| {
            format!("Failed to read config directory: {}", source_root.display())
        })? {
            let entry = entry.wrap_err("Failed to read config directory entry")?;
            let source_path = entry.path();
            let destination_path = target_dir.join(entry.file_name());
            Self::ensure_copy_target_available(&source_path, &destination_path)?;
        }

        for entry in std::fs::read_dir(&source_root).wrap_err_with(|| {
            format!("Failed to read config directory: {}", source_root.display())
        })? {
            let entry = entry.wrap_err("Failed to read config directory entry")?;
            let source_path = entry.path();
            let destination_path = target_dir.join(entry.file_name());
            Self::copy_entry(&source_path, &destination_path)?;
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

    fn ensure_copy_target_available(source: &Path, destination: &Path) -> Result<()> {
        if destination.exists() {
            bail!(
                "Refusing to overwrite existing path: {}",
                destination.display()
            );
        }

        if source.is_dir() {
            for entry in std::fs::read_dir(source).wrap_err_with(|| {
                format!("Failed to read source directory: {}", source.display())
            })? {
                let entry = entry.wrap_err("Failed to read source directory entry")?;
                let child_source = entry.path();
                let child_destination = destination.join(entry.file_name());
                Self::ensure_copy_target_available(&child_source, &child_destination)?;
            }
        }

        Ok(())
    }

    fn copy_entry(source: &Path, destination: &Path) -> Result<()> {
        if destination.exists() {
            bail!(
                "Refusing to overwrite existing path: {}",
                destination.display()
            );
        }

        if source.is_dir() {
            std::fs::create_dir_all(destination).wrap_err_with(|| {
                format!(
                    "Failed to create destination directory: {}",
                    destination.display()
                )
            })?;

            for entry in std::fs::read_dir(source).wrap_err_with(|| {
                format!("Failed to read source directory: {}", source.display())
            })? {
                let entry = entry.wrap_err("Failed to read source directory entry")?;
                let child_source = entry.path();
                let child_destination = destination.join(entry.file_name());
                Self::copy_entry(&child_source, &child_destination)?;
            }
        } else if source.is_file() {
            std::fs::copy(source, destination).wrap_err_with(|| {
                format!(
                    "Failed to copy file from {} to {}",
                    source.display(),
                    destination.display()
                )
            })?;
        } else {
            bail!("Unsupported config entry: {}", source.display());
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

/// Attempts to derive a config name from a config path by checking if it lives
/// inside a known config store directory.
pub fn config_name_from_path(config_path: &Path, store: &ConfigStore) -> Option<String> {
    let store_dir = store.dir().canonicalize().ok()?;
    let config_canonical = config_path.canonicalize().ok()?;

    if !config_canonical.starts_with(&store_dir) {
        return None;
    }

    let relative = config_canonical.strip_prefix(&store_dir).ok()?;
    relative
        .components()
        .next()
        .and_then(|c| c.as_os_str().to_str())
        .map(String::from)
}

/// Runs a config subcommand.
pub fn run_command(action: ConfigAction, store: &ConfigStore, editor: &str) -> Result<()> {
    match action {
        ConfigAction::Ui => {
            let entries = store.list();
            if entries.is_empty() {
                println!("(no configs)");
                return Ok(());
            }
            let mut delete_cb = |item: &ui::ConfigItem| {
                if let Err(e) = store.rm(&item.0.name) {
                    log::warn!("Failed to remove config '{}': {e}", item.0.name);
                }
            };
            let selected =
                ui::pick_config(entries, ui::PickerOpts::default(), Some(&mut delete_cb))?;
            if let Some(config) = selected {
                info!("Opening config '{}' for editing...", config.name);
                std::process::Command::new(editor)
                    .arg(&config.root)
                    .output()?;
            }
        }
        ConfigAction::List { long } => {
            let entries = store.list();
            if entries.is_empty() {
                println!("(no configs)");
                return Ok(());
            }
            if long {
                let name_width = entries.iter().map(|e| e.name.len()).max().unwrap_or(4);
                let desc_width = entries
                    .iter()
                    .map(|e| e.description.as_deref().unwrap_or("").len())
                    .max()
                    .unwrap_or(4);
                println!("{:<name_width$}  {:<desc_width$}  PATH", "NAME", "DESC");
                for entry in &entries {
                    println!(
                        "{:<name_width$}  {:<desc_width$}  {}",
                        entry.name,
                        entry.description.as_deref().unwrap_or(""),
                        entry.root.display()
                    );
                }
            } else {
                for entry in &entries {
                    println!("{}", entry.name);
                }
            }
        }
        ConfigAction::Dir => {
            println!("{}", store.dir().display());
        }
        ConfigAction::Add { name } => {
            let root = store.add(&name)?;
            info!("Created config '{}' at {}", name, root.display());
        }
        ConfigAction::Copy { name, path } => {
            let target_dir = path.canonicalize().unwrap_or(path);
            store.copy_into(&name, &target_dir)?;
            info!("Copied config '{}' into {}", name, target_dir.display());
        }
        ConfigAction::Rm { name } => {
            let root = store.dir().join(&name);
            eprint!("Remove config '{name}' at {}? [y/N] ", root.display());
            std::io::stderr().flush()?;
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            if answer.trim().eq_ignore_ascii_case("y") {
                store.rm(&name)?;
                info!("Removed config '{name}'");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ConfigStore;
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn unique_test_dir(name: &str) -> PathBuf {
        let unique = format!(
            "vscli-config-store-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(OsString::from(unique))
    }

    #[test]
    fn copy_into_copies_config_contents_to_target_directory() {
        let store_dir = unique_test_dir("copy-success-store");
        let target_dir = unique_test_dir("copy-success-target");
        let store = ConfigStore::new(Some(store_dir.clone()));

        std::fs::create_dir_all(&target_dir).unwrap();
        let created_root = store.add("python-dev").unwrap();
        let script_path = created_root.join("scripts").join("setup.sh");
        std::fs::create_dir_all(script_path.parent().unwrap()).unwrap();
        std::fs::write(&script_path, "#!/bin/sh\n").unwrap();

        let copied_path = store.copy_into("python-dev", &target_dir).unwrap();

        assert_eq!(copied_path, target_dir.join(".devcontainer"));
        assert!(
            target_dir
                .join(".devcontainer")
                .join("devcontainer.json")
                .is_file()
        );
        assert!(target_dir.join("scripts").join("setup.sh").is_file());
    }

    #[test]
    fn copy_into_refuses_to_overwrite_existing_paths() {
        let store_dir = unique_test_dir("copy-overwrite-store");
        let target_dir = unique_test_dir("copy-overwrite-target");
        let store = ConfigStore::new(Some(store_dir));

        std::fs::create_dir_all(target_dir.join(".devcontainer")).unwrap();
        std::fs::write(
            target_dir.join(".devcontainer").join("devcontainer.json"),
            "{}\n",
        )
        .unwrap();
        store.add("python-dev").unwrap();

        let err = store.copy_into("python-dev", &target_dir).unwrap_err();

        assert!(
            err.to_string()
                .contains("Refusing to overwrite existing path")
        );
    }

    #[test]
    fn copy_into_does_not_partially_write_when_nested_conflict_exists() {
        let store_dir = unique_test_dir("copy-nested-conflict-store");
        let target_dir = unique_test_dir("copy-nested-conflict-target");
        let store = ConfigStore::new(Some(store_dir.clone()));

        std::fs::create_dir_all(&target_dir).unwrap();
        let created_root = store.add("python-dev").unwrap();
        let scripts_dir = created_root.join("scripts");
        std::fs::create_dir_all(&scripts_dir).unwrap();
        std::fs::write(created_root.join("README.md"), "template\n").unwrap();
        std::fs::write(scripts_dir.join("setup.sh"), "#!/bin/sh\n").unwrap();

        std::fs::create_dir_all(target_dir.join("scripts")).unwrap();
        std::fs::write(target_dir.join("scripts").join("setup.sh"), "existing\n").unwrap();

        let err = store.copy_into("python-dev", &target_dir).unwrap_err();

        assert!(
            err.to_string()
                .contains("Refusing to overwrite existing path")
        );
        assert!(!target_dir.join(".devcontainer").exists());
        assert!(!target_dir.join("README.md").exists());
    }
}
