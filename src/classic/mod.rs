//! The compatibility shim for the archived 2016-08-06 client.
//!
//! The game is KRP's (see [`crate::game`]); this lets the original 2016
//! client play in the same rooms. It keeps the 2016 client's own transport
//! (Engine.IO 3 long-polling, Socket.IO 1.x on the default namespace),
//! serves its page and files from the archive on a port of its own, and
//! [`adapt`] translates the few events whose shape changed between the 2016
//! client and KRP's.

pub mod adapt;
pub mod codec3;
pub mod eio3;
pub mod http;
pub mod sio1;
