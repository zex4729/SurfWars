//! Player movement.
//!
//! This is a line by line port of the Counter-Strike 1.6 player movement
//! code (`pm_shared.c` from the Half-Life SDK plus the CS specific changes:
//! jump stamina via `fuser2`, landing slowdown, duck timing and the bunny hop
//! speed cap). Only the constants that surf servers traditionally change
//! (`sv_airaccelerate`, `sv_maxvelocity` and the bunny hop cap) differ from a
//! stock server, see [`MoveVars::surf_server`].

use macroquad::math::{vec3, Vec3};

use crate::collision::{CollisionWorld, Trace, DIST_EPSILON};

pub const IN_ATTACK: u32 = 1 << 0;
pub const IN_JUMP: u32 = 1 << 1;
pub const IN_DUCK: u32 = 1 << 2;
pub const IN_USE: u32 = 1 << 5;
pub const IN_ATTACK2: u32 = 1 << 11;
pub const IN_RELOAD: u32 = 1 << 13;

pub const PLAYER_MINS: [Vec3; 2] = [vec3(-16.0, -16.0, -36.0), vec3(-16.0, -16.0, -18.0)];
pub const PLAYER_MAXS: [Vec3; 2] = [vec3(16.0, 16.0, 36.0), vec3(16.0, 16.0, 18.0)];

const VEC_VIEW: f32 = 17.0;
const VEC_DUCK_VIEW: f32 = 12.0;
const VEC_DUCK_HULL_MIN: f32 = -18.0;
const VEC_HULL_MIN: f32 = -36.0;
const TIME_TO_DUCK: f32 = 0.4;
const PLAYER_DUCKING_MULTIPLIER: f32 = 0.333;
const STOP_EPSILON: f32 = 0.1;
/// `pmove->friction`, the per player friction multiplier. Always 1 here.
const PLAYER_FRICTION: f32 = 1.0;
const MAX_CLIP_PLANES: usize = 5;
const BUNNYJUMP_MAX_SPEED_FACTOR: f32 = 1.2;
const PLAYER_FALL_PUNCH_THRESHOLD: f32 = 350.0;
pub const PLAYER_FATAL_FALL_SPEED: f32 = 1100.0;
pub const PLAYER_MAX_SAFE_FALL_SPEED: f32 = 500.0;
pub const DAMAGE_FOR_FALL_SPEED: f32 = 100.0 / (PLAYER_FATAL_FALL_SPEED - PLAYER_MAX_SAFE_FALL_SPEED);

/// Server movement variables (the `movevars_t` of GoldSrc).
#[derive(Clone, Copy, Debug)]
pub struct MoveVars {
    pub gravity: f32,
    pub stopspeed: f32,
    pub maxspeed: f32,
    pub accelerate: f32,
    pub airaccelerate: f32,
    pub friction: f32,
    pub edgefriction: f32,
    pub stepsize: f32,
    pub maxvelocity: f32,
    pub bounce: f32,
    /// CS 1.6 `PM_PreventMegaBunnyJumping`. Surf servers remove it.
    pub bhop_cap: bool,
    /// Hold jump to bunny hop. Off by default, like stock CS.
    pub autobhop: bool,
}

impl MoveVars {
    /// Stock Counter-Strike 1.6 server settings.
    pub fn stock() -> MoveVars {
        MoveVars {
            gravity: 800.0,
            stopspeed: 75.0,
            maxspeed: 320.0,
            accelerate: 5.0,
            airaccelerate: 10.0,
            friction: 4.0,
            edgefriction: 2.0,
            stepsize: 18.0,
            maxvelocity: 2000.0,
            bounce: 1.0,
            bhop_cap: true,
            autobhop: false,
        }
    }

    /// A more forgiving surf setup: more air control, floatier gravity,
    /// higher speed limit and auto bunny hop.
    pub fn easy_surf() -> MoveVars {
        MoveVars {
            airaccelerate: 150.0,
            gravity: 650.0,
            maxvelocity: 5000.0,
            bhop_cap: false,
            autobhop: true,
            ..MoveVars::stock()
        }
    }

