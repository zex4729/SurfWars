//! SurfWars: a Counter-Strike 1.6 style surf combat game.

mod audio;
mod bot;
mod collision;
mod editor;
mod game;
mod hud;
mod map;
mod mapfile;
mod pmove;
mod render;
mod util;
mod weapons;

#[cfg(test)]
mod tests;

use macroquad::prelude::*;

use crate::audio::Audio;
use crate::bot::Difficulty;
use crate::game::{Game, Mode, Settings, TICK, TICK_MSEC};
use crate::hud::{text, text_centered, text_shadow, text_width, HUD_COLOR};
use crate::map::Team;
use crate::pmove::*;
use crate::render::{Renderer, View};
use crate::util::{angle_norm, horizontal};
use crate::weapons::{Slot, ALL_BUYABLE};

fn window_conf() -> macroquad::conf::Conf {
    macroquad::conf::Conf {
        miniquad_conf: miniquad::conf::Conf {
            window_title: "SurfWars".to_owned(),
            window_width: 1280,
            window_height: 720,
            high_dpi: false,
            sample_count: 4,
            window_resizable: true,
            ..Default::default()
        },
        draw_call_vertex_capacity: 20000,
        draw_call_index_capacity: 60000,
        ..Default::default()
    }
}

/// Command line options, mostly for automated screenshots.
#[derive(Clone, Debug, Default)]
struct Args {
    shot: Option<String>,
    after: f32,
    cam: Option<String>,
    follow: Option<usize>,
    map: Option<String>,
    menu: bool,
    no_audio: bool,
    team: Option<Team>,
    look: Option<(f32, f32)>,
    /// Free camera position for screenshots (with --look).
    at: Option<Vec3>,
    ui: Option<String>,
}

fn parse_args() -> Args {
    let mut a = Args { after: 3.0, ..Default::default() };
    let v: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < v.len() {
        let next = v.get(i + 1).cloned();
        match v[i].as_str() {
            "--shot" => {
                a.shot = next;
                i += 1;
            }
            "--after" => {
                a.after = next.and_then(|s| s.parse().ok()).unwrap_or(3.0);
                i += 1;
            }
            "--cam" => {
                a.cam = next;
                i += 1;
            }
            "--follow" => {
                a.follow = next.and_then(|s| s.parse().ok());
                i += 1;
            }
            "--map" => {
                a.map = next;
                i += 1;
            }
            "--team" => {
                a.team = match next.as_deref() {
                    Some("ct") => Some(Team::CT),
                    _ => Some(Team::T),
                };
                i += 1;
            }
            "--look" => {
                if let Some(s) = next {
                    let p: Vec<f32> = s.split(',').filter_map(|x| x.parse().ok()).collect();
                    if p.len() == 2 {
                        a.look = Some((p[0], p[1]));
                    }
                }
                i += 1;
            }
            "--ui" => {
                a.ui = next;
                i += 1;
            }
            "--at" => {
                if let Some(s) = next {
                    let p: Vec<f32> = s.split(',').filter_map(|x| x.parse().ok()).collect();
                    if p.len() == 3 {
                        a.at = Some(vec3(p[0], p[1], p[2]));
                    }
                }
                i += 1;
            }
            "--menu" => a.menu = true,
            "--no-audio" | "--mute" => a.no_audio = true,
            _ => {}
        }
        i += 1;
    }
    a
}

// ---------------------------------------------------------------------------
// Persistent options

#[derive(Clone, Debug)]
struct Options {
    settings: Settings,
    team_choice: usize, // 0 T, 1 CT, 2 auto, 3 spectate
    sensitivity: f32,
    volume: f32,
}

impl Options {
    fn path() -> std::path::PathBuf {
        std::path::PathBuf::from("surfwars.cfg")
    }

    fn load() -> Options {
        let mut o = Options { settings: Settings::default(), team_choice: 2, sensitivity: 2.5, volume: 0.7 };
        if let Ok(s) = std::fs::read_to_string(Self::path()) {
            for line in s.lines() {
                let mut it = line.splitn(2, ' ');
                let (Some(k), Some(v)) = (it.next(), it.next()) else { continue };
                let v = v.trim().trim_matches('"');
                match k {
                    "sensitivity" => o.sensitivity = v.parse().unwrap_or(o.sensitivity),
                    "volume" => o.volume = v.parse().unwrap_or(o.volume),
                    "name" => o.settings.player_name = v.to_string(),
                    "map" => o.settings.map = v.to_string(),
                    "bots_t" => o.settings.bots_t = v.parse().unwrap_or(o.settings.bots_t),
                    "bots_ct" => o.settings.bots_ct = v.parse().unwrap_or(o.settings.bots_ct),
                    "team" => o.team_choice = v.parse().unwrap_or(o.team_choice),
                    "autobhop" => o.settings.vars.autobhop = v == "1",
                    "bhop_cap" => o.settings.vars.bhop_cap = v == "1",
                    "sv_airaccelerate" => o.settings.vars.airaccelerate = v.parse().unwrap_or(100.0),
                    "sv_gravity" => o.settings.vars.gravity = v.parse().unwrap_or(800.0),
                    "sv_maxvelocity" => o.settings.vars.maxvelocity = v.parse().unwrap_or(3500.0),
                    "sv_accelerate" => o.settings.vars.accelerate = v.parse().unwrap_or(5.0),
                    "sv_friction" => o.settings.vars.friction = v.parse().unwrap_or(4.0),
                    "ramp_climb" => o.settings.vars.ramp_climb = v.parse().unwrap_or(100.0),
                    "ramp_accuracy" => o.settings.ramp_accuracy = v == "1",
                    "deathmatch" => o.settings.mode = if v == "1" { Mode::Deathmatch } else { Mode::Rounds },
                    "difficulty" => {
                        o.settings.difficulty = match v {
                            "easy" => Difficulty::Easy,
                            "hard" => Difficulty::Hard,
                            "expert" => Difficulty::Expert,
                            _ => Difficulty::Normal,
                        }
                    }
                    _ => {}
                }
            }
        }
        o
    }

    fn save(&self) {
        let s = &self.settings;
        let text = format!(
            "sensitivity {}\nvolume {}\nname \"{}\"\nmap {}\nbots_t {}\nbots_ct {}\nteam {}\nautobhop {}\ndeathmatch {}\ndifficulty {}\nramp_accuracy {}\nbhop_cap {}\nsv_airaccelerate {}\nsv_gravity {}\nsv_maxvelocity {}\nsv_accelerate {}\nsv_friction {}\nramp_climb {}\n",
            self.sensitivity,
            self.volume,
            s.player_name,
            s.map,
            s.bots_t,
            s.bots_ct,
            self.team_choice,
            if s.vars.autobhop { 1 } else { 0 },
            if s.mode == Mode::Deathmatch { 1 } else { 0 },
            s.difficulty.name().to_lowercase(),
            if s.ramp_accuracy { 1 } else { 0 },
            if s.vars.bhop_cap { 1 } else { 0 },
            s.vars.airaccelerate,
            s.vars.gravity,
            s.vars.maxvelocity,
            s.vars.accelerate,
            s.vars.friction,
            s.vars.ramp_climb,
        );
        let _ = std::fs::write(Self::path(), text);
    }
}

// ---------------------------------------------------------------------------
// Tiny immediate mode UI

