//! Bots. They play by the same movement rules as the player: every tick
//! they produce a `UserCmd` (view angles, analog move values and buttons)
//! that goes through the regular player movement code. Surfing and bunny
//! hopping are done with the real air acceleration model, the bot just picks
//! good wish directions.

use macroquad::math::{vec3, Vec3};

use crate::game::{Game, Phase, TICK, TICK_MSEC};
use crate::map::{RouteKind, WpMode};
use crate::pmove::*;
use crate::util::{angle_norm, horizontal, vec_to_angles, Rng};
use crate::weapons::{Attachments, Slot, WeaponId, ROCKET_SPEED};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
    Expert,
}

impl Difficulty {
    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Normal => "Normal",
            Difficulty::Hard => "Hard",
            Difficulty::Expert => "Expert",
        }
    }

    pub fn next(self) -> Difficulty {
        match self {
            Difficulty::Easy => Difficulty::Normal,
            Difficulty::Normal => Difficulty::Hard,
            Difficulty::Hard => Difficulty::Expert,
            Difficulty::Expert => Difficulty::Easy,
        }
    }
}

struct Skill {
    reaction: f64,
    turn_rate: f32,
    aim_error: f32,
    error_decay: f32,
    headshot: f32,
    recoil_comp: f32,
    lead: f32,
    fov_cos: f32,
}

fn skill(d: Difficulty) -> Skill {
    match d {
        Difficulty::Easy => Skill {
            reaction: 0.6,
            turn_rate: 260.0,
            aim_error: 7.0,
            error_decay: 1.0,
            headshot: 0.05,
            recoil_comp: 0.3,
            lead: 0.0,
            fov_cos: 0.5,
        },
        Difficulty::Normal => Skill {
            reaction: 0.38,
            turn_rate: 420.0,
            aim_error: 4.5,
            error_decay: 0.65,
            headshot: 0.2,
            recoil_comp: 0.6,
            lead: 0.5,
            fov_cos: 0.3,
        },
        Difficulty::Hard => Skill {
            reaction: 0.24,
            turn_rate: 650.0,
            aim_error: 2.8,
            error_decay: 0.42,
            headshot: 0.4,
            recoil_comp: 0.85,
            lead: 0.85,
            fov_cos: 0.1,
        },
        Difficulty::Expert => Skill {
            reaction: 0.16,
            turn_rate: 950.0,
            aim_error: 1.5,
            error_decay: 0.28,
            headshot: 0.6,
            recoil_comp: 1.0,
            lead: 1.0,
            fov_cos: -0.1,
        },
    }
}

#[derive(Clone, Copy, Debug)]
pub enum BotAction {
    Buy(WeaponId, Attachments),
    Switch(Slot),
    /// Stuck for too long: die and come back (like a `kill` command).
    Suicide,
}

pub struct BotBrain {
    pub difficulty: Difficulty,
    rng: Rng,
    pub actions: Vec<BotAction>,
    pub route: Option<usize>,
    pub wp: usize,
    need_plan: bool,
    hold_until: f64,
    aim: Vec3,
    aim_error: Vec3,
    target: Option<usize>,
    target_since: f64,
    target_seen: f64,
    target_pos: Vec3,
    aim_head: bool,
    hurt_by: Option<usize>,
    hurt_at: f64,
    pref: WeaponId,
    burst: u32,
    burst_pause_until: f64,
    strafe_sign: f32,
    strafe_until: f64,
    progress_pos: Vec3,
    progress_time: f64,
    stuck_jump: bool,
    alt: bool,
    exit_dir: Vec3,
    sidestep_until: f64,
    /// Where we were when we last made real progress (stuck detection).
    anchor_pos: Vec3,
    anchor_time: f64,
    /// Destination for maps without bot routes.
    roam_target: Option<Vec3>,
    /// Testing hook: always pick this route.
    pub force_route: Option<usize>,
}

impl BotBrain {
    pub fn new(difficulty: Difficulty, seed: u64) -> BotBrain {
        let mut rng = Rng::new(seed);
        let pref = pick_weapon(&mut rng);
        BotBrain {
            difficulty,
            rng,
            actions: Vec::new(),
            route: None,
            wp: 0,
            need_plan: true,
            hold_until: 0.0,
            aim: Vec3::ZERO,
            aim_error: Vec3::ZERO,
            target: None,
            target_since: 0.0,
            target_seen: -10.0,
            target_pos: Vec3::ZERO,
            aim_head: false,
            hurt_by: None,
            hurt_at: -10.0,
            pref,
            burst: 0,
            burst_pause_until: 0.0,
            strafe_sign: 1.0,
            strafe_until: 0.0,
            progress_pos: Vec3::ZERO,
            progress_time: 0.0,
            stuck_jump: false,
            alt: false,
            exit_dir: Vec3::X,
            sidestep_until: 0.0,
            anchor_pos: Vec3::ZERO,
            anchor_time: f64::NAN,
            roam_target: None,
            force_route: None,
        }
    }

