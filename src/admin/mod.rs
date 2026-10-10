//! The server admin panel and dev console.
//!
//! One command language drives everything: the browser panel on the admin
//! port, its console, `POST /api/cmd`, and the server's standard input.
//! Every command is a line of words (quotes group words), optionally with
//! `@ROOM` to pick the room; [`crate::game::Game::admin`] runs it inside the
//! game task, so commands see and change the same state the players do.
//!
//! [`COMMANDS`] lists every command with its usage; `help` prints it and
//! the panel builds its command hints from it. docs/ADMIN.md has the same
//! list for reading.

pub mod http;
pub mod stdin;

use std::fmt::Write as _;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::{broadcast, oneshot};

/// One command line from an admin session.
#[derive(Debug)]
pub struct Request {
    pub line: String,
    /// The session's selected room (`use`); the first room if `None`.
    pub room: Option<String>,
    pub reply: oneshot::Sender<Reply>,
}

/// The answer to one command.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Reply {
    pub ok: bool,
    /// Human-readable result, for the console.
    pub text: String,
    /// Structured result, for the panel (`null` for most commands).
    pub data: Value,
    /// Set by `use`: the room the session now works in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
}

impl Reply {
    #[must_use]
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            text: text.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn err(text: impl Into<String>) -> Self {
        Self {
            ok: false,
            text: text.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }
}

/// One line of the admin log the panel and console show live.
#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    /// Milliseconds since the server started.
    pub t: u64,
    pub room: String,
    /// `join`, `leave`, `chat`, `feed`, `round`, `admin`.
    pub kind: &'static str,
    pub text: String,
}

/// Where admin log lines go; cloning it is cheap.
pub type Log = broadcast::Sender<LogLine>;

/// A new log channel. Slow readers lose old lines rather than slowing the
/// game.
#[must_use]
pub fn log_channel() -> Log {
    broadcast::channel(512).0
}

/// A command's name, arguments and one-line description.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CommandInfo {
    pub name: &'static str,
    pub args: &'static str,
    pub group: &'static str,
    pub help: &'static str,
}

const fn cmd(
    group: &'static str,
    name: &'static str,
    args: &'static str,
    help: &'static str,
) -> CommandInfo {
    CommandInfo {
        name,
        args,
        group,
        help,
    }
}

/// Every command. A player is an index (`3` or `#3`), a name, or a unique
/// name prefix; a team is `red` or `blue`.
pub const COMMANDS: &[CommandInfo] = &[
    cmd("info", "help", "[command]", "List commands, or show one"),
    cmd(
        "info",
        "status",
        "",
        "Every room's mode, map, round and players",
    ),
    cmd(
        "info",
        "state",
        "",
        "Full server state as JSON (what the panel shows)",
    ),
    cmd(
        "info",
        "catalog",
        "",
        "Modes, maps, classes, weapons, presets as JSON",
    ),
    cmd("info", "players", "", "Players in the room"),
    cmd(
        "info",
        "list",
        "modes|maps|classes|weapons|hats|shirts|camos|sprays|presets|versions",
        "List game data",
    ),
    cmd(
        "info",
        "use",
        "<room>",
        "Work in this room from now on (this session)",
    ),
    cmd(
        "round",
        "mode",
        "<mode> [map]",
        "Start a new round in a mode (code, name or index)",
    ),
    cmd(
        "round",
        "map",
        "<map id>",
        "Start a new round on a map, same mode",
    ),
    cmd(
        "round",
        "restart",
        "",
        "Start a new round now, same mode, random map",
    ),
    cmd(
        "round",
        "win",
        "<team|player|none>",
        "End the round with this winner",
    ),
    cmd(
        "round",
        "lose",
        "<team|player>",
        "End the round with this team or player losing",
    ),
    cmd(
        "round",
        "score",
        "<team|player> <points>",
        "Set a score (can end the round)",
    ),
    cmd(
        "round",
        "addscore",
        "<team|player> <points>",
        "Add to a score (negative subtracts)",
    ),
    cmd(
        "round",
        "scorelimit",
        "<points|default>",
        "Score limit for this room",
    ),
    cmd(
        "round",
        "pause",
        "",
        "Stop the room's server tick (bullets, timers, objectives)",
    ),
    cmd("round", "resume", "", "Restart the room's server tick"),
    cmd(
        "players",
        "kick",
        "<player> [reason]",
        "Disconnect a player with a message",
    ),
    cmd(
        "players",
        "kickall",
        "[reason]",
        "Disconnect everyone in the room",
    ),
    cmd(
        "players",
        "kill",
        "<player>",
        "Slay a player (no score for anyone)",
    ),
    cmd(
        "players",
        "killall",
        "",
        "Slay every living player in the room",
    ),
    cmd(
        "players",
        "health",
        "<player> <hp>",
        "Set a living player's health",
    ),
    cmd(
        "players",
        "protect",
        "<player> on|off",
        "Spawn protection until their next spawn",
    ),
    cmd(
        "players",
        "team",
        "<player> <team>",
        "Move a player to a team (slays them)",
    ),
    cmd(
        "players",
        "rename",
        "<player> <name>",
        "Rename (others see it from next spawn)",
    ),
    cmd(
        "players",
        "tp",
        "<player> <x> <y>",
        "Teleport (Zone War's teleport event)",
    ),
    cmd(
        "room",
        "maxplayers",
        "<n>",
        "Players the room takes (up to 64)",
    ),
    cmd(
        "room",
        "healthmult",
        "<x>",
        "Health multiplier, from next spawn",
    ),
    cmd(
        "room",
        "speedmult",
        "<x>",
        "Speed multiplier, from next spawn",
    ),
    cmd(
        "room",
        "pickups",
        "",
        "Make every health pack and loot crate available",
    ),
    cmd("room", "say", "<text>", "Chat line from the server"),
    cmd(
        "room",
        "announce",
        "<title> [text]",
        "Big centre text for every living player",
    ),
    cmd("room", "sync", "", "Resend everyone's positions"),
    cmd("server", "rooms", "", "List rooms"),
    cmd("server", "open", "<room> <mode>", "Open a new room"),
    cmd(
        "server",
        "close",
        "<room>",
        "Close a room (kicks its players)",
    ),
    cmd(
        "server",
        "classic",
        "<room>",
        "Room the 2016 client's players join",
    ),
    cmd(
        "server",
        "rule",
        "[name] [value]",
        "List, show or change a rule constant (all rooms)",
    ),
    cmd(
        "server",
        "reload",
        "rules",
        "Reload the rule layers from disk",
    ),
    cmd(
        "server",
        "balance",
        "[preset]",
        "Show or switch the balance preset (from next spawn)",
    ),
    cmd(
        "server",
        "version",
        "[set <text> | use <version> [label] | reset]",
        "Version string clients show; `use` also loads that version's balance",
    ),
    cmd(
        "server",
        "tune",
        "[list | reset | show <class|weapon> <name> | <class|weapon> <name> <field> <value>]",
        "Set any class or weapon value over the balance preset",
    ),
    cmd(
        "dev",
        "emit",
        "<player|all> <event> [json args]",
        "Send any event to clients",
    ),
    cmd(
        "dev",
        "inject",
        "<player> <event> [json args]",
        "Handle an event as if a player sent it",
    ),
];

