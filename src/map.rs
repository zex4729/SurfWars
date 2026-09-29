//! Map definitions. Maps are built from convex brushes in code, the same way
//! a Hammer map is made of brushes, plus spawn points, trigger volumes and
//! bot routes.

use macroquad::math::{vec3, Vec3};

use crate::collision::{Brush, CollisionWorld};

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

pub struct Map {
    pub name: &'static str,
    pub world: CollisionWorld,
    pub spawns: [Vec<Spawn>; 2],
    /// Falling into one of these sends you back to your team spawn.
    pub teleports: Vec<Aabb>,
    pub buyzones: [Aabb; 2],
    pub routes: Vec<Route>,
    pub sky_top: [f32; 3],
    pub sky_horizon: [f32; 3],
    pub fog_color: [f32; 3],
    pub fog_start: f32,
    pub fog_end: f32,
}

impl Map {
    pub fn random_spawn(&self, team: Team, taken: &[Vec3], seed: usize) -> Spawn {
        let list = &self.spawns[team.index()];
        for i in 0..list.len() {
            let s = list[(i + seed) % list.len()];
            if taken.iter().all(|p| p.distance(s.pos) > 40.0) {
                return s;
            }
        }
        list[seed % list.len()]
    }
}

pub fn map_names() -> &'static [&'static str] {
    &["surf_wars", "surf_canyon"]
}

pub fn load(name: &str) -> Map {
    match name {
        "surf_canyon" => surf_canyon(),
        _ => surf_wars(),
    }
}

// ---------------------------------------------------------------------------
// Helpers

const LANE_HALF_WIDTH: f32 = 512.0;
const LANE_HEIGHT: f32 = 768.0; // 56.3 degree faces, normal.z = 0.555