    pub fn on_spawn(&mut self) {
        self.need_plan = true;
        self.anchor_time = f64::NAN;
        self.target = None;
        self.hurt_by = None;
        if self.rng.chance(0.3) {
            self.pref = pick_weapon(&mut self.rng);
        }
    }

    pub fn on_teleport(&mut self) {
        self.need_plan = true;
        self.anchor_time = f64::NAN;
    }

    pub fn on_hurt(&mut self, attacker: usize) {
        self.hurt_by = Some(attacker);
        self.hurt_at = f64::NAN; // resolved in think() where the time is known
    }
}

/// Bots buy from the same short list as players; the good guns come from
/// map pickups.
fn pick_weapon(rng: &mut Rng) -> WeaponId {
    if rng.chance(0.6) {
        WeaponId::Mp5
    } else {
        WeaponId::M3
    }
}

fn plan_route(g: &Game, i: usize, b: &mut BotBrain) {
    let p = &g.players[i];
    // Sniper routes lead to the map's AWP, so some bots go fetch it.
    let sniper = b.rng.chance(0.35);
    let mut total = 0.0;
    let mut cands: Vec<(usize, f32)> = Vec::new();
    for (ri, r) in g.map.routes.iter().enumerate() {
        if r.team != p.team {
            continue;
        }
        let w = match r.kind {
            RouteKind::Surf => 1.0,
            RouteKind::Bhop => 0.7,
            RouteKind::Sniper => {
                if sniper {
                    2.5
                } else {
                    0.25
                }
            }
        };
        let first = r.points[0].pos;
        let near = 1.0 / (1.0 + (first - p.pm.origin).length() / 900.0);
        let w = w * near;
        total += w;
        cands.push((ri, w));
    }
    let mut pick = b.rng.f32() * total;
    b.route = None;
    for (ri, w) in cands {
        if pick <= w {
            b.route = Some(ri);
            break;
        }
        pick -= w;
    }
    if b.force_route.is_some() {
        b.route = b.force_route;
    }
    // Start at the first waypoint that is not behind us.
    b.wp = 0;
    b.hold_until = 0.0;
    b.need_plan = false;
    b.progress_pos = p.pm.origin;
    b.progress_time = g.time;
}

thread_local! {
    static GRAVITY: std::cell::Cell<f32> = const { std::cell::Cell::new(800.0) };
}

/// Solves `z0 + vz t - g/2 t^2 = z1` for the later root, with the server
/// gravity (which can be changed in the menu).
fn time_to_height(z0: f32, vz: f32, z1: f32) -> Option<f32> {
    let a = GRAVITY.with(|g| g.get()) * 0.5;
    let disc = vz * vz - 4.0 * a * (z1 - z0);
    if disc < 0.0 {
        return None;
    }
    let t = (vz + disc.sqrt()) / (2.0 * a);
    if t > 0.0 {
        Some(t)
    } else {
        None
    }
}

/// Picks an air acceleration wish vector that bends the horizontal velocity
/// toward `desired`. Uses direct acceleration where the Quake air model
/// allows it and perpendicular strafing to gain speed otherwise.
fn air_steer(v: Vec3, desired: Vec3, alt: bool) -> Vec3 {
    let vh = horizontal(v);
    let delta = desired - vh;
    let dl = delta.length();
    if dl < 4.0 {
        return Vec3::ZERO;
    }
    let ddir = delta / dl;
    if vh.dot(ddir) < 25.0 {
        return ddir * dl.min(250.0);
    }
    // Need more speed along our heading: strafe sideways.
    let speed = vh.length().max(1.0);
    let fwd = vh / speed;
    let cross = fwd.x * desired.y - fwd.y * desired.x;
    let side = if cross.abs() > 0.05 * desired.length() {
        cross.signum()
    } else if alt {
        1.0
    } else {
        -1.0
    };
    vec3(-fwd.y * side, fwd.x * side, 0.0) * 250.0
}

