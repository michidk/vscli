use super::ConfigStore;
use std::path::Path;

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
        .and_then(|component| component.as_os_str().to_str())
        .map(String::from)
}
