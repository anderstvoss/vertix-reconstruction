//! `vertix_reconstruction` — compatibility server for the original
//! 2016 Vertix.io browser client.
//!
//! The server speaks the wire protocol the archived client expects
//! (Engine.IO 3 / Socket.IO 1.x) and reconstructs the game logic the
//! original server held. Original client files and assets are never
//! part of this crate; they are loaded from a local archive at run
//! time and verified by hash.

/// Deterministic, explicitly NEW authoritative FFA simulation core.
pub mod simulation;

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {}
}
