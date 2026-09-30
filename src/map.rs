//! Map definitions. Maps are built from convex brushes in code, the same way
//! a Hammer map is made of brushes, plus spawn points, trigger volumes and
//! bot routes.

use macroquad::math::{vec3, Vec3};

use crate::collision::{Brush, CollisionWorld};
use crate::weapons::{AttItem, Grip, Muzzle, Sight, Stock, WeaponId};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Team {
    T,
    CT,
}

impl Team {
    pub fn index(self) -> usize {
        match self {
            Team::T => 0,
            Team::CT => 1,
        }
    }

    pub fn other(self) -> Team {
        match self {
            Team::T => Team::CT,
            Team::CT => Team::T,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Team::T => "Terrorists",
            Team::CT => "Counter-Terrorists",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tex {
    Grid,
    Crate,
    Metal,
    Concrete,
    Water,
    /// Invisible player clip.
    Clip,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mat {
    pub tex: Tex,
    pub color: [u8; 3],
}

impl Mat {
    pub const fn new(tex: Tex, color: [u8; 3]) -> Mat {
        Mat { tex, color }
    }

    pub fn solid(&self) -> bool {
        self.tex != Tex::Water
    }

    pub fn visible(&self) -> bool {
        self.tex != Tex::Clip
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub mins: Vec3,
    pub maxs: Vec3,
}

impl Aabb {
    pub fn new(mins: Vec3, maxs: Vec3) -> Aabb {
        Aabb { mins, maxs }
    }

    /// Overlap test against a player hull at `origin`.
    pub fn touches(&self, origin: Vec3, mins: Vec3, maxs: Vec3) -> bool {
        let a = origin + mins;
        let b = origin + maxs;
        a.x <= self.maxs.x
            && b.x >= self.mins.x
            && a.y <= self.maxs.y
            && b.y >= self.mins.y
            && a.z <= self.maxs.z
            && b.z >= self.mins.z
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Spawn {
    pub pos: Vec3,
    pub yaw: f32,
}

/// How a bot should move to reach a waypoint.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WpMode {
    /// Run on the ground.
    Walk,
    /// Run and jump off at the end (used for drops onto ramps).
    Drop,
    /// Bunny hop to a pillar top.
    Hop,
    /// Surf a slope. The waypoint lies on the slope face.
    Surf,
    /// Camp at a spot (sniper nests, the island).
    Hold,
}

#[derive(Clone, Copy, Debug)]
pub struct Waypoint {
    pub pos: Vec3,
    pub mode: WpMode,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteKind {
    Surf,
    Bhop,
    Sniper,
}

#[derive(Clone, Debug)]
pub struct Route {
    pub team: Team,
    pub kind: RouteKind,
    pub name: String,
    pub points: Vec<Waypoint>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickupKind {
    Health,
    Weapon(WeaponId),
    /// A weapon attachment for the inventory.
    Attachment(AttItem),
}

impl PickupKind {
    pub fn key(self) -> &'static str {
        match self {
            PickupKind::Health => "health",
            PickupKind::Weapon(w) => w.key(),
            PickupKind::Attachment(a) => a.key(),
        }
    }

    pub fn from_key(k: &str) -> Option<PickupKind> {
        if k == "health" {
            Some(PickupKind::Health)
        } else if let Some(a) = AttItem::from_key(k) {
            Some(PickupKind::Attachment(a))
        } else {
            WeaponId::from_key(k).map(PickupKind::Weapon)
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PickupKind::Health => "Health",
            PickupKind::Weapon(w) => w.def().name,
            PickupKind::Attachment(a) => a.name(),
        }
    }

    /// Seconds until a taken pickup comes back.
    pub fn respawn(self) -> f64 {
        match self {
            PickupKind::Health => 15.0,
            PickupKind::Weapon(_) => 25.0,
            PickupKind::Attachment(a) if a.rare() => 60.0,
            PickupKind::Attachment(_) => 35.0,
        }
    }
}

/// A pickup spawner placed in the map.
#[derive(Clone, Copy, Debug)]
pub struct PickupDef {
    pub pos: Vec3,
    pub kind: PickupKind,
}

pub const PICKUP_HALF: f32 = 20.0;

/// What a booster volume does to a player touching it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Push {
    /// Accelerates along `dir` (unit) up to `speed` while touched, like a
    /// `trigger_push` on a surf ramp. A two way booster pushes along `dir`
    /// or against it, whichever way you are already going.
    Boost { dir: Vec3, speed: f32, two_way: bool },
    /// A launch pad: throws the player through `target` in `secs` seconds.
    Launch { target: Vec3, secs: f32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Booster {
    pub zone: Aabb,
    pub push: Push,
}

/// Acceleration of a booster, in units per second squared.
pub const BOOST_ACCEL: f32 = 2400.0;

impl Booster {
    pub fn boost(zone: Aabb, dir: Vec3, speed: f32) -> Booster {
        Booster { zone, push: Push::Boost { dir: dir.normalize(), speed, two_way: false } }
    }

    pub fn two_way(zone: Aabb, dir: Vec3, speed: f32) -> Booster {
        Booster { zone, push: Push::Boost { dir: dir.normalize(), speed, two_way: true } }
    }

    /// A launch pad whose top centre is `pad`.
    pub fn launcher(pad: Vec3, target: Vec3, secs: f32) -> Booster {
        Booster {
            zone: Aabb::new(pad - vec3(56.0, 56.0, 8.0), pad + vec3(56.0, 56.0, 24.0)),
            push: Push::Launch { target, secs },
        }
    }

    /// Top centre of a launch pad (or the zone's bottom centre).
    pub fn pad(&self) -> Vec3 {
        let c = (self.zone.mins + self.zone.maxs) * 0.5;
        vec3(c.x, c.y, self.zone.mins.z + 8.0)
    }

    /// Launch velocity for a player standing on the pad.
    pub fn launch_velocity(&self, gravity: f32) -> Option<Vec3> {
        match self.push {
            Push::Launch { target, secs } => {
                // Stretch the flight until the throw goes up enough to leave
                // the ground (GoldSrc keeps you grounded below 180 up).
                let start = self.pad() + vec3(0.0, 0.0, 36.0);
                let mut t = secs.max(0.2);
                loop {
                    let v = (target - start) / t + vec3(0.0, 0.0, 0.5 * gravity * t);
                    if v.z >= 260.0 || t > 6.0 {
                        return Some(v);
                    }
                    t += 0.05;
                }
            }
            Push::Boost { .. } => None,
        }
    }
}

pub struct Map {
    pub name: String,
    pub world: CollisionWorld,
    pub spawns: [Vec<Spawn>; 2],
    /// Falling below this height sends you back to your team spawn.
    pub kill_z: f32,
    pub buyzones: [Aabb; 2],
    pub pickups: Vec<PickupDef>,
    pub boosters: Vec<Booster>,
    /// Sky ramps of the built-in maps (used by tests to check they work).
    pub sky: Vec<SkyRamp>,
    pub routes: Vec<Route>,
    pub sky_top: [f32; 3],
    pub sky_horizon: [f32; 3],
    pub fog_color: [f32; 3],
    pub fog_start: f32,
    pub fog_end: f32,
}

impl Map {
    pub fn in_kill_zone(&self, origin: Vec3, mins: Vec3) -> bool {
        origin.z + mins.z < self.kill_z
    }

    /// Buy zones around each team's spawn points (used for custom maps).
    pub fn buyzones_from_spawns(spawns: &[Vec<Spawn>; 2]) -> [Aabb; 2] {
        let zone = |list: &Vec<Spawn>| {
            if list.is_empty() {
                return Aabb::new(Vec3::splat(1e9), Vec3::splat(1e9));
            }
            let mut lo = Vec3::splat(f32::MAX);
            let mut hi = Vec3::splat(f32::MIN);
            for sp in list {
                lo = lo.min(sp.pos);
                hi = hi.max(sp.pos);
            }
            Aabb::new(lo - vec3(400.0, 400.0, 150.0), hi + vec3(400.0, 400.0, 300.0))
        };
        [zone(&spawns[0]), zone(&spawns[1])]
    }

    pub fn random_spawn(&self, team: Team, taken: &[Vec3], seed: usize) -> Spawn {
        let list = &self.spawns[team.index()];
        for i in 0..list.len() {
            let s = list[(i + seed) % list.len()];
            if taken.iter().all(|p| p.distance(s.pos) > 40.0) {
                return s;
            }
        }
        if list.is_empty() {
            return Spawn { pos: vec3(0.0, 0.0, 200.0), yaw: 0.0 };
        }
        list[seed % list.len()]
    }
}

pub const BUILTIN_MAPS: [&str; 3] = ["surf_wars", "surf_canyon", "surf_hairpin"];

/// Built-in maps followed by the custom maps saved with the editor.
pub fn map_names() -> Vec<String> {
    let mut v: Vec<String> = BUILTIN_MAPS.iter().map(|s| s.to_string()).collect();
    v.extend(crate::mapfile::list_custom());
    v
}

pub fn load(name: &str) -> Map {
    match name {
        "surf_wars" => surf_wars(),
        "surf_canyon" => surf_canyon(),
        "surf_hairpin" => surf_hairpin(),
        _ => match crate::mapfile::load(name) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("could not load map {name}: {e}");
                surf_wars()
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Helpers

const LANE_HALF_WIDTH: f32 = 512.0;
const LANE_HEIGHT: f32 = 768.0; // 56.3 degree faces, normal.z = 0.555

/// A surf ramp segment: a triangular prism running along X whose ridge goes
/// from `z0` at `x0` to `z1` at `x1`.
#[allow(clippy::too_many_arguments)]
pub fn ramp_x(x0: f32, x1: f32, yc: f32, z0: f32, z1: f32, hw: f32, h: f32, mat: Mat) -> Brush {
    Brush::hull(
        &[
            vec3(x0, yc, z0),
            vec3(x0, yc - hw, z0 - h),
            vec3(x0, yc + hw, z0 - h),
            vec3(x1, yc, z1),
            vec3(x1, yc - hw, z1 - h),
            vec3(x1, yc + hw, z1 - h),
        ],
        mat,
    )
}

/// Same as [`ramp_x`] but running along Y.
#[allow(clippy::too_many_arguments)]
pub fn ramp_y(y0: f32, y1: f32, xc: f32, z0: f32, z1: f32, hw: f32, h: f32, mat: Mat) -> Brush {
    Brush::hull(
        &[
            vec3(xc, y0, z0),
            vec3(xc - hw, y0, z0 - h),
            vec3(xc + hw, y0, z0 - h),
            vec3(xc, y1, z1),
            vec3(xc - hw, y1, z1 - h),
            vec3(xc + hw, y1, z1 - h),
        ],
        mat,
    )
}

/// Spawn floor tinted toward the colour of the ramp below it, so it is
/// obvious where to drop from the spawn.
fn tint(base: Mat, lane: Mat) -> Mat {
    let c = |i: usize| ((base.color[i] as u16 + lane.color[i] as u16 * 2) / 3) as u8;
    Mat::new(base.tex, [c(0), c(1), c(2)])
}

/// A floor slab split into bands along Y, each with its own material.
fn banded_slab(b: &mut Vec<Brush>, x0: f32, x1: f32, z0: f32, z1: f32, bands: &[(f32, f32, Mat)]) {
    for &(y0, y1, mat) in bands {
        b.push(cuboid(vec3(x0.min(x1), y0.min(y1), z0), vec3(x0.max(x1), y0.max(y1), z1), mat));
    }
}

fn cuboid(mins: Vec3, maxs: Vec3, mat: Mat) -> Brush {
    Brush::cuboid(mins, maxs, mat)
}

/// Axis aligned box centered on (x, y) with top at `top`.
fn block(x: f32, y: f32, half: f32, bottom: f32, top: f32, mat: Mat) -> Brush {
    cuboid(vec3(x - half, y - half, bottom), vec3(x + half, y + half, top), mat)
}

/// Octagonal pillar.
fn octa_pillar(x: f32, y: f32, radius: f32, bottom: f32, top: f32, mat: Mat) -> Brush {
    let mut pts = Vec::new();
    for i in 0..8 {
        let a = (i as f32 + 0.5) * std::f32::consts::TAU / 8.0;
        let (s, c) = a.sin_cos();
        pts.push(vec3(x + c * radius, y + s * radius, bottom));
        pts.push(vec3(x + c * radius, y + s * radius, top));
    }
    Brush::hull(&pts, mat)
}

fn mirror_point(p: Vec3) -> Vec3 {
    vec3(-p.x, p.y, p.z)
}

fn mirror_route(r: &Route) -> Route {
    Route {
        team: r.team.other(),
        kind: r.kind,
        name: r.name.clone(),
        points: r.points.iter().map(|w| Waypoint { pos: mirror_point(w.pos), mode: w.mode }).collect(),
    }
}

/// Point reflection through the map centre, for maps that are symmetric
/// under a 180 degree rotation instead of a mirror.
fn rotate_route(r: &Route) -> Route {
    Route {
        team: r.team.other(),
        kind: r.kind,
        name: r.name.clone(),
        points: r.points.iter().map(|w| Waypoint { pos: vec3(-w.pos.x, -w.pos.y, w.pos.z), mode: w.mode }).collect(),
    }
}

fn health(x: f32, y: f32, z: f32) -> PickupDef {
    PickupDef { pos: vec3(x, y, z), kind: PickupKind::Health }
}

fn gun(w: WeaponId, x: f32, y: f32, z: f32) -> PickupDef {
    PickupDef { pos: vec3(x, y, z), kind: PickupKind::Weapon(w) }
}

/// A row of tiny pillars ending in a small perch: the hard to reach spots.
fn perch_line(b: &mut Vec<Brush>, pts: &[(f32, f32)], perch: (f32, f32), top: f32, mat: Mat) {
    for &(x, y) in pts {
        b.push(block(x, y, 20.0, top - 160.0, top, mat));
    }
    b.push(block(perch.0, perch.1, 48.0, top - 200.0, top, mat));
}

/// A launch pad, a rising boosted surf ramp and the sky platform at its top.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct SkyRamp {
    pub pad: Vec3,
    /// +1 when the ramp climbs toward +x, -1 toward -x.
    pub dir: f32,
    pub platform: Aabb,
}

impl Aabb {
    /// The box rotated 180 degrees about the Z axis when `s` is -1.
    fn turned(self, s: f32) -> Aabb {
        let a = vec3(self.mins.x * s, self.mins.y * s, self.mins.z);
        let b = vec3(self.maxs.x * s, self.maxs.y * s, self.maxs.z);
        Aabb::new(a.min(b), a.max(b))
    }
}

const SKY_RAMP: Mat = Mat::new(Tex::Grid, [240, 110, 200]);
const SKY_PLATFORM: Mat = Mat::new(Tex::Metal, [250, 215, 120]);

/// Launch pad on the ground.
fn pad_brush(b: &mut Vec<Brush>, pad: Vec3) {
    b.push(cuboid(pad - vec3(56.0, 56.0, 8.0), pad + vec3(56.0, 56.0, 2.0), Mat::new(Tex::Metal, [255, 150, 40])));
}

/// Adds launch pads that throw players from `pads` through `targets`.
fn add_launchers(b: &mut Vec<Brush>, boosters: &mut Vec<Booster>, list: &[(Vec3, Vec3, f32)]) {
    for &(pad, target, secs) in list {
        pad_brush(b, pad);
        boosters.push(Booster::launcher(pad, target, secs));
    }
}

/// Settings for a pair of sky ramps. The T ramp runs along X at `y` from
/// `x0` to `x1` with its ridge climbing from `z0` to `z1`; the CT ramp is
/// the same rotated 180 degrees about the map centre. A launch pad at
/// `pad` throws players onto the start of the ramp, three boosters push
/// them up it, and the platform at the top holds `items`.
struct SkyDef {
    y: f32,
    x0: f32,
    x1: f32,
    z0: f32,
    z1: f32,
    pad: Vec3,
    secs: f32,
    items: &'static [PickupKind],
}

fn sky_ramps(
    b: &mut Vec<Brush>,
    boosters: &mut Vec<Booster>,
    pickups: &mut Vec<PickupDef>,
    sky: &mut Vec<SkyRamp>,
    d: &SkyDef,
) {
    let (hw, h) = (LANE_HALF_WIDTH, LANE_HEIGHT);
    let ridge = |x: f32| d.z0 + (d.z1 - d.z0) * (x - d.x0) / (d.x1 - d.x0);
    let sgn = (d.x1 - d.x0).signum();
    for s in [1.0f32, -1.0] {
        let r = |p: Vec3| vec3(p.x * s, p.y * s, p.z);
        let mut local = vec![ramp_x(
            d.x0.min(d.x1),
            d.x0.max(d.x1),
            d.y,
            ridge(d.x0.min(d.x1)),
            ridge(d.x0.max(d.x1)),
            hw,
            h,
            SKY_RAMP,
        )];
        // The platform starts right where the ramp ends and sits below the
        // ridge, so anyone flying off the top of the ramp lands on it.
        let top = d.z1 - h * 0.5;
        let (px0, px1) = (d.x1, d.x1 + sgn * 1200.0);
        let plat = Aabb::new(vec3(px0.min(px1), d.y - 420.0, top - 64.0), vec3(px0.max(px1), d.y + 420.0, top));
        local.push(cuboid(plat.mins, plat.maxs, SKY_PLATFORM));
        // A tall wall at the far end catches you flying off the ramp, and
        // low walls on the sides.
        let (e0, e1) = (px1, px1 + sgn * 48.0);
        local.push(cuboid(
            vec3(e0.min(e1), d.y - 468.0, top - 64.0),
            vec3(e0.max(e1), d.y + 468.0, top + 900.0),
            SKY_PLATFORM,
        ));
        for ys in [-1.0f32, 1.0] {
            let (a, c) = (d.y + ys * 420.0, d.y + ys * 468.0);
            local.push(cuboid(
                vec3(px0.min(px1), a.min(c), top),
                vec3(px0.max(px1), a.max(c), top + 160.0),
                SKY_PLATFORM,
            ));
        }
        for br in local {
            let pts: Vec<Vec3> = br.points.iter().map(|p| r(*p)).collect();
            b.push(Brush::hull(&pts, br.mat));
        }

        // Boosters along the ramp.
        let up = vec3(d.x1 - d.x0, 0.0, d.z1 - d.z0).normalize();
        for t0 in [0.04f32, 0.34, 0.64] {
            let t1 = t0 + 0.14;
            let xa = d.x0 + (d.x1 - d.x0) * t0;
            let xb = d.x0 + (d.x1 - d.x0) * t1;
            let (za, zb) = (ridge(xa), ridge(xb));
            let zone =
                Aabb::new(vec3(xa.min(xb), d.y - hw, za.min(zb) - h), vec3(xa.max(xb), d.y + hw, za.max(zb) + 48.0));
            boosters.push(Booster::boost(zone.turned(s), vec3(up.x * s, 0.0, up.z), 2300.0));
        }

        // Launch pad onto the face nearest to it.
        let side = (d.pad.y - d.y).signum();
        let tx = d.x0 + sgn * 500.0;
        let target = vec3(tx, d.y + side * 280.0, ridge(tx) - 280.0 * h / hw + 80.0);
        pad_brush(b, r(d.pad));
        boosters.push(Booster::launcher(r(d.pad), r(target), d.secs));

        for (i, k) in d.items.iter().enumerate() {
            let x = d.x1 + sgn * (500.0 + 180.0 * i as f32);
            pickups.push(PickupDef { pos: r(vec3(x, d.y, top + 24.0)), kind: *k });
        }
        sky.push(SkyRamp { pad: r(d.pad), dir: sgn * s, platform: plat.turned(s) });
    }
}

fn att(a: AttItem, x: f32, y: f32, z: f32) -> PickupDef {
    PickupDef { pos: vec3(x, y, z), kind: PickupKind::Attachment(a) }
}

fn wp(x: f32, y: f32, z: f32, mode: WpMode) -> Waypoint {
    Waypoint { pos: vec3(x, y, z), mode }
}

const RAMP_A: Mat = Mat::new(Tex::Grid, [70, 170, 235]);
const RAMP_B: Mat = Mat::new(Tex::Grid, [245, 140, 60]);
const RAMP_C: Mat = Mat::new(Tex::Grid, [150, 110, 230]);
const SPAWN_T: Mat = Mat::new(Tex::Concrete, [205, 170, 115]);
const SPAWN_CT: Mat = Mat::new(Tex::Concrete, [120, 150, 200]);
const TRIM_T: Mat = Mat::new(Tex::Metal, [190, 70, 50]);
const TRIM_CT: Mat = Mat::new(Tex::Metal, [60, 100, 200]);
const PILLAR_MID: Mat = Mat::new(Tex::Concrete, [225, 205, 95]);
const PILLAR_SIDE: Mat = Mat::new(Tex::Concrete, [120, 205, 130]);
const METAL: Mat = Mat::new(Tex::Metal, [150, 155, 165]);
const CONCRETE: Mat = Mat::new(Tex::Concrete, [165, 160, 150]);
const CRATE: Mat = Mat::new(Tex::Crate, [200, 160, 105]);
const WATER: Mat = Mat::new(Tex::Water, [40, 110, 150]);
const FLOOR: Mat = Mat::new(Tex::Grid, [70, 75, 85]);
const CLIP: Mat = Mat::new(Tex::Clip, [0, 0, 0]);
const PERCH: Mat = Mat::new(Tex::Metal, [230, 90, 90]);
const RAMP_D: Mat = Mat::new(Tex::Grid, [90, 210, 150]);
const RAMP_E: Mat = Mat::new(Tex::Grid, [230, 200, 80]);

fn boundary(brushes: &mut Vec<Brush>, ext: Vec3, top: f32) {
    let t = 64.0;
    brushes.push(cuboid(vec3(-ext.x - t, -ext.y - t, -200.0), vec3(-ext.x, ext.y + t, top), CLIP));
    brushes.push(cuboid(vec3(ext.x, -ext.y - t, -200.0), vec3(ext.x + t, ext.y + t, top), CLIP));
    brushes.push(cuboid(vec3(-ext.x, -ext.y - t, -200.0), vec3(ext.x, -ext.y, top), CLIP));
    brushes.push(cuboid(vec3(-ext.x, ext.y, -200.0), vec3(ext.x, ext.y + t, top), CLIP));
    brushes.push(cuboid(vec3(-ext.x, -ext.y, top), vec3(ext.x, ext.y, top + t), CLIP));
}

// ---------------------------------------------------------------------------
// surf_wars
//
// Two spawn platforms at the ends of the map, two long V shaped surf lanes
// that dip into a valley in the middle, a middle island reached by a
// descending line of bunny hop pillars, and two sniper towers reached by
// long bunny hop paths along the outside.

const SW_SPAWN_Z: f32 = 2400.0;
const SW_LANE_Y: f32 = 1100.0;
const SW_END_Z: f32 = 2000.0;
const SW_VALLEY_Z: f32 = 1300.0;

/// Ridge height of the surf_wars lanes at a given x.
pub fn sw_ridge(x: f32) -> f32 {
    let ax = x.abs();
    if ax <= 500.0 {
        SW_VALLEY_Z
    } else if ax >= 3400.0 {
        SW_END_Z
    } else {
        SW_VALLEY_Z + (SW_END_Z - SW_VALLEY_Z) * (ax - 500.0) / 2900.0
    }
}

/// Middle pillar line (CT side, positive x): (x, y, top).
fn sw_mid_pillars() -> Vec<(f32, f32, f32)> {
    let n = 15;
    (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1) as f32;
            let x = 3350.0 - t * (3350.0 - 880.0);
            let y = if i % 2 == 0 { 60.0 } else { -60.0 };
            (x, y, SW_SPAWN_Z)
        })
        .collect()
}

/// Side pillar path toward a tower (CT side, positive x).
fn sw_side_pillars(ys: f32) -> Vec<(f32, f32, f32)> {
    let n = 21;
    (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1) as f32;
            let x = 3850.0 - t * (3850.0 - 420.0);
            let y = ys * (2050.0 + if i % 2 == 0 { 50.0 } else { -50.0 });
            (x, y, SW_SPAWN_Z)
        })
        .collect()
}

fn surf_wars() -> Map {
    let mut b: Vec<Brush> = Vec::new();

    // Water at the bottom of the pit (visual only) and a floor below it.
    b.push(cuboid(vec3(-5200.0, -4000.0, -64.0), vec3(5200.0, 4000.0, 0.0), FLOOR));
    b.push(cuboid(vec3(-5200.0, -4000.0, 0.0), vec3(5200.0, 4000.0, 120.0), WATER));

    // Surf lanes.
    for (yc, mat) in [(SW_LANE_Y, RAMP_A), (-SW_LANE_Y, RAMP_B)] {
        let xs = [-4400.0, -3400.0, -500.0, 500.0, 3400.0, 4400.0];
        for w in xs.windows(2) {
            b.push(ramp_x(w[0], w[1], yc, sw_ridge(w[0]), sw_ridge(w[1]), LANE_HALF_WIDTH, LANE_HEIGHT, mat));
        }
    }

    // End walls behind the lanes.
    for s in [-1.0f32, 1.0] {
        let x0 = s * 4700.0;
        let x1 = s * 4764.0;
        b.push(cuboid(vec3(x0.min(x1), -2000.0, 0.0), vec3(x0.max(x1), 2000.0, 2900.0), CONCRETE));
    }

    for (s, team_mat, trim) in [(-1.0f32, SPAWN_T, TRIM_T), (1.0, SPAWN_CT, TRIM_CT)] {
        let fx = |x: f32| x * s; // mirror helper, T side is negative x
        let lo = |a: f32, c: f32| a.min(c);
        let hi = |a: f32, c: f32| a.max(c);

        // Spawn platform.
        banded_slab(
            &mut b,
            fx(4400.0),
            fx(3500.0),
            SW_SPAWN_Z - 64.0,
            SW_SPAWN_Z,
            &[
                (-900.0, -588.0, tint(team_mat, RAMP_B)),
                (-588.0, 588.0, team_mat),
                (588.0, 900.0, tint(team_mat, RAMP_A)),
            ],
        );
        // Back wall.
        let (a, c) = (fx(4400.0), fx(4464.0));
        b.push(cuboid(vec3(lo(a, c), -1900.0, SW_SPAWN_Z - 64.0), vec3(hi(a, c), 1900.0, SW_SPAWN_Z + 320.0), trim));
        // Support column under the platform.
        let (a, c) = (fx(4350.0), fx(4100.0));
        b.push(cuboid(vec3(lo(a, c), -150.0, 0.0), vec3(hi(a, c), 150.0, SW_SPAWN_Z - 64.0), CONCRETE));
        // Bridges to the side bunny hop paths, over the outer lane faces.
        for ys in [-1.0f32, 1.0] {
            let (a, c) = (fx(4400.0), fx(4000.0));
            let lane = if ys > 0.0 { RAMP_A } else { RAMP_B };
            banded_slab(
                &mut b,
                a,
                c,
                SW_SPAWN_Z - 64.0,
                SW_SPAWN_Z,
                &[
                    (ys * 900.0, ys * 1100.0, team_mat),
                    (ys * 1100.0, ys * 1612.0, tint(team_mat, lane)),
                    (ys * 1612.0, ys * 2150.0, team_mat),
                ],
            );
            // railing on the outside of the bridge
            let (y0, y1) = (ys * 2150.0, ys * 2214.0);
            b.push(cuboid(vec3(lo(a, c), lo(y0, y1), SW_SPAWN_Z), vec3(hi(a, c), hi(y0, y1), SW_SPAWN_Z + 48.0), trim));
        }
        // Cover on the spawn.
        for (x, y) in [(3800.0, 450.0), (3800.0, -450.0), (4300.0, 760.0)] {
            b.push(block(fx(x), y, 32.0, SW_SPAWN_Z, SW_SPAWN_Z + 64.0, CRATE));
        }
        b.push(block(fx(3800.0), 450.0, 24.0, SW_SPAWN_Z + 64.0, SW_SPAWN_Z + 112.0, CRATE));

        // Middle bunny hop line: level pillars from the spawn to the island.
        for (i, (x, y, top)) in sw_mid_pillars().into_iter().enumerate() {
            let _ = i;
            b.push(block(fx(x), y, 52.0, top - 200.0, top, PILLAR_MID));
        }

        // Side bunny hop paths to the sniper towers, also level.
        for ys in [-1.0f32, 1.0] {
            for (i, (x, y, top)) in sw_side_pillars(ys).into_iter().enumerate() {
                if i % 3 == 1 {
                    b.push(octa_pillar(fx(x), y, 56.0, 0.0, top, PILLAR_SIDE));
                } else {
                    b.push(block(fx(x), y, 50.0, 0.0, top, PILLAR_SIDE));
                }
            }
        }
    }

    // Sniper towers, level with the spawns.
    let tz = SW_SPAWN_Z;
    for ys in [-1.0f32, 1.0] {
        let (y0, y1) = (ys * 1850.0, ys * 2350.0);
        let (ylo, yhi) = (y0.min(y1), y0.max(y1));
        b.push(cuboid(vec3(-300.0, ylo, 0.0), vec3(300.0, yhi, tz), CONCRETE));
        // parapet toward the lanes
        let (p0, p1) = (ys * 1850.0, ys * 1880.0);
        b.push(cuboid(vec3(-300.0, p0.min(p1), tz), vec3(-60.0, p0.max(p1), tz + 46.0), METAL));
        b.push(cuboid(vec3(60.0, p0.min(p1), tz), vec3(300.0, p0.max(p1), tz + 46.0), METAL));
        b.push(block(0.0, ys * 2250.0, 32.0, tz, tz + 64.0, CRATE));
        b.push(block(-200.0, ys * 2150.0, 28.0, tz, tz + 56.0, CRATE));
        b.push(block(200.0, ys * 2150.0, 28.0, tz, tz + 56.0, CRATE));
    }

    // Middle island, up at spawn height and reached over the pillars.
    let iz = SW_SPAWN_Z;
    b.push(cuboid(vec3(-700.0, -420.0, iz - 64.0), vec3(700.0, 420.0, iz), METAL));
    b.push(cuboid(vec3(-200.0, -200.0, 0.0), vec3(200.0, 200.0, iz - 64.0), CONCRETE));
    for (x, y, h) in [(-380.0, 220.0, 32.0), (380.0, -220.0, 32.0), (-380.0, -250.0, 28.0), (380.0, 250.0, 28.0)] {
        b.push(block(x, y, h, iz, iz + h * 2.0, CRATE));
    }
    b.push(cuboid(vec3(-24.0, -300.0, iz), vec3(24.0, -120.0, iz + 60.0), METAL));
    b.push(cuboid(vec3(-24.0, 120.0, iz), vec3(24.0, 300.0, iz + 60.0), METAL));
    // Hard to reach perches off the island: tiny pillars with long gaps.
    for ys in [-1.0f32, 1.0] {
        perch_line(&mut b, &[(0.0, ys * 640.0), (0.0, ys * 860.0), (0.0, ys * 1075.0)], (0.0, ys * 1300.0), iz, PERCH);
    }

    boundary(&mut b, vec3(5100.0, 3950.0, 0.0), 4600.0);

    // Spawns.
    let mut spawns: [Vec<Spawn>; 2] = [Vec::new(), Vec::new()];
    for (ti, s) in [(0usize, -1.0f32), (1, 1.0)] {
        for x in [4250.0, 4050.0] {
            for y in [-600.0, -300.0, 0.0, 300.0, 600.0] {
                spawns[ti]
                    .push(Spawn { pos: vec3(x * s, y, SW_SPAWN_Z + 37.0), yaw: if s < 0.0 { 0.0 } else { 180.0 } });
            }
        }
    }

    let buyzones = [
        Aabb::new(vec3(-4500.0, -2200.0, SW_SPAWN_Z - 100.0), vec3(-3450.0, 2200.0, SW_SPAWN_Z + 400.0)),
        Aabb::new(vec3(3450.0, -2200.0, SW_SPAWN_Z - 100.0), vec3(4500.0, 2200.0, SW_SPAWN_Z + 400.0)),
    ];

    let mut pickups = vec![
        att(AttItem::Grip(Grip::Vertical), 0.0, 1300.0, iz + 62.0),
        att(AttItem::Sight(Sight::RedDot), 0.0, -1300.0, iz + 62.0),
        att(AttItem::Muzzle(Muzzle::Compensator), -120.0, 1960.0, tz + 62.0),
        att(AttItem::Stock(Stock::Light), 120.0, -1960.0, tz + 62.0),
        gun(WeaponId::Ak47, 0.0, 1300.0, iz + 24.0),
        gun(WeaponId::Scout, 0.0, -1300.0, iz + 24.0),
        gun(WeaponId::Awp, -120.0, 1960.0, tz + 24.0),
        gun(WeaponId::Awp, 120.0, -1960.0, tz + 24.0),
        health(-300.0, 0.0, iz + 24.0),
        health(300.0, 0.0, iz + 24.0),
        health(150.0, 2250.0, tz + 24.0),
        health(-150.0, -2250.0, tz + 24.0),
        // floating over the lane faces in the valley: grab them while surfing
        health(0.0, 913.0, 1060.0),
        health(0.0, -913.0, 1060.0),
        health(0.0, 1287.0, 1060.0),
        health(0.0, -1287.0, 1060.0),
    ];

    // Bot routes (T side, mirrored for CT).
    let mut routes: Vec<Route> = Vec::new();
    let depth = 280.0;
    for (ys, lane) in [(1.0f32, "north"), (-1.0f32, "south")] {
        for (face, name) in [(-1.0f32, "inner"), (1.0f32, "outer")] {
            let face_y = ys * (SW_LANE_Y + face * depth / 1.5);
            let mut pts = Vec::new();
            if face < 0.0 {
                // Walk off the front of the spawn above the inner face.
                pts.push(wp(-3700.0, ys * 760.0, SW_SPAWN_Z, WpMode::Walk));
                pts.push(wp(-3440.0, ys * 800.0, SW_SPAWN_Z, WpMode::Drop));
            } else {
                // Use the bridge to reach the outer face.
                pts.push(wp(-4200.0, ys * 1050.0, SW_SPAWN_Z, WpMode::Walk));
                pts.push(wp(-4200.0, ys * 1330.0, SW_SPAWN_Z, WpMode::Walk));
                pts.push(wp(-3960.0, ys * 1330.0, SW_SPAWN_Z, WpMode::Drop));
            }
            for x in [-3300.0, -2400.0, -1400.0, -500.0, 0.0, 500.0, 1400.0, 2400.0, 3400.0, 4350.0] {
                // On the far (climbing) half keep the height of the valley
                // line: bots ride it out at speed instead of trying to climb.
                let z = (sw_ridge(x) - depth).min(if x > 0.0 { SW_VALLEY_Z - depth + 40.0 } else { f32::MAX });
                pts.push(wp(x, face_y, z, WpMode::Surf));
            }
            routes.push(Route {
                team: Team::T,
                kind: RouteKind::Surf,
                name: format!("lane {lane} {name}"),
                points: pts,
            });
        }
    }
    // Middle pillars to the island.
    {
        let mut pts = vec![wp(-3620.0, 0.0, SW_SPAWN_Z, WpMode::Walk)];
        for (x, y, top) in sw_mid_pillars() {
            pts.push(wp(-x, y, top, WpMode::Hop));
        }
        pts.push(wp(-640.0, 0.0, iz, WpMode::Hop));
        pts.push(wp(-300.0, 0.0, iz, WpMode::Hold));
        routes.push(Route { team: Team::T, kind: RouteKind::Bhop, name: "mid pillars".into(), points: pts });
    }
    // Side paths to the towers.
    for ys in [1.0f32, -1.0] {
        let mut pts = vec![
            wp(-4200.0, ys * 1100.0, SW_SPAWN_Z, WpMode::Walk),
            wp(-4150.0, ys * 2020.0, SW_SPAWN_Z, WpMode::Walk),
        ];
        for (x, y, top) in sw_side_pillars(ys) {
            pts.push(wp(-x, y, top, WpMode::Hop));
        }
        pts.push(wp(-200.0, ys * 2100.0, tz, WpMode::Hop));
        pts.push(wp(-120.0, ys * 1960.0, tz, WpMode::Hold));
        routes.push(Route {
            team: Team::T,
            kind: RouteKind::Sniper,
            name: format!("tower {}", if ys > 0.0 { "north" } else { "south" }),
            points: pts,
        });
    }
    let mirrored: Vec<Route> = routes.iter().map(mirror_route).collect();
    routes.extend(mirrored);

    // Launch pads at the front of the spawns that throw you onto the lanes.
    let mut boosters = Vec::new();
    let mut pads = Vec::new();
    for ys in [-1.0f32, 1.0] {
        let tx = -1200.0;
        let target = vec3(tx, ys * (SW_LANE_Y - 300.0), sw_ridge(tx) - 450.0 + 60.0);
        pads.push((vec3(-3620.0, ys * 380.0, SW_SPAWN_Z), target, 1.5));
    }
    let mirrored: Vec<_> = pads.iter().map(|(p, t, s)| (mirror_point(*p), mirror_point(*t), *s)).collect();
    pads.extend(mirrored);
    add_launchers(&mut b, &mut boosters, &pads);
    // Two way boosters in the lane valleys.
    for yc in [SW_LANE_Y, -SW_LANE_Y] {
        let zone = Aabb::new(
            vec3(-450.0, yc - LANE_HALF_WIDTH, SW_VALLEY_Z - LANE_HEIGHT),
            vec3(450.0, yc + LANE_HALF_WIDTH, SW_VALLEY_Z + 48.0),
        );
        boosters.push(Booster::two_way(zone, Vec3::X, 1700.0));
    }

    // Sky ramps along the outside, with the rare attachments on top.
    let mut sky = Vec::new();
    sky_ramps(
        &mut b,
        &mut boosters,
        &mut pickups,
        &mut sky,
        &SkyDef {
            y: 3350.0,
            x0: -4000.0,
            x1: 2400.0,
            z0: 2200.0,
            z1: 3700.0,
            pad: vec3(-4320.0, 1780.0, SW_SPAWN_Z),
            secs: 1.7,
            items: &[
                PickupKind::Attachment(AttItem::Sight(Sight::Acog)),
                PickupKind::Attachment(AttItem::Muzzle(Muzzle::Suppressor)),
                PickupKind::Health,
            ],
        },
    );

    Map {
        name: "surf_wars".into(),
        world: CollisionWorld::new(b),
        spawns,
        kill_z: 450.0,
        buyzones,
        pickups,
        boosters,
        sky,
        routes,
        sky_top: [0.20, 0.38, 0.72],
        sky_horizon: [0.78, 0.86, 0.95],
        fog_color: [0.72, 0.80, 0.90],
        fog_start: 3000.0,
        fog_end: 14000.0,
    }
}

// ---------------------------------------------------------------------------
// surf_canyon
//
// A long canyon: from each spawn a steep "ski" ramp drops into a wide
// double sided ramp running across the map. In the middle a crossing ramp
// runs along Y, and a field of bunny hop pillars links the two spawns
// through the middle.

const SC_SPAWN_Z: f32 = 2600.0;

pub fn sc_main_ridge(x: f32) -> f32 {
    // Descends from both ends toward the middle.
    let ax = x.abs();
    if ax >= 3600.0 {
        2100.0
    } else if ax <= 900.0 {
        1500.0
    } else {
        1500.0 + 600.0 * (ax - 900.0) / 2700.0
    }
}

/// Canyon pillar line (CT side, positive x).
fn sc_pillars(ys: f32) -> Vec<(f32, f32, f32)> {
    let n = 21;
    (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1) as f32;
            let x = 4300.0 - t * (4300.0 - 700.0);
            let y = ys * (880.0 + if i % 2 == 0 { 55.0 } else { -55.0 });
            (x, y, SC_SPAWN_Z)
        })
        .collect()
}

fn surf_canyon() -> Map {
    let mut b: Vec<Brush> = Vec::new();
    b.push(cuboid(vec3(-5600.0, -4600.0, -64.0), vec3(5600.0, 4600.0, 0.0), FLOOR));
    b.push(cuboid(vec3(-5600.0, -4600.0, 0.0), vec3(5600.0, 4600.0, 120.0), WATER));

    // Three parallel lanes: a central wide one and two narrower outside.
    let lanes: [(f32, f32, f32, Mat); 3] =
        [(0.0, 620.0, 930.0, RAMP_C), (1750.0, 512.0, 768.0, RAMP_A), (-1750.0, 512.0, 768.0, RAMP_B)];
    let xs = [-4600.0, -3600.0, -900.0, 900.0, 3600.0, 4600.0];
    for (yc, hw, h, mat) in lanes {
        let offset = if yc == 0.0 { 0.0 } else { -150.0 };
        for w in xs.windows(2) {
            b.push(ramp_x(w[0], w[1], yc, sc_main_ridge(w[0]) + offset, sc_main_ridge(w[1]) + offset, hw, h, mat));
        }
    }

    // Short cross ramps at the far sides, running along Y, for style points.
    for s in [-1.0f32, 1.0] {
        b.push(ramp_y(-2900.0, 2900.0, s * 2500.0, 900.0, 900.0, 300.0, 450.0, RAMP_A));
    }

    for (s, team_mat, trim) in [(-1.0f32, SPAWN_T, TRIM_T), (1.0, SPAWN_CT, TRIM_CT)] {
        let fx = |x: f32| x * s;
        let lo = |a: f32, c: f32| a.min(c);
        let hi = |a: f32, c: f32| a.max(c);
        banded_slab(
            &mut b,
            fx(5400.0),
            fx(4500.0),
            SC_SPAWN_Z - 64.0,
            SC_SPAWN_Z,
            &[
                (-2300.0, -1238.0, tint(team_mat, RAMP_B)),
                (-1238.0, -620.0, team_mat),
                (-620.0, 620.0, tint(team_mat, RAMP_C)),
                (620.0, 1238.0, team_mat),
                (1238.0, 2300.0, tint(team_mat, RAMP_A)),
            ],
        );
        let (a, c) = (fx(5400.0), fx(5464.0));
        b.push(cuboid(vec3(lo(a, c), -2300.0, SC_SPAWN_Z - 64.0), vec3(hi(a, c), 2300.0, SC_SPAWN_Z + 320.0), trim));
        let (a, c) = (fx(5300.0), fx(5000.0));
        b.push(cuboid(vec3(lo(a, c), -200.0, 0.0), vec3(hi(a, c), 200.0, SC_SPAWN_Z - 64.0), CONCRETE));
        for (x, y) in [(4800.0, 1200.0), (4800.0, -1200.0), (5100.0, 0.0), (4700.0, 0.0)] {
            b.push(block(fx(x), y, 32.0, SC_SPAWN_Z, SC_SPAWN_Z + 64.0, CRATE));
        }

        // Level pillar lines between the middle lane and the outer lanes.
        for ys in [-1.0f32, 1.0] {
            for (i, (x, y, top)) in sc_pillars(ys).into_iter().enumerate() {
                if i % 2 == 0 {
                    b.push(octa_pillar(fx(x), y, 56.0, top - 260.0, top, PILLAR_SIDE));
                } else {
                    b.push(block(fx(x), y, 50.0, top - 260.0, top, PILLAR_MID));
                }
            }
        }
    }
    // Middle platforms at the end of the pillar lines, level with spawn.
    let pz = SC_SPAWN_Z;
    for ys in [-1.0f32, 1.0] {
        let (y0, y1) = (ys * 700.0, ys * 1100.0);
        b.push(cuboid(vec3(-560.0, y0.min(y1), pz - 40.0), vec3(560.0, y0.max(y1), pz), METAL));
        b.push(block(0.0, ys * 900.0, 36.0, pz, pz + 72.0, CRATE));
        b.push(block(-300.0, ys * 830.0, 28.0, pz, pz + 56.0, CRATE));
        b.push(block(300.0, ys * 970.0, 28.0, pz, pz + 56.0, CRATE));
        // tiny pillars from the platform toward the AWP perch in the middle
        perch_line(&mut b, &[(0.0, ys * 520.0), (0.0, ys * 330.0)], (0.0, 0.0), pz, PERCH);
    }

    boundary(&mut b, vec3(5700.0, 4500.0, 0.0), 4800.0);

    let mut spawns: [Vec<Spawn>; 2] = [Vec::new(), Vec::new()];
    for (ti, s) in [(0usize, -1.0f32), (1, 1.0)] {
        for x in [5200.0, 5000.0] {
            for y in [-1000.0, -500.0, 0.0, 500.0, 1000.0] {
                spawns[ti]
                    .push(Spawn { pos: vec3(x * s, y, SC_SPAWN_Z + 37.0), yaw: if s < 0.0 { 0.0 } else { 180.0 } });
            }
        }
    }
    let buyzones = [
        Aabb::new(vec3(-5500.0, -2400.0, SC_SPAWN_Z - 100.0), vec3(-4450.0, 2400.0, SC_SPAWN_Z + 400.0)),
        Aabb::new(vec3(4450.0, -2400.0, SC_SPAWN_Z - 100.0), vec3(5500.0, 2400.0, SC_SPAWN_Z + 400.0)),
    ];
    let mut pickups = vec![
        att(AttItem::Sight(Sight::RedDot), 0.0, 0.0, pz + 62.0),
        att(AttItem::Grip(Grip::Angled), -420.0, 1020.0, pz + 62.0),
        att(AttItem::Muzzle(Muzzle::LongBarrel), 420.0, -1020.0, pz + 62.0),
        gun(WeaponId::Awp, 0.0, 0.0, pz + 24.0),
        gun(WeaponId::Ak47, -420.0, 1020.0, pz + 24.0),
        gun(WeaponId::Ak47, 420.0, -1020.0, pz + 24.0),
        health(400.0, 780.0, pz + 24.0),
        health(-400.0, -780.0, pz + 24.0),
        health(0.0, 200.0, 1236.0),
        health(0.0, -200.0, 1236.0),
        health(0.0, 1550.0, 1086.0),
        health(0.0, -1550.0, 1086.0),
    ];

    let mut routes: Vec<Route> = Vec::new();
    // Lanes: centre lane faces and outer lanes.
    let lane_defs: [(f32, f32, f32, f32, &str); 3] = [
        (0.0, 620.0, 930.0, 0.0, "centre"),
        (1750.0, 512.0, 768.0, -150.0, "north"),
        (-1750.0, 512.0, 768.0, -150.0, "south"),
    ];
    for (yc, hw, h, offset, lname) in lane_defs {
        let slope = h / hw;
        let depth = 300.0;
        for face in [-1.0f32, 1.0] {
            let face_y = yc + face * depth / slope;
            let edge_y = if yc == 0.0 { face * 420.0 } else { yc + face * 300.0 };
            let pts_start = vec![
                wp(-5050.0, edge_y.clamp(-2250.0, 2250.0), SC_SPAWN_Z, WpMode::Walk),
                wp(-4540.0, edge_y.clamp(-2250.0, 2250.0), SC_SPAWN_Z, WpMode::Drop),
            ];
            let mut pts = pts_start;
            for x in [-4300.0, -3600.0, -2400.0, -900.0, 0.0, 900.0, 2400.0, 3600.0, 4550.0] {
                let z = (sc_main_ridge(x) + offset - depth).min(if x > 0.0 {
                    1500.0 + offset - depth + 40.0
                } else {
                    f32::MAX
                });
                pts.push(wp(x, face_y, z, WpMode::Surf));
            }
            routes.push(Route {
                team: Team::T,
                kind: RouteKind::Surf,
                name: format!("{lname} lane {}", if face < 0.0 { "south face" } else { "north face" }),
                points: pts,
            });
        }
    }
    for ys in [-1.0f32, 1.0] {
        let mut pts =
            vec![wp(-4800.0, ys * 900.0, SC_SPAWN_Z, WpMode::Walk), wp(-4560.0, ys * 900.0, SC_SPAWN_Z, WpMode::Walk)];
        for (x, y, top) in sc_pillars(ys) {
            pts.push(wp(-x, y, top, WpMode::Hop));
        }
        pts.push(wp(-450.0, ys * 900.0, pz, WpMode::Hop));
        pts.push(wp(-150.0, ys * 800.0, pz, WpMode::Hold));
        routes.push(Route {
            team: Team::T,
            kind: if ys > 0.0 { RouteKind::Sniper } else { RouteKind::Bhop },
            name: format!("pillars {}", if ys > 0.0 { "north" } else { "south" }),
            points: pts,
        });
    }
    let mirrored: Vec<Route> = routes.iter().map(mirror_route).collect();
    routes.extend(mirrored);

    let mut boosters = Vec::new();
    let mut pads = Vec::new();
    for ys in [-1.0f32, 1.0] {
        let tx = -2000.0;
        let target = vec3(tx, ys * (1750.0 - 300.0), sc_main_ridge(tx) - 150.0 - 450.0 + 60.0);
        pads.push((vec3(-4620.0, ys * 1150.0, SC_SPAWN_Z), target, 1.4));
    }
    let mirrored: Vec<_> = pads.iter().map(|(p, t, s)| (mirror_point(*p), mirror_point(*t), *s)).collect();
    pads.extend(mirrored);
    add_launchers(&mut b, &mut boosters, &pads);
    // Two way booster in the middle of the centre lane.
    boosters.push(Booster::two_way(
        Aabb::new(vec3(-700.0, -620.0, 1500.0 - 930.0), vec3(700.0, 620.0, 1548.0)),
        Vec3::X,
        1700.0,
    ));

    let mut sky = Vec::new();
    sky_ramps(
        &mut b,
        &mut boosters,
        &mut pickups,
        &mut sky,
        &SkyDef {
            y: 3500.0,
            x0: -5000.0,
            x1: 2600.0,
            z0: 2350.0,
            z1: 3950.0,
            pad: vec3(-5000.0, 2180.0, SC_SPAWN_Z),
            secs: 1.7,
            items: &[
                PickupKind::Attachment(AttItem::Sight(Sight::Holo)),
                PickupKind::Attachment(AttItem::Sight(Sight::Acog)),
                PickupKind::Health,
            ],
        },
    );

    Map {
        name: "surf_canyon".into(),
        world: CollisionWorld::new(b),
        spawns,
        kill_z: 450.0,
        buyzones,
        pickups,
        boosters,
        sky,
        routes,
        sky_top: [0.55, 0.30, 0.35],
        sky_horizon: [0.98, 0.72, 0.50],
        fog_color: [0.92, 0.70, 0.55],
        fog_start: 2500.0,
        fog_end: 13000.0,
    }
}

// ---------------------------------------------------------------------------
// surf_hairpin
//
// A big map made of turning ramps. Each team starts on a high platform,
// drops onto a ramp that bends 90 degrees into a long straight, then a 180
// degree hairpin brings it back down the middle of the map, where it meets
// the other team coming the other way. Every turn goes left, so you stay on
// the same face of the ramp the whole run. The CT half is the T half rotated
// 180 degrees around the map centre. Flat pillar lines lead from both spawns
// to a sky fort in the middle holding the AWP; the AKs sit on perches in the
// middle of the hairpins where only a well timed jump off the ramp gets you.

const HP_SPAWN_Z: f32 = 4300.0;
const HP_HW: f32 = 512.0;
const HP_H: f32 = 768.0;

/// A turning ramp: the triangular ramp profile swept around `center` from
/// angle `a0` to `a1` (degrees, counter clockwise), ridge from `z0` to `z1`.
#[allow(clippy::too_many_arguments)]
pub fn turn_ramp(
    b: &mut Vec<Brush>,
    center: Vec3,
    radius: f32,
    a0: f32,
    a1: f32,
    z0: f32,
    z1: f32,
    hw: f32,
    h: f32,
    segs: usize,
    mat: Mat,
) {
    for i in 0..segs {
        let t0 = i as f32 / segs as f32;
        let t1 = (i + 1) as f32 / segs as f32;
        let mut pts = Vec::new();
        for t in [t0, t1] {
            let a = (a0 + (a1 - a0) * t).to_radians();
            let dir = vec3(a.cos(), a.sin(), 0.0);
            let z = z0 + (z1 - z0) * t;
            let c = vec3(center.x, center.y, 0.0);
            pts.push(c + dir * radius + vec3(0.0, 0.0, z));
            pts.push(c + dir * (radius - hw) + vec3(0.0, 0.0, z - h));
            pts.push(c + dir * (radius + hw) + vec3(0.0, 0.0, z - h));
        }
        b.push(Brush::hull(&pts, mat));
    }
}

/// Points on the inner face of a turn, `depth` below the ridge.
#[allow(clippy::too_many_arguments)]
fn turn_route(
    pts: &mut Vec<Waypoint>,
    center: Vec3,
    radius: f32,
    a0: f32,
    a1: f32,
    z0: f32,
    z1: f32,
    depth: f32,
    step: f32,
) {
    let n = ((a1 - a0).abs() / step).ceil().max(1.0) as usize;
    let r = radius - depth * HP_HW / HP_H;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let a = (a0 + (a1 - a0) * t).to_radians();
        let z = z0 + (z1 - z0) * t - depth;
        pts.push(wp(center.x + a.cos() * r, center.y + a.sin() * r, z, WpMode::Surf));
    }
}

fn surf_hairpin() -> Map {
    let mut b: Vec<Brush> = Vec::new();
    b.push(cuboid(vec3(-9800.0, -5600.0, -64.0), vec3(9800.0, 5600.0, 0.0), FLOOR));
    b.push(cuboid(vec3(-9800.0, -5600.0, 0.0), vec3(9800.0, 5600.0, 120.0), WATER));

    // Geometry of the T track; the CT track is the same rotated by 180.
    // leg0: south along x=-8500 from y=200 to -1500
    // turn A: centre (-7000,-1500) r 1500, 180 -> 270 degrees
    // leg1: east along y=-3000 from x=-7000 to 4400
    // turn B (hairpin): centre (4400,-2050) r 950, -90 -> 90 degrees
    // leg2: west along y=-1100 from x=4400 to -5500
    let z_leg0 = (4000.0, 3850.0);
    let z_a = (3850.0, 3600.0);
    let z_leg1 = (3600.0, 3100.0);
    let z_b = (3100.0, 2950.0);
    let z_leg2 = (2950.0, 2500.0);
    let turn_a = vec3(-7000.0, -1500.0, 0.0);
    let turn_b = vec3(4400.0, -2050.0, 0.0);

    for s in [1.0f32, -1.0] {
        let (team_mat, trim, m0, m1, m2) = if s > 0.0 {
            (SPAWN_T, TRIM_T, RAMP_B, RAMP_E, RAMP_D)
        } else {
            (SPAWN_CT, TRIM_CT, RAMP_A, RAMP_C, RAMP_D)
        };
        let r = |p: Vec3| vec3(p.x * s, p.y * s, p.z);
        let mut local: Vec<Brush> = Vec::new();
        local.push(ramp_y(-1500.0, 200.0, -8500.0, z_leg0.1, z_leg0.0, HP_HW, HP_H, m0));
        turn_ramp(&mut local, turn_a, 1500.0, 180.0, 270.0, z_a.0, z_a.1, HP_HW, HP_H, 18, m0);
        // leg1 split so the slope is gentle at first
        local.push(ramp_x(-7000.0, -1300.0, -3000.0, z_leg1.0, 3350.0, HP_HW, HP_H, m1));
        local.push(ramp_x(-1300.0, 4400.0, -3000.0, 3350.0, z_leg1.1, HP_HW, HP_H, m1));
        turn_ramp(&mut local, turn_b, 950.0, -90.0, 90.0, z_b.0, z_b.1, HP_HW, HP_H, 30, m1);
        local.push(ramp_x(-5500.0, 4400.0, -1100.0, z_leg2.1, z_leg2.0, HP_HW, HP_H, m2));

        // Spawn platform north of leg0, with its floor tinted over the ramp.
        let sz = HP_SPAWN_Z;
        local.push(cuboid(vec3(-9300.0, 300.0, sz - 64.0), vec3(-8988.0, 1500.0, sz), team_mat));
        local.push(cuboid(vec3(-8988.0, 300.0, sz - 64.0), vec3(-8012.0, 1500.0, sz), tint(team_mat, m0)));
        local.push(cuboid(vec3(-8012.0, 300.0, sz - 64.0), vec3(-7700.0, 1500.0, sz), team_mat));
        // back wall, with a gap at the west end for the sky ramp launch pad
        local.push(cuboid(vec3(-8950.0, 1500.0, sz - 64.0), vec3(-7700.0, 1564.0, sz + 300.0), trim));
        local.push(cuboid(vec3(-8700.0, 1200.0, 0.0), vec3(-8300.0, 1500.0, sz - 64.0), CONCRETE));
        local.push(block(-9100.0, 1300.0, 32.0, sz, sz + 64.0, CRATE));
        local.push(block(-7900.0, 1300.0, 32.0, sz, sz + 64.0, CRATE));

        // Level pillar line to the sky fort, with a rest platform halfway.
        for (i, (x, y, top)) in hp_pillars().into_iter().enumerate() {
            if i % 3 == 1 {
                local.push(octa_pillar(x, y, 54.0, top - 200.0, top, PILLAR_SIDE));
            } else {
                local.push(block(x, y, 50.0, top - 200.0, top, PILLAR_MID));
            }
        }
        local.push(cuboid(vec3(-4400.0, -200.0, sz - 48.0), vec3(-4000.0, 200.0, sz), METAL));

        // AK perch in the middle of the hairpin.
        local.push(block(turn_b.x - 150.0, turn_b.y, 40.0, 2500.0, 3080.0, PERCH));

        for br in local {
            let pts: Vec<Vec3> = br.points.iter().map(|p| r(*p)).collect();
            b.push(Brush::hull(&pts, br.mat));
        }
    }

    // Sky fort in the middle.
    let fz = HP_SPAWN_Z;
    b.push(cuboid(vec3(-700.0, -450.0, fz - 64.0), vec3(700.0, 450.0, fz), METAL));
    b.push(cuboid(vec3(-150.0, -150.0, 0.0), vec3(150.0, 150.0, fz - 64.0), CONCRETE));
    for (x, y, h) in [(-350.0, 200.0, 32.0), (350.0, -200.0, 32.0), (0.0, 300.0, 28.0), (0.0, -300.0, 28.0)] {
        b.push(block(x, y, h, fz, fz + h * 2.0, CRATE));
    }

    boundary(&mut b, vec3(9700.0, 5500.0, 0.0), 6600.0);

    let mut spawns: [Vec<Spawn>; 2] = [Vec::new(), Vec::new()];
    for (ti, s) in [(0usize, 1.0f32), (1, -1.0)] {
        for x in [-9100.0, -8800.0, -8500.0, -8200.0, -7900.0] {
            for y in [900.0, 1150.0] {
                spawns[ti].push(Spawn {
                    pos: vec3(x * s, y * s, HP_SPAWN_Z + 37.0),
                    yaw: if s > 0.0 { 270.0 } else { 90.0 },
                });
            }
        }
    }
    let buyzones = [
        Aabb::new(vec3(-9400.0, 250.0, HP_SPAWN_Z - 100.0), vec3(-7600.0, 1600.0, HP_SPAWN_Z + 400.0)),
        Aabb::new(vec3(7600.0, -1600.0, HP_SPAWN_Z - 100.0), vec3(9400.0, -250.0, HP_SPAWN_Z + 400.0)),
    ];

    let mut pickups = vec![
        att(AttItem::Sight(Sight::RedDot), 0.0, 0.0, fz + 62.0),
        gun(WeaponId::Awp, 0.0, 0.0, fz + 24.0),
        health(-450.0, 0.0, fz + 24.0),
        health(450.0, 0.0, fz + 24.0),
    ];
    for s in [1.0f32, -1.0] {
        let a = if s > 0.0 { AttItem::Stock(Stock::Heavy) } else { AttItem::Grip(Grip::Stubby) };
        pickups.push(att(a, (turn_b.x - 150.0) * s, turn_b.y * s, 3142.0));
        pickups.push(gun(WeaponId::Ak47, (turn_b.x - 150.0) * s, turn_b.y * s, 3104.0));
        pickups.push(health(-4200.0 * s, 0.0, fz + 24.0));
        // floating in the surf line along the straights
        pickups.push(health(-1300.0 * s, -2800.0 * s, 3080.0));
        pickups.push(health(0.0, -1300.0 * s, 2400.0));
    }

    // Bot routes for T, rotated for CT.
    let mut routes = Vec::new();
    let depth = 300.0;
    let off = depth * HP_HW / HP_H; // horizontal distance of the line from the ridge
    let lerp = |a: (f32, f32), t: f32| a.0 + (a.1 - a.0) * t;
    {
        let mut pts = vec![wp(-8300.0, 900.0, HP_SPAWN_Z, WpMode::Walk), wp(-8300.0, 380.0, HP_SPAWN_Z, WpMode::Drop)];
        for y in [0.0, -800.0, -1500.0] {
            let t = (200.0 - y) / 1700.0;
            pts.push(wp(-8500.0 + off, y, lerp(z_leg0, t) - depth, WpMode::Surf));
        }
        turn_route(&mut pts, turn_a, 1500.0, 180.0, 270.0, z_a.0, z_a.1, depth, 15.0);
        for x in [-5000.0, -2500.0, 0.0, 2500.0, 4400.0] {
            let z = if x < -1300.0 {
                z_leg1.0 + (3350.0 - z_leg1.0) * (x + 7000.0) / 5700.0
            } else {
                3350.0 + (z_leg1.1 - 3350.0) * (x + 1300.0) / 5700.0
            };
            pts.push(wp(x, -3000.0 + off, z - depth, WpMode::Surf));
        }
        turn_route(&mut pts, turn_b, 950.0, -90.0, 90.0, z_b.0, z_b.1, depth, 15.0);
        for x in [3000.0, 1000.0, -1000.0, -3000.0, -5000.0, -6000.0] {
            let t = (4400.0 - x) / 9900.0;
            pts.push(wp(x, -1100.0 - off, lerp(z_leg2, t.min(1.0)) - depth, WpMode::Surf));
        }
        routes.push(Route { team: Team::T, kind: RouteKind::Surf, name: "hairpin".into(), points: pts });
    }
    {
        let mut pts = vec![wp(-7850.0, 800.0, HP_SPAWN_Z, WpMode::Walk)];
        for (x, y, top) in hp_pillars() {
            pts.push(wp(x, y, top, WpMode::Hop));
        }
        pts.push(wp(-640.0, 0.0, fz, WpMode::Hop));
        pts.push(wp(-250.0, 80.0, fz, WpMode::Hold));
        routes.push(Route { team: Team::T, kind: RouteKind::Sniper, name: "sky fort".into(), points: pts });
    }
    let rotated: Vec<Route> = routes.iter().map(rotate_route).collect();
    routes.extend(rotated);

    // Launch pad from the spawn onto the far face of the first ramp.
    let mut boosters = Vec::new();
    let t = (-400.0 + 1500.0) / 1700.0;
    let target = vec3(-8500.0 - 280.0, -400.0, lerp(z_leg0, 1.0 - t) - 420.0 + 60.0);
    let pad = vec3(-9150.0, 480.0, HP_SPAWN_Z);
    let rot = |p: Vec3| vec3(-p.x, -p.y, p.z);
    add_launchers(&mut b, &mut boosters, &[(pad, target, 1.3), (rot(pad), rot(target), 1.3)]);
    // Boosters on the long straights, pushing the way each track runs.
    for s in [1.0f32, -1.0] {
        let leg1 = Aabb::new(vec3(-2600.0, -3000.0 - HP_HW, 3350.0 - HP_H), vec3(-1300.0, -3000.0 + HP_HW, 3450.0));
        boosters.push(Booster::boost(leg1.turned(s), vec3(s, 0.0, 0.0), 1900.0));
        let leg2 = Aabb::new(vec3(-1200.0, -1100.0 - HP_HW, 2750.0 - HP_H), vec3(200.0, -1100.0 + HP_HW, 2800.0));
        boosters.push(Booster::boost(leg2.turned(s), vec3(-s, 0.0, 0.0), 1900.0));
    }

    let mut sky = Vec::new();
    sky_ramps(
        &mut b,
        &mut boosters,
        &mut pickups,
        &mut sky,
        &SkyDef {
            y: 4700.0,
            x0: -9200.0,
            x1: -1400.0,
            z0: 4000.0,
            z1: 5700.0,
            pad: vec3(-9125.0, 1400.0, HP_SPAWN_Z),
            secs: 2.2,
            items: &[
                PickupKind::Attachment(AttItem::Muzzle(Muzzle::Suppressor)),
                PickupKind::Attachment(AttItem::Sight(Sight::Holo)),
                PickupKind::Health,
            ],
        },
    );

    Map {
        name: "surf_hairpin".into(),
        world: CollisionWorld::new(b),
        spawns,
        kill_z: 450.0,
        buyzones,
        pickups,
        boosters,
        sky,
        routes,
        sky_top: [0.12, 0.22, 0.45],
        sky_horizon: [0.62, 0.72, 0.88],
        fog_color: [0.55, 0.64, 0.80],
        fog_start: 4000.0,
        fog_end: 20000.0,
    }
}

/// T side pillar line from the spawn to the sky fort: (x, y, top).
fn hp_pillars() -> Vec<(f32, f32, f32)> {
    let mut v = Vec::new();
    // spawn edge (-7700, 800) east to the rest platform at x=-4400..-4000,
    // then on to the fort edge at x=-700
    let a: Vec<f32> = (0..18).map(|i| -7560.0 + i as f32 * 178.0).collect();
    let bb: Vec<f32> = (0..18).map(|i| -3860.0 + i as f32 * 176.0).collect();
    for (i, x) in a.iter().enumerate() {
        let t = i as f32 / 17.0;
        let y = 800.0 * (1.0 - t) + if i % 2 == 0 { 50.0 } else { -50.0 };
        v.push((*x, y, HP_SPAWN_Z));
    }
    v.push((-4200.0, 0.0, HP_SPAWN_Z));
    for (i, x) in bb.iter().enumerate() {
        let y = if i % 2 == 0 { 50.0 } else { -50.0 };
        v.push((*x, y, HP_SPAWN_Z));
    }
    v
}
