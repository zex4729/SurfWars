//! Text map files written by the editor (`maps/<name>.map`).
//!
//! ```text
//! name my_map
//! sky 0.2 0.38 0.72 0.78 0.86 0.95
//! fog 0.72 0.8 0.9 3000 14000
//! kill_z 450
//! backdrop city
//! brush grid 70 170 235 | x y z | x y z | ...
//! spawn t x y z yaw
//! pickup health x y z
//! boost minx miny minz maxx maxy maxz dirx diry dirz speed two_way(0/1)
//! launch padx pady padz targetx targety targetz seconds
//! teleport minx miny minz maxx maxy maxz spawn
//! teleport minx miny minz maxx maxy maxz x y z yaw
//! route t surf name_without_spaces | x y z mode | x y z mode ...
//! ```
//!
//! Every brush is stored as the corner points of its convex hull, so any
//! shape (boxes, ramps, turning ramp segments, pillars) round trips.

use std::path::PathBuf;

use macroquad::math::{vec3, Vec3};

use crate::collision::{Brush, CollisionWorld};
use crate::map::{
    Aabb, Backdrop, Booster, Map, Mat, PickupDef, PickupKind, Push, Route, RouteKind, Spawn, Team, TeleDest, Teleport,
    Tex, Waypoint, WpMode,
};

pub fn dir() -> PathBuf {
    PathBuf::from("maps")
}

pub fn path(name: &str) -> PathBuf {
    dir().join(format!("{name}.map"))
}

/// Names of the maps in the `maps` folder.
pub fn list_custom() -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir()) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "map") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    if !crate::map::BUILTIN_MAPS.contains(&stem) {
                        v.push(stem.to_string());
                    }
                }
            }
        }
    }
    v.sort();
    v
}

pub fn tex_key(t: Tex) -> &'static str {
    match t {
        Tex::Grid => "grid",
        Tex::Crate => "crate",
        Tex::Metal => "metal",
        Tex::Concrete => "concrete",
        Tex::Water => "water",
        Tex::Clip => "clip",
    }
}

fn tex_from(k: &str) -> Option<Tex> {
    Some(match k {
        "grid" => Tex::Grid,
        "crate" => Tex::Crate,
        "metal" => Tex::Metal,
        "concrete" => Tex::Concrete,
        "water" => Tex::Water,
        "clip" => Tex::Clip,
        _ => return None,
    })
}

pub fn to_text(m: &Map) -> String {
    let mut s = String::new();
    s += &format!("name {}\n", m.name);
    s += &format!(
        "sky {} {} {} {} {} {}\n",
        m.sky_top[0], m.sky_top[1], m.sky_top[2], m.sky_horizon[0], m.sky_horizon[1], m.sky_horizon[2]
    );
    s += &format!("fog {} {} {} {} {}\n", m.fog_color[0], m.fog_color[1], m.fog_color[2], m.fog_start, m.fog_end);
    s += &format!("kill_z {}\n", m.kill_z);
    s += &format!("backdrop {}\n", m.backdrop.key());
    for b in &m.world.brushes {
        s += &format!("brush {} {} {} {}", tex_key(b.mat.tex), b.mat.color[0], b.mat.color[1], b.mat.color[2]);
        for p in &b.points {
            s += &format!(" | {} {} {}", p.x, p.y, p.z);
        }
        s += "\n";
    }
    for (ti, team) in ["t", "ct"].iter().enumerate() {
        for sp in &m.spawns[ti] {
            s += &format!("spawn {} {} {} {} {}\n", team, sp.pos.x, sp.pos.y, sp.pos.z, sp.yaw);
        }
    }
    for p in &m.pickups {
        s += &format!("pickup {} {} {} {}\n", p.kind.key(), p.pos.x, p.pos.y, p.pos.z);
    }
    for b in &m.boosters {
        match b.push {
            Push::Boost { dir, speed, two_way } => {
                let (a, c) = (b.zone.mins, b.zone.maxs);
                s += &format!(
                    "boost {} {} {} {} {} {} {} {} {} {} {}\n",
                    a.x, a.y, a.z, c.x, c.y, c.z, dir.x, dir.y, dir.z, speed, two_way as u8
                );
            }
            Push::Launch { target, secs } => {
                let p = b.pad();
                s += &format!("launch {} {} {} {} {} {} {}\n", p.x, p.y, p.z, target.x, target.y, target.z, secs);
            }
        }
    }
    for t in &m.teleports {
        let (a, c) = (t.zone.mins, t.zone.maxs);
        s += &format!("teleport {} {} {} {} {} {}", a.x, a.y, a.z, c.x, c.y, c.z);
        match t.dest {
            TeleDest::TeamSpawn => s += " spawn\n",
            TeleDest::Point { pos, yaw } => s += &format!(" {} {} {} {}\n", pos.x, pos.y, pos.z, yaw),
        }
    }
    for r in &m.routes {
        let kind = match r.kind {
            RouteKind::Surf => "surf",
            RouteKind::Bhop => "bhop",
            RouteKind::Sniper => "sniper",
        };
        let team = if r.team == Team::T { "t" } else { "ct" };
        s += &format!("route {team} {kind} {}", r.name.replace(' ', "_"));
        for w in &r.points {
            let mode = match w.mode {
                WpMode::Walk => "walk",
                WpMode::Drop => "drop",
                WpMode::Hop => "hop",
                WpMode::Surf => "surf",
                WpMode::Hold => "hold",
            };
            s += &format!(" | {} {} {} {mode}", w.pos.x, w.pos.y, w.pos.z);
        }
        s += "\n";
    }
    s
}

