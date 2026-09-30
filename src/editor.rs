//! In-game map editor.
//!
//! Fly around with the right mouse button held (WASD, Space / Ctrl for up
//! and down, Shift for speed). Left click selects brushes, spawns, pickups
//! and boosters; drag the red / green / blue arrows on the selection to move
//! it along X / Y / Z. Tab toggles a Hammer style four pane layout: the 3D
//! view plus Top (X/Y), Front (Y/Z) and Side (X/Z) grid views where you drag
//! things to move them and drag the handles on the selection box to resize
//! it, snapped to the grid (wheel zooms, right or middle drag pans).
//!
//! The panel on the left adds shapes (boxes, slopes, surf ramps, turning
//! ramps, pillars), spawns, pickups (health, weapons including the laser
//! and rocket launcher, attachments), boosters and launch pads, and moves,
//! resizes, rotates, restyles and deletes the selection. Maps are saved to
//! `maps/<name>.map` and show up in the main menu.

use macroquad::prelude::*;

use crate::collision::{Brush, CollisionWorld};
use crate::game::{Game, PickupState, Settings};
use crate::hud::{text, text_shadow, text_width, HUD_COLOR};
use crate::map::{
    self, Aabb, Backdrop, Booster, Map, Mat, PickupDef, PickupKind, Push, Spawn, Team, TeleDest, Teleport, Tex, WpMode,
};
use crate::mapfile;
use crate::pmove::angle_vectors;
use crate::render::{camera_for, clear_depth, Renderer, View};
use crate::util::angle_norm;
use crate::weapons::{AttItem, WeaponId, ALL_PICKUP_WEAPONS};

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
    Booster(usize),
    Teleport(usize),
}

/// What left clicks do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    /// Select and move whole objects.
    Select,
    /// Select one face of a brush and drag it (reshapes slopes).
    Face,
    /// Draw a line in a 2D view to cut the selected brush in two.
    Clip,
}

/// Which drop down list is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drop {
    PickupAdd,
    PickupSel,
    Colour,
    Texture,
    Sky,
    Scenery,
    Speed,
    Keep,
    Load,
}

const SKY_NAMES: [&str; 4] = ["Day", "Sunset", "Dusk", "Night"];
const SPEEDS: [f32; 6] = [1000.0, 1400.0, 1800.0, 2200.0, 2600.0, 3200.0];
const KEEP_NAMES: [&str; 3] = ["Keep both halves", "Keep front (green)", "Keep back (red)"];

thread_local! {
    /// Screen area of an open drop down list: buttons underneath ignore the mouse.
    static MODAL: std::cell::Cell<Option<(f32, f32, f32, f32)>> = const { std::cell::Cell::new(None) };
}

fn in_modal(m: Vec2) -> bool {
    MODAL.with(|c| c.get()).is_some_and(|(x, y, w, h)| m.x >= x && m.x <= x + w && m.y >= y && m.y <= y + h)
}

/// A screen rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.x < self.x + self.w && p.y >= self.y && p.y < self.y + self.h
    }

    fn center(&self) -> Vec2 {
        vec2(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pane {
    Persp,
    Top,
    Front,
    Side,
}

impl Pane {
    /// World axes shown right and up in a 2D view.
    fn axes(self) -> (usize, usize) {
        match self {
            Pane::Top => (0, 1),
            Pane::Front => (1, 2),
            _ => (0, 2),
        }
    }

    fn index(self) -> usize {
        match self {
            Pane::Top => 0,
            Pane::Front => 1,
            _ => 2,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Pane::Persp => "3D",
            Pane::Top => "Top (X / Y)",
            Pane::Front => "Front (Y / Z)",
            Pane::Side => "Side (X / Z)",
        }
    }
}

/// Where a 2D view is looking and how zoomed in it is.
#[derive(Clone, Copy, Debug)]
struct Ortho {
    center: Vec2,
    /// World units per pixel.
    scale: f32,
}

/// The selection as it was when a drag started. Drags re-apply their
/// transform to this every frame, so snapping never accumulates error.
#[derive(Clone, Debug)]
enum Snapshot {
    Brush(usize, Vec<Vec3>, Mat),
    Spawn(usize, usize, Vec3),
    Pickup(usize, Vec3),
    Booster(usize, Booster),
    Teleport(usize, Teleport),
    /// One face of a brush: the brush points and the face's corners.
    Face(usize, Vec<Vec3>, Mat, Vec<Vec3>),
}

#[derive(Clone, Copy, Debug)]
enum DragKind {
    /// Along one world axis with a 3D arrow.
    Axis { axis: usize, screen_dir: Vec2, px_per_unit: f32 },
    /// Moving in a 2D view.
    Move { pane: Pane, start: Vec2 },
    /// Dragging a handle of the selection box in a 2D view. `hu` / `hv` are
    /// -1 (min edge), 0 (untouched) or 1 (max edge) on the view's axes.
    Resize { pane: Pane, start: Vec2, hu: i32, hv: i32, mins: Vec3, maxs: Vec3 },
    /// Panning a 2D view.
    Pan { pane: Pane, last: Vec2 },
    /// Drawing the clip tool's cutting line in a 2D view.
    ClipLine { pane: Pane, start: Vec2 },
}

#[derive(Clone, Debug)]
struct Drag {
    kind: DragKind,
    start_mouse: Vec2,
    snap: Option<Snapshot>,
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
    /// Four pane layout on (Tab).
    quad: bool,
    ortho: [Ortho; 3],
    drag: Option<Drag>,
    /// Which gizmo arrow the mouse is over (for highlighting).
    hover_axis: Option<usize>,
    /// Panes of the last frame, for mouse handling.
    panes: Vec<(Pane, Rect)>,
    tool: Tool,
    /// Normal of the selected face (face tool).
    face: Option<Vec3>,
    /// Clip line: pane and two world points on the view's axes.
    clip: Option<(Pane, Vec2, Vec2)>,
    /// 0 keep both halves, 1 front, 2 back.
    clip_keep: usize,
    /// Open drop down list, where its button is, and the list area.
    drop: Option<(Drop, Rect)>,
    modal: Option<Rect>,
    show_paths: bool,
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
        boosters: Vec::new(),
        teleports: Vec::new(),
        sky: Vec::new(),
        routes: Vec::new(),
        sky_top: SKIES[0].0,
        sky_horizon: SKIES[0].1,
        fog_color: SKIES[0].2,
        fog_start: 5000.0,
        fog_end: 26000.0,
        backdrop: map::Backdrop::City,
    }
}

/// A copy of a map (maps are not `Clone` because of the collision world).
pub fn clone_map(m: &Map) -> Map {
    let mut c = mapfile::from_text(&mapfile::to_text(m)).expect("map round trip");
    c.sky = m.sky.clone();
    c.backdrop = m.backdrop;
    c.buyzones = m.buyzones;
    c
}

fn pickup_kinds() -> Vec<PickupKind> {
    let mut v = vec![PickupKind::Health];
    v.extend(ALL_PICKUP_WEAPONS.iter().map(|w| PickupKind::Weapon(*w)));
    v.extend(AttItem::ALL.iter().map(|a| PickupKind::Attachment(*a)));
    v
}

fn pickup_name(k: PickupKind) -> &'static str {
    k.name()
}

/// Ray against a convex brush. Returns the entry distance.
fn ray_brush(b: &Brush, o: Vec3, d: Vec3) -> Option<f32> {
    ray_brush_face(b, o, d).map(|x| x.0)
}

/// Ray against a convex brush: entry distance and the normal of the face it
/// enters through.
fn ray_brush_face(b: &Brush, o: Vec3, d: Vec3) -> Option<(f32, Vec3)> {
    let mut t0 = 0.0f32;
    let mut t1 = f32::MAX;
    let mut n0 = Vec3::Z;
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
                if t > t0 {
                    t0 = t;
                    n0 = p.normal;
                }
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    // bevel planes are not faces: use the face closest to the plane hit
    let n = b.faces.iter().map(|f| f.normal).max_by(|a, c| a.dot(n0).total_cmp(&c.dot(n0))).unwrap_or(n0);
    Some((t0, n))
}

/// Cuts a brush with the plane n.p = d. Returns the part in front (the
/// side `n` points to) and the part behind; either can be None.
fn split_brush(b: &Brush, n: Vec3, d: f32) -> (Option<Brush>, Option<Brush>) {
    let mut front = Vec::new();
    let mut back = Vec::new();
    let eps = 0.05;
    for face in &b.faces {
        let vs = &face.verts;
        for k in 0..vs.len() {
            let (p, q) = (vs[k], vs[(k + 1) % vs.len()]);
            let (dp, dq) = (n.dot(p) - d, n.dot(q) - d);
            if dp >= -eps {
                front.push(p);
            }
            if dp <= eps {
                back.push(p);
            }
            if (dp > eps && dq < -eps) || (dp < -eps && dq > eps) {
                let x = p + (q - p) * (dp / (dp - dq));
                front.push(x);
                back.push(x);
            }
        }
    }
    let dedup = |v: Vec<Vec3>| {
        let mut out: Vec<Vec3> = Vec::new();
        for p in v {
            if out.iter().all(|q| q.distance(p) > 0.05) {
                out.push(p);
            }
        }
        out
    };
    let (front, back) = (dedup(front), dedup(back));
    let solid = |pts: &Vec<Vec3>| pts.iter().any(|p| (n.dot(*p) - d).abs() > 1.0);
    let f = if solid(&front) { Brush::try_hull(&front, b.mat) } else { None };
    let k = if solid(&back) { Brush::try_hull(&back, b.mat) } else { None };
    (f, k)
}