struct Ui {
    clicked: bool,
    mouse: Vec2,
    scale: f32,
}

impl Ui {
    fn new() -> Ui {
        let (x, y) = mouse_position();
        Ui { clicked: is_mouse_button_pressed(MouseButton::Left), mouse: vec2(x, y), scale: screen_height() / 720.0 }
    }

    fn button(&self, label: &str, x: f32, y: f32, w: f32, h: f32) -> bool {
        let hover = self.mouse.x >= x && self.mouse.x <= x + w && self.mouse.y >= y && self.mouse.y <= y + h;
        let bg = if hover { Color::new(1.0, 0.69, 0.1, 0.35) } else { Color::new(0.0, 0.0, 0.0, 0.55) };
        draw_rectangle(x, y, w, h, bg);
        draw_rectangle_lines(x, y, w, h, 2.0, Color::new(1.0, 0.69, 0.1, if hover { 0.9 } else { 0.4 }));
        let size = h * 0.55;
        let tw = text_width(label, size);
        text_shadow(label, x + w * 0.5 - tw * 0.5, y + h * 0.5 + size * 0.33, size, WHITE);
        hover && self.clicked
    }
}

/// A number being typed into one of the settings boxes.
#[derive(Clone, Debug, Default)]
struct NumEdit {
    field: Option<usize>,
    text: String,
}

/// The surf movement settings: presets, typed values and toggles.
/// Returns true if anything changed.
fn movement_panel(ui: &Ui, vars: &mut MoveVars, edit: &mut NumEdit, x: f32, y: f32, w: f32) -> bool {
    let s = ui.scale;
    let h = 32.0 * s;
    let step = h * 1.16;
    let rows = 11.0;
    draw_rectangle(x - 10.0 * s, y - 40.0 * s, w + 20.0 * s, step * rows + 56.0 * s, Color::new(0.0, 0.0, 0.0, 0.6));
    text_shadow("Movement settings", x, y - 12.0 * s, 24.0 * s, HUD_COLOR);
    let mut changed = false;
    let mut yy = y;
    let presets: [(&str, MoveVars); 3] = [
        ("Surf server", MoveVars::surf_server()),
        ("Easy surf", MoveVars::easy_surf()),
        ("CS stock", MoveVars::stock()),
    ];
    let current = presets
        .iter()
        .find(|(_, p)| {
            p.airaccelerate == vars.airaccelerate
                && p.gravity == vars.gravity
                && p.maxvelocity == vars.maxvelocity
                && p.accelerate == vars.accelerate
                && p.friction == vars.friction
                && p.bhop_cap == vars.bhop_cap
                && p.ramp_climb == vars.ramp_climb
        })
        .map(|(n, _)| *n)
        .unwrap_or("Custom");
    if ui.button(&format!("Preset: {current}"), x, yy, w, h) {
        let i = presets.iter().position(|(n, _)| *n == current).map(|i| i + 1).unwrap_or(0) % presets.len();
        let auto = vars.autobhop;
        *vars = presets[i].1;
        if presets[i].0 != "Easy surf" {
            vars.autobhop = auto;
        }
        edit.field = None;
        changed = true;
    }
    yy += step;

    // Typed values: click a box, type, Enter (or Tab for the next box).
    let fields: [(&str, f32, f32); 6] = [
        ("sv_airaccelerate", 0.0, 10000.0),
        ("sv_gravity", 50.0, 4000.0),
        ("sv_maxvelocity", 100.0, 20000.0),
        ("sv_accelerate", 0.0, 100.0),
        ("sv_friction", 0.0, 20.0),
        ("Ramp climb (surf up)", 30.0, 3000.0),
    ];
    let get = |v: &MoveVars, i: usize| match i {
        0 => v.airaccelerate,
        1 => v.gravity,
        2 => v.maxvelocity,
        3 => v.accelerate,
        4 => v.friction,
        _ => v.ramp_climb,
    };
    let commit = |v: &mut MoveVars, i: usize, text: &str| -> bool {
        let Ok(x) = text.trim().parse::<f32>() else { return false };
        let x = x.clamp(fields[i].1, fields[i].2);
        let slot = match i {
            0 => &mut v.airaccelerate,
            1 => &mut v.gravity,
            2 => &mut v.maxvelocity,
            3 => &mut v.accelerate,
            4 => &mut v.friction,
            _ => &mut v.ramp_climb,
        };
        let c = *slot != x;
        *slot = x;
        c
    };
    // keyboard input for the focused box
    if let Some(f) = edit.field {
        while let Some(c) = get_char_pressed() {
            if (c.is_ascii_digit() || c == '.' || c == '-') && edit.text.len() < 9 {
                edit.text.push(c);
            }
        }
        if is_key_pressed(KeyCode::Backspace) {
            edit.text.pop();
        }
        if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
            changed |= commit(vars, f, &edit.text);
            edit.field = None;
        } else if is_key_pressed(KeyCode::Tab) {
            changed |= commit(vars, f, &edit.text);
            let n = (f + 1) % fields.len();
            edit.field = Some(n);
            edit.text = format!("{}", get(vars, n));
        }
    }
    let bw = w * 0.36;
    for (i, (label, lo, hi)) in fields.iter().enumerate() {
        text_shadow(label, x, yy + h * 0.68, 19.0 * s, WHITE);
        let bx = x + w - bw;
        let hover = ui.mouse.x >= bx && ui.mouse.x <= bx + bw && ui.mouse.y >= yy && ui.mouse.y <= yy + h;
        let focused = edit.field == Some(i);
        draw_rectangle(bx, yy, bw, h, Color::new(0.0, 0.0, 0.0, if focused { 0.8 } else { 0.5 }));
        let border = if focused {
            1.0
        } else if hover {
            0.8
        } else {
            0.4
        };
        draw_rectangle_lines(bx, yy, bw, h, 2.0, Color::new(1.0, 0.69, 0.1, border));
        let shown = if focused {
            let blink = (get_time() * 2.0) as i64 % 2 == 0;
            format!("{}{}", edit.text, if blink { "_" } else { " " })
        } else {
            format!("{}", get(vars, i))
        };
        text(&shown, bx + 8.0 * s, yy + h * 0.68, 19.0 * s, if focused { HUD_COLOR } else { WHITE });
        if hover {
            text(&format!("{lo} - {hi}"), bx - 110.0 * s, yy + h * 0.68, 13.0 * s, GRAY);
        }
        if ui.clicked && hover && !focused {
            if let Some(f) = edit.field {
                changed |= commit(vars, f, &edit.text);
            }
            edit.field = Some(i);
            edit.text = format!("{}", get(vars, i));
            while get_char_pressed().is_some() {}
        }
        yy += step;
    }
    // clicking anywhere else applies the value being typed
    if ui.clicked {
        if let Some(f) = edit.field {
            let bx = x + w - bw;
            let fy = y + step * (f as f32 + 1.0);
            let inside = ui.mouse.x >= bx && ui.mouse.x <= bx + bw && ui.mouse.y >= fy && ui.mouse.y <= fy + h;
            if !inside {
                changed |= commit(vars, f, &edit.text);
                edit.field = None;
            }
        }
    }
    if ui.button(&format!("Auto bunnyhop: {}", if vars.autobhop { "On" } else { "Off" }), x, yy, w, h) {
        vars.autobhop = !vars.autobhop;
        changed = true;
    }
    yy += step;
    if ui.button(&format!("Bunnyhop speed cap: {}", if vars.bhop_cap { "On (stock CS)" } else { "Off" }), x, yy, w, h) {
        vars.bhop_cap = !vars.bhop_cap;
        changed = true;
    }
    yy += step;
    for l in [
        "Higher airaccelerate and lower gravity make ramps more forgiving.",
        "Ramp climb: 30 is stock CS; higher lets you surf up ramps and fly high.",
    ] {
        text(l, x, yy + 14.0 * s, 14.0 * s, Color::new(0.85, 0.85, 0.85, 1.0));
        yy += 18.0 * s;
    }
    changed
}

