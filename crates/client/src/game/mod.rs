//! The game: KRP's `app.tsx` loop, prediction, socket events and drawing.
//!
//! The code follows KRP's client function by function (names in the doc
//! comments), including its frame-rate-dependent parts, so that it feels
//! the same. Deliberate differences are listed in `crates/client/README.md`.

#![forbid(unsafe_code)]

pub mod animtext;
mod draw;
pub mod effects;
mod events;
pub mod hud;
pub mod menu;
pub mod model;
pub mod rand;
pub mod world;

use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;
use std::rc::Rc;

use macroquad::prelude::*;
use serde_json::{Value, json};
use vertix_sim::map::{Body, Clutter, Map, World};
use vertix_sim::projectile::{Owner, Projectile, Shot};

use crate::assets::{ClassInfo, Sprites};
use crate::gfx::{Canvas, Materials, Painter};
use crate::platform::{Fetch, now_ms};
use crate::sio::{SioClient, SioEvent};
use crate::text::Text;
use animtext::{AnimTexts, Screen};
use effects::{Effects, View, WallBox};
use model::{Flag, GameMode, Pickup, Player};
use rand::random_float;
use world::RenderTile;

pub const TEAM_RED: &str = "#d95151";
pub const TEAM_BLUE: &str = "#5151d9";

/// How input reaches the server.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    /// One `"4"` per rendered frame, as KRP's client does.
    PerFrame,
    /// One per tick at this rate, whatever the frame rate.
    Fixed(f64),
}

/// Launch settings (command line or page query).
#[derive(Debug, Clone)]
pub struct Options {
    pub name: String,
    pub room: String,
    pub class: Option<String>,
    pub input: InputMode,
    /// Skip the start menu and join at once.
    pub autoplay: bool,
    /// Scripted input for tests and comparisons, e.g. `w:500,d:300`.
    pub script: Option<String>,
}

impl Options {
    #[must_use]
    pub fn from_pairs(pairs: &[(String, String)]) -> Self {
        let get = |k: &str| {
            pairs
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
        };
        let input = match get("input").as_deref() {
            None | Some("" | "frame") => InputMode::PerFrame,
            Some(hz) => hz
                .trim_end_matches("hz")
                .parse::<f64>()
                .ok()
                .filter(|h| *h >= 10.0 && *h <= 1000.0)
                .map_or(InputMode::PerFrame, InputMode::Fixed),
        };
        Self {
            name: get("name").unwrap_or_default(),
            room: get("room").unwrap_or_default(),
            class: get("class"),
            input,
            autoplay: get("autoplay").is_some(),
            script: get("script"),
        }
    }
}

