//! A desktop window for KRP's browser client.
//!
//! The window loads KRP's client from a reconstruction server, the same
//! page a browser gets, and runs `inject.js` in it before the client's own
//! scripts. Nothing of KRP's is built into this program: the client comes
//! from the server at run time, and `inject.js` only wraps browser APIs the
//! client calls (see that file).
//!
//! Options (`--key value`; flags take no value):
//!
//! - `--server URL`   reconstruction server, default this machine on 8080
//!   (or `VERTIX_SERVER`)
//! - `--room NAME`    room to join, as KRP's `/?ROOM` address does
//! - `--name NAME`    player name
//! - `--input frame|HZ`, `--display sharp|krp`, `--effects persist|krp`,
//!   `--autoplay`, `--script KEYS` as in `inject.js`
//! - `--duration S`   quit after S seconds and report metrics
//! - `--metrics PATH` where to write the metrics JSON (stdout otherwise)

use std::net::Ipv4Addr;

use serde_json::{Map, Value, json};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

const INJECT: &str = include_str!("../inject.js");

/// Options that take no value.
const FLAGS: &[&str] = &["autoplay"];

fn parse_args(args: impl IntoIterator<Item = String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut it = args.into_iter().peekable();
    while let Some(arg) = it.next() {
        let Some(key) = arg.strip_prefix("--") else {
            continue;
        };
        if let Some((k, v)) = key.split_once('=') {
            out.push((k.to_owned(), v.to_owned()));
        } else if FLAGS.contains(&key) {
            out.push((key.to_owned(), "1".to_owned()));
        } else {
            let value = it.next_if(|v| !v.starts_with("--")).unwrap_or_default();
            out.push((key.to_owned(), value));
        }
    }
    out
}

/// The page to open: the server's KRP client, in the given room.
fn page_url(server: &str, room: Option<&str>) -> String {
    let base = server.trim_end_matches('/');
    match room {
        Some(r) if !r.is_empty() => format!("{base}/?{r}"),
        _ => format!("{base}/"),
    }
}

/// `window.__vertixShell` for `inject.js`, then the script itself.
fn init_script(options: &[(String, String)]) -> String {
    let mut shell = Map::new();
    for (k, v) in options {
        if matches!(k.as_str(), "server" | "room" | "metrics") {
            continue;
        }
        let value = if FLAGS.contains(&k.as_str()) {
            Value::Bool(true)
        } else {
            Value::String(v.clone())
        };
        shell.insert(k.clone(), value);
    }
    shell.insert("label".into(), json!("desktop"));
    format!("window.__vertixShell = {};\n{INJECT}", Value::Object(shell))
}

enum UserEvent {
    Report(String),
}

fn main() -> wry::Result<()> {
    let options = parse_args(std::env::args().skip(1));
    let get = |k: &str| {
        options
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
    };
    let server = get("server")
        .or_else(|| std::env::var("VERTIX_SERVER").ok())
        .unwrap_or_else(|| format!("http://{}:8080", Ipv4Addr::LOCALHOST));
    let url = page_url(&server, get("room").as_deref());
    let metrics_path = get("metrics");

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title("Vertix (KRP client)")
        .with_inner_size(LogicalSize::new(1280.0, 720.0))
        .build(&event_loop)
        .expect("could not open a window");

    let builder = WebViewBuilder::new()
        .with_url(&url)
        .with_initialization_script(init_script(&options))
        .with_ipc_handler(move |req| {
            let _ = proxy.send_event(UserEvent::Report(req.body().clone()));
        });
    #[cfg(not(target_os = "linux"))]
    let _webview = builder.build(&window)?;
    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().expect("tao always makes a vbox"))?
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            Event::UserEvent(UserEvent::Report(body)) => {
                let metrics = serde_json::from_str::<Value>(&body)
                    .ok()
                    .and_then(|v| v.get("metrics").cloned())
                    .unwrap_or(Value::Null);
                let text = metrics.to_string();
                match &metrics_path {
                    Some(path) if !path.is_empty() => {
                        if let Err(e) = std::fs::write(path, text) {
                            eprintln!("could not write {path}: {e}");
                        }
                    }
                    _ => println!("{text}"),
                }
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<(String, String)> {
        parse_args(s.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn options_parse_with_flags_and_values() {
        assert_eq!(
            args("--room DEV0 --autoplay --input 60 --display=krp"),
            vec![
                ("room".into(), "DEV0".into()),
                ("autoplay".into(), "1".into()),
                ("input".into(), "60".into()),
                ("display".into(), "krp".into()),
            ]
        );
    }

    #[test]
    fn page_joins_the_room_like_krp_addresses_do() {
        assert_eq!(
            page_url("http://example:8080/", Some("DEV1")),
            "http://example:8080/?DEV1"
        );
        assert_eq!(
            page_url("http://example:8080", None),
            "http://example:8080/"
        );
    }

    #[test]
    fn init_script_passes_options_but_not_shell_settings() {
        let s = init_script(&args(
            "--server http://example --room DEV0 --autoplay --input 60",
        ));
        let first = s.lines().next().unwrap();
        assert!(first.contains(r#""autoplay":true"#), "{first}");
        assert!(first.contains(r#""input":"60""#), "{first}");
        assert!(first.contains(r#""label":"desktop""#), "{first}");
        assert!(
            !first.contains("server") && !first.contains("DEV0"),
            "{first}"
        );
        assert!(s.contains("vertixMetrics"));
    }
}
