//! In-game map editor.
//!
//! Fly around with the right mouse button held (WASD, Space / Ctrl for up
//! and down, Shift for speed). Left click selects brushes, spawns and
//! pickups. The panel on the left adds shapes (boxes, slopes, surf ramps,
//! turning ramps, pillars), spawns and pickups (weapons including the laser
//! and rocket launcher, and health), and moves, resizes, rotates, recolours
//! and deletes the selection. Maps are saved to `maps/<name>.map` and show
//! up in the main menu.

use macroquad::prelude::*;

use crate::collision::{Brush, CollisionWorld};
use crate::game::{Game, PickupState, Settings};
use crate::hud::{text, text_shadow, text_width, HUD_COLOR};
use crate::map::{self, Map, Mat, PickupDef, PickupKind, Spawn, Team, Tex};
use crate::mapfile;
use crate::pmove::angle_vectors;
use crate::render::{camera_for, clear_depth, Renderer, View};
use crate::util::angle_norm;
use crate::weapons::{WeaponId, ALL_PICKUP_WEAPONS};

const PALETTE: [[u8; 3]; 12] = [
    [70, 170, 235],
    [245, 140, 60],
    [150, 110, 230],
    [90, 210, 150],
    [230, 200, 80],
    [230, 90, 90],
    [205, 170, 115],
    [120, 150, 200],
    [165, 160, 150],
    [150, 155, 165],
    [200, 160, 105],
    [70, 75, 85],
];

const TEXTURES: [Tex; 6] = [Tex::Grid, Tex::Concrete, Tex::Metal, Tex::Crate, Tex::Water, Tex::Clip];

const SKIES: [([f32; 3], [f32; 3], [f32; 3]); 4] = [
    ([0.20, 0.38, 0.72], [0.78, 0.86, 0.95], [0.72, 0.80, 0.90]),
    ([0.55, 0.30, 0.35], [0.98, 0.72, 0.50], [0.92, 0.70, 0.55]),
    ([0.12, 0.22, 0.45], [0.62, 0.72, 0.88], [0.55, 0.64, 0.80]),
    ([0.05, 0.05, 0.12], [0.25, 0.20, 0.35], [0.18, 0.15, 0.25]),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sel {
    Brush(usize),
    Spawn(usize, usize),
    Pickup(usize),
}

pub enum EditorAction {
    None,
    Exit,
    /// Play the map being edited.
    Play(Box<Map>),
}

pub struct Editor {
    pub game: Game,
    cam_pos: Vec3,
    cam_ang: Vec3,
    sel: Option<Sel>,
    pickup_kind: usize,
    grid: f32,
    color: usize,
    tex: usize,
    sky: usize,
    naming: bool,
    msg: Option<(String, f64)>,
    last_mouse: Vec2,
    looking: bool,
    load_index: usize,
    dirty: bool,
}

fn empty_settings() -> Settings {
    Settings { player_team: None, bots_t: 0, bots_ct: 0, ..Default::default() }
}

/// A fresh map: water, two spawn platforms and a ramp between them.
fn new_map() -> Map {
    let mut b = Vec::new();
    let floor = Mat::new(Tex::Grid, [70, 75, 85]);
    b.push(Brush::cuboid(vec3(-6000.0, -6000.0, -64.0), vec3(6000.0, 6000.0, 0.0), floor));
    b.push(Brush::cuboid(
        vec3(-6000.0, -6000.0, 0.0),
        vec3(6000.0, 6000.0, 120.0),
        Mat::new(Tex::Water, [40, 110, 150]),
    ));
    let mut spawns: [Vec<Spawn>; 2] = [Vec::new(), Vec::new()];
    for (ti, s, c) in [(0usize, -1.0f32, [205u8, 170, 115]), (1, 1.0, [120, 150, 200])] {
        let x0 = s * 2400.0;
        let x1 = s * 3000.0;
        b.push(Brush::cuboid(
            vec3(x0.min(x1), -300.0, 1936.0),
            vec3(x0.max(x1), 300.0, 2000.0),
            Mat::new(Tex::Concrete, c),
        ));
        for y in [-150.0, 0.0, 150.0] {
            spawns[ti].push(Spawn { pos: vec3(s * 2800.0, y, 2037.0), yaw: if s < 0.0 { 0.0 } else { 180.0 } });
        }
    }
    b.push(Brush::hull(
        &[
            vec3(-2200.0, 600.0, 1700.0),
            vec3(-2200.0, 200.0, 1100.0),
            vec3(-2200.0, 1000.0, 1100.0),
            vec3(2200.0, 600.0, 1700.0),
            vec3(2200.0, 200.0, 1100.0),
            vec3(2200.0, 1000.0, 1100.0),
        ],
        Mat::new(Tex::Grid, PALETTE[0]),
    ));
    let buyzones = Map::buyzones_from_spawns(&spawns);
    Map {
        name: "my_map".into(),
        world: CollisionWorld::new(b),
        spawns,
        kill_z: 450.0,
        buyzones,
        pickups: vec![PickupDef { pos: vec3(0.0, 0.0, 2024.0), kind: PickupKind::Health }],
        routes: Vec::new(),
        sky_top: SKIES[0].0,
        sky_horizon: SKIES[0].1,
        fog_color: SKIES[0].2,
        fog_start: 3000.0,
        fog_end: 16000.0,
    }
}

/// A copy of a map (maps are not `Clone` because of the collision world).
pub fn clone_map(m: &Map) -> Map {
    let mut c = mapfile::from_text(&mapfile::to_text(m)).expect("map round trip");
    c.routes = m.routes.clone();
    c.buyzones = m.buyzones;
    c
}

fn pickup_kinds() -> Vec<PickupKind> {
    let mut v = vec![PickupKind::Health];
    v.extend(ALL_PICKUP_WEAPONS.iter().map(|w| PickupKind::Weapon(*w)));
    v
}

fn pickup_name(k: PickupKind) -> &'static str {
    match k {
        PickupKind::Health => "Health",
        PickupKind::Weapon(w) => w.def().name,
    }
}

