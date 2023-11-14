use serde::{ser::SerializeMap, Serialize};
use url::Url;

/// Represents a single file path to a dev container config as expected by the code CLI.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileUriJson {
    path: Url,
    authority: Option<String>,
}

impl FileUriJson {
    /// Creates a new `FileUri` from a given string slice
    pub fn new(uri: &str) -> Self {
        let fixed_uri = format!("file://{uri}")
            .replace("\\\\", "")
            .replace('\\', "/");
        let parsed_url = Url::parse(&fixed_uri).expect("Invalid URI");

        Self {
            authority: parsed_url.host_str().map(ToString::to_string),
            path: parsed_url,
        }
    }
}

impl Serialize for FileUriJson {
    /// Creates the JSON representation of the `FileUri`.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("scheme", "file")?;
        if let Some(authority) = &self.authority {
            map.serialize_entry("authority", authority)?;
        }
        map.serialize_entry("path", self.path.path())?;
        map.end()
    }
}

/// Represents a dev container launch argument as expected by the code CLI.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct DevcontainerUriJson {
    /// The path to the dev container workspace
    #[serde(rename = "hostPath")]
    pub host_path: String,
    // The path to the dev container config file
    #[serde(rename = "configFile")]
    pub config_file: FileUriJson,
}
