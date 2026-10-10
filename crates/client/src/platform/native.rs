#![forbid(unsafe_code)]

//! Desktop build: a WebSocket thread per connection and a thread per
//! download, both reporting over channels.

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use tungstenite::Message;

use super::WsEvent;

/// Launch options from the command line: `--key value` or `--flag`.
#[must_use]
pub fn launch_options() -> Vec<(String, String)> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(key) = args[i].strip_prefix("--") {
            if let Some((k, v)) = key.split_once('=') {
                out.push((k.to_owned(), v.to_owned()));
            } else if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                out.push((key.to_owned(), args[i + 1].clone()));
                i += 1;
            } else {
                out.push((key.to_owned(), String::new()));
            }
        }
        i += 1;
    }
    out
}

/// The server to play on: `--server`, else `VERTIX_SERVER`, else the
/// default local server.
#[must_use]
pub fn server_base(options: &[(String, String)]) -> String {
    options
        .iter()
        .find(|(k, _)| k == "server")
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("VERTIX_SERVER").ok())
        .unwrap_or_else(|| format!("http://{}:8080", std::net::Ipv4Addr::LOCALHOST))
        .trim_end_matches('/')
        .to_owned()
}

/// Writes measurements to `--metrics <path>`, or prints them.
pub fn report(json: &str, options: &[(String, String)]) {
    match options.iter().find(|(k, _)| k == "metrics") {
        Some((_, path)) if !path.is_empty() => {
            if let Err(e) = std::fs::write(path, json) {
                eprintln!("could not write {path}: {e}");
            }
        }
        _ => println!("{json}"),
    }
}

/// A WebSocket served by a background thread.
pub struct Socket {
    out: Sender<Option<String>>,
    events: Receiver<WsEvent>,
}

impl Socket {
    /// Starts connecting to `url` (`ws://host:port/path`).
    #[must_use]
    pub fn connect(url: &str) -> Self {
        let (out, out_rx) = channel::<Option<String>>();
        let (ev_tx, events) = channel();
        let url = url.to_owned();
        std::thread::spawn(move || {
            let reason = run_socket(&url, &out_rx, &ev_tx).err().unwrap_or_default();
            let _ = ev_tx.send(WsEvent::Closed(reason));
        });
        Self { out, events }
    }

    pub fn send(&self, text: &str) {
        let _ = self.out.send(Some(text.to_owned()));
    }

    pub fn close(&self) {
        let _ = self.out.send(None);
    }

    /// Everything that happened since the last call.
    pub fn poll(&mut self) -> Vec<WsEvent> {
        let mut out = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(e) => out.push(e),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !matches!(out.last(), Some(WsEvent::Closed(_))) {
                        out.push(WsEvent::Closed(String::from("connection thread ended")));
                    }
                    break;
                }
            }
        }
        out
    }
}

fn host_port(url: &str) -> Result<(String, String), String> {
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .ok_or_else(|| format!("bad URL {url}"))?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let addr = if authority.contains(':') {
        authority.to_owned()
    } else {
        format!("{authority}:80")
    };
    Ok((addr, format!("/{path}")))
}

fn run_socket(
    url: &str,
    out: &Receiver<Option<String>>,
    events: &Sender<WsEvent>,
) -> Result<(), String> {
    let (addr, _) = host_port(url)?;
    let stream = TcpStream::connect(&addr).map_err(|e| format!("{addr}: {e}"))?;
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    let (mut ws, _) = tungstenite::client(url, stream).map_err(|e| e.to_string())?;
    ws.get_mut()
        .set_nonblocking(true)
        .map_err(|e| e.to_string())?;
    let _ = events.send(WsEvent::Open);
    loop {
        let mut idle = true;
        loop {
            match out.try_recv() {
                Ok(Some(text)) => {
                    idle = false;
                    match ws.write(Message::text(text)) {
                        Ok(()) => {}
                        Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
                        Err(e) => return Err(e.to_string()),
                    }
                }
                Ok(None) | Err(TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    return Err(String::from("closed by client"));
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        match ws.flush() {
            Ok(()) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.to_string()),
        }
        loop {
            match ws.read() {
                Ok(Message::Text(t)) => {
                    idle = false;
                    let _ = events.send(WsEvent::Message(t.as_str().to_owned(), super::now_ms()));
                }
                Ok(Message::Close(_)) => return Err(String::from("closed by server")),
                Ok(_) => idle = false,
                Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.to_string()),
            }
        }
        if idle {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.close();
    }
}

/// An HTTP download running on a background thread.
pub struct Fetch {
    rx: Receiver<Result<Vec<u8>, String>>,
}

impl Fetch {
    /// Starts downloading `url` (`http://host:port/path`).
    #[must_use]
    pub fn start(url: &str) -> Self {
        let (tx, rx) = channel();
        let url = url.to_owned();
        std::thread::spawn(move || {
            let _ = tx.send(http_get(&url));
        });
        Self { rx }
    }

    /// The result, once it is there.
    pub fn poll(&mut self) -> Option<Result<Vec<u8>, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(String::from("download thread ended"))),
        }
    }
}

/// A plain HTTP/1.1 GET; the reconstruction server speaks nothing else.
fn http_get(url: &str) -> Result<Vec<u8>, String> {
    if !url.starts_with("http://") {
        return Err(format!("only http:// is supported: {url}"));
    }
    let (addr, path) = host_port(url)?;
    let mut stream = TcpStream::connect(&addr).map_err(|e| format!("{addr}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nAccept-Encoding: identity\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| String::from("malformed HTTP response"))?;
    let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    let body = &raw[split + 4..];
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("");
    if status != "200" {
        return Err(format!("{url}: HTTP {status}"));
    }
    if head.contains("transfer-encoding: chunked") {
        return dechunk(body);
    }
    Ok(body.to_vec())
}

fn dechunk(mut body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = body
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| String::from("bad chunk"))?;
        let size_text = String::from_utf8_lossy(&body[..line_end]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| String::from("bad chunk size"))?;
        body = &body[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if body.len() < size + 2 {
            return Err(String::from("truncated chunk"));
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_bodies_are_joined() {
        assert_eq!(
            dechunk(b"3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n").unwrap(),
            b"abcde"
        );
    }

    #[test]
    fn urls_split_into_address_and_path() {
        assert_eq!(
            host_port("ws://example:8080/socket.io/?EIO=4").unwrap(),
            ("example:8080".into(), "/socket.io/?EIO=4".into())
        );
        assert_eq!(host_port("http://example").unwrap().0, "example:80");
    }
}