/// Ray against a convex brush. Returns the entry distance.
fn ray_brush(b: &Brush, o: Vec3, d: Vec3) -> Option<f32> {
    let mut t0 = 0.0f32;
    let mut t1 = f32::MAX;
    for p in &b.planes {
        let denom = p.normal.dot(d);
        let dist = p.dist - p.normal.dot(o);
        if denom.abs() < 1e-8 {
            if dist < 0.0 {
                return None;
            }
        } else {
            let t = dist / denom;
            if denom < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(t0)
}

fn ray_aabb(o: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> Option<f32> {
    let mut t0 = 0.0f32;
    let mut t1 = f32::MAX;
    for a in 0..3 {
        if d[a].abs() < 1e-8 {
            if o[a] < lo[a] || o[a] > hi[a] {
                return None;
            }
        } else {
            let mut a0 = (lo[a] - o[a]) / d[a];
            let mut a1 = (hi[a] - o[a]) / d[a];
            if a0 > a1 {
                std::mem::swap(&mut a0, &mut a1);
            }
            t0 = t0.max(a0);
            t1 = t1.min(a1);
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(t0)
}

fn button(label: &str, x: f32, y: f32, w: f32, h: f32) -> bool {
    let (mx, my) = mouse_position();
    let hover = mx >= x && mx <= x + w && my >= y && my <= y + h;
    let bg = if hover { Color::new(1.0, 0.69, 0.1, 0.35) } else { Color::new(0.0, 0.0, 0.0, 0.55) };
    draw_rectangle(x, y, w, h, bg);
    draw_rectangle_lines(x, y, w, h, 1.5, Color::new(1.0, 0.69, 0.1, if hover { 0.9 } else { 0.4 }));
    let size = h * 0.55;
    let tw = text_width(label, size);
    text(label, x + w * 0.5 - tw * 0.5, y + h * 0.5 + size * 0.33, size, WHITE);
    hover && is_mouse_button_pressed(MouseButton::Left)
}

impl Editor {
    /// Test helper: point the camera somewhere.
    #[cfg(test)]
    pub fn look_from(&mut self, pos: Vec3, angles: Vec3) {
        self.cam_pos = pos;
        self.cam_ang = angles;
    }

    #[cfg(test)]
    pub fn selected_brush(&self) -> Option<usize> {
        match self.sel {
            Some(Sel::Brush(i)) => Some(i),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn set_pickup_kind(&mut self, k: usize) {
        self.pickup_kind = k;
    }

    pub fn new(map_name: Option<&str>) -> Editor {
        let map = match map_name {
            Some(n) => map::load(n),
            None => new_map(),
        };
        let mut e = Editor {
            game: Game::with_map(map, empty_settings(), 1),
            cam_pos: vec3(-3200.0, -1800.0, 3000.0),
            cam_ang: vec3(25.0, 35.0, 0.0),
            sel: None,
            pickup_kind: 0,
            grid: 32.0,
            color: 0,
            tex: 0,
            sky: 0,
            naming: false,
            msg: None,
            last_mouse: Vec2::ZERO,
            looking: false,
            load_index: 0,
            dirty: true,
        };
        if let Some(sp) = e.game.map.spawns[0].first() {
            e.cam_pos = sp.pos + vec3(-600.0, -600.0, 500.0);
            e.cam_ang = vec3(30.0, 45.0, 0.0);
        }
        e
    }

    /// Screenshot helper: select a ramp so the panel shows a selection.
    pub fn debug_select_first_ramp(&mut self) {
        let i = self
            .game
            .map
            .world
            .brushes
            .iter()
            .position(|b| b.faces.iter().any(|f| f.normal.z > 0.3 && f.normal.z < 0.8));
        self.sel = i.map(Sel::Brush);
        self.pickup_kind = 7; // laser
    }

    fn map(&mut self) -> &mut Map {
        &mut self.game.map
    }

    fn say(&mut self, m: impl Into<String>) {
        self.msg = Some((m.into(), self.game.time + 3.0));
    }

    fn sync_pickups(&mut self) {
        self.game.pickups = self.game.map.pickups.iter().map(|d| PickupState { def: *d, available_at: 0.0 }).collect();
    }

    fn snap(&self, v: Vec3) -> Vec3 {
        (v / self.grid).round() * self.grid
    }

    /// Where new things go: what the camera looks at, or a bit ahead.
    fn place_point(&self) -> (Vec3, Vec3) {
        let (f, _, _) = angle_vectors(self.cam_ang);
        let tr = self.game.map.world.trace_ray(self.cam_pos, self.cam_pos + f * 6000.0);
        if tr.hit() {
            (self.snap(tr.endpos), tr.normal)
        } else {
            (self.snap(self.cam_pos + f * 900.0), Vec3::Z)
        }
    }

    fn current_mat(&self) -> Mat {
        Mat::new(TEXTURES[self.tex], PALETTE[self.color])
    }

    fn add_brush(&mut self, b: Brush) {
        self.map().world.brushes.push(b);
        let i = self.game.map.world.brushes.len() - 1;
        self.sel = Some(Sel::Brush(i));
        self.dirty = true;
    }

    fn cam_axis(&self) -> (bool, f32) {
        // (along x?, sign of the camera's facing on the other axis)
        let (f, _, _) = angle_vectors(vec3(0.0, self.cam_ang.y, 0.0));
        if f.x.abs() > f.y.abs() {
            (true, f.y.signum())
        } else {
            (false, f.x.signum())
        }
    }

    pub(crate) fn add_shape(&mut self, kind: &str) {
        let (p, _) = self.place_point();
        let mat = self.current_mat();
        let (along_x, side) = self.cam_axis();
        match kind {
            "box" => {
                self.add_brush(Brush::cuboid(p - vec3(128.0, 128.0, 64.0), p + vec3(128.0, 128.0, 0.0), mat));
            }
            "pillar" => {
                self.add_brush(Brush::cuboid(p - vec3(48.0, 48.0, 200.0), p + vec3(48.0, 48.0, 0.0), mat));
            }
            "ramp" => {
                let (l, hw, h) = (512.0, 384.0, 576.0);
                let top = p.z + h;
                let b = if along_x {
                    map::ramp_x(p.x - l, p.x + l, p.y, top, top, hw, h, mat)
                } else {
                    map::ramp_y(p.y - l, p.y + l, p.x, top, top, hw, h, mat)
                };
                self.add_brush(b);
            }
            "slope" => {
                // one sided ramp with its slope facing the camera
                let (l, w, h) = (512.0, 384.0, 576.0);
                let s = -side;
                let pts: Vec<Vec3> = if along_x {
                    let back = p.y - s * w;
                    let front = p.y + s * w;
                    [p.x - l, p.x + l]
                        .iter()
                        .flat_map(|x| [vec3(*x, back, p.z), vec3(*x, back, p.z + h), vec3(*x, front, p.z)])
                        .collect()
                } else {
                    let back = p.x - s * w;
                    let front = p.x + s * w;
                    [p.y - l, p.y + l]
                        .iter()
                        .flat_map(|y| [vec3(back, *y, p.z), vec3(back, *y, p.z + h), vec3(front, *y, p.z)])
                        .collect()
                };
                if let Some(b) = Brush::try_hull(&pts, mat) {
                    self.add_brush(b);
                }
            }
            "turn90" | "turn180" => {
                let (sweep, segs) = if kind == "turn90" { (90.0, 12) } else { (180.0, 24) };
                let a0 = self.cam_ang.y - 90.0;
                let top = p.z + 576.0;
                let mut v = Vec::new();
                map::turn_ramp(&mut v, p, 1024.0, a0, a0 + sweep, top, top - 96.0, 384.0, 576.0, segs, mat);
                for b in v {
                    self.map().world.brushes.push(b);
                }
                self.sel = Some(Sel::Brush(self.game.map.world.brushes.len() - 1));
                self.dirty = true;
                self.say("Turning ramp added (each segment is its own brush)");
            }
            _ => {}
        }
    }

    pub(crate) fn add_spawn(&mut self, team: Team) {
        let (p, _) = self.place_point();
        let yaw = self.cam_ang.y.round();
        self.map().spawns[team.index()].push(Spawn { pos: p + vec3(0.0, 0.0, 37.0), yaw });
        let i = self.game.map.spawns[team.index()].len() - 1;
        self.sel = Some(Sel::Spawn(team.index(), i));
        self.refresh_buyzones();
    }

    fn refresh_buyzones(&mut self) {
        let z = Map::buyzones_from_spawns(&self.game.map.spawns);
        self.map().buyzones = z;
    }

    pub(crate) fn add_pickup(&mut self) {
        let (p, _) = self.place_point();
        let kind = pickup_kinds()[self.pickup_kind];
        self.map().pickups.push(PickupDef { pos: p + vec3(0.0, 0.0, 28.0), kind });
        self.sel = Some(Sel::Pickup(self.game.map.pickups.len() - 1));
        self.sync_pickups();
    }

    /// Applies a point transform to the selected brush.
    fn transform_brush(&mut self, i: usize, f: impl Fn(Vec3, Vec3) -> Vec3) {
        let b = &self.game.map.world.brushes[i];
        let c = mapfile::centroid(&b.points);
        let pts: Vec<Vec3> = b.points.iter().map(|p| f(*p, c)).collect();
        let mat = b.mat;
        match Brush::try_hull(&pts, mat) {
            Some(nb) => {
                self.map().world.brushes[i] = nb;
                self.dirty = true;
            }
            None => self.say("That would make the brush flat"),
        }
    }

    pub(crate) fn move_sel(&mut self, d: Vec3) {
        match self.sel {
            Some(Sel::Brush(i)) => self.transform_brush(i, |p, _| p + d),
            Some(Sel::Spawn(t, i)) => {
                self.map().spawns[t][i].pos += d;
                self.refresh_buyzones();
            }
            Some(Sel::Pickup(i)) => {
                self.map().pickups[i].pos += d;
                self.sync_pickups();
            }
            None => {}
        }
    }

    pub(crate) fn resize_sel(&mut self, axis: usize, delta: f32) {
        if let Some(Sel::Brush(i)) = self.sel {
            let b = &self.game.map.world.brushes[i];
            let ext = b.maxs[axis] - b.mins[axis];
            let new = (ext + delta).max(8.0);
            let k = new / ext.max(1.0);
            self.transform_brush(i, |p, c| {
                let mut q = p;
                q[axis] = c[axis] + (p[axis] - c[axis]) * k;
                q
            });
        }
    }

    pub(crate) fn rotate_sel(&mut self, deg: f32) {
        match self.sel {
            Some(Sel::Brush(i)) => {
                let (s, c0) = deg.to_radians().sin_cos();
                self.transform_brush(i, |p, c| {
                    let d = p - c;
                    vec3(c.x + d.x * c0 - d.y * s, c.y + d.x * s + d.y * c0, p.z)
                });
            }
            Some(Sel::Spawn(t, i)) => {
                let y = &mut self.map().spawns[t][i].yaw;
                *y = angle_norm(*y + deg);
            }
            _ => {}
        }
    }

    pub(crate) fn delete_sel(&mut self) {
        match self.sel.take() {
            Some(Sel::Brush(i)) => {
                self.map().world.brushes.remove(i);
                self.dirty = true;
            }
            Some(Sel::Spawn(t, i)) => {
                self.map().spawns[t].remove(i);
                self.refresh_buyzones();
            }
            Some(Sel::Pickup(i)) => {
                self.map().pickups.remove(i);
                self.sync_pickups();
            }
            None => {}
        }
    }

    pub(crate) fn duplicate_sel(&mut self) {
        let off = vec3(self.grid * 4.0, self.grid * 4.0, 0.0);
        match self.sel {
            Some(Sel::Brush(i)) => {
                let b = &self.game.map.world.brushes[i];
                let pts: Vec<Vec3> = b.points.iter().map(|p| *p + off).collect();
                if let Some(nb) = Brush::try_hull(&pts, b.mat) {
                    self.add_brush(nb);
                }
            }
            Some(Sel::Spawn(t, i)) => {
                let mut sp = self.game.map.spawns[t][i];
                sp.pos += off;
                self.map().spawns[t].push(sp);
                self.sel = Some(Sel::Spawn(t, self.game.map.spawns[t].len() - 1));
                self.refresh_buyzones();
            }
            Some(Sel::Pickup(i)) => {
                let mut pk = self.game.map.pickups[i];
                pk.pos += off;
                self.map().pickups.push(pk);
                self.sel = Some(Sel::Pickup(self.game.map.pickups.len() - 1));
                self.sync_pickups();
            }
            None => {}
        }
    }

    pub(crate) fn restyle_sel(&mut self, color: bool) {
        if color {
            self.color = (self.color + 1) % PALETTE.len();
        } else {
            self.tex = (self.tex + 1) % TEXTURES.len();
        }
        let mat = self.current_mat();
        match self.sel {
            Some(Sel::Brush(i)) => {
                self.map().world.brushes[i].mat = mat;
                self.dirty = true;
            }
            Some(Sel::Pickup(i)) => {
                if color {
                    return;
                }
                let kinds = pickup_kinds();
                let cur = kinds.iter().position(|k| *k == self.game.map.pickups[i].kind).unwrap_or(0);
                self.map().pickups[i].kind = kinds[(cur + 1) % kinds.len()];
                self.sync_pickups();
            }
            _ => {}
        }
    }

    fn pick(&mut self, mx: f32, my: f32) {
        // Build the ray through the mouse position.
        let sw = screen_width();
        let sh = screen_height();
        let (f, r, u) = angle_vectors(self.cam_ang);
        let vf = crate::render::vfov_pub(90.0);
        let th = (vf * 0.5).tan();
        let tw = th * sw / sh;
        let nx = (mx / sw) * 2.0 - 1.0;
        let ny = 1.0 - (my / sh) * 2.0;
        let dir = (f + r * (nx * tw) + u * (ny * th)).normalize();
        let o = self.cam_pos;
        let mut best: Option<(f32, Sel)> = None;
        let mut consider = |t: Option<f32>, s: Sel| {
            if let Some(t) = t {
                if best.is_none_or(|b| t < b.0) {
                    best = Some((t, s));
                }
            }
        };
        for (i, b) in self.game.map.world.brushes.iter().enumerate() {
            // skip the giant floor / water slabs unless nothing else is hit
            let huge = (b.maxs - b.mins).x > 8000.0 && (b.maxs - b.mins).y > 8000.0;
            consider(ray_brush(b, o, dir).map(|t| if huge { t + 1e6 } else { t }), Sel::Brush(i));
        }
        for t in 0..2 {
            for (i, sp) in self.game.map.spawns[t].iter().enumerate() {
                consider(
                    ray_aabb(o, dir, sp.pos - vec3(16.0, 16.0, 36.0), sp.pos + vec3(16.0, 16.0, 36.0)),
                    Sel::Spawn(t, i),
                );
            }
        }
        for (i, pk) in self.game.map.pickups.iter().enumerate() {
            consider(ray_aabb(o, dir, pk.pos - Vec3::splat(24.0), pk.pos + Vec3::splat(24.0)), Sel::Pickup(i));
        }
        self.sel = best.map(|b| b.1);
    }

    fn sel_label(&self) -> String {
        match self.sel {
            Some(Sel::Brush(i)) => {
                let b = &self.game.map.world.brushes[i];
                let e = b.maxs - b.mins;
                format!("Brush {} ({:.0} x {:.0} x {:.0}, {})", i, e.x, e.y, e.z, mapfile::tex_key(b.mat.tex))
            }
            Some(Sel::Spawn(t, i)) => {
                let sp = self.game.map.spawns[t][i];
                format!("{} spawn {} (yaw {:.0})", if t == 0 { "T" } else { "CT" }, i, sp.yaw)
            }
            Some(Sel::Pickup(i)) => format!("Pickup: {}", pickup_name(self.game.map.pickups[i].kind)),
            None => "Nothing selected (left click to select)".into(),
        }
    }

    pub(crate) fn save(&mut self) {
        if map::BUILTIN_MAPS.contains(&self.game.map.name.as_str()) {
            let n = format!("{}_edit", self.game.map.name);
            self.map().name = n;
        }
        match mapfile::save(&self.game.map) {
            Ok(p) => self.say(format!("Saved {}", p.display())),
            Err(e) => self.say(format!("Save failed: {e}")),
        }
    }

    fn load_next(&mut self, renderer: &mut Renderer) {
        let names = map::map_names();
        self.load_index = (self.load_index + 1) % names.len();
        let name = names[self.load_index].clone();
        self.game = Game::with_map(map::load(&name), empty_settings(), 1);
        self.sel = None;
        self.dirty = true;
        renderer.rebuild_world(&self.game);
        self.say(format!("Loaded {name}"));
    }

    fn fly(&mut self, dt: f32) {
        let (mx, my) = mouse_position();
        let m = vec2(mx, my);
        let rmb = is_mouse_button_down(MouseButton::Right);
        if rmb && !self.looking {
            self.looking = true;
            set_cursor_grab(true);
            show_mouse(false);
            self.last_mouse = m;
        } else if !rmb && self.looking {
            self.looking = false;
            set_cursor_grab(false);
            show_mouse(true);
        }
        if !self.looking {
            return;
        }
        let d = m - self.last_mouse;
        self.last_mouse = m;
        if d.length() < 2000.0 {
            self.cam_ang.y = angle_norm(self.cam_ang.y - d.x * 0.12);
            self.cam_ang.x = (self.cam_ang.x + d.y * 0.12).clamp(-89.0, 89.0);
        }
        let (f, r, _) = angle_vectors(self.cam_ang);
        let mut v = Vec3::ZERO;
        if is_key_down(KeyCode::W) {
            v += f;
        }
        if is_key_down(KeyCode::S) {
            v -= f;
        }
        if is_key_down(KeyCode::D) {
            v += r;
        }
        if is_key_down(KeyCode::A) {
            v -= r;
        }
        if is_key_down(KeyCode::Space) || is_key_down(KeyCode::E) {
            v += Vec3::Z;
        }
        if is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::Q) {
            v -= Vec3::Z;
        }
        let speed = if is_key_down(KeyCode::LeftShift) { 3000.0 } else { 900.0 };
        self.cam_pos += v.normalize_or_zero() * speed * dt;
    }

    fn shortcuts(&mut self) {
        if self.looking || self.naming {
            return;
        }
        let g = self.grid;
        // arrow keys move along the world axis closest to the camera
        let (f, r, _) = angle_vectors(vec3(0.0, self.cam_ang.y, 0.0));
        let snap = |v: Vec3| {
            if v.x.abs() > v.y.abs() {
                vec3(v.x.signum(), 0.0, 0.0)
            } else {
                vec3(0.0, v.y.signum(), 0.0)
            }
        };
        let fwd = snap(f);
        let right = snap(r);
        let step = if is_key_down(KeyCode::LeftShift) { g * 4.0 } else { g };
        if is_key_pressed(KeyCode::Up) {
            self.move_sel(fwd * step);
        }
        if is_key_pressed(KeyCode::Down) {
            self.move_sel(-fwd * step);
        }
        if is_key_pressed(KeyCode::Right) {
            self.move_sel(right * step);
        }
        if is_key_pressed(KeyCode::Left) {
            self.move_sel(-right * step);
        }
        if is_key_pressed(KeyCode::PageUp) {
            self.move_sel(Vec3::Z * step);
        }
        if is_key_pressed(KeyCode::PageDown) {
            self.move_sel(-Vec3::Z * step);
        }
        if is_key_pressed(KeyCode::Delete) || is_key_pressed(KeyCode::Backspace) {
            self.delete_sel();
        }
        if is_key_pressed(KeyCode::R) {
            self.rotate_sel(15.0);
        }
        if is_key_pressed(KeyCode::F) {
            self.rotate_sel(-15.0);
        }
        if is_key_down(KeyCode::LeftControl) && is_key_pressed(KeyCode::D) {
            self.duplicate_sel();
        }
        if is_key_down(KeyCode::LeftControl) && is_key_pressed(KeyCode::S) {
            self.save();
        }
    }

    /// One editor frame. Draws everything and returns what the app should do.
    pub fn frame(&mut self, renderer: &mut Renderer, dt: f32) -> EditorAction {
        self.fly(dt);
        self.shortcuts();
        if self.dirty {
            renderer.rebuild_world(&self.game);
            self.dirty = false;
        }
        self.game.time += dt as f64;

        let view = View { pos: self.cam_pos, angles: self.cam_ang, fov: 90.0, first_person: None };
        renderer.draw(&self.game, &view, 1.0, false);
        self.draw_overlays(&view);

        let s = screen_height() / 720.0;
        let pw = 330.0 * s;
        // selection by clicking in the viewport
        let (mx, my) = mouse_position();
        if !self.looking && is_mouse_button_pressed(MouseButton::Left) && mx > pw + 10.0 {
            self.pick(mx, my);
        }
        self.panel(renderer, s, pw)
    }

    fn draw_overlays(&self, view: &View) {
        set_camera(&camera_for(view));
        clear_depth();
        for (t, list) in self.game.map.spawns.iter().enumerate() {
            let c = if t == 0 { Color::new(1.0, 0.4, 0.2, 1.0) } else { Color::new(0.3, 0.6, 1.0, 1.0) };
            for sp in list {
                draw_cube_wires(sp.pos, vec3(32.0, 32.0, 72.0), c);
                let (f, _, _) = angle_vectors(vec3(0.0, sp.yaw, 0.0));
                draw_line_3d(sp.pos, sp.pos + f * 60.0, c);
            }
        }
        match self.sel {
            Some(Sel::Brush(i)) => {
                if let Some(b) = self.game.map.world.brushes.get(i) {
                    for face in &b.faces {
                        for k in 0..face.verts.len() {
                            draw_line_3d(face.verts[k], face.verts[(k + 1) % face.verts.len()], YELLOW);
                        }
                    }
                }
            }
            Some(Sel::Spawn(t, i)) => {
                if let Some(sp) = self.game.map.spawns[t].get(i) {
                    draw_cube_wires(sp.pos, vec3(40.0, 40.0, 80.0), YELLOW);
                }
            }
            Some(Sel::Pickup(i)) => {
                if let Some(pk) = self.game.map.pickups.get(i) {
                    draw_cube_wires(pk.pos, Vec3::splat(48.0), YELLOW);
                }
            }
            None => {}
        }
        // kill height plane outline
        let kz = self.game.map.kill_z;
        let c = Color::new(1.0, 0.2, 0.2, 0.6);
        let e = 6000.0;
        for (a, b) in [
            (vec3(-e, -e, kz), vec3(e, -e, kz)),
            (vec3(e, -e, kz), vec3(e, e, kz)),
            (vec3(e, e, kz), vec3(-e, e, kz)),
            (vec3(-e, e, kz), vec3(-e, -e, kz)),
        ] {
            draw_line_3d(a, b, c);
        }
        set_default_camera();
    }

    fn panel(&mut self, renderer: &mut Renderer, s: f32, pw: f32) -> EditorAction {
        let sh = screen_height();
        let sw = screen_width();
        draw_rectangle(0.0, 0.0, pw + 10.0 * s, sh, Color::new(0.0, 0.0, 0.0, 0.6));
        let x = 10.0 * s;
        let h = 26.0 * s;
        let gap = 4.0 * s;
        let mut y = 8.0 * s;
        text_shadow("MAP EDITOR", x, y + 22.0 * s, 26.0 * s, HUD_COLOR);
        y += 34.0 * s;

        // Name (click to type).
        let name_label = if self.naming {
            format!("Name: {}_", self.game.map.name)
        } else {
            format!("Name: {}", self.game.map.name)
        };
        if button(&name_label, x, y, pw - x, h) {
            self.naming = !self.naming;
        }
        if self.naming {
            while let Some(c) = get_char_pressed() {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    let n = &mut self.map().name;
                    if n.len() < 32 {
                        n.push(c);
                    }
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                self.map().name.pop();
            }
            if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Escape) {
                self.naming = false;
                if self.game.map.name.is_empty() {
                    self.map().name = "my_map".into();
                }
            }
        }
        y += h + gap * 2.0;

        let third = (pw - x - gap * 2.0) / 3.0;
        let half = (pw - x - gap) / 2.0;
        text("Add (placed where you look):", x, y + 14.0 * s, 15.0 * s, GRAY);
        y += 20.0 * s;
        for (i, (label, kind)) in [("Box", "box"), ("Slope", "slope"), ("Surf ramp", "ramp")].iter().enumerate() {
            if button(label, x + (third + gap) * i as f32, y, third, h) {
                self.add_shape(kind);
            }
        }
        y += h + gap;
        for (i, (label, kind)) in
            [("Turn 90", "turn90"), ("Turn 180", "turn180"), ("Pillar", "pillar")].iter().enumerate()
        {
            if button(label, x + (third + gap) * i as f32, y, third, h) {
                self.add_shape(kind);
            }
        }
        y += h + gap;
        if button("T spawn", x, y, half, h) {
            self.add_spawn(Team::T);
        }
        if button("CT spawn", x + half + gap, y, half, h) {
            self.add_spawn(Team::CT);
        }
        y += h + gap;
        let kinds = pickup_kinds();
        if button("<", x, y, h, h) {
            self.pickup_kind = (self.pickup_kind + kinds.len() - 1) % kinds.len();
        }
        if button(&format!("Add {}", pickup_name(kinds[self.pickup_kind])), x + h + gap, y, pw - x - 2.0 * (h + gap), h)
        {
            self.add_pickup();
        }
        if button(">", pw - h, y, h, h) {
            self.pickup_kind = (self.pickup_kind + 1) % kinds.len();
        }
        y += h + gap * 3.0;

        // Selection.
        text(&self.sel_label(), x, y + 14.0 * s, 15.0 * s, YELLOW);
        y += 22.0 * s;
        let g = self.grid;
        let small = (pw - x - 60.0 * s - gap * 5.0) / 6.0;
        for (row, label) in ["Move", "Size"].iter().enumerate() {
            text(label, x, y + h * 0.7, 16.0 * s, WHITE);
            for (a, axis) in ["X", "Y", "Z"].iter().enumerate() {
                let bx = x + 60.0 * s + (small * 2.0 + gap) * a as f32;
                if button(&format!("{axis}-"), bx, y, small, h) {
                    let mut d = Vec3::ZERO;
                    d[a] = -g;
                    if row == 0 {
                        self.move_sel(d);
                    } else {
                        self.resize_sel(a, -g * 2.0);
                    }
                }
                if button(&format!("{axis}+"), bx + small, y, small, h) {
                    let mut d = Vec3::ZERO;
                    d[a] = g;
                    if row == 0 {
                        self.move_sel(d);
                    } else {
                        self.resize_sel(a, g * 2.0);
                    }
                }
            }
            y += h + gap;
        }
        if button("Rotate -15", x, y, half, h) {
            self.rotate_sel(-15.0);
        }
        if button("Rotate +15", x + half + gap, y, half, h) {
            self.rotate_sel(15.0);
        }
        y += h + gap;
        let tex_name = mapfile::tex_key(TEXTURES[self.tex]);
        if button("Colour >", x, y, half, h) {
            self.restyle_sel(true);
        }
        let tlabel =
            if matches!(self.sel, Some(Sel::Pickup(_))) { "Kind >".to_string() } else { format!("Tex: {tex_name} >") };
        if button(&tlabel, x + half + gap, y, half, h) {
            self.restyle_sel(false);
        }
        let c = PALETTE[self.color];
        draw_rectangle(
            x + half - 16.0 * s,
            y + 5.0 * s,
            10.0 * s,
            h - 10.0 * s,
            Color::from_rgba(c[0], c[1], c[2], 255),
        );
        y += h + gap;
        if button("Duplicate", x, y, half, h) {
            self.duplicate_sel();
        }
        if button("Delete", x + half + gap, y, half, h) {
            self.delete_sel();
        }
        y += h + gap * 3.0;

        // Map settings.
        if button(&format!("Grid: {}", self.grid), x, y, half, h) {
            self.grid = match self.grid as i32 {
                8 => 16.0,
                16 => 32.0,
                32 => 64.0,
                64 => 128.0,
                _ => 8.0,
            };
        }
        if button("Sky >", x + half + gap, y, half, h) {
            self.sky = (self.sky + 1) % SKIES.len();
            let (t, hz, f) = SKIES[self.sky];
            let m = self.map();
            m.sky_top = t;
            m.sky_horizon = hz;
            m.fog_color = f;
            self.dirty = true;
        }
        y += h + gap;
        text(&format!("Fall teleport height: {:.0}", self.game.map.kill_z), x, y + h * 0.7, 16.0 * s, WHITE);
        if button("-", pw - 2.0 * h - gap, y, h, h) {
            self.map().kill_z -= 64.0;
        }
        if button("+", pw - h, y, h, h) {
            self.map().kill_z += 64.0;
        }
        y += h + gap * 3.0;

        if button("Save", x, y, third, h) {
            self.save();
        }
        if button("Load >", x + third + gap, y, third, h) {
            self.load_next(renderer);
        }
        if button("New", x + (third + gap) * 2.0, y, third, h) {
            self.game = Game::with_map(new_map(), empty_settings(), 1);
            self.sel = None;
            self.dirty = true;
        }
        y += h + gap;
        let mut action = EditorAction::None;
        if button("Test play", x, y, half, h) {
            if self.game.map.spawns[0].is_empty() && self.game.map.spawns[1].is_empty() {
                self.say("Add at least one spawn first");
            } else {
                action = EditorAction::Play(Box::new(clone_map(&self.game.map)));
            }
        }
        if button("Exit", x + half + gap, y, half, h) {
            action = EditorAction::Exit;
        }
        y += h + gap * 2.0;
        let help = [
            "Hold RMB: look, WASD fly, Space/Ctrl up/down",
            "LMB: select   Arrows / PgUp PgDn: move",
            "R / F: rotate   Del: delete   Ctrl+D: duplicate",
            "Ctrl+S: save   Shift: bigger steps / faster",
        ];
        for l in help {
            text(l, x, y + 13.0 * s, 13.5 * s, Color::new(0.8, 0.8, 0.8, 1.0));
            y += 16.0 * s;
        }
        if let Some((m, t)) = &self.msg {
            if self.game.time < *t {
                let w = text_width(m, 20.0 * s);
                draw_rectangle(sw * 0.5 - w * 0.5 - 10.0, 14.0 * s, w + 20.0, 32.0 * s, Color::new(0.0, 0.0, 0.0, 0.6));
                text_shadow(m, sw * 0.5 - w * 0.5, 36.0 * s, 20.0 * s, WHITE);
            }
        }
        // crosshair for placement
        draw_line(sw * 0.5 - 6.0, sh * 0.5, sw * 0.5 + 6.0, sh * 0.5, 1.0, WHITE);
        draw_line(sw * 0.5, sh * 0.5 - 6.0, sw * 0.5, sh * 0.5 + 6.0, 1.0, WHITE);
        let _ = WeaponId::Knife;
        action
    }
}
