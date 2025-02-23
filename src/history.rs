use chrono::{DateTime, Utc};
use color_eyre::eyre::{Context, Result, eyre};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::HashMap,
    fs::{self, File},
    path::PathBuf,
    sync::atomic::AtomicUsize,
};

use crate::launch::Behavior;

/// The maximum number of entries to keep in the history
// This is an arbitrary number, but it should be enough to keep the history manageable
const MAX_HISTORY_ENTRIES: usize = 35;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId(usize);

impl EntryId {
    pub fn new() -> Self {
        static GLOBAL_ID: AtomicUsize = AtomicUsize::new(0);
        Self(GLOBAL_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

/// Contains the recent used workspaces
///
/// # Note
/// We use a `BTreeSet` so it's sorted and does not contain duplicates
#[derive(Default, Debug, Clone)]
pub struct History(HashMap<EntryId, Entry>);

impl History {
    pub fn from_entries(entries: Vec<Entry>) -> Self {
        Self(
            entries
                .into_iter()
                .map(|entry| (EntryId::new(), entry))
                .collect(),
        )
    }

    pub fn insert(&mut self, entry: Entry) -> EntryId {
        let id = EntryId::new();
        assert_eq!(self.0.insert(id, entry), None);
        id
    }

    pub fn update(&mut self, id: EntryId, entry: Entry) -> Option<Entry> {
        if let std::collections::hash_map::Entry::Occupied(mut e) = self.0.entry(id) {
            return Some(e.insert(entry));
        }
        None
    }

    pub fn delete(&mut self, id: EntryId) -> Option<Entry> {
        self.0.remove(&id)
    }

    pub fn upsert(&mut self, entry: Entry) -> EntryId {
        if let Some(id) = self
            .0
            .iter_mut()
            .find_map(|(id, history_entry)| (history_entry == &entry).then_some(*id))
        {
            assert!(
                self.update(id, entry).is_some(),
                "Existing history entry to be replaced"
            );
            id
        } else {
            self.insert(entry)
        }
    }

    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, EntryId, Entry> {
        self.0.iter()
    }

    pub fn into_entries(self) -> Vec<Entry> {
        self.0.into_values().collect()
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
            match serde_json::from_reader::<_, Vec<Entry>>(file) {
                Ok(entries) => {
                    debug!("Imported {:?} history entries", entries.len());

                    Ok(Tracker {
                        path,
                        history: History::from_entries(entries),
                    })
                }
                Err(err) => {
                    // ignore parsing errors
                    // move the file and start from scratch

                    // find a non-existent backup file
                    let new_path = (0..10_000) // Set an upper limit of filename checks.
                        .map(|i| path.with_file_name(format!(".history_{i}.json.bak")))
                        .find(|path| !path.exists())
                        .unwrap_or_else(|| path.with_file_name(".history.json.bak"));

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

    /// Saves the history, guaranteeing a size of `MAX_HISTORY_ENTRIES`
    pub fn store(self) -> Result<()> {
        fs::create_dir_all(
            self.path
                .parent()
                .ok_or_else(|| eyre!("Parent directory not found"))?,
        )?;
        let file = File::create(self.path)?;

        // since history is sorted, we can remove the first entries to limit the max size
        let entries: Vec<Entry> = self
            .history
            .into_entries()
            .into_iter()
            .take(MAX_HISTORY_ENTRIES)
            .collect();

        serde_json::to_writer_pretty(file, &entries)?;
        Ok(())
    }
}