pub fn think(g: &Game, i: usize, b: &mut BotBrain) -> UserCmd {
    let now = g.time;
    let p = &g.players[i];
    let mut cmd = UserCmd { msec: TICK_MSEC, viewangles: b.aim, ..Default::default() };
    if !p.alive {
        b.aim = p.angles;
        return cmd;
    }
    if b.hurt_at.is_nan() {
        b.hurt_at = now;
    }
    GRAVITY.with(|gr| gr.set(g.vars.gravity.max(1.0)));

    // Stuck for too long (sliding around on some low ramp, wedged in a
    // corner...): kill ourselves and respawn instead of wasting the round.
    if g.phase == Phase::Live {
        let holding = b
            .route
            .and_then(|r| g.map.routes[r].points.get(b.wp))
            .is_some_and(|w| w.mode == WpMode::Hold && now < b.hold_until);
        if b.anchor_time.is_nan() || holding || p.pm.origin.distance(b.anchor_pos) > 350.0 {
            b.anchor_pos = p.pm.origin;
            b.anchor_time = now;
        } else if now - b.anchor_time > 7.0 {
            b.anchor_time = f64::NAN;
            b.actions.push(BotAction::Suicide);
        }
    }
    b.alt = !b.alt;
    let sk = skill(b.difficulty);
    let vel = p.pm.velocity;
    let eye = p.pm.eye();

    // Buying and route planning.
    if b.need_plan {
        if p.primary.is_none() && g.in_buyzone(i) {
            // Random attachments; the bot's aim logic reads their effects.
            let att = Attachments::random(&mut b.rng);
            b.actions.push(BotAction::Buy(b.pref, att));
        }
        b.aim = p.angles;
        plan_route(g, i, b);
    }

    // ---------------------------------------------------------------
    // Perception
    if (g.tick + i as u64).is_multiple_of(2) {
        let (fwd, _, _) = angle_vectors(b.aim);
        let mut best: Option<(usize, f32, Vec3)> = None;
        for (j, e) in g.players.iter().enumerate() {
            if !e.alive || e.team == p.team {
                continue;
            }
            let chest = e.pm.origin + vec3(0.0, 0.0, if e.pm.ducking { 0.0 } else { 12.0 });
            let head = e.pm.origin + vec3(0.0, 0.0, if e.pm.ducking { 10.0 } else { 27.0 });
            let to = chest - eye;
            let d = to.length();
            if d > 7000.0 {
                continue;
            }
            let aware = fwd.dot(to / d) > sk.fov_cos
                || d < 350.0
                || b.target == Some(j)
                || (b.hurt_by == Some(j) && now - b.hurt_at < 2.0);
            if !aware {
                continue;
            }
            let seen = if g.visible(eye, chest) {
                Some(chest)
            } else if g.visible(eye, head) {
                Some(head)
            } else {
                None
            };
            if let Some(pt) = seen {
                let score = if b.target == Some(j) { d * 0.6 } else { d };
                if best.is_none_or(|bb| score < bb.1) {
                    best = Some((j, score, pt));
                }
            }
        }
        match best {
            Some((j, _, pt)) => {
                if b.target != Some(j) {
                    b.target = Some(j);
                    b.target_since = now;
                    b.aim_head = b.rng.chance(sk.headshot);
                    let e = sk.aim_error;
                    b.aim_error = vec3(b.rng.range(-e, e) * 0.6, b.rng.range(-e, e), 0.0);
                }
                b.target_seen = now;
                b.target_pos = pt;
            }
            None => {
                if b.target.is_some() && now - b.target_seen > 1.0 {
                    b.target = None;
                }
            }
        }
    }
    if let Some(t) = b.target {
        if !g.players[t].alive {
            b.target = None;
        }
    }
    let target_visible = b.target.is_some() && now - b.target_seen < 0.25;

    // ---------------------------------------------------------------
    // Movement
    let (wish, jump, duck, look_dir) = movement(g, i, b, target_visible);

    // ---------------------------------------------------------------
    // Aiming
    let dt = TICK;
    b.aim_error *= (-dt / sk.error_decay).exp();
    let mut desired = b.aim;
    if let Some(t) = b.target {
        let e = &g.players[t];
        let base = if b.aim_head {
            e.pm.origin + vec3(0.0, 0.0, if e.pm.ducking { 10.0 } else { 26.0 })
        } else {
            e.pm.origin + vec3(0.0, 0.0, if e.pm.ducking { 0.0 } else { 10.0 })
        };
        let mut aim_pt = if target_visible { base + (e.pm.velocity - vel) * (0.05 * sk.lead) } else { b.target_pos };
        if p.active_id() == WeaponId::Rocket && target_visible {
            // projectile: lead by the flight time and aim at the feet
            let d = (e.pm.origin - eye).length();
            aim_pt = e.pm.origin + e.pm.velocity * (d / ROCKET_SPEED) * sk.lead.max(0.3) - vec3(0.0, 0.0, 30.0);
        }
        let (pitch, yaw) = vec_to_angles(aim_pt - eye);
        desired = vec3(pitch, yaw, 0.0) - p.pm.punchangle * sk.recoil_comp + b.aim_error;
    } else if look_dir.length_squared() > 0.01 {
        let (pitch, yaw) = vec_to_angles(look_dir);
        desired = vec3(pitch.clamp(-20.0, 30.0), yaw, 0.0);
    }
    let max_step = sk.turn_rate * dt;
    let dyaw = angle_norm(desired.y - b.aim.y);
    let dpitch = desired.x - b.aim.x;
    let k = if b.target.is_some() { 0.3 } else { 0.12 };
    b.aim.y = angle_norm(b.aim.y + (dyaw * k).clamp(-max_step, max_step));
    b.aim.x = (b.aim.x + (dpitch * k).clamp(-max_step, max_step)).clamp(-89.0, 89.0);
    b.aim.z = 0.0;
    cmd.viewangles = b.aim;

    // Convert the wish vector into analog move values for the view yaw.
    let (f, r, _) = angle_vectors(vec3(0.0, b.aim.y, 0.0));
    cmd.forwardmove = wish.dot(f);
    cmd.sidemove = wish.dot(r);
    if jump {
        cmd.buttons |= IN_JUMP;
    }
    if duck {
        cmd.buttons |= IN_DUCK;
    }

    // ---------------------------------------------------------------
    // Weapons
    weapons(g, i, b, &mut cmd, target_visible, &sk);
    cmd
}

