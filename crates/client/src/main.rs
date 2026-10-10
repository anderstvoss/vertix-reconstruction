//! The reconstruction's Rust client: KRP's browser client ported to Rust,
//! for the browser (WebAssembly) and as a desktop build.
//!
//! Launch options (desktop: `--key value`; browser: `?key=value`):
//! `server`, `room`, `name`, `class`, `input` (`frame` or a rate in Hz),
//! `display` (`sharp` or `krp`), `floor` (`2016` or `krp`), `autoplay`, `script`, `duration`
//! (seconds, then report and quit), `metrics` (desktop: where to write
//! them), `screenshot` and `shot-at` (desktop).

// `unsafe` is denied crate-wide (Cargo.toml) and forbidden module by
// module, except in platform/web.rs, which declares the page's imports.

mod assets;
mod game;
mod gfx;
mod platform;
mod sio;
mod text;

use std::collections::HashSet;

use macroquad::prelude::*;
use serde_json::{Value, json};

use game::hud::Ui;
use game::{FrameInput, Game, Gfx, Options};
use gfx::{Canvas, hex};
use platform::{Fetch, launch_options, now_ms, report, server_base};

fn window_conf() -> Conf {
    Conf {
        window_title: "Vertix".to_owned(),
        window_width: 1280,
        window_height: 720,
        high_dpi: true,
        sample_count: 1,
        ..Default::default()
    }
}

/// Waits for a download, keeping the window responsive.
async fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut f = Fetch::start(url);
    loop {
        if let Some(r) = f.poll() {
            return r;
        }
        clear_background(hex("#2e3031"));
        next_frame().await;
    }
}

/// Frame timing and network counts for the comparison tooling.
struct Metrics {
    started: f64,
    frame_ms: Vec<f64>,
    last: f64,
}

impl Metrics {
    fn json(&self, game: &Game, opts: &[(String, String)]) -> String {
        let mut sorted = self.frame_ms.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let pct = |p: f64| {
            if sorted.is_empty() {
                0.0
            } else {
                sorted[((sorted.len() - 1) as f64 * p).round() as usize]
            }
        };
        let secs = (now_ms() - self.started) / 1000.0;
        let mean = if sorted.is_empty() {
            0.0
        } else {
            sorted.iter().sum::<f64>() / sorted.len() as f64
        };
        json!({
            "client": "rust",
            "target": if cfg!(target_arch = "wasm32") { "browser" } else { "desktop" },
            "options": opts.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect::<serde_json::Map<_, _>>(),
            "seconds": secs,
            "frames": self.frame_ms.len(),
            "fps_mean": if mean > 0.0 { 1000.0 / mean } else { 0.0 },
            "frame_ms": { "mean": mean, "p50": pct(0.5), "p95": pct(0.95), "p99": pct(0.99), "max": pct(1.0) },
            "inputs_sent": game.inputs_sent,
            "inputs_per_second": game.inputs_sent as f64 / secs.max(0.001),
            "updates_received": game.updates_received,
            "updates_per_second": game.updates_received as f64 / secs.max(0.001),
            "ping_ms": game.ping,
            "dpi_scale": screen_dpi_scale(),
            "css_size": [screen_width(), screen_height()],
            "log": game.log,
        })
        .to_string()
    }
}