fn nums(parts: &[&str]) -> Result<Vec<f32>, String> {
    parts.iter().map(|x| x.parse::<f32>().map_err(|e| format!("bad number {x}: {e}"))).collect()
}

pub fn from_text(text: &str) -> Result<Map, String> {
    let mut name = "custom".to_string();
    let mut sky_top = [0.20, 0.38, 0.72];
    let mut sky_horizon = [0.78, 0.86, 0.95];
    let mut fog_color = [0.72, 0.80, 0.90];
    let mut fog = (3000.0, 14000.0);
    let mut kill_z = 450.0;
    let mut backdrop = Backdrop::City;
    let mut brushes = Vec::new();
    let mut spawns: [Vec<Spawn>; 2] = [Vec::new(), Vec::new()];
    let mut pickups = Vec::new();
    let mut boosters = Vec::new();
    let mut teleports = Vec::new();
    let mut routes = Vec::new();
    for (ln, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let err = |m: String| format!("line {}: {m}", ln + 1);
        let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
        let parts: Vec<&str> = rest.split_whitespace().collect();
        match key {
            "name" => name = rest.trim().to_string(),
            "sky" => {
                let v = nums(&parts).map_err(err)?;
                if v.len() == 6 {
                    sky_top = [v[0], v[1], v[2]];
                    sky_horizon = [v[3], v[4], v[5]];
                }
            }
            "fog" => {
                let v = nums(&parts).map_err(err)?;
                if v.len() == 5 {
                    fog_color = [v[0], v[1], v[2]];
                    fog = (v[3], v[4]);
                }
            }
            "backdrop" => {
                backdrop = Backdrop::from_key(rest.trim()).ok_or_else(|| err(format!("unknown backdrop {rest}")))?
            }
            "kill_z" => kill_z = nums(&parts).map_err(err)?.first().copied().unwrap_or(kill_z),
            "brush" => {
                let mut groups = rest.split('|');
                let head: Vec<&str> = groups.next().unwrap_or("").split_whitespace().collect();
                if head.len() != 4 {
                    return Err(err("brush needs: tex r g b".into()));
                }
                let tex = tex_from(head[0]).ok_or_else(|| err(format!("unknown texture {}", head[0])))?;
                let c: Vec<u8> = head[1..].iter().map(|x| x.parse().unwrap_or(128)).collect();
                let mut pts = Vec::new();
                for g in groups {
                    let v = nums(&g.split_whitespace().collect::<Vec<_>>()).map_err(err)?;
                    if v.len() != 3 {
                        return Err(err("points need x y z".into()));
                    }
                    pts.push(vec3(v[0], v[1], v[2]));
                }
                if pts.len() < 4 {
                    return Err(err("brush needs at least 4 points".into()));
                }
                if let Some(b) = try_hull(&pts, Mat::new(tex, [c[0], c[1], c[2]])) {
                    brushes.push(b);
                }
            }
            "spawn" => {
                let v = nums(&parts[1..]).map_err(err)?;
                if v.len() != 4 {
                    return Err(err("spawn needs: team x y z yaw".into()));
                }
                let ti = if parts[0] == "ct" { 1 } else { 0 };
                spawns[ti].push(Spawn { pos: vec3(v[0], v[1], v[2]), yaw: v[3] });
            }
            "pickup" => {
                let kind = PickupKind::from_key(parts.first().copied().unwrap_or(""))
                    .ok_or_else(|| err(format!("unknown pickup {}", parts.first().unwrap_or(&""))))?;
                let v = nums(&parts[1..]).map_err(err)?;
                if v.len() != 3 {
                    return Err(err("pickup needs: kind x y z".into()));
                }
                pickups.push(PickupDef { pos: vec3(v[0], v[1], v[2]), kind });
            }
            "boost" => {
                let v = nums(&parts).map_err(err)?;
                if v.len() != 10 && v.len() != 11 {
                    return Err(err("boost needs: min xyz, max xyz, dir xyz, speed [two_way]".into()));
                }
                let zone = Aabb::new(vec3(v[0], v[1], v[2]), vec3(v[3], v[4], v[5]));
                let dir = vec3(v[6], v[7], v[8]);
                if v.get(10).is_some_and(|x| *x != 0.0) {
                    boosters.push(Booster::two_way(zone, dir, v[9]));
                } else {
                    boosters.push(Booster::boost(zone, dir, v[9]));
                }
            }
            "launch" => {
                let v = nums(&parts).map_err(err)?;
                if v.len() != 7 {
                    return Err(err("launch needs: pad xyz, target xyz, seconds".into()));
                }
                boosters.push(Booster::launcher(vec3(v[0], v[1], v[2]), vec3(v[3], v[4], v[5]), v[6].max(0.2)));
            }
            "teleport" => {
                if parts.len() == 7 && parts[6] == "spawn" {
                    let v = nums(&parts[..6]).map_err(err)?;
                    teleports.push(Teleport::to_spawn(Aabb::new(vec3(v[0], v[1], v[2]), vec3(v[3], v[4], v[5]))));
                } else {
                    let v = nums(&parts).map_err(err)?;
                    if v.len() != 10 {
                        return Err(err("teleport needs: min xyz, max xyz, then spawn or x y z yaw".into()));
                    }
                    let zone = Aabb::new(vec3(v[0], v[1], v[2]), vec3(v[3], v[4], v[5]));
                    teleports.push(Teleport::to_point(zone, vec3(v[6], v[7], v[8]), v[9]));
                }
            }
            "route" => {
                let mut groups = rest.split('|');
                let head: Vec<&str> = groups.next().unwrap_or("").split_whitespace().collect();
                if head.len() != 3 {
                    return Err(err("route needs: team kind name".into()));
                }
                let team = if head[0] == "ct" { Team::CT } else { Team::T };
                let kind = match head[1] {
                    "bhop" => RouteKind::Bhop,
                    "sniper" => RouteKind::Sniper,
                    _ => RouteKind::Surf,
                };
                let mut points = Vec::new();
                for g in groups {
                    let p: Vec<&str> = g.split_whitespace().collect();
                    if p.len() != 4 {
                        return Err(err("waypoints need x y z mode".into()));
                    }
                    let v = nums(&p[..3]).map_err(err)?;
                    let mode = match p[3] {
                        "walk" => WpMode::Walk,
                        "drop" => WpMode::Drop,
                        "hop" => WpMode::Hop,
                        "hold" => WpMode::Hold,
                        _ => WpMode::Surf,
                    };
                    points.push(Waypoint { pos: vec3(v[0], v[1], v[2]), mode });
                }
                routes.push(Route { team, kind, name: head[2].replace('_', " "), points });
            }
            _ => return Err(err(format!("unknown key {key}"))),
        }
    }
    let buyzones = Map::buyzones_from_spawns(&spawns);
    Ok(Map {
        name,
        world: CollisionWorld::new(brushes),
        spawns,
        kill_z,
        buyzones,
        pickups,
        boosters,
        teleports,
        sky: Vec::new(),
        routes,
        sky_top,
        sky_horizon,
        fog_color,
        fog_start: fog.0,
        fog_end: fog.1,
        backdrop,
    })
}

/// Hull that tolerates degenerate input (flat or collinear points).
pub fn try_hull(pts: &[Vec3], mat: Mat) -> Option<Brush> {
    Brush::try_hull(pts, mat)
}

pub fn load(name: &str) -> Result<Map, String> {
    let text = std::fs::read_to_string(path(name)).map_err(|e| e.to_string())?;
    from_text(&text)
}

pub fn save(m: &Map) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let p = path(&m.name);
    std::fs::write(&p, to_text(m)).map_err(|e| e.to_string())?;
    Ok(p)
}

/// Centre of a set of points.
pub fn centroid(pts: &[Vec3]) -> Vec3 {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for p in pts {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    (lo + hi) * 0.5
}