/// `settings` from KRP's state, with its defaults.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct Settings {
    pub show_names: bool,
    pub show_particles: bool,
    pub show_sprays: bool,
    pub show_fade: bool,
    pub show_shadows: bool,
    pub show_glows: bool,
    pub show_trails: bool,
    pub show_chat: bool,
    pub show_ui: bool,
    pub show_ping_fps: bool,
    pub show_leader: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_names: true,
            show_particles: true,
            show_sprays: true,
            show_fade: true,
            show_shadows: true,
            show_glows: true,
            show_trails: true,
            show_chat: true,
            show_ui: true,
            show_ping_fps: true,
            show_leader: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Keys {
    u: bool,
    d: bool,
    l: bool,
    r: bool,
    lm: bool,
    s: bool,
    rl: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct AimTarget {
    f: f64,
    d: f64,
    d_offset: f64,
}

#[derive(Debug, Clone)]
struct SentInput {
    hdt: f64,
    vdt: f64,
    isn: f64,
    delta: f64,
}

/// A bullet and the client-only parts KRP keeps on it.
#[derive(Debug, Clone, Default)]
pub struct Bullet {
    pub p: Projectile,
    pub trail_alpha: f64,
    pub trail_width: f64,
    pub glow_width: f64,
    pub glow_height: f64,
    pub dust_timer: f64,
}

/// The round's map: KRP's tiles for drawing, the shared crate's world for
/// collision, and the objects on it.
pub struct MapState {
    pub tiles: Vec<RenderTile>,
    pub world: World,
    pub flags: Vec<Flag>,
    pub pickups: Vec<Pickup>,
    pub width: f64,
    pub height: f64,
    pub tile_scale: f64,
    pub mode: GameMode,
    pub walls: Vec<WallBox>,
}

pub struct SprayState {
    pub owner: u32,
    pub src: String,
    pub active: bool,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub alpha: f64,
}

/// A line in the chat box.
pub struct ChatLine {
    pub author: String,
    pub text: String,
    /// `system`, `notif`, `me`, `blue` or `red`.
    pub source: String,
}

/// Images loaded by URL at run time (hats, shirts, camos, sprays).
#[derive(Default)]
pub struct Remote {
    pending: HashMap<String, Fetch>,
    pub ready: HashMap<String, Texture2D>,
    failed: HashSet<String>,
    base: String,
}

impl Remote {
    /// The texture at `path` (`/images/...`), starting a download if needed.
    pub fn get(&mut self, path: &str) -> Option<Texture2D> {
        if let Some(t) = self.ready.get(path) {
            return Some(t.clone());
        }
        if !self.failed.contains(path) && !self.pending.contains_key(path) {
            let url = if path.starts_with("http") {
                path.to_owned()
            } else {
                format!("{}{}", self.base, path)
            };
            self.pending.insert(path.to_owned(), Fetch::start(&url));
        }
        None
    }

    pub fn poll(&mut self) {
        let mut done = Vec::new();
        for (k, f) in &mut self.pending {
            if let Some(r) = f.poll() {
                done.push((k.clone(), r));
            }
        }
        for (k, r) in done {
            self.pending.remove(&k);
            match r.ok().and_then(|b| crate::assets::texture_from_png(&b)) {
                Some(t) => {
                    self.ready.insert(k, t);
                }
                None => {
                    self.failed.insert(k);
                }
            }
        }
    }
}

/// Drawing resources shared by the game, HUD and menus.
pub struct Gfx {
    /// Draws the frame.
    pub painter: Painter,
    /// Draws offscreen canvases (players, camos, minimap).
    pub off: Painter,
    pub shadows: draw::Shadows,
    pub minimap: Option<Canvas>,
    pub text: Text,
    pub tiles: world::TileCache,
    pub player_canvases: HashMap<u32, Canvas>,
    pub weapon_cache: HashMap<(usize, i64, i32), crate::gfx::Image>,
    pub weapon_canvases: Vec<Canvas>,
    /// The camera of whatever the frame is being drawn into right now
    /// (`Camera2D` is not `Clone`, so it is kept as its parts).
    pub cam: (Rect, Option<RenderTarget>),
    pub edge: Option<Texture2D>,
    pub minimap_base: Option<Canvas>,
    pub remote: Remote,
}

impl Gfx {
    #[must_use]
    pub fn new(font: Font, base: &str) -> Self {
        let mats = Rc::new(Materials::new());
        Self {
            painter: Painter::new(Rc::clone(&mats)),
            off: Painter::new(Rc::clone(&mats)),
            shadows: draw::Shadows::default(),
            minimap: None,
            text: Text::new(font, Rc::clone(&mats)),
            tiles: world::TileCache::new(Rc::clone(&mats)),
            player_canvases: HashMap::new(),
            weapon_cache: HashMap::new(),
            weapon_canvases: Vec::new(),
            cam: (Rect::new(0.0, 0.0, 1.0, 1.0), None),
            edge: None,
            minimap_base: None,
            remote: Remote {
                base: base.to_owned(),
                ..Remote::default()
            },
        }
    }

    /// A closure that puts the current target back after offscreen work.
    pub fn restorer(&self) -> impl FnMut() + use<> {
        let cam = self.cam.clone();
        move || set_camera(&main_camera(&cam))
    }

    /// Makes the frame's camera current again.
    pub fn set_main_camera(&self) {
        set_camera(&main_camera(&self.cam));
    }
}

/// The frame camera from its display rectangle and target.
#[must_use]
pub fn main_camera(cam: &(Rect, Option<RenderTarget>)) -> Camera2D {
    match &cam.1 {
        Some(rt) => Camera2D {
            render_target: Some(rt.clone()),
            ..Camera2D::from_display_rect(cam.0)
        },
        None => screen_camera(cam.0),
    }
}

/// A camera onto the window with `rect` as its view, y pointing down.
/// (`from_display_rect` is meant for render targets: on the window it
/// comes out upside down.)
#[must_use]
pub fn screen_camera(rect: Rect) -> Camera2D {
    let mut c = Camera2D::from_display_rect(rect);
    c.zoom.y = -c.zoom.y;
    c
}

/// Everything KRP keeps in `st` and its module globals.
pub struct Game {
    pub opts: Options,
    pub base: String,
    pub classes: Vec<ClassInfo>,
    pub sprites: Sprites,
    pub settings: Settings,
    sio: Option<SioClient>,
    pub room: Option<String>,
    pub player_name: String,
    pub loadout_class: usize,
    pub players: Vec<Player>,
    pub me: Option<u32>,
    placeholder: Player,
    pub max_w: f64,
    pub max_h: f64,
    pub view_mult: f64,
    pub start_x: f64,
    pub start_y: f64,
    pub game_start: bool,
    pub game_over: bool,
    pub kicked: bool,
    pub disconnected: bool,
    pub reason: Option<String>,
    pub in_main_menu: bool,
    pub starting_game: bool,
    pub changing_lobby: bool,
    pub map: Option<MapState>,
    pub bullets: Vec<Bullet>,
    bullet_index: usize,
    target: AimTarget,
    target_changed: bool,
    keys: Keys,
    key_map: HashSet<KeyCode>,
    user_scroll: f64,
    input_number: f64,
    this_input: Vec<SentInput>,
    current_time: f64,
    old_time: f64,
    input_accum: f64,
    pub fps: f64,
    fps_delta: f64,
    fps_samples: Vec<f64>,
    pub ping: f64,
    ping_start: f64,
    ping_timer: f64,
    overlay_alpha: f64,
    animate_overlay: bool,
    game_over_fade: bool,
    minimap_counter: i32,
    pub fx: Effects,
    pub anim: AnimTexts,
    pub sprays: Vec<SprayState>,
    pub chat: Vec<ChatLine>,
    pub leaderboard: Vec<u32>,
    pub team_scores: Option<(f64, f64)>,
    pub mode_text: String,
    pub next_game_text: String,
    pub winner_text: Option<(String, String)>,
    pub votes: Vec<(String, f64)>,
    pub my_vote: Option<usize>,
    pub start_menu: bool,
    pub stat_table: bool,
    pub showing_scoreboard: bool,
    pub cooldowns: Vec<(f64, f64)>,
    pub chat_input: Option<String>,
    pub chat_team: bool,
    pub chat_visible: bool,
    pub current_liked: Option<u32>,
    /// Team progress bars: mine, theirs (percent) and the first row's label.
    pub progress: (f64, f64, &'static str),
    pub menu: menu::Menu,
    /// Inputs sent and position updates received, for measurements.
    pub inputs_sent: u64,
    pub updates_received: u64,
    timers: Vec<(f64, TimerKind)>,
    script: Vec<(Vec<KeyCode>, f64)>,
    script_at: f64,
    pub frame: u64,
    pub log: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum TimerKind {
    ShowStartMenuAfterDeath,
    GameOverFade,
    ShowStatTable,
}

pub(super) const OVERLAY_MAX_ALPHA: f64 = 0.5;
const OVERLAY_FADE_UP: f64 = 0.01;
const OVERLAY_FADE_DOWN: f64 = 0.04;
pub(super) const MINIMAP_EVERY: i32 = 4;

impl Game {
    #[must_use]
    pub fn new(opts: Options, base: String, classes: Vec<ClassInfo>, sprites: Sprites) -> Self {
        let loadout_class = opts
            .class
            .as_ref()
            .and_then(|c| {
                classes
                    .iter()
                    .position(|k| &k.folder == c || k.name.eq_ignore_ascii_case(c))
                    .or_else(|| c.parse().ok())
            })
            .unwrap_or(0);
        let player_name = opts.name.clone();
        let script = opts.script.as_deref().map(parse_script).unwrap_or_default();
        let now = now_ms().floor();
        Self {
            opts,
            base,
            classes,
            sprites,
            settings: Settings::default(),
            sio: None,
            room: None,
            player_name,
            loadout_class,
            players: Vec::new(),
            me: None,
            placeholder: Player {
                dead: true,
                ..Player::default()
            },
            max_w: 1920.0,
            max_h: 1080.0,
            view_mult: 1.0,
            start_x: 0.0,
            start_y: 0.0,
            game_start: false,
            game_over: false,
            kicked: false,
            disconnected: false,
            reason: None,
            in_main_menu: true,
            starting_game: false,
            changing_lobby: false,
            map: None,
            bullets: Vec::new(),
            bullet_index: 0,
            target: AimTarget::default(),
            target_changed: true,
            keys: Keys::default(),
            key_map: HashSet::new(),
            user_scroll: 0.0,
            input_number: 0.0,
            this_input: Vec::new(),
            current_time: now,
            old_time: now,
            input_accum: 0.0,
            fps: 0.0,
            fps_delta: 0.0,
            fps_samples: Vec::new(),
            ping: 0.0,
            ping_start: 0.0,
            ping_timer: 0.0,
            overlay_alpha: OVERLAY_MAX_ALPHA,
            animate_overlay: true,
            game_over_fade: false,
            minimap_counter: 0,
            fx: Effects::new(),
            anim: AnimTexts::new(),
            sprays: Vec::new(),
            chat: Vec::new(),
            leaderboard: Vec::new(),
            team_scores: None,
            mode_text: String::new(),
            next_game_text: String::new(),
            winner_text: None,
            votes: Vec::new(),
            my_vote: None,
            start_menu: true,
            stat_table: false,
            showing_scoreboard: false,
            cooldowns: Vec::new(),
            chat_input: None,
            chat_team: false,
            chat_visible: false,
            current_liked: None,
            progress: (0.0, 0.0, "A"),
            menu: menu::Menu::default(),
            inputs_sent: 0,
            updates_received: 0,
            timers: Vec::new(),
            script,
            script_at: 0.0,
            frame: 0,
            log: Vec::new(),
        }
    }

    // ---- players -------------------------------------------------------

    /// KRP `findUserByIndex`.
    #[must_use]
    pub fn find(&self, index: u32) -> Option<&Player> {
        self.players.iter().find(|p| p.index == index)
    }

    pub fn find_mut(&mut self, index: u32) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.index == index)
    }

    /// KRP `st.player`.
    #[must_use]
    pub fn me(&self) -> &Player {
        self.me
            .and_then(|i| self.find(i))
            .unwrap_or(&self.placeholder)
    }

    pub fn me_mut(&mut self) -> &mut Player {
        match self.me {
            Some(i) if self.players.iter().any(|p| p.index == i) => self
                .players
                .iter_mut()
                .find(|p| p.index == i)
                .expect("checked"),
            _ => &mut self.placeholder,
        }
    }

    #[must_use]
    pub fn screen(&self) -> Screen {
        Screen {
            max_w: self.max_w,
            max_h: self.max_h,
            view_mult: self.view_mult,
        }
    }

    // ---- connection ----------------------------------------------------

    /// KRP `joinRoom`: asks `/api/getIP` for the room, then connects to it.
    pub fn join_room(&mut self, room: &str) -> Option<Fetch> {
        if self.changing_lobby {
            return None;
        }
        self.changing_lobby = true;
        Some(Fetch::start(&format!(
            "{}/api/getIP?room={room}",
            self.base
        )))
    }

    /// The `/api/getIP` answer arrived.
    pub fn joined(&mut self, answer: &Value) {
        let room = answer
            .get("room")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if Some(&room) == self.room.as_ref() && self.sio.is_some() {
            self.changing_lobby = false;
            return;
        }
        if let Some(s) = &mut self.sio {
            s.close();
        }
        self.sio = Some(SioClient::connect(&self.base, &format!("/{room}")));
        self.in_main_menu = true;
        self.chat_visible = false;
        self.chat.clear();
    }

    pub fn emit(&mut self, event: &str, args: Vec<Value>) {
        if let Some(s) = &mut self.sio {
            s.emit(event, args);
        }
    }

    /// Handles everything the server sent since the last frame.
    pub fn poll_network(&mut self, gfx: &mut Gfx) {
        let events = match &mut self.sio {
            Some(s) => s.poll(),
            None => return,
        };
        for ev in events {
            match ev {
                SioEvent::Connected => {}
                SioEvent::Event(name, args) => self.on_event(&name, &args, gfx),
                SioEvent::Disconnected(reason) => {
                    self.log.push(format!("disconnected: {reason}"));
                    self.kick("Disconnected. Your connection timed out.");
                }
            }
        }
    }

    /// KRP `kickPlayer`.
    pub fn kick(&mut self, second_reason: &str) {
        if self.disconnected || self.changing_lobby {
            return;
        }
        self.start_menu = false;
        self.stat_table = false;
        self.disconnected = true;
        self.game_over = true;
        if self.reason.is_none() {
            self.reason = Some(second_reason.to_owned());
        }
        self.kicked = true;
        if let Some(s) = &mut self.sio {
            s.close();
        }
    }

    /// KRP `startGame` + `enterGame`.
    pub fn start_game(&mut self) {
        if self.starting_game || self.changing_lobby {
            return;
        }
        self.starting_game = true;
        self.player_name = clean_name(&self.player_name);
        self.start_menu = false;
        if self.room.is_none() {
            self.emit("create", vec![]);
        }
        self.animate_overlay = true;
        if self.me().dead {
            self.emit("respawn", vec![]);
            self.cooldowns.clear();
        } else {
            self.in_main_menu = false;
            self.starting_game = false;
            if self.game_over {
                self.stat_table = true;
            }
        }
    }

    /// KRP `showESCMenu`.
    pub fn show_esc_menu(&mut self) {
        self.anim.clear();
        self.starting_game = false;
        self.in_main_menu = true;
        self.start_menu = true;
        self.stat_table = false;
    }

    // ---- the frame -----------------------------------------------------

    /// KRP `callUpdate` + `updateGameLoop`, drawing into the current target.
    #[allow(clippy::too_many_lines)]
    pub fn frame(&mut self, gfx: &mut Gfx, input: &FrameInput) {
        self.frame += 1;
        self.current_time = now_ms().floor();
        let delta = self.current_time - self.old_time;
        self.run_timers();
        self.read_input(input, gfx);
        self.ping_timer += delta;
        if self.ping_timer >= 2000.0 {
            self.ping_timer = 0.0;
            self.ping_start = now_ms();
            self.emit("ping1", vec![]);
        }

        let cur_fps = if delta > 0.0 { 1000.0 / delta } else { 0.0 };
        self.fps_samples.push(cur_fps);
        self.fps_delta += delta;
        if self.fps_delta >= 1000.0 {
            self.fps = self.fps_samples.iter().sum::<f64>() / self.fps_samples.len() as f64;
            self.fps_delta = 0.0;
            self.fps_samples.clear();
        }
        self.old_time = self.current_time;

        let (mut b, mut d) = (0.0_f64, 0.0_f64);
        if self.keys.u {
            d = -1.0;
        }
        if self.keys.d {
            d = 1.0;
        }
        if self.keys.r {
            b = 1.0;
        }
        if self.keys.l {
            b = -1.0;
        }
        let e = (b * b + d * d).sqrt();
        if e != 0.0 {
            b /= e;
            d /= e;
        }

        // In fixed mode, movement and the input packet happen on ticks.
        let (send_now, move_delta) = match self.opts.input {
            InputMode::PerFrame => (true, delta),
            InputMode::Fixed(hz) => {
                self.input_accum += delta;
                let step = 1000.0 / hz;
                if self.input_accum >= step {
                    let t = self.input_accum;
                    self.input_accum = 0.0;
                    (true, t)
                } else {
                    (false, 0.0)
                }
            }
        };

        let me = self.me;
        let game_over = self.game_over;
        let tf = self.target.f;
        let mut do_jump = 0;
        let mut i = 0;
        while i < self.players.len() {
            let is_me = Some(self.players[i].index) == me;
            if is_me {
                let world = self.map.as_ref().map(|m| &m.world);
                let plr = &mut self.players[i];
                plr.old_x = plr.x;
                plr.old_y = plr.y;
                if !plr.dead && !game_over {
                    plr.x += b * plr.speed * move_delta;
                    plr.y += d * plr.speed * move_delta;
                }
                wall_col(plr, world);
                plr.x = plr.x.round();
                plr.y = plr.y.round();
                plr.angle = ((tf + PI * 2.0) % (PI * 2.0)) * (180.0 / PI) + 90.0;
                let front = is_weapon_facing_front(snap_angle(plr.angle));
                if let Some(w) = plr.weapon_mut() {
                    w.front = front;
                }
                if plr.jump_countdown > 0.0 {
                    plr.jump_countdown -= delta;
                }
                if self.keys.s && plr.jump_countdown <= 0.0 && !game_over {
                    player_jump(plr);
                    do_jump = 1;
                }
            }
            {
                let plr = &mut self.players[i];
                if plr.jump_y != 0.0 {
                    plr.jump_delta -= plr.gravity_strength * delta;
                    plr.jump_y += plr.jump_delta * delta;
                    if plr.jump_y > 0.0 {
                        plr.anim_index = 1;
                    } else {
                        plr.jump_y = 0.0;
                        plr.jump_delta = 0.0;
                        plr.jump_countdown = 250.0;
                    }
                    plr.jump_y = plr.jump_y.round();
                }
            }
            if is_me && !game_over {
                if send_now {
                    let isn = self.input_number;
                    self.input_number += 1.0;
                    self.this_input.push(SentInput {
                        hdt: b,
                        vdt: d,
                        isn,
                        delta: move_delta,
                    });
                    let ts = self.current_time;
                    self.inputs_sent += 1;
                    self.emit(
                        "4",
                        // isn goes out as an integer, as KRP's client sends it: the
                        // server reads it as one and would ack nothing otherwise.
                        vec![json!({"hdt": b, "vdt": d, "ts": ts, "isn": isn as u64, "s": do_jump, "delta": move_delta})],
                    );
                }
                if self.user_scroll != 0.0 {
                    let s = self.user_scroll;
                    self.user_scroll = 0.0;
                    self.swap_weapon(i, s);
                }
                if self.keys.rl {
                    self.reload(i, gfx);
                }
                if self.keys.lm && !self.players[i].weapons.is_empty() {
                    let ready = self.players[i]
                        .weapon()
                        .is_some_and(|w| self.current_time - w.last_shot >= w.fire_rate);
                    if ready {
                        self.shoot(i, gfx);
                    }
                }
            }
            let plr = &mut self.players[i];
            if game_over {
                plr.anim_index = 0;
            } else {
                let moving = if is_me {
                    b.abs() + d.abs()
                } else {
                    plr.x_speed.abs() + plr.y_speed.abs()
                };
                if moving > 0.0 {
                    plr.frame_countdown -= delta / 4.0;
                    if plr.frame_countdown <= 0.0 {
                        plr.anim_index += 1;
                        if plr.jump_y == 0.0 && plr.on_screen && !plr.dead {
                            let (x, y) = (plr.x, plr.y);
                            self.fx.still_dust(x, y, false);
                        }
                        let plr = &mut self.players[i];
                        if plr.anim_index >= 3 {
                            plr.anim_index = 1;
                        }
                        plr.frame_countdown = 40.0;
                    }
                } else if plr.anim_index != 0 {
                    plr.anim_index = 0;
                }
                let plr = &mut self.players[i];
                if plr.jump_y > 0.0 {
                    plr.anim_index = 1;
                }
            }
            i += 1;
        }
        self.players
            .sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));

        if !self.kicked {
            if self.game_over {
                self.do_game(gfx, delta);
                if self.game_over_fade && self.settings.show_fade {
                    self.draw_overlay(gfx, true, false);
                }
            } else if self.me().dead && !self.in_main_menu {
                self.do_game(gfx, delta);
                self.draw_overlay(gfx, true, false);
            } else if self.game_start {
                self.do_game(gfx, delta);
                self.draw_overlay(gfx, false, true);
                if self.target_changed {
                    self.target_changed = false;
                    let f = self.target.f;
                    self.emit("0", vec![json!(f)]);
                }
            } else {
                self.draw_overlay(gfx, false, false);
            }
        }
        if self.disconnected || self.kicked {
            self.draw_overlay(gfx, false, false);
            let text = if self.kicked {
                self.reason
                    .clone()
                    .unwrap_or_else(|| "You were kicked".into())
            } else {
                "Disconnected".into()
            };
            let mut restore = gfx.restorer();
            let img = gfx.text.render_shaded(
                &text,
                (self.view_mult * 48.0) as f32,
                "#ffffff",
                6,
                false,
                &mut restore,
            );
            gfx.painter.invalidate();
            gfx.painter.set_alpha(1.0);
            gfx.painter.draw_image(
                &img.image,
                (self.max_w / 2.0) as f32 - img.width / 2.0,
                (self.max_h / 2.0) as f32 - img.height / 2.0,
                img.width,
                img.height,
            );
        }
    }

    fn run_timers(&mut self) {
        let now = self.current_time;
        let due: Vec<TimerKind> = self
            .timers
            .iter()
            .filter(|(t, _)| *t <= now)
            .map(|(_, k)| *k)
            .collect();
        self.timers.retain(|(t, _)| *t > now);
        for k in due {
            match k {
                TimerKind::ShowStartMenuAfterDeath => {
                    if !self.game_over {
                        self.start_menu = true;
                    }
                }
                TimerKind::GameOverFade => self.game_over_fade = true,
                TimerKind::ShowStatTable => self.stat_table = true,
            }
        }
    }

    fn after(&mut self, ms: f64, kind: TimerKind) {
        self.timers.push((self.current_time + ms, kind));
    }

    /// KRP `drawOverlay`.
    fn draw_overlay(&mut self, gfx: &mut Gfx, fade_up: bool, fade_down: bool) {
        if self.animate_overlay {
            if fade_up {
                self.overlay_alpha = (self.overlay_alpha + OVERLAY_FADE_UP).min(OVERLAY_MAX_ALPHA);
            } else if fade_down {
                self.overlay_alpha = (self.overlay_alpha - OVERLAY_FADE_DOWN).max(0.0);
            } else {
                self.overlay_alpha = OVERLAY_MAX_ALPHA;
            }
        }
        if self.overlay_alpha > 0.0 {
            let p = &mut gfx.painter;
            p.set_alpha(self.overlay_alpha as f32);
            p.fill_rect(
                0.0,
                0.0,
                self.max_w as f32,
                self.max_h as f32,
                crate::gfx::hex("#2e3031"),
            );
            p.set_alpha(1.0);
        }
    }

    // ---- input ---------------------------------------------------------

    fn read_input(&mut self, input: &FrameInput, gfx: &mut Gfx) {
        // Scripted input (tests and comparisons) replaces the keyboard.
        let mut down: HashSet<KeyCode> = input.down.clone();
        let mut pressed = input.pressed.clone();
        let mut released = input.released.clone();
        let mut scripted_fire = None;
        if !self.script.is_empty() && self.game_start && !self.me().dead {
            if self.script_at == 0.0 {
                self.script_at = self.current_time;
            }
            let mut t = self.current_time - self.script_at;
            let mut keys_now: Vec<KeyCode> = Vec::new();
            for (keys, ms) in &self.script {
                if t < *ms {
                    keys_now.clone_from(keys);
                    break;
                }
                t -= ms;
            }
            let now: HashSet<KeyCode> = keys_now.into_iter().collect();
            pressed = now.difference(&self.key_map).copied().collect();
            released = self.key_map.difference(&now).copied().collect();
            scripted_fire = Some(now.contains(&FIRE));
            down = now;
        }
        let _ = &down;
        if self.chat_input.is_some() {
            return;
        }
        let me_dead = self.me().dead;
        for k in &pressed {
            self.key_map.insert(*k);
            match k {
                KeyCode::Escape if self.game_start => self.show_esc_menu(),
                KeyCode::W if !self.keys.u => {
                    self.keys.u = !self.key_map.contains(&KeyCode::S);
                    self.keys.d = false;
                }
                KeyCode::S if !self.keys.d => {
                    self.keys.u = false;
                    self.keys.d = !self.key_map.contains(&KeyCode::W);
                }
                KeyCode::A if !self.keys.l => {
                    self.keys.l = !self.key_map.contains(&KeyCode::D);
                    self.keys.r = false;
                }
                KeyCode::D if !self.keys.r => {
                    self.keys.l = false;
                    self.keys.r = !self.key_map.contains(&KeyCode::A);
                }
                KeyCode::Enter if self.settings.show_chat && self.game_start => {
                    self.chat_input = Some(String::new());
                }
                _ => {}
            }
            if self.key_map.contains(&KeyCode::Space) && !self.keys.s {
                self.keys.s = true;
            }
            if self.key_map.contains(&KeyCode::R) && !self.keys.rl {
                self.keys.rl = true;
            }
            if self.key_map.contains(&KeyCode::LeftShift)
                && self.game_start
                && !self.showing_scoreboard
                && !me_dead
                && !self.game_over
            {
                self.showing_scoreboard = true;
                self.stat_table = true;
            }
        }
        for k in &released {
            self.key_map.remove(k);
            match k {
                KeyCode::W => {
                    self.keys.u = false;
                    self.keys.d = self.key_map.contains(&KeyCode::S);
                }
                KeyCode::S => {
                    self.keys.u = self.key_map.contains(&KeyCode::W);
                    self.keys.d = false;
                }
                KeyCode::A => {
                    self.keys.l = false;
                    self.keys.r = self.key_map.contains(&KeyCode::D);
                }
                KeyCode::D => {
                    self.keys.l = self.key_map.contains(&KeyCode::A);
                    self.keys.r = false;
                }
                KeyCode::Space => self.keys.s = false,
                KeyCode::R => self.keys.rl = false,
                KeyCode::E | KeyCode::Q => {
                    if let Some(i) = self
                        .me
                        .and_then(|m| self.players.iter().position(|p| p.index == m))
                    {
                        self.swap_weapon(i, if *k == KeyCode::E { 1.0 } else { -1.0 });
                    }
                }
                KeyCode::F => self.emit("crtSpr", vec![]),
                KeyCode::LeftShift if self.showing_scoreboard && !me_dead && !self.game_over => {
                    self.hide_stat_table(gfx);
                }
                _ => {}
            }
        }
        if input.in_game_area {
            if input.mouse_pressed {
                self.keys.lm = true;
            }
            if input.mouse_released {
                self.keys.lm = false;
            }
            if input.wheel != 0.0 {
                self.user_scroll = input.wheel.clamp(-1.0, 1.0);
            }
        }
        if !input.mouse_down {
            self.keys.lm = false;
        }
        if let Some(fire) = scripted_fire {
            self.keys.lm = fire;
        }
        if input.mouse_moved {
            self.aim(input);
        }
    }

    /// KRP `gameInput` (mousemove), in CSS pixels like the browser.
    fn aim(&mut self, input: &FrameInput) {
        let b = self.me().weapon().map_or(0.0, |w| w.spec.y_offset);
        let (mx, my) = input.mouse;
        let (w, h) = input.css_size;
        let last_angle = self.target.f;
        let last_dist = self.target.d;
        let mut d = ((my - (h / 2.0 - b / 2.0)).powi(2) + (mx - w / 2.0).powi(2)).sqrt();
        d *= (self.max_w / w).min(self.max_h / h);
        let f = (h / 2.0 - b / 2.0 - my).atan2(w / 2.0 - mx);
        self.target.f = round_to(f, 2);
        self.target.d = round_to(d, 2);
        self.target.d_offset = round_to(self.target.d / 4.0, 1);
        if last_angle != self.target.f || last_dist != self.target.d {
            self.target_changed = true;
        }
    }

    /// KRP `hideStatTable`.
    pub fn hide_stat_table(&mut self, gfx: &mut Gfx) {
        self.overlay_alpha = 0.0;
        self.showing_scoreboard = false;
        self.animate_overlay = true;
        // KRP also draws the overlay here, between frames; the next frame
        // covers it, so only the alpha change matters.
        let _ = gfx;
        self.stat_table = false;
    }

    // ---- weapons -------------------------------------------------------

    /// KRP `playerSwapWeapon`.
    fn swap_weapon(&mut self, i: usize, change: f64) {
        let plr = &mut self.players[i];
        if plr.dead || plr.weapons.is_empty() {
            return;
        }
        let n = plr.weapons.len() as i64;
        let mut c = plr.current_weapon as i64 + change as i64;
        if c < 0 {
            c = n - 1;
        }
        if c >= n {
            c = 0;
        }
        plr.current_weapon = c as usize;
        self.emit("sw", vec![json!(c)]);
    }

    /// KRP `playerReload`.
    fn reload(&mut self, i: usize, gfx: &mut Gfx) {
        let cw = self.players[i].current_weapon;
        let Some(w) = self.players[i].weapon_mut() else {
            return;
        };
        if w.reload_time <= 0.0 && w.ammo != w.max_ammo {
            w.reload_time = w.spec.reload_speed;
            w.spread_index = 0;
            let rt = w.reload_time;
            self.notify("Reloading", gfx);
            self.emit("r", vec![]);
            self.set_cooldown(cw, rt);
        }
    }

    fn set_cooldown(&mut self, slot: usize, ms: f64) {
        if self.cooldowns.len() <= slot {
            self.cooldowns.resize(slot + 1, (0.0, 0.0));
        }
        self.cooldowns[slot] = (self.current_time, ms);
    }

    /// KRP `getNextBullet`.
    fn next_bullet(&mut self) -> Option<usize> {
        if self.bullets.is_empty() {
            return None;
        }
        self.bullet_index += 1;
        if self.bullet_index >= self.bullets.len() {
            self.bullet_index = 0;
        }
        Some(self.bullet_index)
    }

    /// KRP `shootNextBullet` for player `pi` into bullet `bi`.
    fn arm_bullet(&mut self, bi: usize, pi: usize, shot: Shot) {
        let plr = &self.players[pi];
        let Some(w) = plr.weapon() else { return };
        let rand_scale = w.spec.b_rand_scale.map(|r| random_float(r[0], r[1]));
        let spread_now = w.spec.spread.get(w.spread_index).copied().unwrap_or(0.0);
        let owner = Owner {
            index: plr.index,
            team: plr.team.clone(),
            height: plr.height,
        };
        let target_d = self.target.d;
        let now = self.current_time;
        let jump_y = plr.jump_y;
        let spec = w.spec.clone();
        let (trail, gw, gh) = (w.b_trail, w.glow_width, w.glow_height);
        let b = &mut self.bullets[bi];
        b.p.shoot(
            shot, &spec, spread_now, owner, jump_y, target_d, now, rand_scale,
        );
        b.trail_width = b.p.width * 0.7;
        b.trail_alpha = trail;
        b.glow_width = gw.unwrap_or(0.0);
        b.glow_height = gh.unwrap_or(0.0);
    }

    /// KRP `shootBullet`.
    fn shoot(&mut self, i: usize, gfx: &mut Gfx) {
        let target = self.target;
        let now = self.current_time;
        let plr = &self.players[i];
        let Some(w) = plr.weapon() else { return };
        if plr.dead || plr.is_spawn_protected || w.reload_time > 0.0 || w.ammo <= 0.0 {
            return;
        }
        let shake = w.shake;
        self.fx.shake.start(shake, target.f);
        let per_shot = w.spec.bullets_per_shot;
        for _ in 0..per_shot {
            let plr = &mut self.players[i];
            let (px, py, pj) = (plr.x, plr.y, plr.jump_y);
            let Some(w) = plr.weapon_mut() else { return };
            w.spread_index += 1;
            if w.spread_index >= w.spec.spread.len() {
                w.spread_index = 0;
            }
            let spread = w.spec.spread.get(w.spread_index).copied().unwrap_or(0.0);
            let spread = round_to(target.f + PI + spread, 2);
            let muzzle = w.spec.hold_dist + w.spec.b_dist;
            let x = (px + muzzle * spread.cos()).round();
            let y = (py - w.spec.y_offset - pj + muzzle * spread.sin()).round();
            let Some(bi) = self.next_bullet() else { break };
            let si = self.bullets[bi].p.server_index;
            self.arm_bullet(
                bi,
                i,
                Shot {
                    x,
                    y,
                    dir: spread,
                    server_index: si,
                },
            );
        }
        let plr = &self.players[i];
        let (x, y, jy) = (plr.x, plr.y, plr.jump_y);
        self.emit(
            "1",
            vec![
                json!(x),
                json!(y),
                json!(jy),
                json!(target.f),
                json!(target.d),
                json!(now),
            ],
        );
        let plr = &mut self.players[i];
        let Some(w) = plr.weapon_mut() else { return };
        w.last_shot = now;
        w.ammo -= 1.0;
        if w.ammo <= 0.0 {
            self.reload(i, gfx);
        }
    }

    // ---- small helpers for event code --------------------------------

    pub fn notify(&mut self, text: &str, gfx: &mut Gfx) {
        let screen = self.screen();
        let mut restore = gfx.restorer();
        self.anim.notify(text, screen, &mut gfx.text, &mut restore);
        gfx.painter.invalidate();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn big_text(
        &mut self,
        gfx: &mut Gfx,
        text: &str,
        secondary: &str,
        delay: f64,
        do_scale: bool,
        color: &str,
        secondary_color: &str,
        removable: bool,
        size_mult: f64,
    ) {
        let screen = self.screen();
        let mut restore = gfx.restorer();
        self.anim.big(
            text,
            secondary,
            delay,
            do_scale,
            color,
            secondary_color,
            removable,
            size_mult,
            screen,
            &mut gfx.text,
            &mut restore,
        );
        gfx.painter.invalidate();
    }

    pub fn moving_text(
        &mut self,
        gfx: &mut Gfx,
        text: &str,
        x: f64,
        y: f64,
        color: &str,
        extra: f64,
    ) {
        let screen = self.screen();
        let mut restore = gfx.restorer();
        self.anim.moving(
            text,
            x,
            y,
            color,
            extra,
            screen,
            &mut gfx.text,
            &mut restore,
        );
        gfx.painter.invalidate();
    }

    #[must_use]
    pub fn view(&self) -> View {
        View {
            start_x: self.start_x,
            start_y: self.start_y,
            max_w: self.max_w,
            max_h: self.max_h,
        }
    }

    /// Builds the round's map from `gameSetup`'s `mapData`.
    fn setup_map(map_data: &Value, tile_scale: f64) -> Option<MapState> {
        let mode: GameMode = serde_json::from_value(map_data.get("gameMode")?.clone()).ok()?;
        let gen_data = map_data.get("genData")?;
        let map = Map::from_gen_data(gen_data).ok()?;
        let rgb = world::gen_data_rgb(gen_data)?;
        let mut never = |lo: i64, _hi: i64| if lo == 0 { 1 } else { lo };
        let mut world = World::new(&map, tile_scale, &mode.sim(), &mut never);
        world.clutter = serde_json::from_value::<Vec<Clutter>>(
            map_data.get("clutter").cloned().unwrap_or(json!([])),
        )
        .unwrap_or_default();
        let pickups: Vec<Pickup> =
            serde_json::from_value(map_data.get("pickups").cloned().unwrap_or(json!([])))
                .unwrap_or_default();
        let (tiles, flags) = world::setup_map(&map, &rgb, tile_scale, &mode);
        let walls = tiles
            .iter()
            .filter(|t| t.wall && t.has_collision)
            .map(|t| WallBox {
                x: t.x,
                y: t.y,
                scale: t.scale,
            })
            .collect();
        Some(MapState {
            width: (map.width as f64 - 4.0) * tile_scale,
            height: (map.height as f64 - 4.0) * tile_scale,
            tiles,
            world,
            flags,
            pickups,
            tile_scale,
            mode,
            walls,
        })
    }

    /// KRP `receiveServerData`'s replay of unacknowledged inputs.
    fn reconcile(&mut self) {
        let Some(me) = self.me else { return };
        let Some(i) = self.players.iter().position(|p| p.index == me) else {
            return;
        };
        if self.players[i].dead || self.game_over || self.this_input.len() > 80 {
            self.this_input.clear();
        }
        if self.players[i].dead {
            return;
        }
        let acked = self.players[i].isn;
        self.this_input.retain(|inp| inp.isn > acked);
        let world = self.map.as_ref().map(|m| &m.world);
        let plr = &mut self.players[i];
        for inp in &self.this_input {
            let (mut h, mut v) = (inp.hdt, inp.vdt);
            let m = (h * h + v * v).sqrt();
            if m != 0.0 {
                h /= m;
                v /= m;
            }
            plr.old_x = plr.x;
            plr.old_y = plr.y;
            plr.x += h * plr.speed * inp.delta;
            plr.y += v * plr.speed * inp.delta;
            wall_col(plr, world);
        }
        plr.x = plr.x.round();
        plr.y = plr.y.round();
    }
}