    /// The usual CS 1.6 surf server configuration: everything stock except
    /// `sv_airaccelerate 100`, `sv_maxvelocity 3500` and no bunny hop cap.
    pub fn surf_server() -> MoveVars {
        MoveVars { airaccelerate: 100.0, maxvelocity: 3500.0, bhop_cap: false, ..MoveVars::stock() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UserCmd {
    pub forwardmove: f32,
    pub sidemove: f32,
    pub upmove: f32,
    pub buttons: u32,
    /// pitch, yaw, roll in degrees (Quake convention, positive pitch looks down)
    pub viewangles: Vec3,
    pub msec: u32,
}

/// Per player movement state (the relevant parts of `playermove_t`).
#[derive(Clone, Copy, Debug)]
pub struct PmState {
    pub origin: Vec3,
    pub velocity: Vec3,
    pub basevelocity: Vec3,
    pub onground: bool,
    pub ground_normal: Vec3,
    /// FL_DUCKING
    pub ducking: bool,
    pub in_duck: bool,
    /// milliseconds, counts down
    pub duck_time: f32,
    pub usehull: usize,
    pub view_ofs: f32,
    pub oldbuttons: u32,
    /// CS jump stamina timer in milliseconds.
    pub fuser2: f32,
    pub fall_velocity: f32,
    pub maxspeed: f32,
    pub punchangle: Vec3,
    pub dead: bool,
    /// Set when the latest move touched a surfable slope.
    pub surf_normal: Vec3,
    /// Seconds since the last contact with a surfable slope.
    pub surf_time: f32,
    /// Speed of the last hard landing, consumed by the game for sounds/damage.
    pub landed_speed: f32,
    pub jumped: bool,
}

impl PmState {
    pub fn new(origin: Vec3) -> PmState {
        PmState {
            origin,
            velocity: Vec3::ZERO,
            basevelocity: Vec3::ZERO,
            onground: false,
            ground_normal: Vec3::Z,
            ducking: false,
            in_duck: false,
            duck_time: 0.0,
            usehull: 0,
            view_ofs: VEC_VIEW,
            oldbuttons: 0,
            fuser2: 0.0,
            fall_velocity: 0.0,
            maxspeed: 250.0,
            punchangle: Vec3::ZERO,
            dead: false,
            surf_normal: Vec3::Z,
            surf_time: 10.0,
            landed_speed: 0.0,
            jumped: false,
        }
    }

    pub fn mins(&self) -> Vec3 {
        PLAYER_MINS[self.usehull]
    }

    pub fn maxs(&self) -> Vec3 {
        PLAYER_MAXS[self.usehull]
    }

    pub fn eye(&self) -> Vec3 {
        self.origin + vec3(0.0, 0.0, self.view_ofs)
    }

    /// Bottom center of the collision hull.
    pub fn feet(&self) -> Vec3 {
        self.origin + vec3(0.0, 0.0, self.mins().z)
    }

    pub fn is_surfing(&self) -> bool {
        self.surf_time < 0.12
    }
}

/// Quake style `AngleVectors`.
pub fn angle_vectors(angles: Vec3) -> (Vec3, Vec3, Vec3) {
    let (sy, cy) = angles.y.to_radians().sin_cos();
    let (sp, cp) = angles.x.to_radians().sin_cos();
    let (sr, cr) = angles.z.to_radians().sin_cos();
    let forward = vec3(cp * cy, cp * sy, -sp);
    let right = vec3(-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp);
    let up = vec3(cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp);
    (forward, right, up)
}

fn vec_normalize(v: &mut Vec3) -> f32 {
    let len = v.length();
    if len > 0.0 {
        *v /= len;
    }
    len
}

pub struct PlayerMove<'a> {
    pub world: &'a CollisionWorld,
    pub vars: &'a MoveVars,
    pub s: &'a mut PmState,
    pub cmd: UserCmd,
    frametime: f32,
    forward: Vec3,
    right: Vec3,
}

impl<'a> PlayerMove<'a> {
    pub fn new(world: &'a CollisionWorld, vars: &'a MoveVars, s: &'a mut PmState, cmd: UserCmd) -> Self {
        PlayerMove { world, vars, s, cmd, frametime: 0.0, forward: Vec3::X, right: -Vec3::Y }
    }

    fn player_trace(&self, start: Vec3, end: Vec3) -> Trace {
        self.world.trace(start, end, self.s.mins(), self.s.maxs())
    }

    fn note_touch(&mut self, normal: Vec3) {
        // Surfable slope: too steep to stand on, but not a wall or ceiling.
        if normal.z > 0.05 && normal.z < 0.7 {
            self.s.surf_normal = normal;
            self.s.surf_time = 0.0;
        }
    }

    /// PM_PlayerMove for MOVETYPE_WALK.
    pub fn run(&mut self) {
        self.s.jumped = false;
        self.s.landed_speed = 0.0;
        self.check_parameters();
        self.frametime = self.cmd.msec as f32 * 0.001;
        self.s.surf_time += self.frametime;
        self.reduce_timers();

        let (f, r, _) = angle_vectors(self.cmd.viewangles);
        self.forward = f;
        self.right = r;

        if self.check_stuck() {
            return;
        }

        self.categorize_position();

        if !self.s.onground {
            self.s.fall_velocity = -self.s.velocity.z;
        }

        self.duck();

        self.add_correct_gravity();

        if self.cmd.buttons & IN_JUMP != 0 {
            self.jump();
        } else {
            self.s.oldbuttons &= !IN_JUMP;
        }

        if self.s.onground {
            self.s.velocity.z = 0.0;
            self.friction();
        }

        self.check_velocity();

        if self.s.onground {
            self.walk_move();
        } else {
            self.air_move();
        }

        self.categorize_position();

        self.s.velocity -= self.s.basevelocity;
        self.check_velocity();
        self.fixup_gravity_velocity();

        if self.s.onground {
            self.s.velocity.z = 0.0;
        }

        self.check_falling();
        self.probe_surf();
    }

    /// Detects a slope right next to the player so the surf board stays
    /// visible while gliding along a ramp without pushing into it.
    fn probe_surf(&mut self) {
        if self.s.onground || self.s.surf_time > 0.3 {
            return;
        }
        let n = self.s.surf_normal;
        let tr = self.player_trace(self.s.origin, self.s.origin - n * 3.0);
        if tr.hit() && tr.normal.z > 0.05 && tr.normal.z < 0.7 {
            self.s.surf_normal = tr.normal;
            self.s.surf_time = 0.0;
        }
    }

    fn check_parameters(&mut self) {
        let c = &mut self.cmd;
        let spd = (c.forwardmove * c.forwardmove + c.sidemove * c.sidemove + c.upmove * c.upmove).sqrt();
        self.s.maxspeed = self.s.maxspeed.min(self.vars.maxspeed);
        if spd != 0.0 && spd > self.s.maxspeed {
            let ratio = self.s.maxspeed / spd;
            c.forwardmove *= ratio;
            c.sidemove *= ratio;
            c.upmove *= ratio;
        }
        if self.s.dead {
            c.forwardmove = 0.0;
            c.sidemove = 0.0;
            c.upmove = 0.0;
        }
        // PM_DropPunchAngle
        let mut punch = self.s.punchangle;
        let mut len = vec_normalize(&mut punch);
        len -= (10.0 + len * 0.5) * (c.msec as f32 * 0.001);
        len = len.max(0.0);
        self.s.punchangle = punch * len;
    }

    fn reduce_timers(&mut self) {
        let msec = self.cmd.msec as f32;
        if self.s.duck_time > 0.0 {
            self.s.duck_time = (self.s.duck_time - msec).max(0.0);
        }
        if self.s.fuser2 > 0.0 {
            self.s.fuser2 = (self.s.fuser2 - msec).max(0.0);
        }
    }

    fn check_stuck(&mut self) -> bool {
        let tr = self.player_trace(self.s.origin, self.s.origin);
        if !tr.startsolid {
            return false;
        }
        // Nudge the player out of the solid, closest offsets first.
        const STEPS: [f32; 7] = [0.125, 0.5, 1.0, 2.0, 4.0, 8.0, 18.0];
        for step in STEPS {
            for z in [0.0, 1.0, -1.0] {
                for y in [0.0, 1.0, -1.0] {
                    for x in [0.0, 1.0, -1.0] {
                        if x == 0.0 && y == 0.0 && z == 0.0 {
                            continue;
                        }
                        let test = self.s.origin + vec3(x, y, z) * step;
                        if !self.player_trace(test, test).startsolid {
                            self.s.origin = test;
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    fn categorize_position(&mut self) {
        let point = self.s.origin - vec3(0.0, 0.0, 2.0);
        if self.s.velocity.z > 180.0 {
            self.s.onground = false;
            return;
        }
        let tr = self.player_trace(self.s.origin, point);
        if tr.normal.z < 0.7 {
            self.s.onground = false;
            if tr.hit() {
                self.note_touch(tr.normal);
            }
        } else {
            self.s.onground = true;
            self.s.ground_normal = tr.normal;
        }
        if self.s.onground && !tr.startsolid && !tr.allsolid {
            self.s.origin = tr.endpos;
        }
    }

    fn check_velocity(&mut self) {
        let mv = self.vars.maxvelocity;
        for i in 0..3 {
            if !self.s.velocity[i].is_finite() {
                self.s.velocity[i] = 0.0;
            }
            if !self.s.origin[i].is_finite() {
                self.s.origin[i] = 0.0;
            }
            self.s.velocity[i] = self.s.velocity[i].clamp(-mv, mv);
        }
    }

    fn add_correct_gravity(&mut self) {
        self.s.velocity.z -= self.vars.gravity * 0.5 * self.frametime;
        self.s.velocity.z += self.s.basevelocity.z * self.frametime;
        self.s.basevelocity.z = 0.0;
        self.check_velocity();
    }

    fn fixup_gravity_velocity(&mut self) {
        self.s.velocity.z -= self.vars.gravity * self.frametime * 0.5;
        self.check_velocity();
    }

    fn prevent_mega_bunny_jumping(&mut self) {
        let maxscaledspeed = BUNNYJUMP_MAX_SPEED_FACTOR * self.s.maxspeed;
        if maxscaledspeed <= 0.0 {
            return;
        }
        let spd = self.s.velocity.length();
        if spd <= maxscaledspeed {
            return;
        }
        let fraction = (maxscaledspeed / spd) * 0.65;
        self.s.velocity *= fraction;
    }

    fn jump(&mut self) {
        if self.s.dead {
            self.s.oldbuttons |= IN_JUMP;
            return;
        }
        if !self.s.onground {
            self.s.oldbuttons |= IN_JUMP;
            return;
        }
        if self.s.oldbuttons & IN_JUMP != 0 && !self.vars.autobhop {
            return; // don't pogo stick
        }

        self.s.onground = false;
        if self.vars.bhop_cap {
            self.prevent_mega_bunny_jumping();
        }

        self.s.velocity.z = (2.0f32 * 800.0 * 45.0).sqrt();

        if self.s.fuser2 > 0.0 {
            let ratio = (100.0 - self.s.fuser2 * 0.001 * 19.0) * 0.01;
            self.s.velocity.z *= ratio;
        }
        #[allow(clippy::excessive_precision)]
        {
            self.s.fuser2 = 1315.789429;
        }
        self.s.jumped = true;

        self.fixup_gravity_velocity();
        self.s.oldbuttons |= IN_JUMP;
    }

    fn spline_fraction(value: f32, scale: f32) -> f32 {
        let v = scale * value;
        let v2 = v * v;
        3.0 * v2 - 2.0 * v2 * v
    }

    fn duck(&mut self) {
        let buttons_changed = self.s.oldbuttons ^ self.cmd.buttons;
        let pressed = buttons_changed & self.cmd.buttons;

        if self.cmd.buttons & IN_DUCK != 0 {
            self.s.oldbuttons |= IN_DUCK;
        } else {
            self.s.oldbuttons &= !IN_DUCK;
        }

        if self.s.dead {
            return;
        }

        if self.s.ducking {
            self.cmd.forwardmove *= PLAYER_DUCKING_MULTIPLIER;
            self.cmd.sidemove *= PLAYER_DUCKING_MULTIPLIER;
            self.cmd.upmove *= PLAYER_DUCKING_MULTIPLIER;
        }

        if self.cmd.buttons & IN_DUCK != 0 || self.s.in_duck || self.s.ducking {
            if self.cmd.buttons & IN_DUCK != 0 {
                if pressed & IN_DUCK != 0 && !self.s.ducking {
                    self.s.duck_time = 1000.0;
                    self.s.in_duck = true;
                }
                let time = (1.0 - self.s.duck_time / 1000.0).max(0.0);
                if self.s.in_duck {
                    if self.s.duck_time / 1000.0 <= 1.0 - TIME_TO_DUCK || !self.s.onground {
                        self.s.usehull = 1;
                        self.s.view_ofs = VEC_DUCK_VIEW;
                        self.s.ducking = true;
                        self.s.in_duck = false;
                        if self.s.onground {
                            self.s.origin -= PLAYER_MINS[1] - PLAYER_MINS[0];
                            self.check_stuck();
                            self.categorize_position();
                        }
                    } else {
                        let f_more = VEC_DUCK_HULL_MIN - VEC_HULL_MIN;
                        let duck_fraction = Self::spline_fraction(time, 1.0 / TIME_TO_DUCK);
                        self.s.view_ofs = (VEC_DUCK_VIEW - f_more) * duck_fraction + VEC_VIEW * (1.0 - duck_fraction);
                    }
                }
            } else {
                self.unduck();
            }
        }
    }

    fn unduck(&mut self) {
        let mut new_origin = self.s.origin;
        if self.s.onground {
            new_origin += PLAYER_MINS[1] - PLAYER_MINS[0];
        }
        if self.world.box_stuck(new_origin, PLAYER_MINS[0], PLAYER_MAXS[0]) {
            // Not enough room to stand up, stay ducked.
            return;
        }
        self.s.usehull = 0;
        self.s.ducking = false;
        self.s.in_duck = false;
        self.s.view_ofs = VEC_VIEW;
        self.s.duck_time = 0.0;
        self.s.origin = new_origin;
        self.categorize_position();
    }

    fn friction(&mut self) {
        let vel = self.s.velocity;
        let speed = vel.length();
        if speed < 0.1 {
            return;
        }
        let mut drop = 0.0;
        if self.s.onground {
            let mut start = self.s.origin + vel / speed * 16.0;
            start.z = self.s.origin.z + self.s.mins().z;
            let mut stop = start;
            stop.z = start.z - 34.0;
            let tr = self.player_trace(start, stop);
            let mut friction =
                if tr.fraction == 1.0 { self.vars.friction * self.vars.edgefriction } else { self.vars.friction };
            friction *= PLAYER_FRICTION;
            let control = if speed < self.vars.stopspeed { self.vars.stopspeed } else { speed };
            drop += control * friction * self.frametime;
        }
        let mut newspeed = speed - drop;
        if newspeed < 0.0 {
            newspeed = 0.0;
        }
        newspeed /= speed;
        self.s.velocity = vel * newspeed;
    }

    fn accelerate(&mut self, wishdir: Vec3, wishspeed: f32, accel: f32) {
        if self.s.dead {
            return;
        }
        let currentspeed = self.s.velocity.dot(wishdir);
        let addspeed = wishspeed - currentspeed;
        if addspeed <= 0.0 {
            return;
        }
        let mut accelspeed = accel * self.frametime * wishspeed;
        if accelspeed > addspeed {
            accelspeed = addspeed;
        }
        self.s.velocity += wishdir * accelspeed;
    }

    fn air_accelerate(&mut self, wishdir: Vec3, wishspeed: f32, accel: f32) {
        if self.s.dead {
            return;
        }
        let wishspd = wishspeed.min(30.0);
        let currentspeed = self.s.velocity.dot(wishdir);
        let addspeed = wishspd - currentspeed;
        if addspeed <= 0.0 {
            return;
        }
        let mut accelspeed = accel * wishspeed * self.frametime;
        if accelspeed > addspeed {
            accelspeed = addspeed;
        }
        self.s.velocity += wishdir * accelspeed;
    }

    fn wish(&mut self) -> (Vec3, f32) {
        let fmove = self.cmd.forwardmove;
        let smove = self.cmd.sidemove;
        let mut f = self.forward;
        let mut r = self.right;
        f.z = 0.0;
        r.z = 0.0;
        vec_normalize(&mut f);
        vec_normalize(&mut r);
        let mut wishvel = vec3(f.x * fmove + r.x * smove, f.y * fmove + r.y * smove, 0.0);
        let mut wishdir = wishvel;
        let mut wishspeed = vec_normalize(&mut wishdir);
        if wishspeed > self.s.maxspeed {
            wishvel *= self.s.maxspeed / wishspeed;
            wishspeed = self.s.maxspeed;
        }
        let _ = wishvel;
        (wishdir, wishspeed)
    }

    fn walk_move(&mut self) {
        if self.s.fuser2 > 0.0 {
            let ratio = (100.0 - self.s.fuser2 * 0.001 * 19.0) * 0.01;
            self.s.velocity.x *= ratio;
            self.s.velocity.y *= ratio;
        }

        let (wishdir, wishspeed) = self.wish();

        self.s.velocity.z = 0.0;
        self.accelerate(wishdir, wishspeed, self.vars.accelerate);
        self.s.velocity.z = 0.0;

        self.s.velocity += self.s.basevelocity;

        let spd = self.s.velocity.length();
        if spd < 1.0 {
            self.s.velocity = Vec3::ZERO;
            return;
        }

        let oldonground = self.s.onground;

        let mut dest = self.s.origin + self.s.velocity * self.frametime;
        dest.z = self.s.origin.z;

        let tr = self.player_trace(self.s.origin, dest);
        if tr.fraction == 1.0 {
            self.s.origin = tr.endpos;
            return;
        }

        if !oldonground {
            return;
        }

        let original = self.s.origin;
        let originalvel = self.s.velocity;

        self.fly_move();

        let down = self.s.origin;
        let downvel = self.s.velocity;

        self.s.origin = original;
        self.s.velocity = originalvel;

        let mut dest = self.s.origin;
        dest.z += self.vars.stepsize;
        let tr = self.player_trace(self.s.origin, dest);
        if !tr.startsolid && !tr.allsolid {
            self.s.origin = tr.endpos;
        }

        self.fly_move();

        let mut dest = self.s.origin;
        dest.z -= self.vars.stepsize;
        let tr = self.player_trace(self.s.origin, dest);

        let usedown;
        if tr.normal.z < 0.7 {
            usedown = true;
        } else {
            if !tr.startsolid && !tr.allsolid {
                self.s.origin = tr.endpos;
            }
            let up = self.s.origin;
            let downdist = (down.x - original.x).powi(2) + (down.y - original.y).powi(2);
            let updist = (up.x - original.x).powi(2) + (up.y - original.y).powi(2);
            usedown = downdist > updist;
        }

        if usedown {
            self.s.origin = down;
            self.s.velocity = downvel;
        } else {
            self.s.velocity.z = downvel.z;
        }
    }

    fn air_move(&mut self) {
        let (wishdir, wishspeed) = self.wish();
        self.air_accelerate(wishdir, wishspeed, self.vars.airaccelerate);
        self.s.velocity += self.s.basevelocity;
        self.fly_move();
    }

    fn clip_velocity(input: Vec3, normal: Vec3, overbounce: f32) -> Vec3 {
        let backoff = input.dot(normal) * overbounce;
        let mut out = input - normal * backoff;
        for i in 0..3 {
            if out[i] > -STOP_EPSILON && out[i] < STOP_EPSILON {
                out[i] = 0.0;
            }
        }
        out
    }

    fn fly_move(&mut self) -> u32 {
        let numbumps = 4;
        let mut blocked = 0;
        let mut numplanes = 0usize;
        let mut planes = [Vec3::ZERO; MAX_CLIP_PLANES];
        let mut original_velocity = self.s.velocity;
        let primal_velocity = self.s.velocity;
        let mut all_fraction = 0.0;
        let mut time_left = self.frametime;
        let mut new_velocity = Vec3::ZERO;

        for _ in 0..numbumps {
            if self.s.velocity == Vec3::ZERO {
                break;
            }
            let end = self.s.origin + self.s.velocity * time_left;
            let tr = self.player_trace(self.s.origin, end);

            all_fraction += tr.fraction;
            if tr.allsolid {
                self.s.velocity = Vec3::ZERO;
                return 4;
            }
            if tr.fraction > 0.0 {
                self.s.origin = tr.endpos;
                original_velocity = self.s.velocity;
                numplanes = 0;
            } else if !tr.startsolid {
                // Ramp bug guard: we are already closer to this plane than
                // DIST_EPSILON, so float noise makes every move along it look
                // like it crosses it and all bumps make no progress. Step back
                // out to the epsilon distance, like a fresh trace would leave us.
                let nudged = self.s.origin + tr.normal * DIST_EPSILON;
                if !self.world.box_stuck(nudged, self.s.mins(), self.s.maxs()) {
                    self.s.origin = nudged;
                    all_fraction += 1e-4;
                }
            }
            if tr.fraction == 1.0 {
                break;
            }

            self.note_touch(tr.normal);

            if tr.normal.z > 0.7 {
                blocked |= 1;
            }
            if tr.normal.z == 0.0 {
                blocked |= 2;
            }

            time_left -= time_left * tr.fraction;

            if numplanes >= MAX_CLIP_PLANES {
                self.s.velocity = Vec3::ZERO;
                break;
            }

            planes[numplanes] = tr.normal;
            numplanes += 1;

            if !self.s.onground {
                // Player friction is always 1, so this is a plain clip
                // (bounce * (1 - friction) == 0). This is what makes surfing work.
                for plane in planes.iter().take(numplanes) {
                    if plane.z > 0.7 {
                        new_velocity = Self::clip_velocity(original_velocity, *plane, 1.0);
                        original_velocity = new_velocity;
                    } else {
                        new_velocity = Self::clip_velocity(
                            original_velocity,
                            *plane,
                            1.0 + self.vars.bounce * (1.0 - PLAYER_FRICTION),
                        );
                    }
                }
                self.s.velocity = new_velocity;
                original_velocity = new_velocity;
            } else {
                let mut i = 0;
                while i < numplanes {
                    self.s.velocity = Self::clip_velocity(original_velocity, planes[i], 1.0);
                    let mut j = 0;
                    while j < numplanes {
                        if j != i && self.s.velocity.dot(planes[j]) < 0.0 {
                            break;
                        }
                        j += 1;
                    }
                    if j == numplanes {
                        break;
                    }
                    i += 1;
                }
                if i == numplanes {
                    if numplanes != 2 {
                        self.s.velocity = Vec3::ZERO;
                        break;
                    }
                    let dir = planes[0].cross(planes[1]);
                    let d = dir.dot(self.s.velocity);
                    self.s.velocity = dir * d;
                }
                if self.s.velocity.dot(primal_velocity) <= 0.0 {
                    self.s.velocity = Vec3::ZERO;
                    break;
                }
            }
        }

        if all_fraction == 0.0 {
            self.s.velocity = Vec3::ZERO;
        }
        blocked
    }

    fn check_falling(&mut self) {
        if self.s.onground && !self.s.dead && self.s.fall_velocity >= 200.0 {
            // The game uses this for landing sounds and (optional) fall damage.
            self.s.landed_speed = self.s.fall_velocity;
        }
        if self.s.onground && !self.s.dead && self.s.fall_velocity >= PLAYER_FALL_PUNCH_THRESHOLD {
            self.s.punchangle.z = self.s.fall_velocity * 0.013;
            if self.s.punchangle.x > 8.0 {
                self.s.punchangle.x = 8.0;
            }
        }
        if self.s.onground {
            self.s.fall_velocity = 0.0;
        }
    }
}