// ---------------------------------------------------------------------------
// App

#[derive(PartialEq, Eq, Clone, Copy)]
enum Screen {
    Menu,
    Playing,
    Editor,
}

struct App {
    screen: Screen,
    opts: Options,
    game: Game,
    renderer: Renderer,
    audio: Audio,
    acc: f32,
    view: Vec3,
    last_mouse: Vec2,
    paused: bool,
    third_person: bool,
    buy_menu: bool,
    num_edit: NumEdit,
    /// Attachment inventory screen.
    inventory: bool,
    inv_tab: usize,
    inv_slot: Slot,
    show_move_panel: bool,
    menu_map_changed: bool,
    editor: Option<editor::Editor>,
    /// Test playing a map from the editor.
    from_editor: bool,
    spec_target: usize,
    spec_first_person: bool,
    wheel_jumps: u32,
    wheel_pressed_last: bool,
    menu_cam_target: usize,
    menu_cam_switch: f64,
    fps: i32,
    fps_acc: f32,
    fps_frames: i32,
    was_alive: bool,
    args: Args,
}

fn team_from_choice(c: usize, rng_seed: u64) -> Option<Team> {
    match c {
        0 => Some(Team::T),
        1 => Some(Team::CT),
        2 => Some(if rng_seed.is_multiple_of(2) { Team::T } else { Team::CT }),
        _ => None,
    }
}

fn menu_game(map: &str) -> Game {
    let s = Settings {
        map: map.to_string(),
        player_team: None,
        bots_t: 4,
        bots_ct: 4,
        mode: Mode::Deathmatch,
        ..Default::default()
    };
    let mut g = Game::new(s, 1234);
    // Let the bots get going before anyone looks.
    for _ in 0..600 {
        g.step(None);
    }
    g.events.clear();
    g
}

impl App {
    fn open_editor(&mut self) {
        let name = self.opts.settings.map.clone();
        let ed = editor::Editor::new(Some(&name));
        self.renderer = Renderer::new(&ed.game);
        self.editor = Some(ed);
        self.screen = Screen::Editor;
        self.from_editor = false;
        self.grab(false);
    }

    fn frame_editor(&mut self, dt: f32) {
        let Some(ed) = self.editor.as_mut() else {
            self.go_menu();
            return;
        };
        match ed.frame(&mut self.renderer, dt) {
            editor::EditorAction::None => {}
            editor::EditorAction::Exit => {
                self.editor = None;
                self.go_menu();
            }
            editor::EditorAction::Play(map) => {
                self.start_match_on(Some(*map));
                self.from_editor = true;
            }
        }
    }

    fn back_to_editor(&mut self) {
        if let Some(ed) = self.editor.as_ref() {
            self.renderer = Renderer::new(&ed.game);
        }
        self.screen = Screen::Editor;
        self.paused = false;
        self.grab(false);
    }

    fn new_match(&mut self) {
        self.from_editor = false;
        self.start_match_on(None);
    }

    fn start_match_on(&mut self, map: Option<map::Map>) {
        let mut s = self.opts.settings.clone();
        // Auto team: join the team with fewer players.
        s.player_team = match self.opts.team_choice {
            2 => Some(if s.bots_t <= s.bots_ct { Team::T } else { Team::CT }),
            c => team_from_choice(c, 0),
        };
        if let Some(t) = self.args.team {
            s.player_team = Some(t);
        }
        let seed = (get_time() * 1000.0) as u64 ^ 0x5eed;
        self.game = match map {
            Some(m) => Game::with_map(m, s, seed),
            None => Game::new(s, seed),
        };
        self.renderer = Renderer::new(&self.game);
        self.acc = 0.0;
        self.view = self.game.local_player().map(|p| p.angles).unwrap_or(Vec3::ZERO);
        self.paused = false;
        self.buy_menu = false;
        self.inventory = false;
        self.third_person = false;
        self.spec_target = 0;
        self.was_alive = true;
        self.screen = Screen::Playing;
        self.grab(true);
    }

    fn grab(&mut self, on: bool) {
        set_cursor_grab(on);
        show_mouse(!on);
        let (x, y) = mouse_position();
        self.last_mouse = vec2(x, y);
    }

    fn mouse_look(&mut self) {
        let (x, y) = mouse_position();
        let m = vec2(x, y);
        let d = m - self.last_mouse;
        self.last_mouse = m;
        if d.length() > 2000.0 {
            return; // cursor warped
        }
        let fov = self.game.local_player().filter(|p| p.alive).map(|p| p.fov()).unwrap_or(90.0);
        let zoom_ratio = if fov < 90.0 { 1.2 * fov / 90.0 } else { 1.0 };
        let sens = self.opts.sensitivity * zoom_ratio;
        self.view.y = angle_norm(self.view.y - d.x * sens * 0.022);
        self.view.x = (self.view.x + d.y * sens * 0.022).clamp(-89.0, 89.0);
    }