/// What the platform layer saw this frame.
#[derive(Debug, Clone, Default)]
pub struct FrameInput {
    pub down: HashSet<KeyCode>,
    pub pressed: HashSet<KeyCode>,
    pub released: HashSet<KeyCode>,
    /// Mouse position in CSS pixels.
    pub mouse: (f64, f64),
    pub mouse_moved: bool,
    pub mouse_pressed: bool,
    pub mouse_released: bool,
    pub mouse_down: bool,
    pub wheel: f64,
    /// Window size in CSS pixels.
    pub css_size: (f64, f64),
    /// The click was not taken by a menu or HUD element.
    pub in_game_area: bool,
}

/// KRP `wallCol` through the shared crate.
fn wall_col(plr: &mut Player, world: Option<&World>) {
    if plr.dead {
        return;
    }
    let Some(world) = world else { return };
    let mut b = Body {
        x: plr.x,
        y: plr.y,
        old_x: plr.old_x,
        old_y: plr.old_y,
        width: plr.width,
        height: plr.height,
        jump_y: plr.jump_y,
    };
    plr.name_y_offset = world.wall_col(&mut b);
    plr.x = b.x;
    plr.y = b.y;
}

/// KRP `playerJump`.
fn player_jump(plr: &mut Player) {
    if plr.jump_y <= 0.0 {
        plr.jump_delta = plr.jump_strength;
        plr.jump_y = plr.jump_delta;
    }
}

