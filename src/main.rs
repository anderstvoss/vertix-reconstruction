//! `vertix-server`: serves the archived 2016-08-06 client and runs the
//! reconstructed game server behind it.
//!
//! ```text
//! vertix-server [--config config/server.toml] [--archive PATH] [--trace FILE] [--port N]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use vertix_reconstruction::config::Config;
use vertix_reconstruction::game::Game;
use vertix_reconstruction::game::assumptions::Assumptions;
use vertix_reconstruction::game::map::Map;
use vertix_reconstruction::originals::{BootManifest, Store};
use vertix_reconstruction::trace::Trace;
use vertix_reconstruction::{eio, http};

struct Args {
    config: PathBuf,
    archive: Option<String>,
    trace: Option<String>,
    port: Option<u16>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        config: PathBuf::from("config/server.toml"),
        archive: None,
        trace: None,
        port: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--config" => args.config = PathBuf::from(value()?),
            "--archive" => args.archive = Some(value()?),
            "--trace" => args.trace = Some(value()?),
            "--port" => {
                args.port = Some(value()?.parse().map_err(|_| "--port needs a number")?);
            }
            "-h" | "--help" => {
                return Err(
                    "usage: vertix-server [--config FILE] [--archive PATH] [--trace FILE] [--port N]"
                        .into(),
                );
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

async fn run() -> Result<(), String> {
    let args = parse_args()?;
    let config = Config::load(&args.config)?;
    let archive = config
        .archive_path(args.archive.as_deref())
        .ok_or("no archive: pass --archive, set VERTIX_ARCHIVE, or set `archive` in the config")?;
    let manifest = BootManifest::load(&config.manifest).map_err(|e| e.to_string())?;
    let store = Store::load(&archive, &manifest).map_err(|e| format!("archive: {e}"))?;
    eprintln!(
        "loaded build {} ({} files, page shell from capture {}), all hashes verified",
        manifest.build,
        store.len(),
        manifest.page_shell.capture
    );
    let rules = Assumptions::load(&config.assumptions).map_err(|e| e.to_string())?;
    let map_text = std::fs::read_to_string(&config.map)
        .map_err(|e| format!("{}: {e}", config.map.display()))?;
    let map = Map::parse(&map_text, rules.world.tile_scale, false).map_err(|e| e.to_string())?;

    let trace_path = args
        .trace
        .or_else(|| (!config.trace.is_empty()).then(|| config.trace.clone()));
    let trace = match trace_path {
        Some(p) => Trace::to_file(p.as_ref(), &config.trace_brief)
            .map_err(|e| format!("trace {p}: {e}"))?,
        None => Trace::disabled(),
    };

    let (tx, rx) = mpsc::unbounded_channel();
    let eio = eio::Server::new(config.engine_io.timing(), tx, trace.clone());
    tokio::spawn(Game::new(rules, map, trace.clone()).run(rx));
    let reaper = eio.clone();
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(5));
        loop {
            every.tick().await;
            reaper.reap();
        }
    });

    let app = http::router(http::AppState {
        store: Arc::new(store),
        eio,
        trace,
    });
    let port = args.port.unwrap_or(config.port);
    let listener = tokio::net::TcpListener::bind((config.bind.as_str(), port))
        .await
        .map_err(|e| format!("bind {}:{port}: {e}", config.bind))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    eprintln!("listening on http://{addr}/");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vertix-server: {e}");
            ExitCode::FAILURE
        }
    }
}