fn weapons(g: &Game, i: usize, b: &mut BotBrain, cmd: &mut UserCmd, visible: bool, sk: &Skill) {
    let now = g.time;
    let p = &g.players[i];
    if g.phase == Phase::Freeze {
        return;
    }
    // Pick the right weapon.
    let primary_ok = p.primary.is_some_and(|w| w.clip + w.reserve > 0);
    let secondary_ok = p.secondary.is_some_and(|w| w.clip + w.reserve > 0);
    if now >= p.next_attack || p.active == Slot::Melee {
        if primary_ok && p.active != Slot::Primary {
            b.actions.push(BotAction::Switch(Slot::Primary));
        } else if !primary_ok && secondary_ok && p.active != Slot::Secondary {
            b.actions.push(BotAction::Switch(Slot::Secondary));
        }
    }

    let id = p.active_id();
    let def = id.def();
    let Some(t) = b.target.filter(|_| visible) else {
        // Nothing to shoot at: reload and unscope.
        if let Some(w) = p.weapon() {
            if w.clip * 2 < def.clip && w.reserve > 0 && now - b.target_seen > 1.5 && !p.reloading() {
                cmd.buttons |= IN_RELOAD;
            }
        }
        if p.zoom > 0 && now - b.target_seen > 3.0 && b.alt && p.prev_buttons & IN_ATTACK2 == 0 {
            // cycle the zoom back to 0
            cmd.buttons |= IN_ATTACK2;
        }
        b.burst = 0;
        return;
    };
    if now - b.target_since < sk.reaction {
        return;
    }
    let e = &g.players[t];
    let eye = p.pm.eye();
    let center = e.pm.origin + vec3(0.0, 0.0, 8.0);
    let dist = (center - eye).length();
    let (fwd, _, _) = angle_vectors(p.angles + p.pm.punchangle);
    let to = (b.target_pos - eye).normalize_or_zero();
    let err = fwd.dot(to).clamp(-1.0, 1.0).acos().to_degrees();
    let size = (16.0 / dist.max(1.0)).atan().to_degrees();

    // What the attachments do to this gun.
    let mods = p.weapon().map(|w| w.mods()).unwrap_or_default();
    let airborne = !(p.pm.onground || (g.settings.ramp_accuracy && p.pm.is_surfing()));
    let speed = horizontal(p.pm.velocity).length();
    let mut accuracy = mods.spread;
    if airborne {
        accuracy *= mods.air_spread;
    } else if speed > 140.0 {
        accuracy *= mods.move_spread;
    }

    // Scoping.
    let sniper = p.weapon().is_some_and(|w| w.is_sniper() && w.zoom_levels() > 0);
    if sniper && p.pm.onground && dist > 450.0 && p.zoom == 0 && p.resume_zoom.is_none() {
        if p.prev_buttons & IN_ATTACK2 == 0 && now >= p.next_attack {
            cmd.buttons |= IN_ATTACK2;
        }
        return;
    }
    // Aim down a zoom sight at range when it pays off (the ACOG slows us
    // down, so only use it when not surfing hard).
    if let Some(w) = p.weapon() {
        let ads_ok = !mods.zooms.is_empty() && !w.is_sniper() && (mods.ads_speed == 0.0 || speed < 400.0);
        if ads_ok && dist > 650.0 && p.zoom == 0 && w.zoom_levels() > 0 && p.prev_buttons & IN_ATTACK2 == 0 {
            cmd.buttons |= IN_ATTACK2;
        }
        if w.is_ads(p.zoom) {
            accuracy *= mods.ads_spread;
        }
    }

    // Rockets: lead the target and aim at its feet, never point blank.
    if id == WeaponId::Rocket {
        if !(200.0..2500.0).contains(&dist) {
            return;
        }
        let lead = e.pm.origin + e.pm.velocity * (dist / ROCKET_SPEED) - vec3(0.0, 0.0, 30.0);
        let to_lead = (lead - eye).normalize_or_zero();
        if fwd.dot(to_lead).clamp(-1.0, 1.0).acos().to_degrees() < 6.0 {
            cmd.buttons |= IN_ATTACK;
        }
        return;
    }

    let tolerance = match id {
        WeaponId::Awp | WeaponId::Scout | WeaponId::Laser => size * 0.9,
        WeaponId::M3 => size * 3.0 + 2.0,
        WeaponId::Knife => 25.0,
        _ => size * 1.6 + 0.6,
    };
    if id == WeaponId::Knife && dist > 90.0 {
        return;
    }
    // Don't waste ammo at silly ranges (spread in the air is huge).
    let max_range = match id {
        WeaponId::M3 => 900.0,
        WeaponId::Usp => {
            if airborne {
                900.0
            } else {
                1500.0
            }
        }
        WeaponId::Mp5 => {
            if airborne {
                1100.0
            } else {
                2000.0
            }
        }
        WeaponId::Ak47 => {
            if airborne {
                1500.0
            } else {
                3000.0
            }
        }
        _ => 8000.0,
    };
    // Better accuracy and less damage falloff from attachments let the bot
    // take longer shots.
    let range_gain = p
        .weapon()
        .map(|w| w.damage_at(1500.0) / crate::weapons::Weapon::new(id).damage_at(1500.0).max(0.01))
        .unwrap_or(1.0)
        .clamp(1.0, 1.6);
    if dist > max_range * range_gain / accuracy.max(0.3) {
        return;
    }
    // Snipers only shoot when they are (nearly) standing still.
    if (id == WeaponId::Awp && (speed > 140.0 || !p.pm.onground))
        || (id == WeaponId::Scout && speed > 170.0 && dist > 500.0)
    {
        return;
    }
    if err > tolerance {
        return;
    }
    if now < b.burst_pause_until {
        return;
    }
    match id {
        WeaponId::Usp => {
            if p.prev_buttons & IN_ATTACK == 0 {
                cmd.buttons |= IN_ATTACK;
            }
        }
        WeaponId::Ak47 | WeaponId::Mp5 => {
            cmd.buttons |= IN_ATTACK;
            if now >= p.next_attack && p.weapon().is_some_and(|w| w.clip > 0) {
                b.burst += 1;
                let base: f32 = if dist > 1800.0 {
                    2.0
                } else if dist > 900.0 {
                    4.0
                } else if dist > 400.0 {
                    7.0
                } else {
                    30.0
                };
                // Less recoil (grips, heavy stock, compensator) = longer bursts.
                let max_burst = (base / mods.recoil).round().max(1.0) as u32;
                if b.burst >= max_burst {
                    b.burst = 0;
                    b.burst_pause_until = now + if dist > 1200.0 { 0.45 } else { 0.3 };
                }
            }
        }
        _ => {
            cmd.buttons |= IN_ATTACK;
        }
    }
}