/// Point in a convex or concave 2D polygon (even-odd rule).
fn point_in_poly(p: Vec2, poly: &[Vec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
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

/// Waypoint colours for the bot path overlay.
fn mode_color(m: WpMode) -> Color {
    match m {
        WpMode::Walk => WHITE,
        WpMode::Drop => YELLOW,
        WpMode::Hop => Color::new(0.4, 1.0, 0.4, 1.0),
        WpMode::Surf => Color::new(0.3, 0.9, 1.0, 1.0),
        WpMode::Hold => Color::new(1.0, 0.3, 0.3, 1.0),
    }
}

fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

/// A world point with `a` on axis `u` and `b` on axis `v`.
fn axis_point(u: usize, v: usize, a: f32, b: f32) -> Vec3 {
    let mut p = Vec3::ZERO;
    p[u] = a;
    p[v] = b;
    p
}

/// Clips 2D drawing to a screen rectangle (None: no clipping).
fn scissor(r: Option<Rect>) {
    unsafe {
        let gl = get_internal_gl().quad_gl;
        gl.scissor(r.map(|r| (r.x as i32, r.y as i32, r.w as i32, r.h as i32)));
    }
}

fn button(label: &str, x: f32, y: f32, w: f32, h: f32) -> bool {
    button_lit(label, x, y, w, h, false)
}

/// A button; `lit` draws it as the active choice.
fn button_lit(label: &str, x: f32, y: f32, w: f32, h: f32, lit: bool) -> bool {
    let (mx, my) = mouse_position();
    let hover = mx >= x && mx <= x + w && my >= y && my <= y + h && !in_modal(vec2(mx, my));
    if lit {
        draw_rectangle(x, y, w, h, Color::new(1.0, 0.69, 0.1, 0.45));
    }
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

    /// Test helper: drag the selection's resize handle (`hu`, `hv`) in the
    /// top view by `delta` world units, as the mouse would.
    #[cfg(test)]
    pub fn drag_top_handle(&mut self, hu: i32, hv: i32, delta: Vec2) {
        self.quad = true;
        let r = Rect { x: 700.0, y: 0.0, w: 580.0, h: 360.0 };
        self.panes = vec![(Pane::Top, r)];
        let (mins, maxs) = self.sel_bounds().unwrap();
        let start = self.s2w(Pane::Top, r, r.center());
        let o = self.ortho[0];
        let m = r.center() + vec2(delta.x, -delta.y) / o.scale;
        let drag = Drag {
            kind: DragKind::Resize { pane: Pane::Top, start, hu, hv, mins, maxs },
            start_mouse: r.center(),
            snap: self.snapshot(),
        };
        self.update_drag(&drag, m);
    }

    /// Test helper: drag the selection along a world axis with the gizmo.
    #[cfg(test)]
    pub fn drag_axis(&mut self, axis: usize, units: f32) {
        let drag = Drag {
            kind: DragKind::Axis { axis, screen_dir: Vec2::X, px_per_unit: 1.0 },
            start_mouse: Vec2::ZERO,
            snap: self.snapshot(),
        };
        self.update_drag(&drag, vec2(units, 0.0));
    }

    #[cfg(test)]
    pub fn select_face(&mut self, brush: usize, normal: Vec3) {
        self.tool = Tool::Face;
        self.sel = Some(Sel::Brush(brush));
        self.face = Some(normal);
    }

    #[cfg(test)]
    pub fn face_normal(&self) -> Option<Vec3> {
        self.face
    }

    /// Test helper: a clip line in the top view.
    #[cfg(test)]
    pub fn set_clip(&mut self, a: Vec2, b: Vec2, keep: usize) {
        self.tool = Tool::Clip;
        self.clip = Some((Pane::Top, a, b));
        self.clip_keep = keep;
    }

    #[cfg(test)]
    pub fn add_tele(&mut self) {
        self.add_teleport();
        self.teleport_dest_here();
    }

    #[cfg(test)]
    pub fn add_boost(&mut self, launch: bool) {
        self.add_booster(launch);
    }

    #[cfg(test)]
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        self.sel_bounds()
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
            quad: false,
            ortho: [Ortho { center: Vec2::ZERO, scale: 8.0 }; 3],
            drag: None,
            hover_axis: None,
            panes: Vec::new(),
            tool: Tool::Select,
            face: None,
            clip: None,
            clip_keep: 0,
            drop: None,
            modal: None,
            show_paths: false,
        };
        if let Some(sp) = e.game.map.spawns[0].first() {
            e.cam_pos = sp.pos + vec3(-600.0, -600.0, 500.0);
            e.cam_ang = vec3(30.0, 45.0, 0.0);
        }
        e.center_views();
        e
    }

    /// Screenshot helper: four pane layout with a ramp selected.
    pub fn debug_quad(&mut self) {
        self.quad = true;
        self.debug_select_first_ramp();
        self.center_views();
    }

    /// Screenshot helper: show one of the editor tools in action.
    pub fn debug_mode(&mut self, mode: &str) {
        self.debug_select_first_ramp();
        match mode {
            "editor_face" => {
                self.quad = true;
                self.tool = Tool::Face;
                self.face = self.sel.and_then(|s| match s {
                    Sel::Brush(i) => self.game.map.world.brushes[i]
                        .faces
                        .iter()
                        .find(|f| f.normal.z > 0.3 && f.normal.z < 0.8)
                        .map(|f| f.normal),
                    _ => None,
                });
                self.center_views();
            }
            "editor_clip" => {
                self.quad = true;
                self.tool = Tool::Clip;
                if let Some((a, b)) = self.sel_bounds() {
                    let c = (a + b) * 0.5;
                    self.clip = Some((Pane::Top, vec2(c.x - 700.0, a.y - 200.0), vec2(c.x + 300.0, b.y + 200.0)));
                }
                self.center_views();
            }
            "editor_drop" => {
                self.sel = None;
                self.drop = Some((Drop::PickupAdd, Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }));
            }
            "editor_paths" => {
                self.quad = true;
                self.show_paths = true;
                self.sel = None;
                for o in self.ortho.iter_mut() {
                    o.center = Vec2::ZERO;
                    o.scale = 22.0;
                }
            }
            _ => {}
        }
    }

    /// Centres the 2D views on the selection (or the camera).
    fn center_views(&mut self) {
        let c = self.sel_bounds().map(|(a, b)| (a + b) * 0.5).unwrap_or(self.cam_pos);
        for pane in [Pane::Top, Pane::Front, Pane::Side] {
            let (u, v) = pane.axes();
            self.ortho[pane.index()].center = vec2(c[u], c[v]);
        }
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
                              // look at it from above and to the side
        if let Some((a, b)) = self.sel_bounds() {
            let c = (a + b) * 0.5;
            self.cam_pos = c + vec3(-c.x.signum() * 500.0, -c.y.signum() * 1700.0, 1300.0);
            let d = c - self.cam_pos;
            let yaw = d.y.atan2(d.x).to_degrees();
            let pitch = -(d.z / d.truncate().length()).atan().to_degrees();
            self.cam_ang = vec3(pitch, yaw, 0.0);
        }
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

    pub(crate) fn add_booster(&mut self, launch: bool) {
        let (p, _) = self.place_point();
        let (f, _, _) = angle_vectors(vec3(0.0, self.cam_ang.y, 0.0));
        let b = if launch {
            Booster::launcher(p + vec3(0.0, 0.0, 2.0), p + f * 1400.0 - vec3(0.0, 0.0, 300.0), 1.5)
        } else {
            Booster::boost(Aabb::new(p - vec3(256.0, 256.0, 96.0), p + vec3(256.0, 256.0, 160.0)), f, 1800.0)
        };
        self.map().boosters.push(b);
        self.sel = Some(Sel::Booster(self.game.map.boosters.len() - 1));
        self.dirty = true;
        self.say(if launch {
            "Launch pad: Size X = distance, Size Z = target height, Size Y = flight time"
        } else {
            "Booster: Colour = speed, Tex = one / two way, Rotate turns the push"
        });
    }

    pub(crate) fn add_teleport(&mut self) {
        let (p, _) = self.place_point();
        let t = Teleport::to_spawn(Aabb::new(p - vec3(192.0, 192.0, 0.0), p + vec3(192.0, 192.0, 160.0)));
        self.map().teleports.push(t);
        self.sel = Some(Sel::Teleport(self.game.map.teleports.len() - 1));
        self.say("Teleport: goes to the team spawn; fly somewhere and press 'Dest here' to send it there");
    }

    /// Makes the selected teleport send players to where the camera looks.
    fn teleport_dest_here(&mut self) {
        if let Some(Sel::Teleport(i)) = self.sel {
            let (p, _) = self.place_point();
            let yaw = self.cam_ang.y.round();
            self.map().teleports[i].dest = TeleDest::Point { pos: p + vec3(0.0, 0.0, 40.0), yaw };
        }
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
        if self.tool == Tool::Face && self.face.is_some() {
            if let Some(snap) = self.snapshot() {
                self.apply(&snap, |p| p + d, true);
            }
            return;
        }
        match self.sel {
            Some(Sel::Brush(i)) => self.transform_brush(i, |p, _| p + d),
            Some(Sel::Teleport(i)) => {
                let t = &mut self.map().teleports[i];
                t.zone = Aabb::new(t.zone.mins + d, t.zone.maxs + d);
            }
            Some(Sel::Spawn(t, i)) => {
                self.map().spawns[t][i].pos += d;
                self.refresh_buyzones();
            }
            Some(Sel::Pickup(i)) => {
                self.map().pickups[i].pos += d;
                self.sync_pickups();
            }
            Some(Sel::Booster(i)) => {
                let b = &mut self.map().boosters[i];
                b.zone = Aabb::new(b.zone.mins + d, b.zone.maxs + d);
                if let Push::Launch { target, .. } = &mut b.push {
                    *target += d;
                }
                self.dirty = true;
            }
            None => {}
        }
    }

    pub(crate) fn resize_sel(&mut self, axis: usize, delta: f32) {
        if let Some(Sel::Teleport(i)) = self.sel {
            let t = &mut self.map().teleports[i];
            let (mut lo, mut hi) = (t.zone.mins, t.zone.maxs);
            let ext = (hi[axis] - lo[axis] + delta).max(16.0);
            let c = (lo[axis] + hi[axis]) * 0.5;
            lo[axis] = c - ext * 0.5;
            hi[axis] = c + ext * 0.5;
            t.zone = Aabb::new(lo, hi);
            return;
        }
        if let Some(Sel::Booster(i)) = self.sel {
            let pad = self.game.map.boosters[i].pad();
            let b = &mut self.map().boosters[i];
            match &mut b.push {
                Push::Boost { .. } => {
                    let mut lo = b.zone.mins;
                    let mut hi = b.zone.maxs;
                    let ext = (hi[axis] - lo[axis] + delta).max(16.0);
                    let c = (lo[axis] + hi[axis]) * 0.5;
                    lo[axis] = c - ext * 0.5;
                    hi[axis] = c + ext * 0.5;
                    b.zone = Aabb::new(lo, hi);
                }
                Push::Launch { target, secs } => match axis {
                    0 => {
                        let h = vec3(target.x - pad.x, target.y - pad.y, 0.0);
                        let len = (h.length() + delta * 2.0).max(64.0);
                        let dir = h.normalize_or(Vec3::X);
                        *target = vec3(pad.x, pad.y, target.z) + dir * len;
                    }
                    1 => *secs = (*secs + delta.signum() * 0.1).clamp(0.3, 4.0),
                    _ => target.z += delta,
                },
            }
            self.dirty = true;
            return;
        }
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
            Some(Sel::Teleport(i)) => {
                if let TeleDest::Point { yaw, .. } = &mut self.map().teleports[i].dest {
                    *yaw = angle_norm(*yaw + deg);
                }
            }
            Some(Sel::Booster(i)) => {
                let (s, c) = deg.to_radians().sin_cos();
                let rot = |v: Vec3| vec3(v.x * c - v.y * s, v.x * s + v.y * c, v.z);
                let pad = self.game.map.boosters[i].pad();
                match &mut self.map().boosters[i].push {
                    Push::Boost { dir, .. } => *dir = rot(*dir),
                    Push::Launch { target, .. } => *target = pad + rot(*target - pad),
                }
                self.dirty = true;
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
            Some(Sel::Booster(i)) => {
                self.map().boosters.remove(i);
                self.dirty = true;
            }
            Some(Sel::Teleport(i)) => {
                self.map().teleports.remove(i);
            }
            None => {}
        }
        self.face = None;
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
            Some(Sel::Booster(i)) => {
                let b = self.game.map.boosters[i];
                self.map().boosters.push(b);
                self.sel = Some(Sel::Booster(self.game.map.boosters.len() - 1));
                self.move_sel(off);
            }
            Some(Sel::Teleport(i)) => {
                let t = self.game.map.teleports[i];
                self.map().teleports.push(t);
                self.sel = Some(Sel::Teleport(self.game.map.teleports.len() - 1));
                self.move_sel(off);
            }
            None => {}
        }
    }

    pub(crate) fn restyle_sel(&mut self, color: bool) {
        if let Some(Sel::Booster(i)) = self.sel {
            if let Push::Boost { speed, two_way, .. } = &mut self.map().boosters[i].push {
                if color {
                    let k = SPEEDS.iter().position(|x| *x > *speed + 1.0).unwrap_or(0);
                    *speed = SPEEDS[k];
                } else {
                    *two_way = !*two_way;
                }
                self.dirty = true;
            }
            return;
        }
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

    /// Camera ray through a point of the 3D pane.
    fn ray(&self, r: Rect, m: Vec2) -> (Vec3, Vec3) {
        let (f, rt, u) = angle_vectors(self.cam_ang);
        let vf = crate::render::vfov_pub(90.0);
        let th = (vf * 0.5).tan();
        let tw = th * r.w / r.h;
        let nx = ((m.x - r.x) / r.w) * 2.0 - 1.0;
        let ny = 1.0 - ((m.y - r.y) / r.h) * 2.0;
        (self.cam_pos, (f + rt * (nx * tw) + u * (ny * th)).normalize())
    }

    /// Screen position of a world point in the 3D pane.
    fn project(&self, r: Rect, p: Vec3) -> Option<Vec2> {
        let (f, rt, u) = angle_vectors(self.cam_ang);
        let d = p - self.cam_pos;
        let z = d.dot(f);
        if z < 1.0 {
            return None;
        }
        let vf = crate::render::vfov_pub(90.0);
        let th = (vf * 0.5).tan();
        let tw = th * r.w / r.h;
        let nx = d.dot(rt) / (z * tw);
        let ny = d.dot(u) / (z * th);
        Some(vec2(r.x + (nx + 1.0) * 0.5 * r.w, r.y + (1.0 - ny) * 0.5 * r.h))
    }

    /// Corners of the selected face (face tool).
    fn face_verts(&self) -> Option<Vec<Vec3>> {
        let n = self.face?;
        let Some(Sel::Brush(i)) = self.sel else { return None };
        let b = self.game.map.world.brushes.get(i)?;
        let f = b.faces.iter().max_by(|a, c| a.normal.dot(n).total_cmp(&c.normal.dot(n)))?;
        Some(f.verts.clone())
    }

    /// Picks the face of the selected brush that a 2D view click lands on:
    /// the one facing the viewer whose outline contains the point.
    fn pick_face_2d(&mut self, pane: Pane, w: Vec2) {
        let Some(Sel::Brush(i)) = self.sel else { return };
        let (u, v) = pane.axes();
        let toward = match pane {
            Pane::Top => Vec3::Z,
            Pane::Front => Vec3::X,
            _ => -Vec3::Y,
        };
        let b = &self.game.map.world.brushes[i];
        let mut best: Option<(f32, Vec3)> = None;
        for f in &b.faces {
            let poly: Vec<Vec2> = f.verts.iter().map(|p| vec2(p[u], p[v])).collect();
            if !point_in_poly(w, &poly) {
                continue;
            }
            let k = f.normal.dot(toward);
            if best.is_none_or(|x| k > x.0) {
                best = Some((k, f.normal));
            }
        }
        self.face = best.map(|x| x.1).or(self.face);
    }

    fn pick(&mut self, r: Rect, m: Vec2) {
        let (o, dir) = self.ray(r, m);
        self.face = None;
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
        for (i, b) in self.game.map.boosters.iter().enumerate() {
            // prefer boosters a little over the ramp they sit on
            consider(ray_aabb(o, dir, b.zone.mins, b.zone.maxs).map(|t| t - 30.0), Sel::Booster(i));
        }
        for (i, t) in self.game.map.teleports.iter().enumerate() {
            // big bottom-of-the-map teleports would swallow every click
            let huge = (t.zone.maxs - t.zone.mins).x > 8000.0;
            consider(
                ray_aabb(o, dir, t.zone.mins, t.zone.maxs).map(|x| if huge { x + 2e6 } else { x - 30.0 }),
                Sel::Teleport(i),
            );
        }
        self.sel = best.map(|b| b.1);
        if self.tool == Tool::Face {
            if let Some(Sel::Brush(i)) = self.sel {
                self.face = ray_brush_face(&self.game.map.world.brushes[i], o, dir).map(|x| x.1);
            }
        }
    }

    /// Bounding box of the selection.
    fn sel_bounds(&self) -> Option<(Vec3, Vec3)> {
        let m = &self.game.map;
        match self.sel? {
            Sel::Brush(i) => m.world.brushes.get(i).map(|b| (b.mins, b.maxs)),
            Sel::Spawn(t, i) => {
                m.spawns[t].get(i).map(|s| (s.pos - vec3(16.0, 16.0, 36.0), s.pos + vec3(16.0, 16.0, 36.0)))
            }
            Sel::Pickup(i) => m.pickups.get(i).map(|p| (p.pos - Vec3::splat(20.0), p.pos + Vec3::splat(20.0))),
            Sel::Booster(i) => m.boosters.get(i).map(|b| (b.zone.mins, b.zone.maxs)),
            Sel::Teleport(i) => m.teleports.get(i).map(|t| (t.zone.mins, t.zone.maxs)),
        }
    }

    /// Bounds used for the gizmo and 2D selection box: the face in face mode.
    fn handle_bounds(&self) -> Option<(Vec3, Vec3)> {
        if self.tool == Tool::Face {
            if let Some(v) = self.face_verts() {
                let lo = v.iter().fold(Vec3::splat(f32::MAX), |a, p| a.min(*p));
                let hi = v.iter().fold(Vec3::splat(f32::MIN), |a, p| a.max(*p));
                return Some((lo, hi));
            }
        }
        self.sel_bounds()
    }

    /// Only brushes and boosters (not launch pads) can be resized by handles.
    fn sel_resizable(&self) -> bool {
        if self.tool == Tool::Face && self.face.is_some() {
            return false;
        }
        match self.sel {
            Some(Sel::Brush(_)) | Some(Sel::Teleport(_)) => true,
            Some(Sel::Booster(i)) => matches!(self.game.map.boosters[i].push, Push::Boost { .. }),
            _ => false,
        }
    }

    fn snapshot(&self) -> Option<Snapshot> {
        let m = &self.game.map;
        if self.tool == Tool::Face {
            if let (Some(Sel::Brush(i)), Some(fv)) = (self.sel, self.face_verts()) {
                let b = &m.world.brushes[i];
                return Some(Snapshot::Face(i, b.points.clone(), b.mat, fv));
            }
        }
        Some(match self.sel? {
            Sel::Teleport(i) => Snapshot::Teleport(i, m.teleports[i]),
            Sel::Brush(i) => Snapshot::Brush(i, m.world.brushes[i].points.clone(), m.world.brushes[i].mat),
            Sel::Spawn(t, i) => Snapshot::Spawn(t, i, m.spawns[t][i].pos),
            Sel::Pickup(i) => Snapshot::Pickup(i, m.pickups[i].pos),
            Sel::Booster(i) => Snapshot::Booster(i, m.boosters[i]),
        })
    }

    /// Re-applies a transform to the snapshot of the selection. `moving`
    /// also carries a launch pad's target along.
    fn apply(&mut self, snap: &Snapshot, f: impl Fn(Vec3) -> Vec3, moving: bool) {
        match snap {
            Snapshot::Brush(i, pts, mat) => {
                let moved: Vec<Vec3> = pts.iter().map(|p| f(*p)).collect();
                if let Some(nb) = Brush::try_hull(&moved, *mat) {
                    let cur = &self.game.map.world.brushes[*i];
                    if cur.points != nb.points {
                        self.map().world.brushes[*i] = nb;
                        self.dirty = true;
                    }
                }
            }
            Snapshot::Face(i, pts, mat, fv) => {
                let on_face = |p: &Vec3| fv.iter().any(|q| q.distance(*p) < 0.5);
                let moved: Vec<Vec3> = pts.iter().map(|p| if on_face(p) { f(*p) } else { *p }).collect();
                if let Some(nb) = Brush::try_hull(&moved, *mat) {
                    // follow the face: it may have turned
                    let target: Vec<Vec3> = fv.iter().map(|p| f(*p)).collect();
                    let score = |face: &crate::collision::Face| {
                        face.verts.iter().filter(|v| target.iter().any(|t| t.distance(**v) < 1.0)).count()
                    };
                    let nf = nb.faces.iter().max_by_key(|face| score(face)).map(|face| face.normal);
                    if self.game.map.world.brushes[*i].points != nb.points {
                        self.map().world.brushes[*i] = nb;
                        self.dirty = true;
                    }
                    if nf.is_some() {
                        self.face = nf;
                    }
                }
            }
            Snapshot::Teleport(i, t) => {
                let (a, c) = (f(t.zone.mins), f(t.zone.maxs));
                self.map().teleports[*i].zone = Aabb::new(a.min(c), a.max(c));
            }
            Snapshot::Spawn(t, i, pos) => {
                self.map().spawns[*t][*i].pos = f(*pos);
                self.refresh_buyzones();
            }
            Snapshot::Pickup(i, pos) => {
                self.map().pickups[*i].pos = f(*pos);
                self.sync_pickups();
            }
            Snapshot::Booster(i, b) => {
                let (a, c) = (f(b.zone.mins), f(b.zone.maxs));
                let mut nb = *b;
                nb.zone = Aabb::new(a.min(c), a.max(c));
                if let Push::Launch { target, .. } = &mut nb.push {
                    if moving {
                        *target = f(*target);
                    }
                }
                let cur = self.game.map.boosters[*i];
                if cur.zone.mins != nb.zone.mins || cur.zone.maxs != nb.zone.maxs || cur.push != nb.push {
                    self.map().boosters[*i] = nb;
                    self.dirty = true;
                }
            }
        }
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
            Some(Sel::Booster(i)) => {
                let b = &self.game.map.boosters[i];
                match b.push {
                    Push::Boost { speed, two_way, .. } => {
                        format!("Booster {i}: {speed:.0} u/s{}", if two_way { ", two way" } else { "" })
                    }
                    Push::Launch { target, secs } => {
                        let d = (target - b.pad()).truncate().length();
                        format!("Launch pad {i}: {d:.0} away, {:+.0} up, {secs:.1} s", target.z - b.pad().z)
                    }
                }
            }
            Some(Sel::Teleport(i)) => match self.game.map.teleports[i].dest {
                TeleDest::TeamSpawn => format!("Teleport {i}: to the team spawn"),
                TeleDest::Point { pos, yaw } => {
                    format!("Teleport {i}: to ({:.0}, {:.0}, {:.0}) yaw {yaw:.0}", pos.x, pos.y, pos.z)
                }
            },
            None => match self.tool {
                Tool::Face => "Face tool: click a face of a brush".into(),
                Tool::Clip => "Clip tool: select a brush, draw a line in a 2D view".into(),
                Tool::Select => "Nothing selected (left click to select)".into(),
            },
        }
    }

    /// The cutting plane of the clip line: normal and distance.
    fn clip_plane(&self) -> Option<(Vec3, f32)> {
        let (pane, a, b) = self.clip?;
        let (u, v) = pane.axes();
        let w = 3 - u - v;
        let mut depth = Vec3::ZERO;
        depth[w] = 1.0;
        let dir = axis_point(u, v, b.x - a.x, b.y - a.y);
        let n = dir.cross(depth).normalize_or_zero();
        if n == Vec3::ZERO {
            return None;
        }
        Some((n, n.dot(axis_point(u, v, a.x, a.y))))
    }

    /// Cuts the selected brush along the clip line.
    pub(crate) fn apply_clip(&mut self) {
        let Some(Sel::Brush(i)) = self.sel else {
            self.say("Select a brush to cut first");
            return;
        };
        let Some((n, d)) = self.clip_plane() else {
            self.say("Draw a cutting line in a 2D view first");
            return;
        };
        let (front, back) = split_brush(&self.game.map.world.brushes[i], n, d);
        let (Some(f), Some(b)) = (front, back) else {
            self.say("The line does not cross the brush");
            return;
        };
        match self.clip_keep {
            1 => self.map().world.brushes[i] = f,
            2 => self.map().world.brushes[i] = b,
            _ => {
                self.map().world.brushes[i] = f;
                self.map().world.brushes.push(b);
            }
        }
        self.clip = None;
        self.dirty = true;
        self.say("Cut");
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

    fn fly(&mut self, dt: f32, persp: Rect) {
        let (mx, my) = mouse_position();
        let m = vec2(mx, my);
        let rmb = is_mouse_button_down(MouseButton::Right);
        let start = is_mouse_button_pressed(MouseButton::Right) && persp.contains(m) && self.drag.is_none();
        if rmb && !self.looking && start {
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
        if is_key_pressed(KeyCode::Tab) {
            self.quad = !self.quad;
            self.center_views();
        }
        if is_key_pressed(KeyCode::C) {
            self.center_views();
        }
        for (k, t) in [(KeyCode::Key1, Tool::Select), (KeyCode::Key2, Tool::Face), (KeyCode::Key3, Tool::Clip)] {
            if is_key_pressed(k) {
                self.set_tool(t);
            }
        }
        if (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter)) && self.tool == Tool::Clip {
            self.apply_clip();
        }
    }

    fn layout(&self, area: Rect) -> Vec<(Pane, Rect)> {
        if !self.quad {
            return vec![(Pane::Persp, area)];
        }
        let g = 3.0;
        let w = (area.w - g) * 0.5;
        let h = (area.h - g) * 0.5;
        vec![
            (Pane::Persp, Rect { x: area.x, y: area.y, w, h }),
            (Pane::Top, Rect { x: area.x + w + g, y: area.y, w, h }),
            (Pane::Front, Rect { x: area.x, y: area.y + h + g, w, h }),
            (Pane::Side, Rect { x: area.x + w + g, y: area.y + h + g, w, h }),
        ]
    }

    /// One editor frame. Draws everything and returns what the app should do.
    pub fn frame(&mut self, renderer: &mut Renderer, dt: f32) -> EditorAction {
        let sw = screen_width();
        let sh = screen_height();
        let s = sh / 720.0;
        let pw = 330.0 * s;
        let area = Rect { x: pw + 10.0 * s, y: 0.0, w: sw - pw - 10.0 * s, h: sh };
        self.panes = self.layout(area);
        let persp = self.panes[0].1;
        self.fly(dt, persp);
        self.shortcuts();
        self.panes = self.layout(area);
        let persp = self.panes[0].1;
        self.mouse();
        if self.dirty {
            renderer.rebuild_world(&self.game);
            self.dirty = false;
        }
        self.game.time += dt as f64;

        let view = View {
            pos: self.cam_pos,
            angles: self.cam_ang,
            fov: 90.0,
            first_person: None,
            viewport: Some((persp.x, persp.y, persp.w, persp.h)),
        };
        renderer.draw(&self.game, &view, 1.0, false);
        self.draw_overlays(&view);
        self.draw_gizmo(persp);
        for (pane, r) in self.panes.clone() {
            if pane != Pane::Persp {
                self.draw_ortho(pane, r);
            }
        }
        if self.quad {
            let (mx, my) = mouse_position();
            for (pane, r) in &self.panes {
                let hot = r.contains(vec2(mx, my));
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, Color::new(1.0, 0.69, 0.1, if hot { 0.8 } else { 0.3 }));
                text_shadow(pane.title(), r.x + 8.0, r.y + 18.0, 16.0 * s, Color::new(1.0, 1.0, 1.0, 0.85));
            }
        }
        self.panel(renderer, s, pw)
    }

    // ------------------------------------------------------------------
    // Mouse: gizmo, 2D views, picking

    fn pane_rect(&self, pane: Pane) -> Option<Rect> {
        self.panes.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r)
    }

    fn w2s(&self, pane: Pane, r: Rect, p: Vec3) -> Vec2 {
        let (u, v) = pane.axes();
        let o = self.ortho[pane.index()];
        let c = r.center();
        vec2(c.x + (p[u] - o.center.x) / o.scale, c.y - (p[v] - o.center.y) / o.scale)
    }

    fn s2w(&self, pane: Pane, r: Rect, m: Vec2) -> Vec2 {
        let o = self.ortho[pane.index()];
        let c = r.center();
        vec2(o.center.x + (m.x - c.x) * o.scale, o.center.y - (m.y - c.y) * o.scale)
    }

    fn snap1(&self, v: f32) -> f32 {
        (v / self.grid).round() * self.grid
    }

    /// Screen positions of the gizmo origin and its X / Y / Z arrow tips.
    fn gizmo(&self, r: Rect) -> Option<(Vec2, [Option<Vec2>; 3], f32)> {
        let (a, b) = self.handle_bounds()?;
        let c = (a + b) * 0.5;
        let len = ((c - self.cam_pos).length() * 0.14).clamp(24.0, 6000.0);
        let o = self.project(r, c)?;
        let mut tips = [None; 3];
        for (axis, tip) in tips.iter_mut().enumerate() {
            let mut d = Vec3::ZERO;
            d[axis] = len;
            *tip = self.project(r, c + d).filter(|t| (*t - o).length() > 12.0);
        }
        Some((o, tips, len))
    }

    fn gizmo_hit(&self, r: Rect, m: Vec2) -> Option<usize> {
        let (o, tips, _) = self.gizmo(r)?;
        let mut best: Option<(f32, usize)> = None;
        for (axis, tip) in tips.iter().enumerate() {
            let Some(t) = tip else { continue };
            let d = dist_to_segment(m, o, *t);
            if d < 9.0 && best.is_none_or(|b| d < b.0) {
                best = Some((d, axis));
            }
        }
        best.map(|b| b.1)
    }

    /// The selection box's resize handle under the mouse in a 2D view.
    fn handle_hit(&self, pane: Pane, r: Rect, m: Vec2) -> Option<(i32, i32)> {
        if !self.sel_resizable() {
            return None;
        }
        let (a, b) = self.sel_bounds()?;
        let p0 = self.w2s(pane, r, a);
        let p1 = self.w2s(pane, r, b);
        for hu in [-1, 0, 1] {
            for hv in [-1, 0, 1] {
                if hu == 0 && hv == 0 {
                    continue;
                }
                let x = match hu {
                    -1 => p0.x,
                    1 => p1.x,
                    _ => (p0.x + p1.x) * 0.5,
                };
                // screen y grows downward: the max edge is p1.y
                let y = match hv {
                    -1 => p0.y,
                    1 => p1.y,
                    _ => (p0.y + p1.y) * 0.5,
                };
                if (m.x - x).abs() <= 6.0 && (m.y - y).abs() <= 6.0 {
                    return Some((hu, hv));
                }
            }
        }
        None
    }

    /// Selects what is under the mouse in a 2D view: the smallest thing
    /// whose outline contains the point.
    fn pick_2d(&mut self, pane: Pane, w: Vec2) {
        let (u, v) = pane.axes();
        let inside = |a: Vec3, b: Vec3| w.x >= a[u] && w.x <= b[u] && w.y >= a[v] && w.y <= b[v];
        let area = |a: Vec3, b: Vec3| (b[u] - a[u]) * (b[v] - a[v]);
        let mut best: Option<(f32, Sel)> = None;
        let mut consider = |a: Vec3, b: Vec3, s: Sel, bias: f32| {
            if inside(a, b) {
                let k = area(a, b) * bias;
                if best.is_none_or(|x| k < x.0) {
                    best = Some((k, s));
                }
            }
        };
        let m = &self.game.map;
        for (i, b) in m.world.brushes.iter().enumerate() {
            consider(b.mins, b.maxs, Sel::Brush(i), 1.0);
        }
        for t in 0..2 {
            for (i, sp) in m.spawns[t].iter().enumerate() {
                consider(sp.pos - vec3(16.0, 16.0, 36.0), sp.pos + vec3(16.0, 16.0, 36.0), Sel::Spawn(t, i), 0.1);
            }
        }
        for (i, p) in m.pickups.iter().enumerate() {
            consider(p.pos - Vec3::splat(20.0), p.pos + Vec3::splat(20.0), Sel::Pickup(i), 0.1);
        }
        for (i, b) in m.boosters.iter().enumerate() {
            consider(b.zone.mins, b.zone.maxs, Sel::Booster(i), 0.5);
        }
        for (i, t) in m.teleports.iter().enumerate() {
            consider(t.zone.mins, t.zone.maxs, Sel::Teleport(i), 0.5);
        }
        let prev = self.sel;
        self.sel = best.map(|b| b.1);
        if self.sel != prev {
            self.face = None;
        }
        if self.tool == Tool::Face {
            self.pick_face_2d(pane, w);
        }
    }

    fn mouse(&mut self) {
        let (mx, my) = mouse_position();
        let m = vec2(mx, my);
        if let Some(drag) = self.drag.clone() {
            let released = match drag.kind {
                DragKind::Pan { .. } => {
                    !is_mouse_button_down(MouseButton::Right) && !is_mouse_button_down(MouseButton::Middle)
                }
                _ => !is_mouse_button_down(MouseButton::Left),
            };
            self.update_drag(&drag, m);
            if released {
                self.drag = None;
            }
            return;
        }
        if self.looking || self.naming || in_modal(m) || self.drop.is_some() {
            return;
        }
        let Some((pane, r)) = self.panes.iter().copied().find(|(_, r)| r.contains(m)) else {
            self.hover_axis = None;
            return;
        };
        if pane == Pane::Persp {
            self.hover_axis = self.gizmo_hit(r, m);
            if is_mouse_button_pressed(MouseButton::Left) {
                match self.hover_axis.zip(self.gizmo(r)) {
                    Some((axis, (o, tips, len))) => {
                        let t = tips[axis].unwrap_or(o + Vec2::X);
                        self.drag = Some(Drag {
                            kind: DragKind::Axis {
                                axis,
                                screen_dir: (t - o).normalize_or(Vec2::X),
                                px_per_unit: (t - o).length() / len,
                            },
                            start_mouse: m,
                            snap: self.snapshot(),
                        });
                    }
                    None => self.pick(r, m),
                }
            }
            return;
        }

        // 2D views.
        self.hover_axis = None;
        let (_, wy) = mouse_wheel();
        if wy != 0.0 {
            let before = self.s2w(pane, r, m);
            let o = &mut self.ortho[pane.index()];
            o.scale = (o.scale * if wy > 0.0 { 1.0 / 1.25 } else { 1.25 }).clamp(0.25, 200.0);
            let after = self.s2w(pane, r, m);
            self.ortho[pane.index()].center += before - after;
        }
        if is_mouse_button_pressed(MouseButton::Right) || is_mouse_button_pressed(MouseButton::Middle) {
            self.drag = Some(Drag { kind: DragKind::Pan { pane, last: m }, start_mouse: m, snap: None });
            return;
        }
        if is_mouse_button_pressed(MouseButton::Left) && self.tool == Tool::Clip {
            let w = self.s2w(pane, r, m);
            let w = vec2(self.snap1(w.x), self.snap1(w.y));
            self.clip = Some((pane, w, w));
            self.drag = Some(Drag { kind: DragKind::ClipLine { pane, start: w }, start_mouse: m, snap: None });
            return;
        }
        if is_mouse_button_pressed(MouseButton::Left) {
            let w = self.s2w(pane, r, m);
            if let Some((hu, hv)) = self.handle_hit(pane, r, m) {
                let (mins, maxs) = self.sel_bounds().unwrap_or_default();
                self.drag = Some(Drag {
                    kind: DragKind::Resize { pane, start: w, hu, hv, mins, maxs },
                    start_mouse: m,
                    snap: self.snapshot(),
                });
                return;
            }
            let (u, v) = pane.axes();
            let in_sel =
                self.sel_bounds().is_some_and(|(a, b)| w.x >= a[u] && w.x <= b[u] && w.y >= a[v] && w.y <= b[v]);
            if !in_sel {
                self.pick_2d(pane, w);
            } else if self.tool == Tool::Face {
                self.pick_face_2d(pane, w);
            }
            if self.sel.is_some() {
                self.drag =
                    Some(Drag { kind: DragKind::Move { pane, start: w }, start_mouse: m, snap: self.snapshot() });
            }
        }
    }

    fn update_drag(&mut self, drag: &Drag, m: Vec2) {
        match drag.kind {
            DragKind::Axis { axis, screen_dir, px_per_unit } => {
                let Some(snap) = &drag.snap else { return };
                let t = (m - drag.start_mouse).dot(screen_dir) / px_per_unit.max(1e-4);
                let mut d = Vec3::ZERO;
                d[axis] = self.snap1(t);
                self.apply(snap, |p| p + d, true);
            }
            DragKind::Move { pane, start } => {
                let (Some(snap), Some(r)) = (&drag.snap, self.pane_rect(pane)) else { return };
                let dd = self.s2w(pane, r, m) - start;
                let (u, v) = pane.axes();
                let mut d = Vec3::ZERO;
                d[u] = self.snap1(dd.x);
                d[v] = self.snap1(dd.y);
                self.apply(snap, |p| p + d, true);
            }
            DragKind::Resize { pane, start, hu, hv, mins, maxs } => {
                let (Some(snap), Some(r)) = (&drag.snap, self.pane_rect(pane)) else { return };
                let dd = self.s2w(pane, r, m) - start;
                let (u, v) = pane.axes();
                let g = self.grid;
                let (mut lo, mut hi) = (mins, maxs);
                for (ax, h, dv) in [(u, hu, dd.x), (v, hv, dd.y)] {
                    if h > 0 {
                        hi[ax] = self.snap1(maxs[ax] + dv).max(lo[ax] + g);
                    } else if h < 0 {
                        lo[ax] = self.snap1(mins[ax] + dv).min(hi[ax] - g);
                    }
                }
                let remap = move |p: Vec3| {
                    let mut q = p;
                    for ax in 0..3 {
                        let ext = maxs[ax] - mins[ax];
                        if ext > 1e-3 {
                            q[ax] = lo[ax] + (p[ax] - mins[ax]) / ext * (hi[ax] - lo[ax]);
                        }
                    }
                    q
                };
                self.apply(snap, remap, false);
            }
            DragKind::ClipLine { pane, start } => {
                let Some(r) = self.pane_rect(pane) else { return };
                let w = self.s2w(pane, r, m);
                let w = vec2(self.snap1(w.x), self.snap1(w.y));
                self.clip = Some((pane, start, w));
                let released = !is_mouse_button_down(MouseButton::Left);
                if released && (m - drag.start_mouse).length() < 5.0 {
                    // a click, not a line: select instead
                    self.clip = None;
                    let w = self.s2w(pane, r, m);
                    self.pick_2d(pane, w);
                }
            }
            DragKind::Pan { pane, last } => {
                let o = &mut self.ortho[pane.index()];
                o.center.x -= (m.x - last.x) * o.scale;
                o.center.y += (m.y - last.y) * o.scale;
                if let Some(d) = self.drag.as_mut() {
                    d.kind = DragKind::Pan { pane, last: m };
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Drawing

    fn draw_gizmo(&self, r: Rect) {
        let Some((o, tips, _)) = self.gizmo(r) else { return };
        scissor(Some(r));
        let dragging = match self.drag.as_ref().map(|d| d.kind) {
            Some(DragKind::Axis { axis, .. }) => Some(axis),
            _ => None,
        };
        let colors = [Color::new(1.0, 0.25, 0.2, 1.0), Color::new(0.3, 1.0, 0.3, 1.0), Color::new(0.3, 0.55, 1.0, 1.0)];
        for (axis, tip) in tips.iter().enumerate() {
            let Some(t) = tip else { continue };
            let hot = self.hover_axis == Some(axis) || dragging == Some(axis);
            let c = if hot { YELLOW } else { colors[axis] };
            let d = (*t - o).normalize_or(Vec2::X);
            let n = vec2(-d.y, d.x);
            draw_line(o.x, o.y, t.x - d.x * 10.0, t.y - d.y * 10.0, if hot { 4.0 } else { 3.0 }, BLACK);
            draw_line(o.x, o.y, t.x - d.x * 10.0, t.y - d.y * 10.0, if hot { 3.0 } else { 2.0 }, c);
            draw_triangle(*t + d * 4.0, *t - d * 12.0 + n * 6.0, *t - d * 12.0 - n * 6.0, c);
            let label = ["X", "Y", "Z"][axis];
            text_shadow(label, t.x + d.x * 8.0 - 4.0, t.y + d.y * 8.0 + 5.0, 16.0, c);
        }
        draw_rectangle(o.x - 4.0, o.y - 4.0, 8.0, 8.0, WHITE);
        scissor(None);
    }

    fn draw_ortho(&self, pane: Pane, r: Rect) {
        scissor(Some(r));
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.09, 0.09, 0.11, 1.0));
        let o = self.ortho[pane.index()];
        let (u, v) = pane.axes();
        // Grid: the editor grid, doubled until lines are at least 6 px apart.
        let mut step = self.grid;
        while step / o.scale < 6.0 {
            step *= 2.0;
        }
        let w0 = self.s2w(pane, r, vec2(r.x, r.y + r.h));
        let w1 = self.s2w(pane, r, vec2(r.x + r.w, r.y));
        let major = step * 8.0;
        let mut k = (w0.x / step).floor() * step;
        while k <= w1.x {
            let x = self.w2s(pane, r, axis_point(u, v, k, 0.0)).x;
            let c = if k.abs() < 0.5 {
                Color::new(0.6, 0.3, 0.3, 0.8)
            } else if (k / major).fract().abs() < 1e-4 {
                Color::new(1.0, 1.0, 1.0, 0.18)
            } else {
                Color::new(1.0, 1.0, 1.0, 0.07)
            };
            draw_line(x, r.y, x, r.y + r.h, 1.0, c);
            k += step;
        }
        let mut k = (w0.y / step).floor() * step;
        while k <= w1.y {
            let y = self.w2s(pane, r, axis_point(u, v, 0.0, k)).y;
            let c = if k.abs() < 0.5 {
                Color::new(0.3, 0.6, 0.3, 0.8)
            } else if (k / major).fract().abs() < 1e-4 {
                Color::new(1.0, 1.0, 1.0, 0.18)
            } else {
                Color::new(1.0, 1.0, 1.0, 0.07)
            };
            draw_line(r.x, y, r.x + r.w, y, 1.0, c);
            k += step;
        }

        let map = &self.game.map;
        // Brushes as wireframes in their own colour.
        for (i, b) in map.world.brushes.iter().enumerate() {
            if self.sel == Some(Sel::Brush(i)) {
                continue;
            }
            let c = b.mat.color;
            let col =
                if b.mat.visible() { Color::from_rgba(c[0], c[1], c[2], 190) } else { Color::new(0.8, 0.3, 0.8, 0.35) };
            self.draw_brush_2d(pane, r, b, col, 1.0);
        }
        for (t, list) in map.spawns.iter().enumerate() {
            let c = if t == 0 { Color::new(1.0, 0.45, 0.2, 1.0) } else { Color::new(0.35, 0.6, 1.0, 1.0) };
            for sp in list {
                self.rect_2d(pane, r, sp.pos - vec3(16.0, 16.0, 36.0), sp.pos + vec3(16.0, 16.0, 36.0), c, 1.5);
            }
        }
        for p in &map.pickups {
            let c = match p.kind {
                PickupKind::Health => Color::new(0.3, 1.0, 0.4, 1.0),
                PickupKind::Weapon(_) => Color::new(1.0, 0.8, 0.3, 1.0),
                PickupKind::Attachment(a) => {
                    let c = a.color();
                    Color::from_rgba(c[0], c[1], c[2], 255)
                }
            };
            self.rect_2d(pane, r, p.pos - Vec3::splat(20.0), p.pos + Vec3::splat(20.0), c, 1.5);
        }
        for b in &map.boosters {
            match b.push {
                Push::Boost { dir, .. } => {
                    let c = Color::new(0.3, 1.0, 0.9, 0.9);
                    self.rect_2d(pane, r, b.zone.mins, b.zone.maxs, c, 1.5);
                    let mid = (b.zone.mins + b.zone.maxs) * 0.5;
                    let a = self.w2s(pane, r, mid);
                    let t = self.w2s(pane, r, mid + dir * 200.0);
                    draw_line(a.x, a.y, t.x, t.y, 2.0, c);
                }
                Push::Launch { target, .. } => {
                    let c = Color::new(1.0, 0.6, 0.2, 0.9);
                    self.rect_2d(pane, r, b.zone.mins, b.zone.maxs, c, 1.5);
                    let mut prev = b.pad() + vec3(0.0, 0.0, 36.0);
                    let v = b.launch_velocity(self.game.vars.gravity).unwrap_or(Vec3::Z);
                    let g = self.game.vars.gravity;
                    for k in 1..=24 {
                        let t = k as f32 * 0.1;
                        let p = b.pad() + vec3(0.0, 0.0, 36.0) + v * t - vec3(0.0, 0.0, 0.5 * g * t * t);
                        let (a, q) = (self.w2s(pane, r, prev), self.w2s(pane, r, p));
                        draw_line(a.x, a.y, q.x, q.y, 1.0, c);
                        prev = p;
                        if (p - b.pad()).length() > (target - b.pad()).length() * 1.4 {
                            break;
                        }
                    }
                    let t = self.w2s(pane, r, target);
                    draw_line(t.x - 5.0, t.y - 5.0, t.x + 5.0, t.y + 5.0, 1.5, c);
                    draw_line(t.x - 5.0, t.y + 5.0, t.x + 5.0, t.y - 5.0, 1.5, c);
                }
            }
        }
        // Teleports and bot paths.
        for (i, t) in map.teleports.iter().enumerate() {
            let c = Color::new(0.8, 0.4, 1.0, 0.9);
            if (t.zone.maxs - t.zone.mins).x > 8000.0 && self.sel != Some(Sel::Teleport(i)) {
                continue;
            }
            self.rect_2d(pane, r, t.zone.mins, t.zone.maxs, c, 1.5);
            if let TeleDest::Point { pos, .. } = t.dest {
                let a = self.w2s(pane, r, (t.zone.mins + t.zone.maxs) * 0.5);
                let q = self.w2s(pane, r, pos);
                draw_line(a.x, a.y, q.x, q.y, 1.0, c);
                draw_circle_lines(q.x, q.y, 6.0, 1.5, c);
            }
        }
        if self.show_paths {
            for rt in &map.routes {
                let c =
                    if rt.team == Team::T { Color::new(1.0, 0.55, 0.2, 0.9) } else { Color::new(0.35, 0.65, 1.0, 0.9) };
                for w in rt.points.windows(2) {
                    let (a, q) = (self.w2s(pane, r, w[0].pos), self.w2s(pane, r, w[1].pos));
                    draw_line(a.x, a.y, q.x, q.y, 1.5, c);
                }
                for w in &rt.points {
                    let q = self.w2s(pane, r, w.pos);
                    draw_circle(q.x, q.y, 3.5, mode_color(w.mode));
                }
            }
        }
        // Clip line, stretched across the view, and the halves it makes.
        if let Some((cp, a, b)) = self.clip {
            if cp == pane && a != b {
                let (sa, sb) =
                    (self.w2s(pane, r, axis_point(u, v, a.x, a.y)), self.w2s(pane, r, axis_point(u, v, b.x, b.y)));
                let d = (sb - sa).normalize_or(Vec2::X) * 5000.0;
                draw_line(sa.x - d.x, sa.y - d.y, sb.x + d.x, sb.y + d.y, 1.0, Color::new(1.0, 0.3, 0.3, 0.6));
                draw_line(sa.x, sa.y, sb.x, sb.y, 2.5, Color::new(1.0, 0.3, 0.3, 1.0));
                draw_circle(sa.x, sa.y, 4.0, WHITE);
                draw_circle(sb.x, sb.y, 4.0, WHITE);
            }
            if let (Some(Sel::Brush(i)), Some((n, d))) = (self.sel, self.clip_plane()) {
                let (f, k) = split_brush(&map.world.brushes[i], n, d);
                if let Some(f) = f {
                    self.draw_brush_2d(pane, r, &f, Color::new(0.3, 1.0, 0.3, 1.0), 1.5);
                }
                if let Some(k) = k {
                    self.draw_brush_2d(pane, r, &k, Color::new(1.0, 0.3, 0.3, 1.0), 1.5);
                }
            }
        }
        // Selection on top with its resize handles.
        if let Some(v) = self.face_verts() {
            for k in 0..v.len() {
                let (a, q) = (self.w2s(pane, r, v[k]), self.w2s(pane, r, v[(k + 1) % v.len()]));
                draw_line(a.x, a.y, q.x, q.y, 3.0, Color::new(0.2, 1.0, 1.0, 1.0));
            }
        }
        if let Some((a, b)) = self.handle_bounds() {
            if let Some(Sel::Brush(i)) = self.sel {
                self.draw_brush_2d(pane, r, &map.world.brushes[i], YELLOW, 1.5);
            }
            self.rect_2d(pane, r, a, b, Color::new(1.0, 1.0, 0.3, 0.8), 1.0);
            if self.sel_resizable() {
                let p0 = self.w2s(pane, r, a);
                let p1 = self.w2s(pane, r, b);
                for x in [p0.x, (p0.x + p1.x) * 0.5, p1.x] {
                    for y in [p0.y, (p0.y + p1.y) * 0.5, p1.y] {
                        if x == (p0.x + p1.x) * 0.5 && y == (p0.y + p1.y) * 0.5 {
                            continue;
                        }
                        draw_rectangle(x - 4.0, y - 4.0, 8.0, 8.0, WHITE);
                        draw_rectangle_lines(x - 4.0, y - 4.0, 8.0, 8.0, 1.0, BLACK);
                    }
                }
                let ext = b - a;
                let label = format!("{:.0} x {:.0}", ext[u], ext[v]);
                text_shadow(&label, p1.x + 8.0, p1.y - 4.0, 14.0, YELLOW);
            }
        }
        // Camera position in the top view.
        let cp = self.w2s(pane, r, self.cam_pos);
        draw_circle(cp.x, cp.y, 4.0, Color::new(1.0, 1.0, 1.0, 0.8));
        if pane == Pane::Top {
            let (f, _, _) = angle_vectors(vec3(0.0, self.cam_ang.y, 0.0));
            let t = self.w2s(pane, r, self.cam_pos + f * 30.0 * o.scale);
            draw_line(cp.x, cp.y, t.x, t.y, 1.5, Color::new(1.0, 1.0, 1.0, 0.8));
        }
        text(
            &format!("grid {}  (x{:.1} px)", self.grid, self.grid / o.scale),
            r.x + 8.0,
            r.y + r.h - 8.0,
            13.0,
            Color::new(1.0, 1.0, 1.0, 0.5),
        );
        scissor(None);
    }

    fn draw_brush_2d(&self, pane: Pane, r: Rect, b: &Brush, c: Color, th: f32) {
        for face in &b.faces {
            let n = face.verts.len();
            for k in 0..n {
                let a = self.w2s(pane, r, face.verts[k]);
                let q = self.w2s(pane, r, face.verts[(k + 1) % n]);
                draw_line(a.x, a.y, q.x, q.y, th, c);
            }
        }
    }

    fn rect_2d(&self, pane: Pane, r: Rect, a: Vec3, b: Vec3, c: Color, th: f32) {
        let p0 = self.w2s(pane, r, a);
        let p1 = self.w2s(pane, r, b);
        let (x0, x1) = (p0.x.min(p1.x), p0.x.max(p1.x));
        let (y0, y1) = (p0.y.min(p1.y), p0.y.max(p1.y));
        draw_rectangle_lines(x0, y0, (x1 - x0).max(2.0), (y1 - y0).max(2.0), th, c);
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
            Some(Sel::Booster(_)) | Some(Sel::Teleport(_)) | None => {}
        }
        // The selected face: outline and a translucent fill.
        if let Some(v) = self.face_verts() {
            let c = Color::new(0.2, 1.0, 1.0, 1.0);
            for k in 0..v.len() {
                let (a, b) = (v[k], v[(k + 1) % v.len()]);
                draw_line_3d(a, b, c);
                draw_line_3d(a + (v[0] - a) * 0.02, b + (v[0] - b) * 0.02, c);
            }
            let n = self.face.unwrap_or(Vec3::Z);
            let verts: Vec<Vertex> = v
                .iter()
                .map(|p| Vertex {
                    position: *p + n * 1.5,
                    uv: Vec2::ZERO,
                    color: [60, 255, 255, 80],
                    normal: Vec4::ZERO,
                })
                .collect();
            let indices: Vec<u16> = (1..v.len() as u16 - 1).flat_map(|k| [0, k, k + 1]).collect();
            draw_mesh(&Mesh { vertices: verts, indices, texture: None });
        }
        // Teleports: purple volumes with a line to where they send you.
        for (i, t) in self.game.map.teleports.iter().enumerate() {
            let c = if self.sel == Some(Sel::Teleport(i)) { YELLOW } else { Color::new(0.8, 0.4, 1.0, 1.0) };
            if (t.zone.maxs - t.zone.mins).x > 8000.0 && self.sel != Some(Sel::Teleport(i)) {
                continue; // the whole map floor: just noise
            }
            draw_cube_wires((t.zone.mins + t.zone.maxs) * 0.5, t.zone.maxs - t.zone.mins, c);
            if let TeleDest::Point { pos, yaw } = t.dest {
                draw_line_3d((t.zone.mins + t.zone.maxs) * 0.5, pos, c);
                draw_cube_wires(pos, vec3(32.0, 32.0, 72.0), c);
                let (f, _, _) = angle_vectors(vec3(0.0, yaw, 0.0));
                draw_line_3d(pos, pos + f * 60.0, c);
            }
        }
        // Clip preview: the two halves.
        if let (Some(Sel::Brush(i)), Some((n, d))) = (self.sel, self.clip_plane()) {
            let (f, b) = split_brush(&self.game.map.world.brushes[i], n, d);
            for (half, c) in [(f, Color::new(0.3, 1.0, 0.3, 1.0)), (b, Color::new(1.0, 0.3, 0.3, 1.0))] {
                if let Some(h) = half {
                    for face in &h.faces {
                        for k in 0..face.verts.len() {
                            draw_line_3d(face.verts[k], face.verts[(k + 1) % face.verts.len()], c);
                        }
                    }
                }
            }
        }
        if self.show_paths {
            for r in &self.game.map.routes {
                let c =
                    if r.team == Team::T { Color::new(1.0, 0.55, 0.2, 1.0) } else { Color::new(0.35, 0.65, 1.0, 1.0) };
                for w in r.points.windows(2) {
                    draw_line_3d(w[0].pos + Vec3::Z * 8.0, w[1].pos + Vec3::Z * 8.0, c);
                }
                for w in &r.points {
                    draw_cube(w.pos + Vec3::Z * 8.0, Vec3::splat(18.0), None, mode_color(w.mode));
                }
            }
        }
        for (i, b) in self.game.map.boosters.iter().enumerate() {
            let sel = self.sel == Some(Sel::Booster(i));
            let c = match b.push {
                Push::Boost { .. } => Color::new(0.3, 1.0, 0.9, 1.0),
                Push::Launch { .. } => Color::new(1.0, 0.6, 0.2, 1.0),
            };
            let c = if sel { YELLOW } else { c };
            draw_cube_wires((b.zone.mins + b.zone.maxs) * 0.5, b.zone.maxs - b.zone.mins, c);
            match b.push {
                Push::Boost { dir, .. } => {
                    let mid = (b.zone.mins + b.zone.maxs) * 0.5;
                    draw_line_3d(mid, mid + dir * 220.0, c);
                }
                Push::Launch { target, .. } => {
                    let g = self.game.vars.gravity;
                    let v = b.launch_velocity(g).unwrap_or(Vec3::Z);
                    let start = b.pad() + vec3(0.0, 0.0, 36.0);
                    let mut prev = start;
                    for k in 1..=40 {
                        let t = k as f32 * 0.06;
                        let p = start + v * t - vec3(0.0, 0.0, 0.5 * g * t * t);
                        draw_line_3d(prev, p, c);
                        prev = p;
                        if (p - start).length() > (target - start).length() * 1.4 {
                            break;
                        }
                    }
                    draw_cube_wires(target, Vec3::splat(24.0), c);
                }
            }
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

    fn set_tool(&mut self, t: Tool) {
        self.tool = t;
        self.face = None;
        self.clip = None;
        match t {
            Tool::Face => self.say("Face tool: click a face, then drag the arrows (or drag it in a 2D view)"),
            Tool::Clip => {
                if !self.quad {
                    self.quad = true;
                    self.center_views();
                }
                self.say("Clip tool: select a brush, draw a line across it in a 2D view, Enter cuts");
            }
            Tool::Select => {}
        }
    }

    /// A button that opens a drop down list.
    fn drop_button(&mut self, id: Drop, label: &str, x: f32, y: f32, w: f32, h: f32) {
        let open = self.drop.is_some_and(|(d, _)| d == id);
        if open {
            // keep the list attached to its button
            self.drop = Some((id, Rect { x, y, w, h }));
        }
        if button_lit(&format!("{label}  v"), x, y, w, h, open) {
            self.drop = if open { None } else { Some((id, Rect { x, y, w, h })) };
        }
    }

    /// Items of a drop down list: label and an optional colour swatch.
    fn drop_items(&self, id: Drop) -> Vec<(String, Option<[u8; 3]>)> {
        match id {
            Drop::PickupAdd | Drop::PickupSel => pickup_kinds()
                .into_iter()
                .map(|k| {
                    let sw = match k {
                        PickupKind::Health => Some([80, 230, 110]),
                        PickupKind::Weapon(_) => Some([240, 200, 80]),
                        PickupKind::Attachment(a) => Some(a.color()),
                    };
                    (pickup_name(k).to_string(), sw)
                })
                .collect(),
            Drop::Colour => PALETTE.iter().enumerate().map(|(i, c)| (format!("Colour {}", i + 1), Some(*c))).collect(),
            Drop::Texture => TEXTURES.iter().map(|t| (mapfile::tex_key(*t).to_string(), None)).collect(),
            Drop::Sky => SKY_NAMES
                .iter()
                .zip(SKIES.iter())
                .map(|(n, (t, _, _))| {
                    (n.to_string(), Some([(t[0] * 255.0) as u8, (t[1] * 255.0) as u8, (t[2] * 255.0) as u8]))
                })
                .collect(),
            Drop::Scenery => Backdrop::ALL.iter().map(|b| (b.name().to_string(), None)).collect(),
            Drop::Speed => SPEEDS.iter().map(|v| (format!("{v:.0} u/s"), None)).collect(),
            Drop::Keep => KEEP_NAMES.iter().map(|n| (n.to_string(), None)).collect(),
            Drop::Load => map::map_names().into_iter().map(|n| (n, None)).collect(),
        }
    }

    fn drop_choose(&mut self, id: Drop, i: usize, renderer: &mut Renderer) {
        match id {
            Drop::PickupAdd => self.pickup_kind = i,
            Drop::PickupSel => {
                if let Some(Sel::Pickup(k)) = self.sel {
                    self.map().pickups[k].kind = pickup_kinds()[i];
                    self.sync_pickups();
                }
                self.pickup_kind = i;
            }
            Drop::Colour => {
                self.color = i;
                if let Some(Sel::Brush(b)) = self.sel {
                    let mat = self.current_mat();
                    self.map().world.brushes[b].mat = mat;
                    self.dirty = true;
                }
            }
            Drop::Texture => {
                self.tex = i;
                if let Some(Sel::Brush(b)) = self.sel {
                    let mat = self.current_mat();
                    self.map().world.brushes[b].mat = mat;
                    self.dirty = true;
                }
            }
            Drop::Sky => {
                self.sky = i;
                let (t, hz, f) = SKIES[i];
                let m = self.map();
                m.sky_top = t;
                m.sky_horizon = hz;
                m.fog_color = f;
                self.dirty = true;
            }
            Drop::Scenery => {
                self.map().backdrop = Backdrop::ALL[i];
                self.dirty = true;
            }
            Drop::Speed => {
                if let Some(Sel::Booster(k)) = self.sel {
                    if let Push::Boost { speed, .. } = &mut self.map().boosters[k].push {
                        *speed = SPEEDS[i];
                    }
                    self.dirty = true;
                }
            }
            Drop::Keep => self.clip_keep = i,
            Drop::Load => {
                let names = map::map_names();
                if let Some(name) = names.get(i) {
                    self.load_index = i;
                    self.game = Game::with_map(map::load(name), empty_settings(), 1);
                    self.sel = None;
                    self.face = None;
                    self.clip = None;
                    self.dirty = true;
                    renderer.rebuild_world(&self.game);
                    self.say(format!("Loaded {name}"));
                }
            }
        }
    }

    /// Draws the open drop down list over everything and handles clicks on it.
    fn drop_list(&mut self, renderer: &mut Renderer, s: f32) {
        self.modal = None;
        let Some((id, anchor)) = self.drop else { return };
        let items = self.drop_items(id);
        let rh = 22.0 * s;
        let sh = screen_height();
        let rows_fit = (((sh - anchor.y - anchor.h - 8.0) / rh).floor() as usize).max(4);
        let cols = items.len().div_ceil(rows_fit).max(1);
        let rows = items.len().div_ceil(cols);
        let cw = anchor.w.max(170.0 * s);
        let list = Rect { x: anchor.x, y: anchor.y + anchor.h, w: cw * cols as f32, h: rh * rows as f32 };
        draw_rectangle(list.x, list.y, list.w, list.h, Color::new(0.05, 0.05, 0.07, 0.95));
        draw_rectangle_lines(list.x, list.y, list.w, list.h, 1.5, Color::new(1.0, 0.69, 0.1, 0.8));
        let (mx, my) = mouse_position();
        let m = vec2(mx, my);
        let mut chosen = None;
        for (i, (label, sw)) in items.iter().enumerate() {
            let (c, r) = (i / rows, i % rows);
            let cell = Rect { x: list.x + cw * c as f32, y: list.y + rh * r as f32, w: cw, h: rh };
            if cell.contains(m) {
                draw_rectangle(cell.x, cell.y, cell.w, cell.h, Color::new(1.0, 0.69, 0.1, 0.35));
                if is_mouse_button_pressed(MouseButton::Left) {
                    chosen = Some(i);
                }
            }
            let mut tx = cell.x + 8.0 * s;
            if let Some(c) = sw {
                draw_rectangle(tx, cell.y + 5.0 * s, 12.0 * s, rh - 10.0 * s, Color::from_rgba(c[0], c[1], c[2], 255));
                tx += 18.0 * s;
            }
            text(label, tx, cell.y + rh * 0.72, 15.0 * s, WHITE);
        }
        if let Some(i) = chosen {
            self.drop = None;
            self.drop_choose(id, i, renderer);
        } else if is_mouse_button_pressed(MouseButton::Left) && !list.contains(m) && !anchor.contains(m) {
            self.drop = None;
        } else {
            self.modal = Some(list);
        }
    }

    fn panel(&mut self, renderer: &mut Renderer, s: f32, pw: f32) -> EditorAction {
        let sh = screen_height();
        let sw = screen_width();
        // buttons under an open list ignore the mouse
        MODAL.with(|c| c.set(self.modal.map(|r| (r.x, r.y, r.w, r.h))));
        draw_rectangle(0.0, 0.0, pw + 10.0 * s, sh, Color::new(0.0, 0.0, 0.0, 0.6));
        let x = 10.0 * s;
        let h = 26.0 * s;
        let gap = 4.0 * s;
        let mut y = 8.0 * s;
        text_shadow("MAP EDITOR", x, y + 22.0 * s, 26.0 * s, HUD_COLOR);
        y += 32.0 * s;

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
        y += h + gap;

        let third = (pw - x - gap * 2.0) / 3.0;
        let half = (pw - x - gap) / 2.0;
        // Tools.
        for (i, (label, t)) in
            [("1 Select", Tool::Select), ("2 Face", Tool::Face), ("3 Clip", Tool::Clip)].iter().enumerate()
        {
            if button_lit(label, x + (third + gap) * i as f32, y, third, h, self.tool == *t) {
                self.set_tool(*t);
            }
        }
        y += h + gap * 2.0;

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
        if button("Booster", x, y, third, h) {
            self.add_booster(false);
        }
        if button("Launch pad", x + third + gap, y, third, h) {
            self.add_booster(true);
        }
        if button("Teleport", x + (third + gap) * 2.0, y, third, h) {
            self.add_teleport();
        }
        y += h + gap;
        let kinds = pickup_kinds();
        let wide = pw - x - third - gap;
        self.drop_button(Drop::PickupAdd, pickup_name(kinds[self.pickup_kind.min(kinds.len() - 1)]), x, y, wide, h);
        if button("Add", x + wide + gap, y, third, h) {
            self.add_pickup();
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
        let quarter = (pw - x - gap * 3.0) / 4.0;
        for (i, label) in ["Rot -15", "Rot +15", "Copy", "Delete"].iter().enumerate() {
            if button(label, x + (quarter + gap) * i as f32, y, quarter, h) {
                match i {
                    0 => self.rotate_sel(-15.0),
                    1 => self.rotate_sel(15.0),
                    2 => self.duplicate_sel(),
                    _ => self.delete_sel(),
                }
            }
        }
        y += h + gap;
        // What the selection can be changed to.
        match self.sel {
            Some(Sel::Pickup(i)) => {
                let k = self.game.map.pickups[i].kind;
                self.drop_button(Drop::PickupSel, &format!("Kind: {}", pickup_name(k)), x, y, pw - x, h);
            }
            Some(Sel::Booster(i)) => match self.game.map.boosters[i].push {
                Push::Boost { speed, two_way, .. } => {
                    self.drop_button(Drop::Speed, &format!("Speed {speed:.0}"), x, y, half, h);
                    if button(if two_way { "Two way" } else { "One way" }, x + half + gap, y, half, h) {
                        self.restyle_sel(false);
                    }
                }
                Push::Launch { .. } => {
                    text("Size X distance, Y time, Z height", x, y + h * 0.7, 15.0 * s, GRAY);
                }
            },
            Some(Sel::Teleport(i)) => {
                let to_spawn = self.game.map.teleports[i].dest == TeleDest::TeamSpawn;
                if button(if to_spawn { "Dest: team spawn" } else { "Dest: point" }, x, y, half, h) {
                    if to_spawn {
                        self.teleport_dest_here();
                    } else {
                        self.map().teleports[i].dest = TeleDest::TeamSpawn;
                    }
                }
                if button("Dest here", x + half + gap, y, half, h) {
                    self.teleport_dest_here();
                }
            }
            _ if self.tool == Tool::Clip => {
                self.drop_button(Drop::Keep, KEEP_NAMES[self.clip_keep], x, y, half, h);
                if button("Cut (Enter)", x + half + gap, y, half, h) {
                    self.apply_clip();
                }
            }
            _ => {
                let c = PALETTE[self.color];
                self.drop_button(Drop::Colour, "Colour", x, y, half, h);
                draw_rectangle(
                    x + 8.0 * s,
                    y + 6.0 * s,
                    10.0 * s,
                    h - 12.0 * s,
                    Color::from_rgba(c[0], c[1], c[2], 255),
                );
                let tex_name = mapfile::tex_key(TEXTURES[self.tex]);
                self.drop_button(Drop::Texture, &format!("Tex: {tex_name}"), x + half + gap, y, half, h);
            }
        }
        y += h + gap * 3.0;

        // Map settings.
        if button(&format!("Grid: {}", self.grid), x, y, third, h) {
            self.grid = match self.grid as i32 {
                8 => 16.0,
                16 => 32.0,
                32 => 64.0,
                64 => 128.0,
                _ => 8.0,
            };
        }
        self.drop_button(Drop::Sky, &format!("Sky: {}", SKY_NAMES[self.sky]), x + third + gap, y, third, h);
        if button(if self.quad { "1 view" } else { "4 views" }, x + (third + gap) * 2.0, y, third, h) {
            self.quad = !self.quad;
            self.center_views();
        }
        y += h + gap;
        self.drop_button(Drop::Scenery, self.game.map.backdrop.name(), x, y, half, h);
        if button_lit("Bot paths", x + half + gap, y, half, h, self.show_paths) {
            self.show_paths = !self.show_paths;
            if self.show_paths && self.game.map.routes.is_empty() {
                self.say("This map has no bot routes: bots roam (built-in maps have routes)");
            }
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
        self.drop_button(Drop::Load, "Load", x + third + gap, y, third, h);
        if button("New", x + (third + gap) * 2.0, y, third, h) {
            self.game = Game::with_map(new_map(), empty_settings(), 1);
            self.sel = None;
            self.face = None;
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
            "RMB look + WASD fly   LMB select / drag arrows",
            "Tab 4 views: drag moves, handles resize, C centre",
            "Arrows PgUp/Dn move  R/F rotate  Del  Ctrl+D/S",
        ];
        for l in help {
            text(l, x, y + 13.0 * s, 13.0 * s, Color::new(0.8, 0.8, 0.8, 1.0));
            y += 15.0 * s;
        }
        if let Some((m, t)) = &self.msg {
            if self.game.time < *t {
                let w = text_width(m, 20.0 * s);
                draw_rectangle(sw * 0.5 - w * 0.5 - 10.0, 14.0 * s, w + 20.0, 32.0 * s, Color::new(0.0, 0.0, 0.0, 0.6));
                text_shadow(m, sw * 0.5 - w * 0.5, 36.0 * s, 20.0 * s, WHITE);
            }
        }
        // crosshair for placement, in the middle of the 3D view
        let c = self.panes.first().map(|(_, r)| r.center()).unwrap_or(vec2(sw * 0.5, sh * 0.5));
        draw_line(c.x - 6.0, c.y, c.x + 6.0, c.y, 1.0, WHITE);
        draw_line(c.x, c.y - 6.0, c.x, c.y + 6.0, 1.0, WHITE);
        self.drop_list(renderer, s);
        MODAL.with(|c| c.set(None));
        let _ = WeaponId::Knife;
        action
    }
}