    fn build_cmd(&mut self) -> UserCmd {
        let mut cmd = UserCmd { msec: TICK_MSEC, viewangles: self.view, ..Default::default() };
        if self.paused {
            return cmd;
        }
        let maxspeed = self.game.local_player().map(|p| p.maxspeed()).unwrap_or(250.0);
        let mut f: f32 = 0.0;
        let mut s: f32 = 0.0;
        if is_key_down(KeyCode::W) {
            f += 400.0;
        }
        if is_key_down(KeyCode::S) {
            f -= 400.0;
        }
        if is_key_down(KeyCode::D) {
            s += 400.0;
        }
        if is_key_down(KeyCode::A) {
            s -= 400.0;
        }
        // clip to maxspeed, then apply the walk key (CS walks at 52%)
        let len = (f * f + s * s).sqrt();
        if len > maxspeed {
            f *= maxspeed / len;
            s *= maxspeed / len;
        }
        if is_key_down(KeyCode::LeftShift) {
            f *= 0.52;
            s *= 0.52;
        }
        cmd.forwardmove = f;
        cmd.sidemove = s;

        let mut jump = is_key_down(KeyCode::Space);
        if self.wheel_jumps > 0 && !self.wheel_pressed_last {
            jump = true;
            self.wheel_jumps -= 1;
            self.wheel_pressed_last = true;
        } else {
            self.wheel_pressed_last = false;
        }
        if jump {
            cmd.buttons |= IN_JUMP;
        }
        if is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::C) {
            cmd.buttons |= IN_DUCK;
        }
        if !self.buy_menu && !self.inventory {
            if is_mouse_button_down(MouseButton::Left) {
                cmd.buttons |= IN_ATTACK;
            }
            if is_mouse_button_down(MouseButton::Right) {
                cmd.buttons |= IN_ATTACK2;
            }
        }
        if is_key_down(KeyCode::R) {
            cmd.buttons |= IN_RELOAD;
        }
        if is_key_down(KeyCode::E) {
            cmd.buttons |= IN_USE;
        }
        cmd
    }

    fn handle_keys(&mut self) {
        let Some(li) = self.game.local else { return };
        let (_, wy) = mouse_wheel();
        if wy != 0.0 && !self.paused {
            self.wheel_jumps = (self.wheel_jumps + 1).min(3);
        }
        if is_key_pressed(KeyCode::V) {
            self.third_person = !self.third_person;
        }
        if is_key_pressed(KeyCode::B) {
            self.buy_menu = !self.buy_menu;
            self.set_inventory(false);
        }
        if is_key_pressed(KeyCode::I) {
            let open = !self.inventory;
            self.set_inventory(open);
            self.buy_menu = false;
            if open {
                self.inv_slot = if self.game.players[li].primary.is_some() { Slot::Primary } else { Slot::Secondary };
            }
        }
        let alive = self.game.players[li].alive;
        let keys = [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4];
        if self.inventory {
            for (i, k) in keys.iter().enumerate() {
                if is_key_pressed(*k) {
                    self.inv_tab = i;
                }
            }
            return;
        }
        if self.buy_menu {
            for (i, id) in ALL_BUYABLE.iter().enumerate() {
                if is_key_pressed(keys[i]) && self.game.buy(li, *id) {
                    self.buy_menu = false;
                }
            }
            if is_key_pressed(KeyCode::Key0) {
                self.buy_menu = false;
            }
            return;
        }
        if !alive {
            return;
        }
        if is_key_pressed(KeyCode::Key1) {
            self.game.switch_weapon(li, Slot::Primary);
        }
        if is_key_pressed(KeyCode::Key2) {
            self.game.switch_weapon(li, Slot::Secondary);
        }
        if is_key_pressed(KeyCode::Key3) {
            self.game.switch_weapon(li, Slot::Melee);
        }
        if is_key_pressed(KeyCode::Q) {
            self.game.last_weapon(li);
        }
        if is_key_pressed(KeyCode::G) {
            self.game.drop_weapon(li);
        }
    }

    fn pick_spec_target(&mut self, dir: i32) {
        let n = self.game.players.len();
        if n == 0 {
            return;
        }
        let local_team = self.game.local_player().map(|p| p.team);
        for k in 1..=n {
            let i = ((self.spec_target as i32 + dir * k as i32).rem_euclid(n as i32)) as usize;
            let p = &self.game.players[i];
            let ok = p.alive && Some(i) != self.game.local;
            // Prefer teammates like CS does, but fall back to anyone.
            if ok && (local_team.is_none() || Some(p.team) == local_team || k == n) {
                self.spec_target = i;
                return;
            }
        }
        for k in 1..=n {
            let i = ((self.spec_target as i32 + dir * k as i32).rem_euclid(n as i32)) as usize;
            if self.game.players[i].alive && Some(i) != self.game.local {
                self.spec_target = i;
                return;
            }
        }
    }

    fn simulate(&mut self, dt: f32) {
        if self.paused {
            self.acc = 0.0;
            return;
        }
        self.acc += dt.min(0.25);
        let mut steps = 0;
        while self.acc >= TICK && steps < 25 {
            let cmd = if self.game.local.is_some() { Some(self.build_cmd()) } else { None };
            self.game.step(cmd);
            let events = std::mem::take(&mut self.game.events);
            for e in &events {
                if let game::Event::Teleport { player } = e {
                    if Some(*player) == self.game.local {
                        // like trigger_teleport: face the destination's direction
                        self.view = vec3(0.0, self.game.players[*player].angles.y, 0.0);
                    }
                }
            }
            let listener = self.listener_pos();
            self.renderer.handle_events(&self.game, &events);
            self.audio.handle_events(&self.game, &events, listener, self.game.local);
            self.acc -= TICK;
            steps += 1;
        }
        if steps == 25 {
            self.acc = 0.0;
        }
    }

    fn listener_pos(&self) -> Vec3 {
        match self.pov() {
            Some(i) => self.game.players[i].pm.eye(),
            None => Vec3::ZERO,
        }
    }

    /// Player whose view / HUD is shown.
    fn pov(&self) -> Option<usize> {
        if let Some(li) = self.game.local {
            if self.game.players[li].alive {
                return Some(li);
            }
            // Show the body for a moment after dying.
            if self.game.time - self.game.players[li].death_time < 2.0 {
                return Some(li);
            }
        }
        if self.game.players.get(self.spec_target).is_some_and(|p| p.alive) {
            Some(self.spec_target)
        } else {
            None
        }
    }

    fn chase_view(&self, target: usize, angles: Vec3, dist: f32, alpha: f32) -> View {
        let p = &self.game.players[target];
        let pos = p.prev_origin.lerp(p.pm.origin, alpha) + vec3(0.0, 0.0, p.pm.view_ofs);
        let (f, _, _) = angle_vectors(angles);
        let want = pos - f * dist + vec3(0.0, 0.0, 28.0);
        let tr = self.game.map.world.trace(pos, want, vec3(-6.0, -6.0, -6.0), vec3(6.0, 6.0, 6.0));
        View { pos: tr.endpos, angles, fov: 90.0, first_person: None, viewport: None }
    }

    fn compute_view(&mut self, alpha: f32) -> (View, bool) {
        if let Some(pos) = self.args.at {
            let (p, y) = self.args.look.unwrap_or((20.0, 0.0));
            return (View { pos, angles: vec3(p, y, 0.0), fov: 90.0, first_person: None, viewport: None }, false);
        }
        let g = &self.game;
        if let Some(li) = g.local {
            let p = &g.players[li];
            if p.alive {
                if self.third_person {
                    return (self.chase_view(li, self.view, 130.0, alpha), false);
                }
                let pos = p.prev_origin.lerp(p.pm.origin, alpha) + vec3(0.0, 0.0, p.pm.view_ofs);
                let angles = self.view + p.pm.punchangle;
                return (View { pos, angles, fov: p.fov(), first_person: Some(li), viewport: None }, true);
            }
            if g.time - p.death_time < 2.0 {
                // death cam: look at our body from above
                return (self.chase_view(li, vec3(35.0, self.view.y, 0.0), 160.0, alpha), false);
            }
        }
        match self.pov() {
            Some(t) => {
                let p = &g.players[t];
                if self.spec_first_person {
                    let pos = p.prev_origin.lerp(p.pm.origin, alpha) + vec3(0.0, 0.0, p.pm.view_ofs);
                    (
                        View {
                            pos,
                            angles: p.angles + p.pm.punchangle,
                            fov: p.fov(),
                            first_person: Some(t),
                            viewport: None,
                        },
                        true,
                    )
                } else {
                    (self.chase_view(t, self.view, 150.0, alpha), false)
                }
            }
            None => {
                // Overview: look at the whole map from a corner.
                let mut lo = Vec3::splat(f32::MAX);
                let mut hi = Vec3::splat(f32::MIN);
                for br in g.map.world.brushes.iter().filter(|b| b.mat.visible() && (b.maxs - b.mins).x < 9000.0) {
                    lo = lo.min(br.mins);
                    hi = hi.max(br.maxs);
                }
                let c = (lo + hi) * 0.5;
                let e = (hi - lo) * 0.5;
                let pos = vec3(c.x - e.x * 1.05, c.y - e.y * 1.25, hi.z + e.x.max(e.y) * 0.45);
                let (pitch, yaw) = util::vec_to_angles(c - pos);
                (View { pos, angles: vec3(pitch, yaw, 0.0), fov: 90.0, first_person: None, viewport: None }, false)
            }
        }
    }

    fn frame_playing(&mut self, dt: f32) {
        if is_key_pressed(KeyCode::Escape) {
            if self.buy_menu {
                self.buy_menu = false;
            } else if self.inventory {
                self.set_inventory(false);
            } else {
                self.paused = !self.paused;
                self.grab(!self.paused);
            }
        }
        if !self.paused {
            if !self.inventory {
                self.mouse_look();
            }
            self.handle_keys();
        }

        // Spectator controls.
        let local_alive = self.game.local_player().is_some_and(|p| p.alive);
        if !local_alive && !self.paused {
            let need = !self.game.players.get(self.spec_target).is_some_and(|p| p.alive)
                || Some(self.spec_target) == self.game.local;
            if need {
                // follow whoever killed us first
                let killer = self.game.local_player().and_then(|p| p.last_attacker);
                match killer {
                    Some(k) if self.game.players[k].alive && self.was_alive => self.spec_target = k,
                    _ => self.pick_spec_target(1),
                }
            }
            if is_mouse_button_pressed(MouseButton::Left) {
                self.pick_spec_target(1);
            }
            if is_mouse_button_pressed(MouseButton::Right) {
                self.pick_spec_target(-1);
            }
            if is_key_pressed(KeyCode::Space) {
                self.spec_first_person = !self.spec_first_person;
            }
        }
        if local_alive && !self.was_alive {
            // respawned: face the spawn direction
            if let Some(p) = self.game.local_player() {
                self.view = p.angles;
            }
        }
        self.was_alive = local_alive;

        // Respawn view angles at round start.
        let prev_spawn = self.game.local_player().map(|p| p.spawn_count);
        self.simulate(dt);
        if let Some(p) = self.game.local_player() {
            if Some(p.spawn_count) != prev_spawn {
                self.view = p.angles;
            }
        }

        let alpha = (self.acc / TICK).clamp(0.0, 1.0);
        self.draw_playing(alpha);
    }

    fn draw_playing(&mut self, alpha: f32) {
        let (view, fp) = self.compute_view(alpha);
        self.renderer.draw(&self.game, &view, alpha, fp);
        let pov = self.pov();
        self.audio.update_loops(&self.game, pov, self.paused);
        let spectating = self
            .game
            .local
            .is_none_or(|li| !self.game.players[li].alive && self.game.time - self.game.players[li].death_time >= 2.0);
        hud::draw(&hud::HudState {
            game: &self.game,
            pov,
            spectating,
            show_scores: is_key_down(KeyCode::Tab)
                || self.game.phase == game::Phase::Over
                || self.args.ui.as_deref() == Some("scores"),
            buy_menu: self.buy_menu,
            view_angles: if fp { view.angles } else { self.view },
            third_person: !fp,
            fps: self.fps,
        });

        if self.inventory && !self.paused {
            self.draw_inventory();
        }
        if self.paused {
            self.draw_pause();
        }
    }

    fn set_inventory(&mut self, open: bool) {
        if self.inventory != open {
            self.inventory = open;
            self.grab(!open);
        }
    }

    /// The attachment inventory: tabs for the four attachment types, the
    /// items you carry, and the two guns you can fit them to.
    fn draw_inventory(&mut self) {
        use weapons::{AttItem, ATT_CATEGORIES};
        let Some(li) = self.game.local else { return };
        let ui = Ui::new();
        let s = ui.scale;
        let sw = screen_width();
        let sh = screen_height();
        let w = (820.0 * s).min(sw - 20.0);
        let h = 500.0 * s;
        let x = sw * 0.5 - w * 0.5;
        let y = sh * 0.5 - h * 0.5;
        draw_rectangle(x, y, w, h, Color::new(0.0, 0.0, 0.0, 0.72));
        draw_rectangle_lines(x, y, w, h, 2.0, Color::new(1.0, 0.69, 0.1, 0.5));
        text_shadow("INVENTORY", x + 16.0 * s, y + 32.0 * s, 28.0 * s, HUD_COLOR);
        text(
            "I / ESC close   1-4 tabs   click an item to fit it",
            x + w - 380.0 * s,
            y + 28.0 * s,
            15.0 * s,
            Color::new(0.8, 0.8, 0.8, 1.0),
        );

        // Gun selector.
        let p = &self.game.players[li];
        let bh = 34.0 * s;
        let mut yy = y + 48.0 * s;
        let gw = (w - 48.0 * s) / 2.0;
        for (i, slot) in [Slot::Primary, Slot::Secondary].into_iter().enumerate() {
            let name = p.slot_weapon(slot).map(|w| w.def().name).unwrap_or("(empty)");
            let label = format!("{}: {name}", if slot == Slot::Primary { "Primary" } else { "Secondary" });
            let bx = x + 16.0 * s + (gw + 16.0 * s) * i as f32;
            if self.inv_slot == slot {
                draw_rectangle(bx, yy, gw, bh, Color::new(1.0, 0.69, 0.1, 0.3));
            }
            if ui.button(&label, bx, yy, gw, bh) {
                self.inv_slot = slot;
            }
        }
        yy += bh + 12.0 * s;

        // Tabs.
        let tw = (w - 32.0 * s) / 4.0;
        for (i, name) in ATT_CATEGORIES.iter().enumerate() {
            let bx = x + 16.0 * s + tw * i as f32;
            let active = self.inv_tab == i;
            draw_rectangle(bx, yy, tw - 4.0 * s, bh, Color::new(1.0, 0.69, 0.1, if active { 0.45 } else { 0.08 }));
            if ui.button(&format!("{}. {name}", i + 1), bx, yy, tw - 4.0 * s, bh) {
                self.inv_tab = i;
            }
        }
        yy += bh + 10.0 * s;
        draw_line(x + 16.0 * s, yy, x + w - 16.0 * s, yy, 1.0, Color::new(1.0, 0.69, 0.1, 0.5));
        yy += 8.0 * s;

        let p = &self.game.players[li];
        let Some(wp) = p.slot_weapon(self.inv_slot).copied() else {
            text_shadow("No gun in this slot", x + 24.0 * s, yy + 30.0 * s, 22.0 * s, GRAY);
            return;
        };
        let cat = self.inv_tab;
        let mounted = wp.att.get(cat);
        let lw = w * 0.58;
        let row = 40.0 * s;
        // Default part first, then every item of this type.
        let default_name = match cat {
            0 => "Iron sights",
            1 => "No muzzle",
            2 => "Standard stock",
            _ => "No grip",
        };
        let mut entries: Vec<(Option<AttItem>, String, String, usize)> =
            vec![(None, default_name.to_string(), "always available".into(), 1)];
        for it in AttItem::ALL.iter().filter(|a| a.category() == cat) {
            let n = p.inventory.iter().filter(|x| *x == it).count();
            entries.push((Some(*it), it.name().to_string(), it.desc().to_string(), n));
        }
        let mut choose = None;
        for (item, name, desc, count) in entries {
            let on = item == mounted;
            let have = count > 0 || on;
            let bx = x + 16.0 * s;
            let hover =
                ui.mouse.x >= bx && ui.mouse.x <= bx + lw && ui.mouse.y >= yy && ui.mouse.y <= yy + row - 4.0 * s;
            let bg = if on {
                Color::new(1.0, 0.69, 0.1, 0.35)
            } else if hover && have {
                Color::new(1.0, 1.0, 1.0, 0.12)
            } else {
                Color::new(0.0, 0.0, 0.0, 0.3)
            };
            draw_rectangle(bx, yy, lw, row - 4.0 * s, bg);
            let c = item.map(|i| i.color()).unwrap_or([150, 150, 150]);
            let a = if have { 1.0 } else { 0.25 };
            draw_rectangle(
                bx + 6.0 * s,
                yy + 6.0 * s,
                8.0 * s,
                row - 16.0 * s,
                Color::from_rgba(c[0], c[1], c[2], (a * 255.0) as u8),
            );
            let tc = if have { WHITE } else { Color::new(0.5, 0.5, 0.5, 1.0) };
            let mut label = name.clone();
            if item.is_some_and(|i| i.rare()) {
                label += "  (rare)";
            }
            text_shadow(&label, bx + 22.0 * s, yy + 17.0 * s, 18.0 * s, tc);
            text(&desc, bx + 22.0 * s, yy + 32.0 * s, 13.5 * s, Color::new(0.75, 0.75, 0.75, a));
            let right = if on {
                "FITTED".to_string()
            } else if item.is_none() {
                String::new()
            } else if count > 0 {
                format!("x{count}")
            } else {
                "not found".to_string()
            };
            text(&right, bx + lw - 90.0 * s, yy + 24.0 * s, 16.0 * s, if on { HUD_COLOR } else { tc });
            if hover && ui.clicked && have && !on {
                choose = Some(item);
            }
            yy += row;
        }
        if let Some(item) = choose {
            self.game.mount(li, self.inv_slot, cat, item);
        }

        // The gun and its current setup.
        let p = &self.game.players[li];
        let wp = p.slot_weapon(self.inv_slot).copied().unwrap_or(wp);
        let rx = x + 16.0 * s + lw + 20.0 * s;
        let mut ry = y + 48.0 * s + bh * 2.0 + 40.0 * s;
        text_shadow(wp.def().name, rx, ry, 24.0 * s, HUD_COLOR);
        ry += 28.0 * s;
        for (c, cat_name) in ATT_CATEGORIES.iter().enumerate() {
            let n = wp.att.get(c).map(|i| i.name()).unwrap_or(match c {
                0 => "Iron sights",
                1 => "No muzzle",
                2 => "Standard stock",
                _ => "No grip",
            });
            text(&format!("{}: {n}", cat_name.trim_end_matches('s')), rx, ry, 16.0 * s, WHITE);
            ry += 22.0 * s;
        }
        ry += 10.0 * s;
        let m = wp.mods();
        for l in [
            format!("damage x{:.2}", m.damage),
            format!("recoil x{:.2}", m.recoil),
            format!("spread x{:.2}", m.spread),
            format!("speed {:+.0}", m.speed),
            if m.silenced { "silenced".to_string() } else { String::new() },
        ] {
            text(&l, rx, ry, 16.0 * s, Color::new(0.85, 0.85, 0.85, 1.0));
            ry += 20.0 * s;
        }
        ry += 10.0 * s;
        let total = p.inventory.len();
        text(&format!("Carried, not fitted: {total}"), rx, ry, 15.0 * s, GRAY);
        text(
            "Find attachments on the map; the rare ones sit on the sky platforms.",
            x + 16.0 * s,
            y + h - 12.0 * s,
            15.0 * s,
            Color::new(0.8, 0.8, 0.8, 1.0),
        );
    }

    fn draw_pause(&mut self) {
        let sw = screen_width();
        let sh = screen_height();
        draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.5));
        let ui = Ui::new();
        let s = ui.scale;
        text_centered("PAUSED", sw * 0.5, sh * 0.25, 48.0 * s, HUD_COLOR);
        let w = 320.0 * s;
        let h = 44.0 * s;
        let x = sw * 0.5 - w * 0.5;
        let mut y = sh * 0.34;
        if ui.button("Resume", x, y, w, h) {
            self.paused = false;
            self.grab(true);
        }
        y += h * 1.3;
        let label = if let Some(li) = self.game.local {
            format!("Switch to {}", self.game.players[li].team.other().name())
        } else {
            "Join the game".to_string()
        };
        if ui.button(&label, x, y, w, h) {
            match self.game.local {
                Some(li) => {
                    let t = self.game.players[li].team.other();
                    self.game.players[li].team = t;
                    self.game.players[li].alive = false;
                    self.game.players[li].death_time = self.game.time - 10.0;
                    if self.game.settings.mode == Mode::Deathmatch {
                        self.game.players[li].respawn_at = Some(self.game.time + 1.0);
                    }
                }
                None => {
                    self.opts.team_choice = 2;
                    self.new_match();
                    return;
                }
            }
            self.paused = false;
            self.grab(true);
        }
        y += h * 1.3;
        if self.from_editor {
            if ui.button("Back to editor", x, y, w, h) {
                self.back_to_editor();
                return;
            }
        } else if ui.button("Restart match", x, y, w, h) {
            self.new_match();
            return;
        }
        y += h * 1.3;
        if ui.button("Main menu", x, y, w, h) {
            self.go_menu();
            return;
        }
        y += h * 1.3;
        if ui.button("Quit", x, y, w, h) {
            self.opts.save();
            std::process::exit(0);
        }
        y += h * 1.3;
        if ui.button("Movement settings", x, y, w, h) {
            self.show_move_panel = !self.show_move_panel;
        }
        if self.show_move_panel {
            let px = (x + w + 40.0 * s).min(sw - 480.0 * s);
            if movement_panel(&ui, &mut self.opts.settings.vars, &mut self.num_edit, px, sh * 0.3, 440.0 * s) {
                self.game.vars = self.opts.settings.vars;
                self.game.settings.vars = self.opts.settings.vars;
                self.opts.save();
            }
        }
        y += h * 1.8;
        let sens = format!("Sensitivity: {:.1}   (- / + keys)", self.opts.sensitivity);
        text_centered(&sens, sw * 0.5, y, 20.0 * s, WHITE);
        if is_key_pressed(KeyCode::Minus) || is_key_pressed(KeyCode::KpSubtract) {
            self.opts.sensitivity = (self.opts.sensitivity - 0.1).max(0.1);
        }
        if is_key_pressed(KeyCode::Equal) || is_key_pressed(KeyCode::KpAdd) {
            self.opts.sensitivity += 0.1;
        }
    }

    fn go_menu(&mut self) {
        self.opts.save();
        self.screen = Screen::Menu;
        self.paused = false;
        self.game = menu_game(&self.opts.settings.map);
        self.renderer = Renderer::new(&self.game);
        self.grab(false);
    }

    fn frame_menu(&mut self, dt: f32) {
        if self.menu_map_changed {
            self.menu_map_changed = false;
            self.game = menu_game(&self.opts.settings.map);
            self.renderer.rebuild_world(&self.game);
        }
        // Background: a live bot match with a camera chasing a surfer.
        self.simulate(dt);
        let g = &self.game;
        let now = g.time;
        let cur_ok = g.players.get(self.menu_cam_target).is_some_and(|p| p.alive && p.board > 0.2);
        if now > self.menu_cam_switch || !g.players.get(self.menu_cam_target).is_some_and(|p| p.alive) {
            // prefer someone surfing
            let mut best = None;
            for (i, p) in g.players.iter().enumerate() {
                if p.alive && p.board > 0.2 {
                    best = Some(i);
                    if i > self.menu_cam_target {
                        break;
                    }
                }
            }
            if let Some(b) = best {
                if !cur_ok || now > self.menu_cam_switch {
                    self.menu_cam_target = b;
                    self.menu_cam_switch = now + 9.0;
                }
            } else if let Some(i) = g.players.iter().position(|p| p.alive) {
                self.menu_cam_target = i;
            }
        }
        let alpha = (self.acc / TICK).clamp(0.0, 1.0);
        let t = self.menu_cam_target.min(g.players.len().saturating_sub(1));
        let p = &g.players[t];
        let vel = horizontal(p.pm.velocity);
        let yaw = if vel.length() > 50.0 { vel.y.atan2(vel.x).to_degrees() } else { p.angles.y };
        // smooth the orbit
        let target = vec3(12.0, yaw + 25.0 * ((now * 0.2).sin() as f32), 0.0);
        let dy = angle_norm(target.y - self.view.y);
        self.view.y = angle_norm(self.view.y + dy * (dt * 2.0).min(1.0));
        self.view.x += (target.x - self.view.x) * (dt * 2.0).min(1.0);
        let view = self.chase_view(t, self.view, 170.0, alpha);
        self.renderer.draw(&self.game, &view, alpha, false);
        self.audio.update_loops(&self.game, None, true);
        self.draw_menu();
    }

    fn draw_menu(&mut self) {
        let sw = screen_width();
        let sh = screen_height();
        let ui = Ui::new();
        let s = ui.scale;
        draw_rectangle(0.0, 0.0, sw * 0.42, sh, Color::new(0.0, 0.0, 0.0, 0.55));
        let x = 40.0 * s;
        text_shadow("SURF WARS", x, 90.0 * s, 72.0 * s, HUD_COLOR);
        text_shadow(
            "Counter-Strike 1.6 style surf combat",
            x + 4.0 * s,
            120.0 * s,
            20.0 * s,
            Color::new(1.0, 1.0, 1.0, 0.8),
        );
        let w = 380.0 * s;
        let h = 40.0 * s;
        let mut y = 160.0 * s;
        let step = h * 1.22;
        let o = &mut self.opts;
        let maps = map::map_names();
        if !maps.contains(&o.settings.map) {
            o.settings.map = maps[0].clone();
        }
        if ui.button(&format!("Map: {}", o.settings.map), x, y, w, h) {
            let i = maps.iter().position(|m| *m == o.settings.map).unwrap_or(0);
            o.settings.map = maps[(i + 1) % maps.len()].to_string();
            self.menu_map_changed = true;
        }
        y += step;
        let teams = ["Terrorists", "Counter-Terrorists", "Auto assign", "Spectate"];
        if ui.button(&format!("Team: {}", teams[o.team_choice]), x, y, w, h) {
            o.team_choice = (o.team_choice + 1) % teams.len();
        }
        y += step;
        let bw = w * 0.5 - 4.0 * s;
        if ui.button(&format!("T bots: {}", o.settings.bots_t), x, y, bw, h) {
            o.settings.bots_t = (o.settings.bots_t + 1) % 10;
        }
        if ui.button(&format!("CT bots: {}", o.settings.bots_ct), x + bw + 8.0 * s, y, bw, h) {
            o.settings.bots_ct = (o.settings.bots_ct + 1) % 10;
        }
        y += step;
        if ui.button(&format!("Bot skill: {}", o.settings.difficulty.name()), x, y, w, h) {
            o.settings.difficulty = o.settings.difficulty.next();
        }
        y += step;
        let mode = match o.settings.mode {
            Mode::Rounds => "Rounds (elimination)",
            Mode::Deathmatch => "Deathmatch (respawn)",
        };
        if ui.button(&format!("Mode: {mode}"), x, y, w, h) {
            o.settings.mode = match o.settings.mode {
                Mode::Rounds => Mode::Deathmatch,
                Mode::Deathmatch => Mode::Rounds,
            };
        }
        y += step;
        let label = if self.show_move_panel { "Movement settings  <<" } else { "Movement settings  >>" };
        if ui.button(label, x, y, w, h) {
            self.show_move_panel = !self.show_move_panel;
        }
        let o = &mut self.opts;
        y += step;
        let acc = if o.settings.ramp_accuracy { "Surf (ramps = ground)" } else { "Classic CS (ramps = air)" };
        if ui.button(&format!("Ramp accuracy: {acc}"), x, y, w, h) {
            o.settings.ramp_accuracy = !o.settings.ramp_accuracy;
        }
        y += step;
        if ui.button(&format!("Sensitivity: {:.1}", o.sensitivity), x, y, w * 0.6, h) {
            o.sensitivity = if o.sensitivity >= 6.0 { 0.5 } else { o.sensitivity + 0.5 };
        }
        if ui.button(&format!("Vol {:.0}%", o.volume * 100.0), x + w * 0.62, y, w * 0.38, h) {
            o.volume = if o.volume >= 1.0 { 0.0 } else { (o.volume + 0.1).min(1.0) };
        }
        self.audio.volume = self.opts.volume;
        y += step * 1.3;
        if ui.button("START", x, y, w, h * 1.3) || is_key_pressed(KeyCode::Enter) {
            self.opts.save();
            self.new_match();
            return;
        }
        y += h * 1.3 + 12.0 * s;
        if ui.button("Map editor", x, y, w * 0.6, h) {
            self.opts.save();
            self.open_editor();
            return;
        }
        if ui.button("Quit", x + w * 0.62, y, w * 0.38, h) {
            self.opts.save();
            std::process::exit(0);
        }

        if self.show_move_panel {
            let px = sw * 0.44 + 20.0 * s;
            if movement_panel(&ui, &mut self.opts.settings.vars, &mut self.num_edit, px, 180.0 * s, 460.0 * s) {
                self.opts.save();
            }
            return;
        }

        // Controls.
        let cx = sw * 0.44 + 20.0 * s;
        let lines = [
            "WASD  move        SPACE / mouse wheel  jump",
            "CTRL  duck        SHIFT  walk",
            "MOUSE1 fire       MOUSE2 scope / stab",
            "1 2 3  weapons    Q  last weapon   R  reload",
            "B  buy menu / attachments   G  drop   TAB  scores",
            "E  swap your gun for a map weapon",
            "V  third person (see your board)   ESC  menu",
            "",
            "Surfing: look along the ramp and hold the",
            "strafe key that pushes you into it (A or D).",
            "Steer with the mouse, never press W on a ramp.",
        ];
        let mut ly = sh - (lines.len() as f32 + 1.0) * 22.0 * s;
        draw_rectangle(
            cx - 12.0 * s,
            ly - 26.0 * s,
            520.0 * s,
            (lines.len() as f32 + 1.0) * 22.0 * s + 8.0 * s,
            Color::new(0.0, 0.0, 0.0, 0.5),
        );
        for l in lines {
            text_shadow(l, cx, ly, 18.0 * s, Color::new(1.0, 1.0, 1.0, 0.9));
            ly += 22.0 * s;
        }
    }
}

