use chrono::{DateTime, Utc};
use color_eyre::eyre::{eyre, Context, Result};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::BTreeSet,
    fs::{self, File},
    ops::{Deref, DerefMut},
    path::PathBuf,
};

use crate::launch::Behavior;

const MAX_HISTORY_ENTRIES: usize = 20;

/// An entry in the history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// The name of the workspace
    pub workspace_name: String,
    /// The name of the dev container, if it exists
    pub dev_container_name: Option<String>,
    /// The path to the vscode workspace
    pub workspace_path: PathBuf,
    /// The path to the dev container config, if it exists
    pub config_path: Option<PathBuf>,
    /// The launch behavior
    #[serde(alias = "behaviour")]
    pub behavior: Behavior,
    /// The time this entry was last opened
    pub last_opened: DateTime<Utc>, // not used in PartialEq, Eq, Hash
}

// Custom comparison which ignores `last_opened` (and `name`)
// This is used so that we don't add duplicate entries with different timestamps
impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.workspace_path == other.workspace_path
            && self.config_path == other.config_path
            && self.behavior == other.behavior
    }
}

impl Eq for Entry {}

// Required by BTreeSet since it's sorted
impl Ord for Entry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // check if two are equal by comparing all properties, ignoring `last_opened` (calling custom `.eq()`)
        if self.eq(other) {
            return Ordering::Equal;
        }
        // If they are not equal, the ordering is given by `last_opened`
        self.last_opened.cmp(&other.last_opened)
    }
}

// Same as `Ord`
impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Contains the recent used workspaces
///
/// # Note
/// We use a `BTreeSet` so it's sorted and does not contain duplicates
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct History(BTreeSet<Entry>);

// Transparent wrapper around `BTreeSet`
impl Deref for History {
    type Target = BTreeSet<Entry>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for History {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl History {
    /// Reverse iteration order, to display the most recent entries first
    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.0.iter().rev()
    }
}

/// Manages the history and tracks the recently used workspaces
pub struct Tracker {
    /// The path to the history file
    path: PathBuf,
    /// The history struct
    pub history: History,
}

impl Tracker {
    /// Loads the history from a file
    pub fn load<P: Into<PathBuf>>(path: P) -> Result<Self> {
        // Code size optimization: With rusts monomorphization it would generate
        // a "new/separate" function for each generic argument used to call this function.
        // Having this inner function does not prevent it but can drastically cuts down on generated code size.
        fn load_inner(path: PathBuf) -> Result<Tracker> {
            if !path.exists() {
                // cap of 1, because in the application lifetime, we only ever add one element before exiting
                return Ok(Tracker {
                    path,
                    history: History::default(),
                });
            }

            let file = File::open(&path)?;
            match serde_jsonc::from_reader::<_, History>(file) {
                Ok(history) => {
                    debug!("Imported {:?} history entries", history.len());

                    Ok(Tracker { path, history })
                }
                Err(err) => {
                    // ignore parsing errors
                    // move the file and start from scratch

                    // find a non-existent backup file
                    let new_path = (0..10_000) // Set an upper limit of filename checks.
                    .map(|i| path.with_file_name(format!(".vscli_history_{i}.json.bak")))
                    .find(|path| !path.exists())
                    .unwrap_or_else(|| path.with_file_name(".vscli_history.json.bak"));

                    fs::rename(&path, &new_path).wrap_err_with(|| {
                        format!(
                            "Could not move history file from `{}` to `{}`",
                            path.display(),
                            new_path.display()
                        )
                    })?;

                    warn!(
                        "Could not read history file: {err}\nMoved broken file to `{}`",
                        new_path.display()
                    );

                    Ok(Tracker {
                        path,
                        history: History::default(),
                    })
                }
            }
        }

        let path = path.into();
        load_inner(path)
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

        // since history is sorted, we can remove the first entries to limit the max size
        let history: Vec<Entry> = self
            .history
            .0
            .into_iter()
            .take(MAX_HISTORY_ENTRIES)
            .collect();

        serde_jsonc::to_writer_pretty(file, &history)?;
        Ok(())
    }
}