/// Returns (wish velocity, jump, duck, look direction).
fn movement(g: &Game, i: usize, b: &mut BotBrain, fighting: bool) -> (Vec3, bool, bool, Vec3) {
    let now = g.time;
    let p = &g.players[i];
    let pos = p.pm.origin;
    let vel = p.pm.velocity;
    let onground = p.pm.onground;
    let mut jump = false;
    let duck = false;

    if g.phase == Phase::Freeze {
        return (Vec3::ZERO, false, false, Vec3::ZERO);
    }

    let Some(ri) = b.route else {
        return roam(g, i, b);
    };
    let route = &g.map.routes[ri];
    if b.wp >= route.points.len() {
        // End of the route: head for the exit direction and fall off
        // somewhere; the pit teleports us home and we plan a new route.
        let dir = if horizontal(vel).length() > 50.0 { horizontal(vel).normalize() } else { b.exit_dir };
        if onground && b.alt && b.rng.chance(0.05) {
            jump = true;
        }
        return (dir * 250.0, jump, false, dir);
    }
    let w = route.points[b.wp];
    let prev = if b.wp > 0 { route.points[b.wp - 1].pos } else { pos };

    // Stuck detection for ground modes.
    if onground && matches!(w.mode, WpMode::Walk | WpMode::Drop | WpMode::Hop) {
        if pos.distance(b.progress_pos) > 24.0 {
            b.progress_pos = pos;
            b.progress_time = now;
        } else if now - b.progress_time > 1.2 {
            b.stuck_jump = true;
            b.progress_time = now;
            b.sidestep_until = now + 0.6;
            b.strafe_sign = if b.rng.chance(0.5) { 1.0 } else { -1.0 };
            if w.mode != WpMode::Hop && b.rng.chance(0.25) {
                b.wp += 1;
            }
        }
    } else {
        b.progress_pos = pos;
        b.progress_time = now;
    }
    if b.stuck_jump && onground {
        b.stuck_jump = false;
        jump = true;
    }
    if now < b.sidestep_until && onground {
        let to = horizontal(w.pos - pos).normalize_or_zero();
        let side = vec3(-to.y, to.x, 0.0) * b.strafe_sign;
        return ((side + to * 0.3).normalize_or_zero() * 250.0, jump, false, to);
    }

    match w.mode {
        WpMode::Walk | WpMode::Drop => {
            let to = horizontal(w.pos - pos);
            let d = to.length();
            let dir = if d > 1.0 { to / d } else { Vec3::X };
            if w.mode == WpMode::Walk && d < 40.0 {
                b.wp += 1;
            }
            if w.mode == WpMode::Drop {
                // Keep running past the drop point until we fall.
                if !onground && pos.z < w.pos.z + 36.0 - 24.0 {
                    b.wp += 1;
                }
                let seg = horizontal(w.pos - prev).normalize_or_zero();
                let aim = if d < 60.0 && seg.length() > 0.5 { seg } else { dir };
                if !onground {
                    // Airborne after the drop: steer toward the next point.
                    let next = route.points.get(b.wp + 1).map(|n| n.pos).unwrap_or(w.pos);
                    let t = time_to_height(pos.z, vel.z, next.z + 20.0).unwrap_or(0.5).max(0.2);
                    let desired = horizontal(next - pos) / t;
                    return (air_steer(vel, desired, b.alt), false, false, aim);
                }
                return (aim * 250.0, jump, duck, aim);
            }
            if !onground {
                let desired = dir * horizontal(vel).length().max(250.0);
                return (air_steer(vel, desired, b.alt), false, false, dir);
            }
            (dir * 250.0, jump, duck, dir)
        }
        WpMode::Hop => hop(g, i, b, w.pos, route.points.get(b.wp + 1).map(|n| n.pos), jump),
        WpMode::Surf => surf(g, i, b, route, jump),
        WpMode::Hold => {
            if b.hold_until == 0.0 {
                let sniper = matches!(p.active_id(), WeaponId::Awp | WeaponId::Scout);
                b.hold_until = now + if sniper { b.rng.range(25.0, 45.0) } else { b.rng.range(8.0, 18.0) } as f64;
                // Pick a direction to leave toward later.
                b.exit_dir = horizontal(-w.pos).normalize_or_zero();
                if b.exit_dir.length() < 0.5 {
                    b.exit_dir = if p.team == crate::map::Team::T { Vec3::X } else { -Vec3::X };
                }
            }
            if now > b.hold_until {
                b.wp += 1;
            }
            let to = horizontal(w.pos - pos);
            let mut wish = if to.length() > 48.0 { to.normalize() * 250.0 } else { Vec3::ZERO };
            if fighting {
                let sniper = matches!(p.active_id(), WeaponId::Awp | WeaponId::Scout);
                if sniper {
                    wish = Vec3::ZERO;
                } else {
                    if now > b.strafe_until {
                        b.strafe_sign = -b.strafe_sign;
                        b.strafe_until = now + b.rng.range(0.3, 0.8) as f64;
                    }
                    let (_, r, _) = angle_vectors(vec3(0.0, b.aim.y, 0.0));
                    wish = r * (250.0 * b.strafe_sign);
                    if to.length() > 150.0 {
                        wish += to.normalize() * 150.0;
                    }
                }
            }
            // Look toward the enemy side while waiting.
            let enemy_side = if p.team == crate::map::Team::T { Vec3::X } else { -Vec3::X };
            let look = enemy_side + vec3(0.0, 0.0, -0.15);
            (wish, jump, duck, look)
        }
    }
}

