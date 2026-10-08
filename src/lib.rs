//! Vertix.io reconstruction compatibility core.
//!
//! Original browser binaries are external and hash-verified. Server-side
//! behavior is a documented reconstruction, not recovered server source.
//! This crate is not yet a networked server: `gameplay` is an isolated
//! authoritative simulation awaiting the Engine.IO 3 compatibility adapter.

pub mod gameplay;

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(crate::gameplay::CLASSES.len(), 9);
    }
}
