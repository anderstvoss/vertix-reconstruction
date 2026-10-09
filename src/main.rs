//! `vertix-server`: the Rust reconstruction of the game's server, serving
//! a `KrunkerRevival` client build.
//!
//! ```text
//! vertix-server [--config config/server.toml] [--archive PATH] [--trace FILE] [--port N]
//! vertix-server --explain-rules [--config FILE]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use tokio::sync::mpsc;
use vertix_reconstruction::config::{Config, MapSourceKind};
use vertix_reconstruction::game::Game;
use vertix_reconstruction::game::assumptions::Assumptions;
use vertix_reconstruction::game::data::GameData;
use vertix_reconstruction::game::maps::{ArchiveGenData, MapSet, MapSource, TextFiles};
use vertix_reconstruction::trace::Trace;
use vertix_reconstruction::{eio, http};

struct Args {
    config: PathBuf,
    archive: Option<String>,
    trace: Option<String>,
    port: Option<u16>,
    explain_rules: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        config: PathBuf::from("config/server.toml"),
        archive: None,
        trace: None,
        port: None,
        explain_rules: false,
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
            "--explain-rules" => args.explain_rules = true,
            "-h" | "--help" => {
                return Err("usage: vertix-server [--config FILE] [--archive PATH] \
                     [--trace FILE] [--port N] [--explain-rules]"
                    .into());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

async fn run() -> Result<(), String> {
    let args = parse_args()?;
    let config = Config::load(&args.config)?;
    let (rules, provenance) = Assumptions::load_layers(&config.rules).map_err(|e| e.to_string())?;
    let data = GameData::load(
        &config.game.krp_data,
        &config.game.balance_dir,
        &config.game.balance,
    )
    .map_err(|e| format!("game data: {e}"))?;
    if args.explain_rules {
        print!("{}", provenance.table());
        println!("{}", provenance.summary());
        print!("{}", data.explain());
        return Ok(());
    }
    eprintln!(
        "rules: {} (sha256 {})",
        provenance.summary(),
        provenance.hash
    );
    eprintln!(
        "game data: KRP {} with balance preset {} ({} values)",
        data.krp_commit,
        data.balance.id,
        data.balance.values.len()
    );
    let archive = config.archive_path(args.archive.as_deref());
    let mut loaded = Vec::new();
    for kind in &config.maps.sources {
        let source: Box<dyn MapSource> = match kind {
            MapSourceKind::Archive => Box::new(ArchiveGenData {
                root: archive.clone().ok_or(
                    "the archive map source needs an archive: pass --archive, set \
                     VERTIX_ARCHIVE, or set `archive` in the config",
                )?,
                dir: config.maps.archive_dir.clone(),
            }),
            MapSourceKind::Files => Box::new(TextFiles {
                files: config.maps.files.clone(),
            }),
        };
        loaded.extend(source.load().map_err(|e| format!("maps ({kind:?}): {e}"))?);
    }
    let maps = MapSet::new(loaded).map_err(|e| format!("maps: {e}"))?;
    eprintln!("maps: {} loaded: {}", maps.len(), maps.ids().join(" "));
    if !config.client_dir.join("index.html").is_file() {
        eprintln!(
            "warning: no client build in {}; run scripts/build-client.sh",
            config.client_dir.display()
        );
    }

    let trace_path = args
        .trace
        .or_else(|| (!config.trace.is_empty()).then(|| config.trace.clone()));
    let trace = match trace_path {
        Some(p) => Trace::to_file(p.as_ref(), &config.trace_brief)
            .map_err(|e| format!("trace {p}: {e}"))?,
        None => Trace::disabled(),
    };

    let (tx, rx) = mpsc::unbounded_channel();
    let (ask_tx, ask_rx) = mpsc::unbounded_channel();
    let timing = config.engine_io.timing();
    let eio = eio::Server::new(timing, tx, trace.clone());
    let game = Game::new(rules, data, maps, &config.game.rooms, trace.clone())?;
    tokio::spawn(game.run(rx, ask_rx));
    let beat = eio.clone();
    tokio::spawn(async move {
        let mut every = tokio::time::interval(timing.ping_interval);
        every.tick().await;
        loop {
            every.tick().await;
            beat.heartbeat();
        }
    });

    let app = http::router(http::AppState {
        client_dir: Arc::new(config.client_dir.clone()),
        eio,
        game: ask_tx,
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
