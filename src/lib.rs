//! `vertix_reconstruction` — compatibility server for the original
//! 2016 Vertix.io browser client.
//!
//! The server speaks the wire protocol the archived client expects
//! (Engine.IO 3 / Socket.IO 1.x) and reconstructs the game logic the
//! original server held. Original client files and assets are never
//! part of this crate; they are loaded from a local archive at run
//! time and verified by hash.

/// Bounded new-code framing for the 2016 Engine.IO 3 / Socket.IO 1.x client.
pub mod transport;

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {}
}
