//! `vertix_reconstruction`: a Rust reconstruction of Vertix.io's game
//! mechanics, played in the browser.
//!
//! The server ports the game rules of `KrunkerRevival` (KRP,
//! `KrunkerRevivalProject/vertix`), with recovered numbers from the research
//! applied over them, and speaks the protocol KRP's browser client uses
//! (Engine.IO 4 / Socket.IO 5, one namespace per room). The client build is
//! loaded from a local directory and never committed.
//!
//! Layers, bottom up: [`eio`] (Engine.IO polling and WebSocket) and [`sio`]
//! (Socket.IO events) form the transport; [`game`] owns all game state and
//! talks to clients only through [`eio::ClientHandle`]; [`originals`] reads
//! verified files (maps, the 2016 client) from an archive clone; [`http`]
//! wires them together. [`classic`] is the compatibility path for the
//! archived 2016 client: its own transport, and event translation into the
//! same rooms. [`admin`] is the admin panel and dev console: one command
//! language, run inside the game task, from a browser or the terminal.

// The player object `json!` builds is deep.
#![recursion_limit = "256"]

pub mod admin;
pub mod classic;
pub mod config;
pub mod eio;
pub mod game;
pub mod http;
pub mod originals;
pub mod ports;
pub mod sio;
pub mod trace;
pub mod version;