fn hop(
    g: &Game,
    i: usize,
    b: &mut BotBrain,
    target: Vec3,
    next: Option<Vec3>,
    mut jump: bool,
) -> (Vec3, bool, bool, Vec3) {
    let p = &g.players[i];
    let pos = p.pm.origin;
    let vel = p.pm.velocity;
    let stand_z = target.z + 36.0;
    let to = horizontal(target - pos);
    let d = to.length();
    let dir = if d > 1.0 { to / d } else { horizontal(vel).normalize_or_zero() };

    if p.pm.onground {
        if d < 60.0 && (pos.z - stand_z).abs() < 24.0 {
            // Landed on the target: move on and jump again right away, on
            // the landing tick, so friction and the landing slowdown never
            // get a chance to eat our speed (a proper bunny hop).
            b.wp += 1;
            let nd = next.map(|n| horizontal(n - pos).normalize_or(dir)).unwrap_or(dir);
            return (nd * 250.0, next.is_some(), false, nd);
        }
        let speed = horizontal(vel).length().max(150.0);
        let ratio = if p.pm.fuser2 > 0.0 { (100.0 - p.pm.fuser2 * 0.001 * 19.0) * 0.01 } else { 1.0 };
        let v0 = 268.33 * ratio;
        let t = time_to_height(pos.z, v0, stand_z).unwrap_or(0.0);
        // Jump so that we come down a little before the centre (air strafing
        // can still add a bit of distance, but can hardly take any away).
        let reach = (speed + 40.0 * t) * t;
        if t > 0.0 && d <= reach + 25.0 {
            jump = true;
        }
        // Also jump if the next step would take us off our current platform.
        let ahead = pos + dir * (speed * TICK * 3.0 + 18.0);
        let floor = g.map.world.trace(ahead, ahead - vec3(0.0, 0.0, 60.0), p.pm.mins(), p.pm.maxs());
        if !floor.hit() {
            jump = true;
        }
        return (dir * 250.0, jump, false, dir);
    }

    // Airborne: aim the landing at the pillar center.
    let t = time_to_height(pos.z, vel.z, stand_z);
    let desired = match t {
        Some(t) if t > 0.03 => to / t,
        _ => dir * horizontal(vel).length().max(250.0),
    };
    let desired = if desired.length() > 1200.0 { desired.normalize() * 1200.0 } else { desired };
    (air_steer(vel, desired, b.alt), false, false, dir)
}