/// KRP `snapAngleToCardinal`.
#[must_use]
pub fn snap_angle(angle: f64) -> i32 {
    (((angle % 360.0) / 90.0).round() * 90.0) as i32
}

/// KRP `isWeaponFacingFront`.
#[must_use]
pub fn is_weapon_facing_front(snapped: i32) -> bool {
    snapped != 180
}

/// KRP `roundNumber`: `+num.toFixed(digits)`.
#[must_use]
pub fn round_to(v: f64, digits: i32) -> f64 {
    let m = 10f64.powi(digits);
    (v * m).round() / m
}

fn clean_name(name: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in name.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.chars().take(25).collect()
}

/// The script's stand-in key for the left mouse button.
const FIRE: KeyCode = KeyCode::Unknown;

/// `w+d:500,space:100,:300`: keys held for a time, in order (`fire`
/// holds the left mouse button).
fn parse_script(s: &str) -> Vec<(Vec<KeyCode>, f64)> {
    s.split(',')
        .filter_map(|step| {
            let (keys, ms) = step.split_once(':')?;
            let ms: f64 = ms.trim().parse().ok()?;
            let keys = keys
                .split('+')
                .filter_map(|k| match k.trim().to_ascii_lowercase().as_str() {
                    "w" => Some(KeyCode::W),
                    "a" => Some(KeyCode::A),
                    "s" => Some(KeyCode::S),
                    "d" => Some(KeyCode::D),
                    "space" => Some(KeyCode::Space),
                    "r" => Some(KeyCode::R),
                    "fire" => Some(FIRE),
                    _ => None,
                })
                .collect();
            Some((keys, ms))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_snap_like_krp() {
        assert_eq!(snap_angle(44.0), 0);
        assert_eq!(snap_angle(46.0), 90);
        assert_eq!(snap_angle(359.0), 360);
        assert!(!is_weapon_facing_front(snap_angle(180.0)));
    }

    #[test]
    fn rounding_matches_to_fixed() {
        assert!((round_to(1.005_1, 2) - 1.01).abs() < 1e-9);
        assert!((round_to(-0.333, 1) + 0.3).abs() < 1e-9);
    }

    #[test]
    fn names_lose_tags_and_length() {
        assert_eq!(clean_name("<b>hi</b>"), "hi");
        assert_eq!(clean_name(&"x".repeat(40)).len(), 25);
    }

    #[test]
    fn scripts_parse() {
        let s = parse_script("w+d:500,:200");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].0, vec![KeyCode::W, KeyCode::D]);
        assert!(s[1].0.is_empty());
        assert_eq!(parse_script("s+fire:100")[0].0, vec![KeyCode::S, FIRE]);
    }
}
