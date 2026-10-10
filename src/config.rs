//! `config/server.toml`, with command-line and environment overrides.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub bind: String,
    /// Preferred ports: a taken one moves up unless `strict_ports`.
    pub port: u16,
    #[serde(default)]
    pub strict_ports: bool,
    /// Where the bound addresses are written; empty for nowhere.
    #[serde(default)]
    pub ports_file: String,
    #[serde(default)]
    pub archive: String,
    /// The built `KrunkerRevival` client (`scripts/build-client.sh`).
    pub client_dir: PathBuf,
    /// Rule layers, merged in order; later layers override earlier ones.
    pub rules: Vec<PathBuf>,
    pub maps: Maps,
    pub game: Game,
    #[serde(default)]
    pub trace: String,
    #[serde(default)]
    pub trace_brief: Vec<String>,
    pub engine_io: EngineIo,
    #[serde(default)]
    pub classic: Classic,
    #[serde(default)]
    pub admin: Admin,
}

/// The admin panel and dev console (`src/admin`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Admin {
    /// Serve the panel on `bind`:`port`.
    pub enabled: bool,
    pub bind: String,
    pub port: u16,
    /// The panel's token; empty for a new random one at every start.
    pub token: String,
    /// Read commands from the server's standard input.
    pub stdin: bool,
    /// Print the admin log (joins, chat, kill feed) to the terminal.
    pub stdin_log: bool,
}

/// The archived 2016-08-06 client, served from the archive on its own
/// port and seated in one of the rooms.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Classic {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub port: u16,
    /// The room its players join; the first room if empty.
    #[serde(default)]
    pub room: String,
    /// Which archived files make up the client (`data/boot`).
    #[serde(default)]
    pub manifest: PathBuf,
}

/// Game data and rooms.
#[derive(Debug, Clone, Deserialize)]
pub struct Game {
    /// KRP's classes, weapons, modes and cosmetics (`data/krp`).
    pub krp_data: PathBuf,
    /// Balance presets (`data/balance`).
    pub balance_dir: PathBuf,
    /// The preset applied over KRP's numbers; `best` by default.
    pub balance: String,
    /// Players per room, unless a room sets its own `max_players`. The
    /// custom server form can lower a room's limit but not raise it.
    #[serde(default = "default_max_players")]
    pub max_players: usize,
    /// The rooms opened at start, in the room list's order.
    pub rooms: Vec<crate::game::RoomSpec>,
    /// `all` opens every room in `rooms`; `single` opens only one of them
    /// (`single_room`, or the first). More can be opened later from the
    /// admin panel either way.
    #[serde(default)]
    pub launch: Launch,
    /// The room `launch = "single"` opens; empty for the first.
    #[serde(default)]
    pub single_room: String,
    /// The version string clients show in their menu; empty for the
    /// server's own (`RECON <version>`).
    #[serde(default)]
    pub version: String,
}

/// Which of the configured rooms open at start.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Launch {
    /// Every room in the list, one per mode as in KRP's dev server.
    #[default]
    All,
    /// One room only.
    Single,
}

const fn default_max_players() -> usize {
    8
}

impl Game {
    /// The rooms to open at start, after `launch`, with `max_players`
    /// filled in from the server default.
    ///
    /// # Errors
    /// Fails if `single_room` names no room in the list, or the list is
    /// empty.
    pub fn room_specs(&self) -> Result<Vec<crate::game::RoomSpec>, String> {
        let chosen: Vec<&crate::game::RoomSpec> = match self.launch {
            Launch::All => self.rooms.iter().collect(),
            Launch::Single if self.single_room.is_empty() => self.rooms.iter().take(1).collect(),
            Launch::Single => {
                let r = self
                    .rooms
                    .iter()
                    .find(|r| r.name.eq_ignore_ascii_case(&self.single_room))
                    .ok_or_else(|| {
                        format!(
                            "single room {} is not in [game] rooms ({})",
                            self.single_room,
                            self.rooms
                                .iter()
                                .map(|r| r.name.as_str())
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                    })?;
                vec![r]
            }
        };
        if chosen.is_empty() {
            return Err("[game] rooms is empty".into());
        }
        Ok(chosen
            .into_iter()
            .cloned()
            .map(|mut r| {
                r.max_players = r.max_players.or(Some(self.max_players));
                r
            })
            .collect())
    }
}

/// Where maps come from.
#[derive(Debug, Clone, Deserialize)]
pub struct Maps {
    /// Sources loaded in order; a later source replaces maps with the same
    /// id. `archive`: every `map-<id>.genData.json` in `archive_dir`.
    /// `files`: the text maps listed in `files`.
    pub sources: Vec<MapSourceKind>,
    #[serde(default = "default_archive_dir")]
    pub archive_dir: String,
    #[serde(default)]
    pub files: Vec<PathBuf>,
}

fn default_archive_dir() -> String {
    crate::game::maps::ARCHIVE_MAP_DIR.to_owned()
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
        assert_eq!(c.engine_io.ping_timeout_ms, 20_000);
        assert!(c.trace_brief.iter().any(|e| e == "rsd"));
        assert_eq!(c.maps.sources, [MapSourceKind::Archive]);
        assert!(c.rules.len() >= 2);
        assert_eq!(c.game.balance, "best");
        assert_eq!(c.game.rooms.len(), 9);
        assert_eq!(c.game.max_players, 8);
        let all = c.game.room_specs().unwrap();
        assert_eq!(all.len(), 9);
        assert!(all.iter().all(|r| r.max_players == Some(8)));
        let mut one = c.game.clone();
        one.launch = Launch::Single;
        assert_eq!(one.room_specs().unwrap().len(), 1);
        one.single_room = "dev2".into();
        assert_eq!(one.room_specs().unwrap()[0].name, "DEV2");
        one.single_room = "NOPE".into();
        assert!(one.room_specs().is_err());
        assert_eq!(c.classic.port, 8081);
        assert!(c.classic.manifest.is_file());
        assert!(c.admin.enabled && c.admin.stdin);
        assert_eq!(c.admin.port, 8082);
        assert!(c.admin.token.is_empty());
        let ip: std::net::IpAddr = c.admin.bind.parse().unwrap();
        assert!(ip.is_loopback(), "the admin port must default to loopback");
    }
}