#[macroquad::main(window_conf)]
#[allow(clippy::too_many_lines)]
async fn main() {
    let pairs = launch_options();
    let get = |k: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
    };
    let base = server_base(&pairs);
    let opts = Options::from_pairs(&pairs);
    let krp_display = get("display").as_deref() == Some("krp");
    let duration: Option<f64> = get("duration").and_then(|d| d.parse().ok());

    let res = match download(&format!("{base}/res.zip")).await {
        Ok(b) => b,
        Err(e) => {
            fail(&format!("Could not load res.zip from {base}: {e}")).await;
            return;
        }
    };
    let font = match download(&format!("{base}/rust/font2.ttf")).await {
        Ok(b) => b,
        Err(e) => {
            fail(&format!("Could not load the font from {base}/rust/: {e}")).await;
            return;
        }
    };
    let Ok(font) = load_ttf_font_from_bytes(&font) else {
        fail("The font did not load.").await;
        return;
    };
    let pack = match assets::Pack::from_zip(&res) {
        Ok(p) if !p.is_empty() => p,
        Ok(_) => {
            fail("res.zip has no sprites.").await;
            return;
        }
        Err(e) => {
            fail(&format!("res.zip did not load: {e}")).await;
            return;
        }
    };
    let (classes, weapon_names) = assets::krp_loadouts();
    let mut sprites = assets::Sprites::pick(&pack, &classes, &weapon_names);
    // The 2016 floor art (KRP's res.zip has the plainer 2017 tiles), when
    // the server has it; `floor=krp` keeps KRP's.
    let mut floor_note = "floor: KRP (res.zip)".to_owned();
    if get("floor").as_deref() != Some("krp") {
        let url = format!("{base}/mods/{}/vertixmod.zip", assets::FLOOR_2016_PACK);
        match download(&url)
            .await
            .and_then(|b| assets::Pack::from_zip(&b))
        {
            Ok(p) if sprites.use_floors_from(&p) => {
                floor_note = format!("floor: 2016 (pack {})", assets::FLOOR_2016_PACK);
            }
            Ok(_) => floor_note = format!("{floor_note}; the 2016 pack has no ground tiles"),
            Err(e) => floor_note = format!("{floor_note}; no 2016 pack: {e}"),
        }
    }
    let mut gfx = Gfx::new(font, &base);
    let mut game = Game::new(opts.clone(), base.clone(), classes, sprites);
    game.log.push(floor_note);
    let mut join = game.join_room(&opts.room);
    let mut world_rt: Option<Canvas> = None;
    let mut ui_hot = false;
    let mut last_mouse = (-1.0_f32, -1.0_f32);
    let mut metrics = Metrics {
        started: now_ms(),
        frame_ms: Vec::new(),
        last: now_ms(),
    };
    let mut reported = false;
    let mut shot_taken = false;

    loop {
        let dpi = screen_dpi_scale();
        let css = (screen_width(), screen_height());

        if let Some(f) = &mut join {
            if let Some(r) = f.poll() {
                join = None;
                if let Some(v) = r
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                {
                    game.joined(&v);
                } else {
                    game.changing_lobby = false;
                    game.log.push("getIP failed".into());
                }
            }
        }
        if let Some(room) = game.menu.join.take() {
            if join.is_none() {
                join = game.join_room(&room);
            }
        }
        game.poll_menu();
        gfx.remote.poll();

        // The world camera: KRP's setTransform, filling the window.
        let (w, h) = css;
        let a = (w / game.max_w as f32).max(h / game.max_h as f32);
        let ox = (w - game.max_w as f32 * a) / 2.0;
        let oy = (h - game.max_h as f32 * a) / 2.0;
        let rect = Rect::new(-ox / a, -oy / a, w / a, h / a);
        let mut cam = game::screen_camera(rect);
        if krp_display {
            // KRP's canvas has one pixel per CSS pixel, scaled up by the
            // browser without smoothing (`image-rendering: pixelated`).
            let (pw, ph) = (w.round().max(1.0) as u32, h.round().max(1.0) as u32);
            if world_rt
                .as_ref()
                .is_none_or(|c| c.width as u32 != pw || c.height as u32 != ph)
            {
                world_rt = Some(Canvas::new(pw, ph));
            }
            cam = Camera2D {
                render_target: world_rt.as_ref().map(|c| c.rt.clone()),
                ..Camera2D::from_display_rect(rect)
            };
        }
        set_camera(&cam);
        clear_background(hex("#2e3031"));
        gfx.cam = (rect, cam.render_target.clone());
        gfx.painter.reset(false);
        game.poll_network(&mut gfx);

        let input = read_input(&mut game, css, ui_hot, &mut last_mouse);
        game.frame(&mut gfx, &input);
        gl_use_default_material();

        // The HTML overlay, in CSS pixels.
        let hud_cam = game::screen_camera(Rect::new(0.0, 0.0, w, h));
        set_camera(&hud_cam);
        if krp_display {
            if let Some(c) = &world_rt {
                let img = c.image();
                draw_texture_ex(
                    &img.tex,
                    0.0,
                    0.0,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(w, h)),
                        flip_y: true,
                        ..Default::default()
                    },
                );
            }
        }
        let (mx, my) = mouse_position();
        let clicked = is_mouse_button_pressed(MouseButton::Left);
        let mut ui = Ui {
            text: &gfx.text,
            px: dpi,
            mouse: vec2(mx, my),
            clicked,
            hot: false,
            dry: false,
        };
        let minimap = gfx.minimap.as_ref().map(Canvas::image);
        game.draw_hud(minimap.as_ref(), &mut ui, css);
        let mut hot = ui.hot;
        if game.start_menu && !game.kicked && !game.disconnected {
            let (scale, origin) = game.menu_transform(css);
            let menu_cam = game::screen_camera(Rect::new(
                -origin.x / scale,
                -origin.y / scale,
                w / scale,
                h / scale,
            ));
            set_camera(&menu_cam);
            let mut ui = Ui {
                text: &gfx.text,
                px: dpi * scale,
                mouse: (vec2(mx, my) - origin) / scale,
                clicked,
                hot: false,
                dry: false,
            };
            game.draw_start_menu(&mut ui);
            hot |= ui.hot;
            if opts.autoplay && game.can_start() && !game.starting_game {
                game.start_game();
            }
        }
        ui_hot = hot;
        set_default_camera();

        let now = now_ms();
        metrics.frame_ms.push(now - metrics.last);
        metrics.last = now;
        let elapsed = (now - metrics.started) / 1000.0;
        #[cfg(not(target_arch = "wasm32"))]
        if !shot_taken {
            if let Some(path) = get("screenshot") {
                let at: f64 = get("shot-at").and_then(|s| s.parse().ok()).unwrap_or(5.0);
                if elapsed >= at {
                    shot_taken = true;
                    get_screen_data().export_png(&path);
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = &mut shot_taken;
        if let Some(d) = duration {
            if elapsed >= d && !reported {
                reported = true;
                report(&metrics.json(&game, &pairs), &pairs);
                if !cfg!(target_arch = "wasm32") {
                    break;
                }
            }
        }
        next_frame().await;
    }
}

/// Shows an error until the window is closed.
async fn fail(msg: &str) {
    loop {
        clear_background(hex("#2e3031"));
        draw_text(msg, 20.0, 40.0, 24.0, WHITE);
        next_frame().await;
    }
}

/// Collects this frame's keyboard and mouse, and handles typing into the
/// chat box and the name field.
fn read_input(
    game: &mut Game,
    css: (f32, f32),
    ui_hot: bool,
    last_mouse: &mut (f32, f32),
) -> FrameInput {
    let mut chars = Vec::new();
    while let Some(c) = get_char_pressed() {
        chars.push(c);
    }
    let pressed: HashSet<KeyCode> = get_keys_pressed();
    let (mx, my) = mouse_position();
    let typing_name = game.start_menu && game.chat_input.is_none();
    if let Some(t) = &mut game.chat_input {
        for c in &chars {
            if !c.is_control() && t.chars().count() < 50 {
                t.push(*c);
            }
        }
        if pressed.contains(&KeyCode::Backspace) {
            t.pop();
        }
        if pressed.contains(&KeyCode::Escape) {
            game.chat_input = None;
        } else if pressed.contains(&KeyCode::Enter) {
            game.send_chat();
        }
        return FrameInput {
            mouse: (f64::from(mx), f64::from(my)),
            css_size: (f64::from(css.0), f64::from(css.1)),
            mouse_down: is_mouse_button_down(MouseButton::Left),
            ..FrameInput::default()
        };
    }
    if typing_name {
        for c in &chars {
            if !c.is_control() && game.player_name.chars().count() < 15 {
                game.player_name.push(*c);
            }
        }
        if pressed.contains(&KeyCode::Backspace) {
            game.player_name.pop();
        }
        if pressed.contains(&KeyCode::Enter) && game.can_start() {
            game.start_game();
        }
    }
    let moved = *last_mouse != (mx, my);
    *last_mouse = (mx, my);
    let in_game = !ui_hot && !game.start_menu;
    FrameInput {
        down: if typing_name {
            HashSet::new()
        } else {
            get_keys_down()
        },
        pressed: if typing_name { HashSet::new() } else { pressed },
        released: get_keys_released(),
        mouse: (f64::from(mx), f64::from(my)),
        mouse_moved: moved,
        mouse_pressed: is_mouse_button_pressed(MouseButton::Left),
        mouse_released: is_mouse_button_released(MouseButton::Left),
        mouse_down: is_mouse_button_down(MouseButton::Left),
        wheel: wheel_step(mouse_wheel().1),
        css_size: (f64::from(css.0), f64::from(css.1)),
        in_game_area: in_game,
    }
}

/// The wheel as a step of -1, 0 or 1. (`f32::signum` gives 1 for 0, which
/// would swap weapons every frame.)
fn wheel_step(dy: f32) -> f64 {
    if dy == 0.0 {
        0.0
    } else {
        f64::from(dy.signum())
    }
}

#[cfg(test)]
mod tests {
    use super::wheel_step;

    #[test]
    fn a_still_wheel_is_no_step() {
        assert_eq!(wheel_step(0.0), 0.0);
        assert_eq!(wheel_step(-0.0), 0.0);
        assert_eq!(wheel_step(3.5), 1.0);
        assert_eq!(wheel_step(-120.0), -1.0);
    }
}
