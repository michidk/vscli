use chrono::{DateTime, Utc};
use color_eyre::eyre::Result;
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::File,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use crate::launch::Behaviour;

const MAX_HISTORY_ENTRIES: usize = 100;

/// An entry in the history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub behaviour: Behaviour,
    pub last_opened: DateTime<Utc>, // not used in PartialEq, Eq, Hash
}

// Custom hash which igonres `last_opened`.
// This is used to ensure our hashsets only ignore one instance of each entry, ignoring the timestamp
impl Hash for Entry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.path.hash(state);
        self.behaviour.hash(state);
    }
}

// When importing hashsets in serde, the elements are compared to each other to check if they have been read before
// So to achieve, what we want to achive with our custom `Hash` impl, we need to impl `Eq` as well
impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.path == other.path && self.behaviour == other.behaviour
    }
}

impl Eq for Entry {}

/// Contains the recent used workspaces
pub type History = HashSet<Entry>;

/// The struct that manages the history and tracks the workspaces
pub struct Tracker<'a> {
    path: &'a Path,
    history: History,
}

impl<'a> Tracker<'a> {
    /// Loads the history from a file
    pub fn load<P: AsRef<Path> + 'a + ?Sized>(path_ref: &'a P) -> Result<Self> {
        let path: &Path = path_ref.as_ref();
        if path.exists() {
            let file = File::open(path.clone())?;
            let history: Result<History, serde_json::Error> = serde_json::from_reader(file);

            // ignore parsing errors, just reset the file
            if history.is_err() {
                warn!("Could not read history file, resetting.");
                return Ok(Self {
                    path,
                    history: HashSet::with_capacity(1),
                });
            }

            let history = history.unwrap(); // UNWRAP: we cool, since we check for err before

            debug!("Imported {:?} history entries", history.len());

            Ok(Self { path, history })
        } else {
            // cap of 1, because in the application lifetime, we only ever add one element before exeting
            Ok(Self {
                path,
                history: HashSet::with_capacity(1),
            })
        }
    }

    /// Pushes a new entry to the history
    pub fn push(&mut self, entry: Entry) {
        // if there is an entry with the same hash already present, we remove it
        if self.history.contains(&entry) {
            self.history.remove(&entry);
        }

        self.history.insert(entry);
    }

    /// Converts history to a vec and sorts it by `last_opened`
    pub fn get_sorted_history_vec(&self) -> Vec<Entry> {
        // convert history to a vec
        let mut history: Vec<Entry> = self.history.clone().into_iter().collect();
        // sort that vec by `last_opened`
        history.sort_by(|a, b| a.last_opened.cmp(&b.last_opened));
        history
    }

    /// Saves the history, guarateering a size of `MAX_HISTORY_ENTRIES`
    pub fn store(self) -> Result<()> {
        let file = File::create(self.path)?;

        // convert history to a vec & sort by `last_opened`
        let history = self.get_sorted_history_vec();
        // since history is sorted, be can remove the first entries to limit the max size
        let history: History = history
            .iter()
            .rev()
            .take(MAX_HISTORY_ENTRIES)
            .rev()
            .cloned()
            .collect();

        serde_json::to_writer_pretty(file, &history)?;
        Ok(())
    }
}
