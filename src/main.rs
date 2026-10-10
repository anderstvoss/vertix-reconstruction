//! `vertix-server`: the Rust reconstruction of the game's server, serving
//! a `KrunkerRevival` client build.
//!
//! ```text
//! vertix-server [--config config/server.toml] [--archive PATH] [--trace FILE] [--port N] [--strict-port]
//! vertix-server --explain-rules [--config FILE]
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use vertix_reconstruction::config::{Config, MapSourceKind};
use vertix_reconstruction::content::{self, Content};
use vertix_reconstruction::game::Game;
use vertix_reconstruction::game::assumptions::Assumptions;
use vertix_reconstruction::game::data::GameData;
use vertix_reconstruction::game::maps::{ArchiveGenData, MapSet, MapSource, TextFiles};
use vertix_reconstruction::originals::{BootManifest, Store};
use vertix_reconstruction::ports::Binder;
use vertix_reconstruction::trace::Trace;
use vertix_reconstruction::version::Version;
use vertix_reconstruction::{admin, classic, eio, http};

struct Args {
    config: PathBuf,
    archive: Option<String>,
    trace: Option<String>,
    port: Option<u16>,
    strict_port: bool,
    explain_rules: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        config: PathBuf::from("config/server.toml"),
        archive: None,
        trace: None,
        port: None,
        strict_port: false,
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
            "--strict-port" => args.strict_port = true,
            "-h" | "--help" => {
                return Err("usage: vertix-server [--config FILE] [--archive PATH] \
                     [--trace FILE] [--port N] [--strict-port] [--explain-rules]"
                    .into());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

/// Loads every configured map source, in order.
fn load_maps(config: &Config, archive: Option<&Path>) -> Result<MapSet, String> {
    let mut loaded = Vec::new();
    for kind in &config.maps.sources {
        let source: Box<dyn MapSource> = match kind {
            MapSourceKind::Archive => Box::new(ArchiveGenData {
                root: archive.map(Path::to_path_buf).ok_or(
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
    MapSet::new(loaded).map_err(|e| format!("maps: {e}"))
}

/// Cosmetics and mod packs restored from the archive, and sprays from the
/// sprays folder (added to `data`). Their routes answer before the client
/// build's fallback, so recovered and added files win.
fn restored(
    config: &Config,
    archive: Option<&Path>,
    data: &mut GameData,
) -> Result<Content, String> {
    let mut restored = Content::load(&config.content, archive)?;
    eprintln!("{}", restored.summary());
    for w in restored.add_folder_sprays(&config.content.sprays_dir, &mut data.cosmetics.sprays) {
        eprintln!("sprays: {w}");
    }
    if !restored.folder_sprays.is_empty() {
        let added = restored.folder_sprays.iter().filter(|s| s.2).count();
        eprintln!(
            "sprays: {} from {} ({added} new, {} replaced)",
            restored.folder_sprays.len(),
            config.content.sprays_dir.display(),
            restored.folder_sprays.len() - added
        );
    }
    Ok(restored)
}

/// The 2016 client's routes, if the archive that holds it is given.
fn classic_app(
    config: &Config,
    archive: Option<&Path>,
    events: mpsc::UnboundedSender<classic::eio3::TransportEvent>,
    trace: &Trace,
    version: &Version,
) -> Result<Option<axum::Router>, String> {
    let Some(root) = archive else {
        eprintln!("2016 client: off, it is served from the archive (pass --archive)");
        return Ok(None);
    };
    let manifest = BootManifest::load(&config.classic.manifest).map_err(|e| e.to_string())?;
    let store = Store::load(root, &manifest).map_err(|e| format!("archive: {e}"))?;
    eprintln!(
        "2016 client: build {} ({} files), all hashes verified",
        manifest.build,
        store.len()
    );
    let server =
        classic::eio3::Server::new(classic::eio3::Timing::default(), events, trace.clone());
    let reaper = server.clone();
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(5));
        loop {
            every.tick().await;
            reaper.reap();
        }
    });
    Ok(Some(classic::http::router(classic::http::AppState {
        store: Arc::new(store),
        eio: server,
        trace: trace.clone(),
        version: version.clone(),
    })))
}

/// The trace file from `--trace`, else the config's, else none.
fn open_trace(cli: Option<String>, config: &Config) -> Result<Trace, String> {
    let path = cli.or_else(|| (!config.trace.is_empty()).then(|| config.trace.clone()));
    match path {
        Some(p) => {
            Trace::to_file(p.as_ref(), &config.trace_brief).map_err(|e| format!("trace {p}: {e}"))
        }
        None => Ok(Trace::disabled()),
    }
}

/// The version string from the config, or the server's own.
fn server_version(config: &Config) -> Result<Version, String> {
    let v = Version::default();
    if !config.game.version.is_empty() {
        v.set(&config.game.version)
            .map_err(|e| format!("[game] version: {e}"))?;
    }
    Ok(v)
}

fn admin_paths(config: &Config) -> vertix_reconstruction::game::admin::Paths {
    vertix_reconstruction::game::admin::Paths {
        krp_data: config.game.krp_data.clone(),
        balance_dir: config.game.balance_dir.clone(),
        rules: config.rules.clone(),
    }
}

/// Starts the terminal console and the admin panel, as configured.
async fn start_admin(
    cfg: &vertix_reconstruction::config::Admin,
    game: &mpsc::UnboundedSender<vertix_reconstruction::game::Ask>,
    log: Option<admin::Log>,
    ports: &mut Binder,
) -> Result<(), String> {
    if cfg.stdin {
        let echo = if cfg.stdin_log { log.clone() } else { None };
        tokio::spawn(admin::stdin::run(game.clone(), echo));
    }
    let (true, Some(log)) = (cfg.enabled, log) else {
        return Ok(());
    };
    let token = if cfg.token.is_empty() {
        admin::random_token()
    } else {
        cfg.token.clone()
    };
    let listener = ports.bind(&cfg.bind, "admin", cfg.port).await?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    if !addr.ip().is_loopback() {
        eprintln!("warning: the admin panel is reachable from other machines on {addr}");
    }
    eprintln!("admin panel on http://{addr}/#token={token}");
    let app = admin::http::router(admin::http::AdminState {
        game: game.clone(),
        log,
        token: token.into(),
    });
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("admin server: {e}");
        }
    });
    Ok(())
}

/// Pings Engine.IO clients every `every`.
fn spawn_heartbeat(eio: Arc<eio::Server>, every: Duration) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.tick().await;
        loop {
            tick.tick().await;
            eio.heartbeat();
        }
    });
}

