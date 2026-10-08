//! `vertix_reconstruction` — compatibility server for the original
//! 2016 Vertix.io browser client.
//!
//! The server speaks the wire protocol the archived client expects
//! (Engine.IO 3 / Socket.IO 1.x) and reconstructs the game logic the
//! original server held. Original client files and assets are never
//! part of this crate; they are loaded from a local archive at run
//! time and verified by hash.
//!
//! Layers, bottom up: [`eio`] (Engine.IO long-polling) and [`sio`]
//! (Socket.IO events) form the transport; [`game`] owns all game state and
//! talks to clients only through [`eio::ClientHandle`]; [`originals`]
//! loads and verifies the archived files; [`http`] wires them together.

pub mod config;
pub mod eio;
pub mod game;
pub mod http;
pub mod originals;
pub mod sio;
pub mod trace;