fn surf(g: &Game, i: usize, b: &mut BotBrain, route: &crate::map::Route, jump: bool) -> (Vec3, bool, bool, Vec3) {
    let p = &g.players[i];
    let pos = p.pm.origin;
    let vel = p.pm.velocity;
    let pts = &route.points;
    let w = pts[b.wp];
    let prev = if b.wp > 0 && pts[b.wp - 1].mode == WpMode::Surf {
        pts[b.wp - 1].pos
    } else {
        w.pos - (pts.get(b.wp + 1).map(|n| n.pos).unwrap_or(w.pos + Vec3::X) - w.pos).normalize_or_zero() * 400.0
    };
    let seg = w.pos - prev;
    let seg_len = seg.length().max(1.0);
    let seg_dir = seg / seg_len;

    // Advance when we pass the waypoint along the lane.
    if (pos - w.pos).dot(seg_dir) > 0.0 {
        b.wp += 1;
        if b.wp >= pts.len() {
            b.exit_dir = horizontal(seg_dir).normalize_or_zero();
        }
        return (Vec3::ZERO, jump, false, seg_dir);
    }
    let t = ((pos - prev).dot(seg_dir) / seg_len).clamp(0.0, 1.0);
    let line = prev + seg * t;
    let travel = horizontal(seg_dir).normalize_or_zero();

    if p.pm.onground {
        // Standing on a ridge or a platform: walk toward the line.
        let to = horizontal(line + seg_dir * 200.0 - pos).normalize_or_zero();
        return (to * 250.0, true, false, travel);
    }

    if p.pm.surf_time < 0.2 {
        let n = p.pm.surf_normal;
        let nh = horizontal(n);
        let nh_len = nh.length().max(0.01);
        let nh_dir = nh / nh_len;
        let u = (Vec3::Z - n * n.z).normalize_or_zero();
        let a = u.cross(n).normalize_or_zero();
        let a_dir = if a.dot(travel) >= 0.0 { a } else { -a };
        let vh = horizontal(vel);
        let speed = vh.length();

        // Going the wrong way along the ramp: turn around.
        if vel.dot(a_dir) < 0.0 && speed > 20.0 && vel.dot(a_dir) < -0.5 * speed {
            return (a_dir * 250.0 - nh_dir * 60.0, jump, false, a_dir);
        }
        if speed < 60.0 {
            return (a_dir * 250.0 - nh_dir * 120.0, jump, false, a_dir);
        }

        // Where we want to be heading: a point on the line ahead of us,
        // projected into the ramp plane.
        let look = (speed * 0.35).clamp(150.0, 700.0);
        let target = line + seg_dir * look;
        let to = target - pos;
        let d = (to - n * to.dot(n)).normalize_or_zero();
        let vn = (vel - n * vel.dot(n)).normalize_or_zero();

        // Rotate the velocity with pushes perpendicular to it. That keeps
        // all our speed; pushing straight into a tilted ramp would brake.
        let perp = vec3(-vh.y, vh.x, 0.0) / speed;
        let mut best = Vec3::ZERO;
        let mut best_score = 0.01;
        for c in [perp, -perp] {
            if c.dot(nh_dir) > 0.05 {
                continue; // pushing away from the ramp would throw us off it
            }
            let pc = c - n * c.dot(n);
            let score = pc.dot(d);
            if score > best_score {
                best_score = score;
                best = c;
            }
        }
        let err = vn.dot(d).clamp(-1.0, 1.0).acos().to_degrees();
        let wish = best * (err * 8.0).clamp(0.0, 250.0);
        return (wish, jump, false, (vn + a_dir) * 0.5 + vec3(0.0, 0.0, -0.08));
    }

    // In the air, heading for the ramp line: keep the speed we have along
    // the lane and only correct sideways.
    let ahead = line + seg_dir * 150.0;
    let tt = time_to_height(pos.z, vel.z, ahead.z).unwrap_or(0.4).max(0.15);
    let side = vec3(-travel.y, travel.x, 0.0);
    let lateral = horizontal(ahead - pos).dot(side);
    let forward = horizontal(vel).dot(travel).max(250.0);
    let desired = travel * forward + side * (lateral / tt).clamp(-700.0, 700.0);
    (air_steer(vel, desired, b.alt), jump, false, travel)
}