async fn run() -> Result<(), String> {
    let args = parse_args()?;
    let config = Config::load(&args.config)?;
    let (rules, provenance) = Assumptions::load_layers(&config.rules).map_err(|e| e.to_string())?;
    let mut data = GameData::load(
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
    let maps = load_maps(&config, archive.as_deref())?;
    eprintln!("maps: {} loaded: {}", maps.len(), maps.ids().join(" "));
    if !config.client_dir.join("index.html").is_file() {
        eprintln!(
            "warning: no client build in {}; run scripts/build-client.sh",
            config.client_dir.display()
        );
    }

    let restored = restored(&config, archive.as_deref(), &mut data)?;
    let trace = open_trace(args.trace, &config)?;

    let (tx, rx) = mpsc::unbounded_channel();
    let (ask_tx, ask_rx) = mpsc::unbounded_channel();
    let (classic_tx, classic_rx) = mpsc::unbounded_channel();
    let timing = config.engine_io.timing();
    let eio = eio::Server::new(timing, tx, trace.clone());
    let mut game = Game::new(rules, data, maps, &config.game.room_specs(), trace.clone())?;
    let version = server_version(&config)?;
    let classic_app = if config.classic.enabled {
        if !config.classic.room.is_empty() {
            game.set_classic_room(&config.classic.room)?;
        }
        classic_app(&config, archive.as_deref(), classic_tx, &trace, &version)?
    } else {
        None
    };
    let a = &config.admin;
    let log = (a.enabled || (a.stdin && a.stdin_log)).then(admin::log_channel);
    game.set_admin(log.clone(), admin_paths(&config), version.clone());
    tokio::spawn(game.run(rx, ask_rx, classic_rx));
    let mut ports = Binder::new(args.strict_port || config.strict_ports);
    let listener = ports
        .bind(&config.bind, "krp", args.port.unwrap_or(config.port))
        .await?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    start_admin(a, &ask_tx, log, &mut ports).await?;
    spawn_heartbeat(eio.clone(), timing.ping_interval);

    let restored = Arc::new(restored);
    let app = content::router(restored.clone()).merge(http::router(http::AppState {
        client_dir: Arc::new(config.client_dir.clone()),
        eio,
        game: ask_tx,
        trace,
        version,
    }));
    // The 2016 client loads hats, camos and sprays too.
    if let Some(app) = classic_app.map(|a| content::router(restored).merge(a)) {
        let listener = ports
            .bind(&config.bind, "classic", config.classic.port)
            .await?;
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        eprintln!("2016 client on http://{addr}/");
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("2016 client server: {e}");
            }
        });
    }
    if !config.ports_file.is_empty() {
        ports.write(config.ports_file.as_ref())?;
    }
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
