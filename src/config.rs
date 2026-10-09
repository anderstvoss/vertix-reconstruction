//! `config/server.toml`, with command-line and environment overrides.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub bind: String,
    pub port: u16,
    #[serde(default)]
    pub archive: String,
    pub manifest: PathBuf,
    /// Rule layers, merged in order; later layers override earlier ones.
    pub rules: Vec<PathBuf>,
    pub maps: Maps,
    #[serde(default)]
    pub trace: String,
    #[serde(default)]
    pub trace_brief: Vec<String>,
    pub engine_io: EngineIo,
}

/// Where maps come from.
#[derive(Debug, Clone, Deserialize)]
pub struct Maps {
    /// `archive`: the 24 provisional maps in the archive clone.
    /// `files`: the text maps listed in `files`.
    pub source: MapSourceKind,
    #[serde(default)]
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MapSourceKind {
    Archive,
    Files,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EngineIo {
    pub ping_interval_ms: u64,
    pub ping_timeout_ms: u64,
}

impl EngineIo {
    #[must_use]
    pub fn timing(&self) -> crate::eio::Timing {
        crate::eio::Timing {
            ping_interval: Duration::from_millis(self.ping_interval_ms),
            ping_timeout: Duration::from_millis(self.ping_timeout_ms),
            ..crate::eio::Timing::default()
        }
    }
}

impl Config {
    /// Reads a config file.
    ///
    /// # Errors
    /// Fails if the file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The archive path: `--archive`, else `VERTIX_ARCHIVE`, else the file.
    #[must_use]
    pub fn archive_path(&self, cli: Option<&str>) -> Option<PathBuf> {
        cli.map(str::to_owned)
            .or_else(|| std::env::var("VERTIX_ARCHIVE").ok())
            .or_else(|| (!self.archive.is_empty()).then(|| self.archive.clone()))
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_config_parses() {
        let c = Config::load(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/config/server.toml"
        )))
        .unwrap();
        assert_eq!(c.engine_io.ping_interval_ms, 25_000);
        assert_eq!(c.engine_io.ping_timeout_ms, 60_000);
        assert!(c.trace_brief.iter().any(|e| e == "rsd"));
        assert_eq!(c.maps.source, MapSourceKind::Archive);
        assert!(c.rules.len() >= 2);
    }
}