/// Splits a command line into words; `"..."` or `'...'` keeps spaces, and
/// a JSON array or object (`[...]`, `{...}`) stays one word.
#[must_use]
pub fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut word = String::new();
        if c == '"' || c == '\'' {
            chars.next();
            for ch in chars.by_ref() {
                if ch == c {
                    break;
                }
                word.push(ch);
            }
        } else if c == '[' || c == '{' {
            // The rest of the line is the JSON value.
            word.extend(chars.by_ref());
            let trimmed = word.trim_end().len();
            word.truncate(trimmed);
        } else {
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                word.push(ch);
                chars.next();
            }
        }
        words.push(word);
    }
    words
}

/// `help` text: every command, or one.
#[must_use]
pub fn help(name: Option<&str>) -> Reply {
    if let Some(n) = name {
        return COMMANDS.iter().find(|c| c.name == n).map_or_else(
            || Reply::err(format!("no command {n:?}; `help` lists them")),
            |c| Reply::ok(format!("{} {}\n  {}", c.name, c.args, c.help)),
        );
    }
    let mut out = String::from(
        "Commands (prefix @ROOM to pick a room; a player is an index, a name or a name prefix):\n",
    );
    let mut group = "";
    for c in COMMANDS {
        if c.group != group {
            group = c.group;
            let _ = writeln!(out, "[{group}]");
        }
        let usage = format!("{} {}", c.name, c.args);
        let _ = writeln!(out, "  {usage:<44} {}", c.help);
    }
    Reply::ok(out.trim_end()).with_data(serde_json::to_value(COMMANDS).unwrap_or(Value::Null))
}

/// Compares two tokens without stopping at the first difference.
#[must_use]
pub fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// A fresh random token: 32 hex digits.
#[must_use]
pub fn random_token() -> String {
    use rand::Rng;
    let bytes: [u8; 16] = rand::rng().random();
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_keep_quotes_and_json() {
        assert_eq!(
            split_words(r#"  kick "Big Bob"  too  rude "#),
            ["kick", "Big Bob", "too", "rude"]
        );
        assert_eq!(
            split_words(r#"emit all 6 ["Hi", "there", 1.25]"#),
            ["emit", "all", "6", r#"["Hi", "there", 1.25]"#]
        );
        assert_eq!(split_words("say 'it''s'"), ["say", "it", "s"]);
        assert!(split_words("   ").is_empty());
    }

    #[test]
    fn every_command_has_help_and_unique_name() {
        for (i, c) in COMMANDS.iter().enumerate() {
            assert!(!c.help.is_empty(), "{}", c.name);
            assert!(
                COMMANDS[i + 1..].iter().all(|d| d.name != c.name),
                "{} twice",
                c.name
            );
        }
        assert!(help(None).text.contains("killall"));
        assert!(help(Some("kick")).ok);
        assert!(!help(Some("nope")).ok);
    }

    #[test]
    fn docs_list_every_command() {
        let doc = include_str!("../../docs/ADMIN.md");
        for c in COMMANDS {
            assert!(
                doc.contains(&format!("`{}", c.name)),
                "docs/ADMIN.md lacks {}",
                c.name
            );
        }
    }

    #[test]
    fn tokens_compare() {
        let t = random_token();
        assert_eq!(t.len(), 32);
        assert!(same_token(&t, &t.clone()));
        assert!(!same_token(&t, &random_token()));
        assert!(!same_token("ab", "abc"));
    }
}
