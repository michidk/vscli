use chrono::{DateTime, Utc};
use color_eyre::eyre::{eyre, Context, Result};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::BTreeSet,
    fs::{self, File},
    path::{Path, PathBuf},
};

use crate::launch::Behavior;

const MAX_HISTORY_ENTRIES: usize = 20;

/// An entry in the history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    #[serde(alias = "behaviour")]
    pub behavior: Behavior,
    pub last_opened: DateTime<Utc>, // not used in PartialEq, Eq, Hash
}

// Custom comparison which ignores `last_opened`
// This is used so that we don't add duplicate entries with different timestamps
impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.path == other.path && self.behavior == other.behavior
    }
}

impl Eq for Entry {}

// Required by BTreeSet since it's sorted
impl Ord for Entry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // check if two are equal by comparing all properties but `last_opened`
        if self.eq(other) {
            return Ordering::Equal;
        }
        // if they are not, sort them by `last_opened`
        self.last_opened.cmp(&other.last_opened)
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.last_opened.cmp(&other.last_opened))
    }
}

/// Contains the recent used workspaces
/// Note: `BTreeSet` so it's sorted and unique
pub type History = BTreeSet<Entry>;

/// Manages the history and tracks the workspaces
pub struct Tracker<'a> {
    path: &'a Path,
    pub history: History,
}

impl<'a> Tracker<'a> {
    /// Loads the history from a file
    pub fn load<P: AsRef<Path> + 'a>(path_ref: &'a P) -> Result<Self> {
        let path: &Path = path_ref.as_ref();
        if path.exists() {
            let file = File::open(path.clone())?;
            let history: Result<History, serde_jsonc::Error> = serde_jsonc::from_reader(file);

            // ignore parsing errors
            // move the file and start from scratch
            if let Err(err) = history {
                // find a non-existent backup file
                let new_path = (0..10_000) // Set an upper limit of filename checks.
                    .map(|i| path.with_file_name(format!(".vscli_history_{i}.json.bak")))
                    .find(|path| !path.exists())
                    .unwrap_or_else(|| path.with_file_name(".vscli_history.json.bak"));

                fs::rename(path, new_path.clone()).wrap_err_with(|| {
                    format!(
                        "Could not move history file from `{}` to `{}`",
                        path.to_string_lossy(),
                        new_path.to_string_lossy()
                    )
                })?;

                warn!(
                    "Could not read history file: {err}\nMoved broken file to `{}`",
                    new_path.to_string_lossy()
                );
                return Ok(Self {
                    path,
                    history: BTreeSet::new(),
                });
            }

            let history = history.unwrap(); // UNWRAP: we cool, since we check for err before
            debug!("Imported {:?} history entries", history.len());

            Ok(Self { path, history })
        } else {
            // cap of 1, because in the application lifetime, we only ever add one element before exiting
            Ok(Self {
                path,
                history: BTreeSet::new(),
            })
        }
    }

    /// Pushes a new entry to the history
    pub fn push(&mut self, entry: Entry) {
        self.history.replace(entry);
    }

    /// Saves the history, guaranteeing a size of `MAX_HISTORY_ENTRIES`
    pub fn store(self) -> Result<()> {
        fs::create_dir_all(
            self.path
                .parent()
                .ok_or_else(|| eyre!("Parent directory not found"))?,
        )?;
        let file = File::create(self.path)?;

        // since history is sorted, be can remove the first entries to limit the max size
        let history: History = self
            .history
            .iter()
            .rev()
            .take(MAX_HISTORY_ENTRIES)
            .cloned()
            .collect();

        serde_jsonc::to_writer_pretty(file, &history)?;
        Ok(())
    }
}
