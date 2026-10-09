# Running it yourself

This runs the 2016-08-06 client against the reconstruction server on your
own machine. It works the same on Windows, macOS and Linux.

## What you need

- **Rust.** Install `rustup` from <https://rustup.rs>. The right toolchain
  (pinned in `rust-toolchain.toml`) installs itself on the first build.
- **Git with Git LFS**, for the two clones below.
- **The private archive** (`vertix-archive`) with these LFS files pulled.
  The server reads them at start-up and refuses to run if any hash differs:
  - `vertix-preservation/originals/wayback/20160806061006/vertix.io/js/app.js.*`
  - `vertix-preservation/originals/wayback/20160806060840/vertix.io/css/main.css.*`
  - `vertix-preservation/originals/wayback/20160807195546/vertix.io_80/root.*` (the page)
  - `vertix-preservation/originals/wayback/20160825094920/vertix.io/images/google-play-badge.png.*`
  - `vertix-preservation/originals/external/jquery-2.1.4.min.js`
  - `vertix-preservation/originals/external/socket.io-1.4.5.js`
  - `vertix-preservation/originals/external/android/tbs.vertix.io-0.0.3.apk`
    (sprites, `res.zip`, workers and fonts come from inside it)

  A full `git lfs pull` in the archive covers all of these.

## Start the server

From the repository root:

```bash
cargo run --release -- --archive PATH/TO/vertix-archive
```

On Windows PowerShell, quote a path that contains spaces:

```powershell
cargo run --release -- --archive "PATH\TO\vertix-archive"
```

The first build takes a few minutes. When it is ready it prints
`loaded build 20160806061006 ... all hashes verified` and the address it
is listening on. Open that address in a browser, type a name and press
**Play**. Open a second browser window (or a private window) to the same
address to get a second player.

Useful options:

- `--port 9000` to use another port. The default bind address and port are
  in `config/server.toml`.
- `--trace out/trace.jsonl` writes every event in and out, one JSON line
  each, for checking protocol behaviour.
- Setting the `VERTIX_ARCHIVE` environment variable replaces `--archive`.

To let another machine on your network join, change `bind` in
`config/server.toml` to your machine's network address (do not commit
that change) and open the matching port in your firewall.

## What works so far

Joining, spawning, walking with wall collision, jumping, switching weapons
and leaving, in one free-for-all room. Shooting does nothing yet, and there
are no rounds, lobbies or saves. See [DEVIATIONS.md](DEVIATIONS.md) for
what differs from the original.

## Automated browser check (optional)

With the server running:

```bash
python3 -m pip install playwright
python3 -m playwright install chromium
python3 scripts/e2e_boot.py --url http://HOST:PORT/
```

It plays the game in two headless browsers with every outside request
blocked and writes screenshots and a report to `out/e2e/`.

## Troubleshooting

- **"hash mismatch" or "is an LFS pointer"** at start-up: that archive file
  was not pulled. Run `git lfs pull` in the archive.
- **"Disconnected"** in the game: the client never reconnects, so a server
  restart drops everyone. Reload the page.
- **Blank page on https:** the 2016 client always connects over plain
  `http://`, so open the page over http.
