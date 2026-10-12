//! Game rules shared by the server and the client.
//!
//! These modules port the parts of `KrunkerRevival`'s (KRP) code that both
//! sides run: the server simulates with them, and the client uses the same
//! code to predict its own movement and shots, so the two always agree.
//! Nothing here does I/O beyond reading data files, so it also builds for
//! WebAssembly.

pub mod data;
pub mod map;
pub mod projectile;