async fn take_screenshot_and_exit(path: &str) -> ! {
    get_screen_data().export_png(path);
    println!("saved {path}");
    std::process::exit(0);
}

#[macroquad::main(window_conf)]
async fn main() {
    let args = parse_args();
    let mut opts = Options::load();
    if let Some(m) = &args.map {
        opts.settings.map = m.clone();
    }
    let audio = Audio::new(!args.no_audio && args.shot.is_none()).await;
    let game = menu_game(&opts.settings.map);
    let renderer = Renderer::new(&game);
    let mut app = App {
        screen: Screen::Menu,
        opts,
        game,
        renderer,
        audio,
        acc: 0.0,
        view: Vec3::ZERO,
        last_mouse: Vec2::ZERO,
        paused: false,
        third_person: false,
        buy_menu: false,
        spec_target: 0,
        spec_first_person: false,
        wheel_jumps: 0,
        wheel_pressed_last: false,
        menu_cam_target: 0,
        menu_cam_switch: 0.0,
        fps: 0,
        fps_acc: 0.0,
        fps_frames: 0,
        was_alive: true,
        num_edit: NumEdit::default(),
        inventory: false,
        inv_tab: 0,
        inv_slot: Slot::Primary,
        show_move_panel: false,
        menu_map_changed: false,
        editor: None,
        from_editor: false,
        args: args.clone(),
    };
    app.audio.volume = app.opts.volume;

    // Screenshot mode: optionally start a match, fast forward and capture.
    if let Some(path) = args.shot.clone() {
        if matches!(args.ui.as_deref(), Some("editor") | Some("editor4")) {
            app.open_editor();
            if let Some(ed) = app.editor.as_mut() {
                if args.ui.as_deref() == Some("editor4") {
                    ed.debug_quad();
                } else {
                    ed.debug_select_first_ramp();
                }
            }
            for frame in 0..3 {
                clear_background(BLACK);
                app.frame_editor(0.016);
                if frame == 2 {
                    take_screenshot_and_exit(&path).await;
                }
                next_frame().await;
            }
        }
        if args.ui.as_deref() == Some("movement") {
            app.show_move_panel = true;
        }
        if !args.menu {
            if args.cam.as_deref() == Some("spectate") {
                app.opts.team_choice = 3;
            }
            app.new_match();
            app.grab(false);
            let ticks = (args.after / TICK) as usize;
            for _ in 0..ticks {
                let cmd = app.game.local.map(|li| UserCmd {
                    msec: TICK_MSEC,
                    viewangles: app.game.players[li].angles,
                    ..Default::default()
                });
                app.game.step(cmd);
                let ev = std::mem::take(&mut app.game.events);
                app.renderer.handle_events(&app.game, &ev);
            }
            if let Some(f) = args.follow {
                app.spec_target = f.min(app.game.players.len() - 1);
            } else if let Some(i) = app.game.players.iter().position(|p| p.alive && p.board > 0.5) {
                // follow someone who is surfing right now
                app.spec_target = i;
            } else {
                app.pick_spec_target(1);
            }
            match args.cam.as_deref() {
                Some("overview") => {
                    for p in app.game.players.iter_mut() {
                        p.alive = false;
                        p.death_time = -100.0;
                    }
                }
                Some("third") => app.third_person = true,
                Some("eye") => app.spec_first_person = true,
                _ => {}
            }
            app.view = match args.look {
                Some((p, y)) => vec3(p, y, 0.0),
                None => app.game.players[app.spec_target].angles,
            };
            if let (Some(li), Some(f)) = (app.game.local, Some(app.spec_target)) {
                // Put the local player's camera on the followed bot.
                if matches!(args.cam.as_deref(), Some("possess") | Some("possess3")) {
                    app.third_person = args.cam.as_deref() == Some("possess3");
                    app.game.players[li].angles = app.game.players[f].angles;
                    app.view = match args.look {
                        Some((p, y)) => vec3(p, y, 0.0),
                        None => app.game.players[f].angles,
                    };
                    let pm = app.game.players[f].pm;
                    app.game.players[li].pm = pm;
                    app.game.players[li].prev_origin = pm.origin;
                    app.game.players[li].board = app.game.players[f].board;
                    app.game.players[li].board_normal = app.game.players[f].board_normal;
                    app.game.players[li].primary = app.game.players[f].primary;
                    app.game.players[li].active = app.game.players[f].active;
                    // hide the bot we took the place of
                    app.game.players[f].alive = false;
                    app.game.players[f].death_time = -100.0;
                }
            }
        }
        match args.ui.as_deref() {
            Some("buy") => app.buy_menu = true,
            Some("pause") => app.paused = true,
            Some("attach") | Some("laser") | Some("rocket") | Some("ads") | Some("holo") => {
                if let Some(li) = app.game.local {
                    use weapons::*;
                    let id = match args.ui.as_deref() {
                        Some("laser") => WeaponId::Laser,
                        Some("rocket") => WeaponId::Rocket,
                        _ => WeaponId::Mp5,
                    };
                    let att = Attachments {
                        sight: if args.ui.as_deref() == Some("holo") { Sight::Holo } else { Sight::RedDot },
                        muzzle: Muzzle::Suppressor,
                        stock: Stock::Light,
                        grip: Grip::Vertical,
                    };
                    let p = &mut app.game.players[li];
                    p.primary = Some(Weapon::with(id, att));
                    p.active = Slot::Primary;
                    p.deploy_time = -10.0;
                    p.inventory = vec![
                        AttItem::Sight(Sight::Holo),
                        AttItem::Sight(Sight::Acog),
                        AttItem::Sight(Sight::Holo),
                        AttItem::Grip(Grip::Angled),
                    ];
                    if args.ui.as_deref() == Some("attach") {
                        app.inventory = true;
                    }
                    if matches!(args.ui.as_deref(), Some("ads") | Some("holo")) {
                        p.zoom = 1;
                        app.renderer.ads = 1.0;
                    }
                }
            }
            Some("scope") => {
                if let Some(li) = app.game.local {
                    let p = &mut app.game.players[li];
                    p.primary = Some(weapons::Weapon::new(weapons::WeaponId::Awp));
                    p.active = Slot::Primary;
                    p.zoom = 1;
                }
            }
            _ => {}
        }
        for frame in 0..3 {
            clear_background(BLACK);
            if app.screen == Screen::Menu {
                app.frame_menu(0.0);
            } else {
                app.draw_playing(0.0);
            }
            if frame == 2 {
                take_screenshot_and_exit(&path).await;
            }
            next_frame().await;
        }
    }

    loop {
        let dt = get_frame_time();
        app.fps_acc += dt;
        app.fps_frames += 1;
        if app.fps_acc >= 0.5 {
            app.fps = (app.fps_frames as f32 / app.fps_acc).round() as i32;
            app.fps_acc = 0.0;
            app.fps_frames = 0;
        }
        match app.screen {
            Screen::Menu => app.frame_menu(dt),
            Screen::Playing => app.frame_playing(dt),
            Screen::Editor => app.frame_editor(dt),
        }
        next_frame().await;
    }
}
