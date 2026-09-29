//! SurfWars: a Counter-Strike 1.6 style surf combat game.

mod audio;
mod bot;
mod collision;
mod game;
mod hud;
mod map;
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
use crate::hud::{text_centered, text_shadow, text_width, HUD_COLOR};
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
}

fn parse_args() -> Args {
    let mut a = Args {
        after: 3.0,
        ..Default::default()
    };
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
        let mut o = Options {
            settings: Settings::default(),
            team_choice: 2,
            sensitivity: 2.5,
            volume: 0.7,
        };
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
                    "autobhop" => o.settings.autobhop = v == "1",
                    "ramp_accuracy" => o.settings.ramp_accuracy = v == "1",
                    "deathmatch" => {
                        o.settings.mode = if v == "1" { Mode::Deathmatch } else { Mode::Rounds }
                    }
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
            "sensitivity {}\nvolume {}\nname \"{}\"\nmap {}\nbots_t {}\nbots_ct {}\nteam {}\nautobhop {}\ndeathmatch {}\ndifficulty {}\nramp_accuracy {}\n",
            self.sensitivity,
            self.volume,
            s.player_name,
            s.map,
            s.bots_t,
            s.bots_ct,
            self.team_choice,
            if s.autobhop { 1 } else { 0 },
            if s.mode == Mode::Deathmatch { 1 } else { 0 },
            s.difficulty.name().to_lowercase(),
            if s.ramp_accuracy { 1 } else { 0 },
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
        Ui {
            clicked: is_mouse_button_pressed(MouseButton::Left),
            mouse: vec2(x, y),
            scale: screen_height() / 720.0,
        }
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

// ---------------------------------------------------------------------------
// App

#[derive(PartialEq, Eq, Clone, Copy)]
enum Screen {
    Menu,
    Playing,
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
        2 => Some(if rng_seed % 2 == 0 { Team::T } else { Team::CT }),
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
    fn new_match(&mut self) {
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
        self.game = Game::new(s, seed);
        self.renderer = Renderer::new(&self.game);
        self.acc = 0.0;
        self.view = self.game.local_player().map(|p| p.angles).unwrap_or(Vec3::ZERO);
        self.paused = false;
        self.buy_menu = false;
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
        let mut cmd = UserCmd {
            msec: TICK_MSEC,
            viewangles: self.view,
            ..Default::default()
        };
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
        if !self.buy_menu {
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
        }
        let alive = self.game.players[li].alive;
        if self.buy_menu {
            let keys = [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4, KeyCode::Key5, KeyCode::Key6];
            for (i, k) in keys.iter().enumerate() {
                if is_key_pressed(*k) {
                    if self.game.buy(li, ALL_BUYABLE[i]) {
                        self.buy_menu = false;
                    }
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
        if self.game.players.get(self.spec_target).map_or(false, |p| p.alive) {
            Some(self.spec_target)
        } else {
            None
        }
    }

    fn chase_view(&self, target: usize, angles: Vec3, dist: f32, alpha: f32) -> View {
        let p = &self.game.players[target];
        let pos = p.prev_origin.lerp(p.pm.origin, alpha) + vec3(0.0, 0.0, p.pm.view_ofs);
        let (f, _, _) = angle_vectors(angles);
        let want = pos - f * dist + vec3(0.0, 0.0, 12.0);
        let tr = self
            .game
            .map
            .world
            .trace(pos, want, vec3(-6.0, -6.0, -6.0), vec3(6.0, 6.0, 6.0));
        View {
            pos: tr.endpos,
            angles,
            fov: 90.0,
            first_person: None,
        }
    }

    fn compute_view(&mut self, alpha: f32) -> (View, bool) {
        let g = &self.game;
        if let Some(li) = g.local {
            let p = &g.players[li];
            if p.alive {
                if self.third_person {
                    return (self.chase_view(li, self.view, 130.0, alpha), false);
                }
                let pos = p.prev_origin.lerp(p.pm.origin, alpha) + vec3(0.0, 0.0, p.pm.view_ofs);
                let angles = self.view + p.pm.punchangle;
                return (
                    View {
                        pos,
                        angles,
                        fov: p.fov(),
                        first_person: Some(li),
                    },
                    true,
                );
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
                        },
                        true,
                    )
                } else {
                    (self.chase_view(t, self.view, 150.0, alpha), false)
                }
            }
            None => (
                View {
                    pos: vec3(-5200.0, -3600.0, 4200.0),
                    angles: vec3(28.0, 35.0, 0.0),
                    fov: 90.0,
                    first_person: None,
                },
                false,
            ),
        }
    }

    fn frame_playing(&mut self, dt: f32) {
        if is_key_pressed(KeyCode::Escape) {
            if self.buy_menu {
                self.buy_menu = false;
            } else {
                self.paused = !self.paused;
                self.grab(!self.paused);
            }
        }
        if !self.paused {
            self.mouse_look();
            self.handle_keys();
        }

        // Spectator controls.
        let local_alive = self.game.local_player().map_or(false, |p| p.alive);
        if !local_alive && !self.paused {
            let need = !self.game.players.get(self.spec_target).map_or(false, |p| p.alive)
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
        let (view, fp) = self.compute_view(alpha);
        self.renderer.draw(&self.game, &view, alpha, fp);
        let pov = self.pov();
        self.audio.update_loops(&self.game, pov, self.paused);
        let spectating = self.game.local.map_or(true, |li| {
            !self.game.players[li].alive && self.game.time - self.game.players[li].death_time >= 2.0
        });
        hud::draw(&hud::HudState {
            game: &self.game,
            pov,
            spectating,
            show_scores: is_key_down(KeyCode::Tab) || self.game.phase == game::Phase::Over,
            buy_menu: self.buy_menu,
            view_angles: if fp { view.angles } else { self.view },
            third_person: !fp,
            fps: self.fps,
        });

        if self.paused {
            self.draw_pause();
        }
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
        if ui.button("Restart match", x, y, w, h) {
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
        // Background: a live bot match with a camera chasing a surfer.
        self.simulate(dt);
        let g = &self.game;
        let now = g.time;
        let cur_ok = g.players.get(self.menu_cam_target).map_or(false, |p| p.alive && p.board > 0.2);
        if now > self.menu_cam_switch || !g.players.get(self.menu_cam_target).map_or(false, |p| p.alive) {
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
        if ui.button(&format!("Map: {}", o.settings.map), x, y, w, h) {
            let i = maps.iter().position(|m| *m == o.settings.map).unwrap_or(0);
            o.settings.map = maps[(i + 1) % maps.len()].to_string();
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
        if ui.button(
            &format!("Auto bunnyhop: {}", if o.settings.autobhop { "On" } else { "Off (CS default)" }),
            x,
            y,
            w,
            h,
        ) {
            o.settings.autobhop = !o.settings.autobhop;
        }
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
        if ui.button("Quit", x, y, w, h) {
            self.opts.save();
            std::process::exit(0);
        }

        // Controls.
        let cx = sw * 0.44 + 20.0 * s;
        let lines = [
            "WASD  move        SPACE / mouse wheel  jump",
            "CTRL  duck        SHIFT  walk",
            "MOUSE1 fire       MOUSE2 scope / stab",
            "1 2 3  weapons    Q  last weapon   R  reload",
            "B  buy menu       G  drop   TAB  scores",
            "V  third person (see your board)   ESC  menu",
            "",
            "Surfing: look along the ramp and hold the",
            "strafe key that pushes you into it (A or D).",
            "Steer with the mouse, never press W on a ramp.",
        ];
        let mut ly = sh - (lines.len() as f32 + 1.0) * 22.0 * s;
        draw_rectangle(cx - 12.0 * s, ly - 26.0 * s, 520.0 * s, (lines.len() as f32 + 1.0) * 22.0 * s + 8.0 * s, Color::new(0.0, 0.0, 0.0, 0.5));
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
        args: args.clone(),
    };
    app.audio.volume = app.opts.volume;

    // Screenshot mode: optionally start a match, fast forward and capture.
    if let Some(path) = args.shot.clone() {
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
            if let (Some(li), Some(f)) = (app.game.local, args.follow) {
                // Put the local player's camera on the followed bot.
                if args.cam.as_deref() == Some("possess") {
                    let pm = app.game.players[f].pm;
                    app.game.players[li].pm = pm;
                    app.game.players[li].prev_origin = pm.origin;
                    app.game.players[li].board = app.game.players[f].board;
                    app.game.players[li].board_normal = app.game.players[f].board_normal;
                    app.game.players[li].primary = app.game.players[f].primary;
                    app.game.players[li].active = app.game.players[f].active;
                }
            }
        }
        for frame in 0..3 {
            clear_background(BLACK);
            if app.screen == Screen::Menu {
                app.frame_menu(0.0);
            } else {
                let alpha = 0.0;
                let (view, fp) = app.compute_view(alpha);
                app.renderer.draw(&app.game, &view, alpha, fp);
                let pov = app.pov();
                let spectating = app.game.local.map_or(true, |li| !app.game.players[li].alive);
                hud::draw(&hud::HudState {
                    game: &app.game,
                    pov,
                    spectating,
                    show_scores: false,
                    buy_menu: false,
                    view_angles: if fp { view.angles } else { app.view },
                    third_person: !fp,
                    fps: 0,
                });
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
        }
        next_frame().await;
    }
}
