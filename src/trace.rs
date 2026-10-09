//! Optional JSON-lines trace of everything that crosses the wire.
//!
//! Each line is one record with milliseconds since start, the connection,
//! and either a Socket.IO event (`in`/`out`) or a transport note. The
//! milestone runs keep these traces as their evidence of what happened.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Value, json};

use crate::sio::Event;

struct Inner {
    out: Mutex<BufWriter<File>>,
    start: Instant,
    /// Events logged only by name and argument count, because they fire
    /// every frame or tick (input, aim, state snapshots).
    brief: HashSet<String>,
}

/// Cheap to clone; a disabled trace does nothing.
#[derive(Clone, Default)]
pub struct Trace(Option<Arc<Inner>>);

impl Trace {
    #[must_use]
    pub fn disabled() -> Self {
        Self(None)
    }

    /// Writes the trace to `path`, replacing it.
    ///
    /// # Errors
    /// Returns the I/O error if the file cannot be created.
    pub fn to_file(path: &Path, brief: &[String]) -> std::io::Result<Self> {
        let file = File::create(path)?;
        Ok(Self(Some(Arc::new(Inner {
            out: Mutex::new(BufWriter::new(file)),
            start: Instant::now(),
            brief: brief.iter().cloned().collect(),
        }))))
    }

    fn write(&self, record: &Value) {
        let Some(inner) = &self.0 else { return };
        let mut out = inner
            .out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = writeln!(out, "{record}");
        let _ = out.flush();
    }

    fn ms(&self) -> u128 {
        self.0.as_ref().map_or(0, |i| i.start.elapsed().as_millis())
    }

    /// Records a Socket.IO event in direction `dir` (`in` or `out`).
    pub fn event(&self, dir: &str, conn: u64, event: &Event) {
        let Some(inner) = &self.0 else { return };
        let record = if inner.brief.contains(&event.name) {
            json!({"t": self.ms(), "dir": dir, "conn": conn, "event": event.name, "argc": event.args.len()})
        } else {
            json!({"t": self.ms(), "dir": dir, "conn": conn, "event": event.name, "args": event.args})
        };
        self.write(&record);
    }

    /// Records a transport or game note.
    pub fn note(&self, kind: &str, conn: u64, detail: &str) {
        if self.0.is_none() {
            return;
        }
        self.write(&json!({"t": self.ms(), "note": kind, "conn": conn, "detail": detail}));
    }
}
