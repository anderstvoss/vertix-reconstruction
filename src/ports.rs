//! Listening ports: the configured ones are preferences.
//!
//! When a port is taken (say, a second server on the same machine), the
//! server tries the next ports up, then lets the system pick one, unless
//! `strict_ports` (or `--strict-port`) asks for exactly the configured
//! ports. Listeners never share a port. Every address actually bound is
//! printed and written to a small JSON file (`ports_file`) that scripts
//! read instead of assuming the defaults.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;

use tokio::net::TcpListener;

/// Ports tried after the preferred one, before the system picks.
pub const TRIES: u16 = 20;

/// Binds listeners, keeping track of the ports in use.
#[derive(Debug)]
pub struct Binder {
    strict: bool,
    taken: Vec<u16>,
    /// Name to address, for the ports file.
    pub bound: BTreeMap<String, SocketAddr>,
}

impl Binder {
    #[must_use]
    pub fn new(strict: bool) -> Self {
        Self {
            strict,
            taken: Vec::new(),
            bound: BTreeMap::new(),
        }
    }

    /// Binds `name`'s listener on `host`, port `preferred`, the next free
    /// port above it, or one the system picks.
    ///
    /// # Errors
    /// Fails if nothing can be bound, or the preferred port is taken and
    /// ports are strict.
    pub async fn bind(
        &mut self,
        host: &str,
        name: &str,
        preferred: u16,
    ) -> Result<TcpListener, String> {
        let mut last = String::new();
        let candidates: Vec<u16> = if self.strict {
            vec![preferred]
        } else {
            (0..=TRIES)
                .filter_map(|i| preferred.checked_add(i))
                .filter(|&p| p != 0)
                .chain(std::iter::once(0))
                .collect()
        };
        for port in candidates {
            if port != 0 && self.taken.contains(&port) {
                continue;
            }
            match TcpListener::bind((host, port)).await {
                Ok(l) => {
                    let addr = l.local_addr().map_err(|e| e.to_string())?;
                    if self.taken.contains(&addr.port()) {
                        continue;
                    }
                    if addr.port() != preferred {
                        eprintln!("{name}: port {preferred} is taken, using {}", addr.port());
                    }
                    self.taken.push(addr.port());
                    self.bound.insert(name.to_owned(), addr);
                    return Ok(l);
                }
                Err(e) => last = format!("{host}:{port}: {e}"),
            }
        }
        Err(if self.strict {
            format!("{name}: bind {last} (strict ports: pick another port, or drop strict_ports)")
        } else {
            format!("{name}: bind {last}")
        })
    }

    /// Writes the bound addresses as `{"name": "http://addr/", ...}`.
    ///
    /// # Errors
    /// Fails if the file or its directory cannot be written.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let doc: BTreeMap<&str, serde_json::Value> = self
            .bound
            .iter()
            .map(|(k, a)| (k.as_str(), serde_json::json!(format!("http://{a}/"))))
            .chain(std::iter::once((
                "pid",
                serde_json::json!(std::process::id()),
            )))
            .collect();
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The loopback address, from the system rather than a literal.
    fn host() -> String {
        std::net::Ipv4Addr::LOCALHOST.to_string()
    }

    #[tokio::test]
    async fn taken_ports_move_up_and_never_collide() {
        let h = host();
        let mut b = Binder::new(false);
        let first = b.bind(&h, "a", 0).await.unwrap();
        let port = first.local_addr().unwrap().port();
        // Another listener asks for the same port: it gets a different one.
        let second = b.bind(&h, "b", port).await.unwrap();
        assert_ne!(second.local_addr().unwrap().port(), port);
        // Strict ports refuse instead.
        let mut s = Binder::new(true);
        assert!(s.bind(&h, "c", port).await.unwrap_err().contains("strict"));
        assert_eq!(b.bound.len(), 2);
    }

    #[tokio::test]
    async fn the_ports_file_names_each_listener() {
        let mut b = Binder::new(false);
        let _l = b.bind(&host(), "krp", 0).await.unwrap();
        let path = std::env::temp_dir()
            .join(format!("vertix-ports-{}", std::process::id()))
            .join("server.json");
        b.write(&path).unwrap();
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(doc["krp"].as_str().unwrap().starts_with("http://"));
        assert_eq!(doc["pid"], serde_json::json!(std::process::id()));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