/// Movement on maps without bot routes (editor maps): head for the enemy
/// spawn or a random pickup, surf any ramp we land on toward it, bunny hop
/// on flat ground. Bots that get stuck die and respawn.
fn roam(g: &Game, i: usize, b: &mut BotBrain) -> (Vec3, bool, bool, Vec3) {
    let p = &g.players[i];
    let pos = p.pm.origin;
    let vel = p.pm.velocity;
    let reached = b.roam_target.is_none_or(|t| horizontal(t - pos).length() < 120.0);
    if reached || b.rng.chance(0.002) {
        let enemy = &g.map.spawns[p.team.other().index()];
        b.roam_target = if !g.map.pickups.is_empty() && b.rng.chance(0.4) {
            let k = b.rng.range_u32(0, g.map.pickups.len() as u32 - 1) as usize;
            Some(g.map.pickups[k].pos)
        } else if !enemy.is_empty() {
            let k = b.rng.range_u32(0, enemy.len() as u32 - 1) as usize;
            Some(enemy[k].pos)
        } else {
            Some(vec3(b.rng.range(-3000.0, 3000.0), b.rng.range(-3000.0, 3000.0), pos.z))
        };
    }
    let target = b.roam_target.unwrap_or(pos);
    let to = horizontal(target - pos);
    let dir = to.normalize_or(Vec3::X);
    if p.pm.onground {
        return (dir * 250.0, b.alt, false, dir);
    }
    if p.pm.surf_time < 0.2 {
        // Ride the ramp toward the target: rotate velocity with pushes
        // perpendicular to it, never away from the ramp.
        let n = p.pm.surf_normal;
        let nh_dir = horizontal(n).normalize_or_zero();
        let vh = horizontal(vel);
        let speed = vh.length();
        let a = (Vec3::Z - n * n.z).normalize_or_zero().cross(n).normalize_or_zero();
        let a_dir = if a.dot(dir) >= 0.0 { a } else { -a };
        if speed < 60.0 {
            return (a_dir * 250.0 - nh_dir * 120.0, false, false, a_dir);
        }
        let perp = vec3(-vh.y, vh.x, 0.0) / speed;
        // keep height: turn toward "along the ramp, slightly up"
        let want = (a_dir + (Vec3::Z - n * n.z).normalize_or_zero() * 0.15).normalize_or_zero();
        let vn = (vel - n * vel.dot(n)).normalize_or_zero();
        let mut best = Vec3::ZERO;
        let mut best_score = 0.01;
        for c in [perp, -perp] {
            if c.dot(nh_dir) > 0.05 {
                continue;
            }
            let score = (c - n * c.dot(n)).dot(want);
            if score > best_score {
                best_score = score;
                best = c;
            }
        }
        let err = vn.dot(want).clamp(-1.0, 1.0).acos().to_degrees();
        return (best * (err * 8.0).clamp(0.0, 250.0), false, false, a_dir);
    }
    let desired = dir * horizontal(vel).length().max(250.0);
    (air_steer(vel, desired, b.alt), false, false, dir)
}