/// A surf ramp segment: a triangular prism running along X whose ridge goes
/// from `z0` at `x0` to `z1` at `x1`.
fn ramp_x(x0: f32, x1: f32, yc: f32, z0: f32, z1: f32, hw: f32, h: f32, mat: Mat) -> Brush {
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
fn ramp_y(y0: f32, y1: f32, xc: f32, z0: f32, z1: f32, hw: f32, h: f32, mat: Mat) -> Brush {
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

fn surf_wars() -> Map {
    let mut b: Vec<Brush> = Vec::new();

    // Water at the bottom of the pit (visual only) and a floor below it.
    b.push(cuboid(vec3(-5200.0, -3000.0, -64.0), vec3(5200.0, 3000.0, 0.0), FLOOR));
    b.push(cuboid(vec3(-5200.0, -3000.0, 0.0), vec3(5200.0, 3000.0, 120.0), WATER));

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

        // Middle bunny hop line: descending pillars from the spawn to the island.
        let n = 13;
        for i in 0..n {
            let t = i as f32 / (n - 1) as f32;
            let x = 3350.0 - t * (3350.0 - 880.0);
            let top = SW_SPAWN_Z - 60.0 - t * (SW_SPAWN_Z - 60.0 - 1090.0);
            let y = if i % 2 == 0 { 70.0 } else { -70.0 };
            b.push(block(fx(x), y, 52.0, top - 200.0, top, PILLAR_MID));
        }

        // Side bunny hop paths to the sniper towers.
        for ys in [-1.0f32, 1.0] {
            let n = 19;
            for i in 0..n {
                let t = i as f32 / (n - 1) as f32;
                let x = 3850.0 - t * (3850.0 - 420.0);
                let top = SW_SPAWN_Z - 24.0 - t * (SW_SPAWN_Z - 24.0 - 1880.0);
                let y = ys * (2050.0 + if i % 2 == 0 { 55.0 } else { -55.0 });
                if i % 3 == 1 {
                    b.push(octa_pillar(fx(x), y, 56.0, 0.0, top, PILLAR_SIDE));
                } else {
                    b.push(block(fx(x), y, 50.0, 0.0, top, PILLAR_SIDE));
                }
            }
        }
    }

    // Sniper towers.
    for ys in [-1.0f32, 1.0] {
        let (y0, y1) = (ys * 1850.0, ys * 2350.0);
        let (ylo, yhi) = (y0.min(y1), y0.max(y1));
        b.push(cuboid(vec3(-300.0, ylo, 0.0), vec3(300.0, yhi, 1850.0), CONCRETE));
        // parapet toward the lanes
        let (p0, p1) = (ys * 1850.0, ys * 1880.0);
        b.push(cuboid(vec3(-300.0, p0.min(p1), 1850.0), vec3(-60.0, p0.max(p1), 1896.0), METAL));
        b.push(cuboid(vec3(60.0, p0.min(p1), 1850.0), vec3(300.0, p0.max(p1), 1896.0), METAL));
        b.push(block(0.0, ys * 2250.0, 32.0, 1850.0, 1914.0, CRATE));
        b.push(block(-200.0, ys * 2150.0, 28.0, 1850.0, 1906.0, CRATE));
        b.push(block(200.0, ys * 2150.0, 28.0, 1850.0, 1906.0, CRATE));
    }

    // Middle island.
    b.push(cuboid(vec3(-700.0, -420.0, 936.0), vec3(700.0, 420.0, 1000.0), METAL));
    b.push(cuboid(vec3(-200.0, -200.0, 0.0), vec3(200.0, 200.0, 936.0), CONCRETE));
    for (x, y, h) in
        [(0.0, 0.0, 40.0), (-380.0, 220.0, 32.0), (380.0, -220.0, 32.0), (-380.0, -250.0, 28.0), (380.0, 250.0, 28.0)]
    {
        b.push(block(x, y, h, 1000.0, 1000.0 + h * 2.0, CRATE));
    }
    b.push(cuboid(vec3(-24.0, -300.0, 1000.0), vec3(24.0, -120.0, 1060.0), METAL));
    b.push(cuboid(vec3(-24.0, 120.0, 1000.0), vec3(24.0, 300.0, 1060.0), METAL));

    boundary(&mut b, vec3(5100.0, 2900.0, 0.0), 3600.0);

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

    let teleports = vec![Aabb::new(vec3(-6000.0, -4000.0, -500.0), vec3(6000.0, 4000.0, 450.0))];

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
        let n = 13;
        for i in 0..n {
            let t = i as f32 / (n - 1) as f32;
            let x = -(3350.0 - t * (3350.0 - 880.0));
            let top = SW_SPAWN_Z - 60.0 - t * (SW_SPAWN_Z - 60.0 - 1090.0);
            let y = if i % 2 == 0 { 70.0 } else { -70.0 };
            pts.push(wp(x, y, top, WpMode::Hop));
        }
        pts.push(wp(-640.0, 0.0, 1000.0, WpMode::Hop));
        pts.push(wp(-300.0, 0.0, 1000.0, WpMode::Hold));
        routes.push(Route { team: Team::T, kind: RouteKind::Bhop, name: "mid pillars".into(), points: pts });
    }
    // Side paths to the towers.
    for ys in [1.0f32, -1.0] {
        let mut pts = vec![
            wp(-4200.0, ys * 1100.0, SW_SPAWN_Z, WpMode::Walk),
            wp(-4150.0, ys * 2020.0, SW_SPAWN_Z, WpMode::Walk),
        ];
        let n = 19;
        for i in 0..n {
            let t = i as f32 / (n - 1) as f32;
            let x = -(3850.0 - t * (3850.0 - 420.0));
            let top = SW_SPAWN_Z - 24.0 - t * (SW_SPAWN_Z - 24.0 - 1880.0);
            let y = ys * (2050.0 + if i % 2 == 0 { 55.0 } else { -55.0 });
            pts.push(wp(x, y, top, WpMode::Hop));
        }
        pts.push(wp(-200.0, ys * 2100.0, 1850.0, WpMode::Hop));
        pts.push(wp(-120.0, ys * 1960.0, 1850.0, WpMode::Hold));
        routes.push(Route {
            team: Team::T,
            kind: RouteKind::Sniper,
            name: format!("tower {}", if ys > 0.0 { "north" } else { "south" }),
            points: pts,
        });
    }
    let mirrored: Vec<Route> = routes.iter().map(mirror_route).collect();
    routes.extend(mirrored);

    Map {
        name: "surf_wars",
        world: CollisionWorld::new(b),
        spawns,
        teleports,
        buyzones,
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

fn surf_canyon() -> Map {
    let mut b: Vec<Brush> = Vec::new();
    b.push(cuboid(vec3(-5600.0, -3400.0, -64.0), vec3(5600.0, 3400.0, 0.0), FLOOR));
    b.push(cuboid(vec3(-5600.0, -3400.0, 0.0), vec3(5600.0, 3400.0, 120.0), WATER));

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

        // Pillar field between the middle lane and the outer lanes.
        for ys in [-1.0f32, 1.0] {
            let n = 16;
            for i in 0..n {
                let t = i as f32 / (n - 1) as f32;
                let x = 4300.0 - t * (4300.0 - 700.0);
                let top = SC_SPAWN_Z - 40.0 - t * (SC_SPAWN_Z - 40.0 - 1650.0);
                let y = ys * (880.0 + if i % 2 == 0 { 60.0 } else { -60.0 });
                if i % 2 == 0 {
                    b.push(octa_pillar(fx(x), y, 56.0, top - 260.0, top, PILLAR_SIDE));
                } else {
                    b.push(block(fx(x), y, 50.0, top - 260.0, top, PILLAR_MID));
                }
            }
        }
    }
    // Middle platforms at the end of the pillar lines.
    for ys in [-1.0f32, 1.0] {
        let (y0, y1) = (ys * 700.0, ys * 1100.0);
        b.push(cuboid(vec3(-560.0, y0.min(y1), 1560.0), vec3(560.0, y0.max(y1), 1600.0), METAL));
        b.push(block(0.0, ys * 900.0, 36.0, 1600.0, 1672.0, CRATE));
        b.push(block(-300.0, ys * 830.0, 28.0, 1600.0, 1656.0, CRATE));
        b.push(block(300.0, ys * 970.0, 28.0, 1600.0, 1656.0, CRATE));
    }

    boundary(&mut b, vec3(5700.0, 3300.0, 0.0), 3800.0);

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
    let teleports = vec![Aabb::new(vec3(-7000.0, -5000.0, -500.0), vec3(7000.0, 5000.0, 450.0))];

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
        let n = 16;
        for i in 0..n {
            let t = i as f32 / (n - 1) as f32;
            let x = -(4300.0 - t * (4300.0 - 700.0));
            let top = SC_SPAWN_Z - 40.0 - t * (SC_SPAWN_Z - 40.0 - 1650.0);
            let y = ys * (880.0 + if i % 2 == 0 { 60.0 } else { -60.0 });
            pts.push(wp(x, y, top, WpMode::Hop));
        }
        pts.push(wp(-450.0, ys * 900.0, 1600.0, WpMode::Hop));
        pts.push(wp(-150.0, ys * 800.0, 1600.0, WpMode::Hold));
        routes.push(Route {
            team: Team::T,
            kind: if ys > 0.0 { RouteKind::Sniper } else { RouteKind::Bhop },
            name: format!("pillars {}", if ys > 0.0 { "north" } else { "south" }),
            points: pts,
        });
    }
    let mirrored: Vec<Route> = routes.iter().map(mirror_route).collect();
    routes.extend(mirrored);

    Map {
        name: "surf_canyon",
        world: CollisionWorld::new(b),
        spawns,
        teleports,
        buyzones,
        routes,
        sky_top: [0.55, 0.30, 0.35],
        sky_horizon: [0.98, 0.72, 0.50],
        fog_color: [0.92, 0.70, 0.55],
        fog_start: 2500.0,
        fog_end: 13000.0,
    }
}
