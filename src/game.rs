//! Game simulation: players, weapons, damage, rounds and triggers.
//! Everything here runs at a fixed 100 Hz tick like a CS 1.6 server with
//! `fps_max 100` clients.

use macroquad::math::{vec3, Vec3};

use crate::bot::{self, BotBrain, Difficulty};
use crate::map::{Map, Team};
use crate::pmove::*;
use crate::util::{horizontal, Rng};
use crate::weapons::*;

pub const TICK_MSEC: u32 = 10;
pub const TICK: f32 = TICK_MSEC as f32 * 0.001;

pub const FREEZE_TIME: f64 = 3.0;
pub const ROUND_TIME: f64 = 150.0;
pub const ROUND_END_DELAY: f64 = 5.0;
pub const DM_RESPAWN_DELAY: f64 = 2.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Rounds,
    Deathmatch,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Freeze,
    Live,
    Over,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub map: String,
    pub mode: Mode,
    pub bots_t: usize,
    pub bots_ct: usize,
    pub difficulty: Difficulty,
    pub player_team: Option<Team>,
    pub player_name: String,
    pub friendly_fire: bool,
    pub fall_damage: bool,
    pub autobhop: bool,
    /// Riding a ramp counts as being on the ground for weapon accuracy.
    /// In stock CS surfing is "in the air" and rifles are hopeless there.
    pub ramp_accuracy: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            map: "surf_wars".into(),
            mode: Mode::Rounds,
            bots_t: 4,
            bots_ct: 5,
            difficulty: Difficulty::Normal,
            player_team: Some(Team::T),
            player_name: "Player".into(),
            friendly_fire: false,
            fall_damage: false,
            autobhop: false,
            ramp_accuracy: true,
        }
    }
}

/// Things that happened during a tick, for sounds and effects.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum Event {
    Shot { player: usize, weapon: WeaponId, pos: Vec3 },
    Tracer { start: Vec3, end: Vec3 },
    Impact { pos: Vec3, normal: Vec3 },
    Hit { victim: usize, attacker: usize, pos: Vec3, headshot: bool },
    Kill { victim: usize, attacker: Option<usize>, weapon: Option<WeaponId>, headshot: bool },
    Jump { player: usize },
    Land { player: usize, speed: f32 },
    Footstep { player: usize },
    Reload { player: usize },
    DryFire { player: usize },
    Zoom { player: usize },
    Deploy { player: usize },
    KnifeSwing { player: usize, hit: bool },
    Teleport { player: usize },
    Pickup { player: usize },
    RoundStart,
    RoundEnd { winner: Option<Team> },
}

#[derive(Clone, Debug)]
pub struct KillFeed {
    pub killer: Option<(String, Team)>,
    pub victim: (String, Team),
    pub weapon: &'static str,
    pub headshot: bool,
    pub time: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct DroppedWeapon {
    pub weapon: Weapon,
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub time: f64,
    pub resting: bool,
}

pub struct Player {
    pub name: String,
    pub team: Team,
    pub alive: bool,
    pub health: f32,
    pub armor: f32,
    pub helmet: bool,
    pub pm: PmState,
    pub prev_origin: Vec3,
    pub angles: Vec3,
    pub primary: Option<Weapon>,
    pub secondary: Option<Weapon>,
    pub active: Slot,
    pub last_active: Slot,
    pub next_attack: f64,
    pub reload_end: Option<f64>,
    pub zoom: u8,
    pub resume_zoom: Option<u8>,
    pub prev_buttons: u32,
    pub kills: i32,
    pub deaths: u32,
    pub death_time: f64,
    pub respawn_at: Option<f64>,
    pub bot: Option<Box<BotBrain>>,
    pub footstep_dist: f32,
    pub last_fire: f64,
    pub deploy_time: f64,
    pub last_hurt: f64,
    pub hurt_from: Vec3,
    pub last_attacker: Option<usize>,
    pub spawn_count: u32,
    /// Visual only: fades the hover board in and out.
    pub board: f32,
    pub board_normal: Vec3,
}

impl Player {
    fn new(name: String, team: Team, bot: Option<Box<BotBrain>>) -> Player {
        Player {
            name,
            team,
            alive: false,
            health: 0.0,
            armor: 0.0,
            helmet: false,
            pm: PmState::new(Vec3::ZERO),
            prev_origin: Vec3::ZERO,
            angles: Vec3::ZERO,
            primary: None,
            secondary: None,
            active: Slot::Secondary,
            last_active: Slot::Melee,
            next_attack: 0.0,
            reload_end: None,
            zoom: 0,
            resume_zoom: None,
            prev_buttons: 0,
            kills: 0,
            deaths: 0,
            death_time: -100.0,
            respawn_at: None,
            bot,
            footstep_dist: 0.0,
            last_fire: -100.0,
            deploy_time: -100.0,
            last_hurt: -100.0,
            hurt_from: Vec3::ZERO,
            last_attacker: None,
            spawn_count: 0,
            board: 0.0,
            board_normal: Vec3::Z,
        }
    }

    pub fn is_bot(&self) -> bool {
        self.bot.is_some()
    }

    pub fn weapon(&self) -> Option<&Weapon> {
        match self.active {
            Slot::Primary => self.primary.as_ref(),
            Slot::Secondary => self.secondary.as_ref(),
            Slot::Melee => None,
        }
    }

    pub fn weapon_mut(&mut self) -> Option<&mut Weapon> {
        match self.active {
            Slot::Primary => self.primary.as_mut(),
            Slot::Secondary => self.secondary.as_mut(),
            Slot::Melee => None,
        }
    }

    pub fn active_id(&self) -> WeaponId {
        self.weapon().map(|w| w.id).unwrap_or(WeaponId::Knife)
    }

    pub fn has_slot(&self, slot: Slot) -> bool {
        match slot {
            Slot::Primary => self.primary.is_some(),
            Slot::Secondary => self.secondary.is_some(),
            Slot::Melee => true,
        }
    }

    pub fn fov(&self) -> f32 {
        let def = self.active_id().def();
        if self.zoom > 0 && (self.zoom as usize) <= def.zoom_fov.len() {
            def.zoom_fov[self.zoom as usize - 1]
        } else {
            90.0
        }
    }

    pub fn maxspeed(&self) -> f32 {
        let def = self.active_id().def();
        if self.zoom > 0 {
            def.zoom_maxspeed
        } else {
            def.maxspeed
        }
    }

    pub fn reloading(&self) -> bool {
        self.reload_end.is_some()
    }
}

pub struct Game {
    pub map: Map,
    pub vars: MoveVars,
    pub settings: Settings,
    pub players: Vec<Player>,
    pub local: Option<usize>,
    pub time: f64,
    pub tick: u64,
    pub rng: Rng,
    pub phase: Phase,
    pub phase_end: f64,
    pub round: u32,
    pub score: [u32; 2],
    pub last_winner: Option<Team>,
    pub events: Vec<Event>,
    pub killfeed: Vec<KillFeed>,
    pub dropped: Vec<DroppedWeapon>,
    pub center_msg: Option<(String, f64)>,
}

const BOT_NAMES: [&str; 20] = [
    "Wavey",
    "RampRat",
    "Glider",
    "Slopey",
    "Airstrafe",
    "Bunny",
    "Skimmer",
    "Drifter",
    "Zephyr",
    "Carver",
    "Swoop",
    "Tsunami",
    "Comet",
    "Rider",
    "Breaker",
    "Kite",
    "Nimbus",
    "Vortex",
    "Ripple",
    "Jetstream",
];

impl Game {
    pub fn new(settings: Settings, seed: u64) -> Game {
        let map = crate::map::load(&settings.map);
        let mut vars = MoveVars::surf_server();
        vars.autobhop = settings.autobhop;
        let mut g = Game {
            map,
            vars,
            settings: settings.clone(),
            players: Vec::new(),
            local: None,
            time: 0.0,
            tick: 0,
            rng: Rng::new(seed),
            phase: Phase::Freeze,
            phase_end: 0.0,
            round: 0,
            score: [0, 0],
            last_winner: None,
            events: Vec::new(),
            killfeed: Vec::new(),
            dropped: Vec::new(),
            center_msg: None,
        };
        if let Some(team) = settings.player_team {
            g.players.push(Player::new(settings.player_name.clone(), team, None));
            g.local = Some(0);
        }
        let mut names: Vec<&str> = BOT_NAMES.to_vec();
        let mut take_name = |rng: &mut Rng| {
            let i = rng.range_u32(0, names.len() as u32 - 1) as usize;
            names.swap_remove(i).to_string()
        };
        for (team, count) in [(Team::T, settings.bots_t), (Team::CT, settings.bots_ct)] {
            for _ in 0..count {
                let name = format!("BOT {}", take_name(&mut g.rng));
                let seed = g.rng.next_u64();
                let brain = BotBrain::new(settings.difficulty, seed);
                g.players.push(Player::new(name, team, Some(Box::new(brain))));
            }
        }
        g.start_round();
        g
    }

    pub fn local_player(&self) -> Option<&Player> {
        self.local.map(|i| &self.players[i])
    }

    pub fn alive_count(&self, team: Team) -> usize {
        self.players.iter().filter(|p| p.alive && p.team == team).count()
    }

    pub fn in_buyzone(&self, idx: usize) -> bool {
        let p = &self.players[idx];
        p.alive && self.map.buyzones[p.team.index()].touches(p.pm.origin, p.pm.mins(), p.pm.maxs())
    }

    pub fn round_time_left(&self) -> f64 {
        match self.phase {
            Phase::Freeze => ROUND_TIME,
            Phase::Live => (self.phase_end - self.time).max(0.0),
            Phase::Over => 0.0,
        }
    }

    fn center_print(&mut self, msg: impl Into<String>, secs: f64) {
        self.center_msg = Some((msg.into(), self.time + secs));
    }

    // ------------------------------------------------------------------
    // Rounds and spawning

    fn start_round(&mut self) {
        self.round += 1;
        self.dropped.clear();
        let mut taken: Vec<Vec3> = Vec::new();
        for i in 0..self.players.len() {
            let keep = self.players[i].alive && self.round > 1;
            self.spawn_player(i, keep, &mut taken);
        }
        if self.settings.mode == Mode::Rounds {
            self.phase = Phase::Freeze;
            self.phase_end = self.time + FREEZE_TIME;
        } else {
            self.phase = Phase::Live;
            self.phase_end = f64::MAX;
        }
        self.events.push(Event::RoundStart);
    }

    fn spawn_player(&mut self, i: usize, keep_weapons: bool, taken: &mut Vec<Vec3>) {
        let team = self.players[i].team;
        let seed = self.rng.range_u32(0, 1000) as usize;
        let sp = self.map.random_spawn(team, taken, seed);
        taken.push(sp.pos);
        let p = &mut self.players[i];
        p.pm = PmState::new(sp.pos);
        p.prev_origin = sp.pos;
        p.angles = vec3(0.0, sp.yaw, 0.0);
        p.alive = true;
        p.health = 100.0;
        p.armor = 100.0;
        p.helmet = true;
        p.zoom = 0;
        p.resume_zoom = None;
        p.reload_end = None;
        p.respawn_at = None;
        p.board = 0.0;
        p.last_attacker = None;
        p.spawn_count += 1;
        if !keep_weapons {
            p.primary = None;
            p.secondary = Some(Weapon::new(WeaponId::Usp));
        } else {
            // refill ammo like a surf server would
            if let Some(w) = p.primary.as_mut() {
                w.reserve = w.def().reserve;
            }
            if let Some(w) = p.secondary.as_mut() {
                w.reserve = w.def().reserve;
            }
            if p.secondary.is_none() {
                p.secondary = Some(Weapon::new(WeaponId::Usp));
            }
        }
        p.active = if p.primary.is_some() { Slot::Primary } else { Slot::Secondary };
        p.last_active = Slot::Melee;
        p.next_attack = self.time + 0.5;
        p.deploy_time = self.time;
        if let Some(b) = p.bot.as_mut() {
            b.on_spawn();
        }
    }

    fn check_round_end(&mut self) {
        if self.settings.mode != Mode::Rounds || self.phase != Phase::Live {
            return;
        }
        let t = self.alive_count(Team::T);
        let ct = self.alive_count(Team::CT);
        let has_t = self.players.iter().any(|p| p.team == Team::T);
        let has_ct = self.players.iter().any(|p| p.team == Team::CT);
        let winner = if self.time >= self.phase_end || (has_t && has_ct && t == 0 && ct == 0) {
            Some(None)
        } else if has_t && has_ct && t == 0 {
            Some(Some(Team::CT))
        } else if has_t && has_ct && ct == 0 {
            Some(Some(Team::T))
        } else {
            None
        };
        if let Some(w) = winner {
            self.phase = Phase::Over;
            self.phase_end = self.time + ROUND_END_DELAY;
            self.last_winner = w;
            match w {
                Some(team) => {
                    self.score[team.index()] += 1;
                    self.center_print(format!("{} Win!", team.name()), ROUND_END_DELAY);
                }
                None => self.center_print("Round Draw!", ROUND_END_DELAY),
            }
            self.events.push(Event::RoundEnd { winner: w });
        }
    }

    // ------------------------------------------------------------------
    // Player actions

    pub fn buy(&mut self, idx: usize, id: WeaponId) -> bool {
        if !self.in_buyzone(idx) {
            return false;
        }
        let def = id.def();
        let now = self.time;
        let p = &mut self.players[idx];
        match def.slot {
            Slot::Primary => {
                if let Some(old) = p.primary.take() {
                    if old.id == id {
                        p.primary = Some(Weapon::new(id));
                        return true;
                    }
                }
                p.primary = Some(Weapon::new(id));
                self.switch_weapon(idx, Slot::Primary);
            }
            Slot::Secondary => {
                p.secondary = Some(Weapon::new(id));
                self.switch_weapon(idx, Slot::Secondary);
            }
            Slot::Melee => {}
        }
        let _ = now;
        self.events.push(Event::Pickup { player: idx });
        true
    }

    pub fn switch_weapon(&mut self, idx: usize, slot: Slot) {
        let now = self.time;
        let p = &mut self.players[idx];
        if !p.alive || !p.has_slot(slot) || p.active == slot {
            return;
        }
        p.last_active = p.active;
        p.active = slot;
        p.zoom = 0;
        p.resume_zoom = None;
        p.reload_end = None;
        let deploy = p.active_id().def().deploy_time as f64;
        p.next_attack = now + deploy;
        p.deploy_time = now;
        self.events.push(Event::Deploy { player: idx });
    }

    pub fn last_weapon(&mut self, idx: usize) {
        let last = self.players[idx].last_active;
        if self.players[idx].has_slot(last) {
            self.switch_weapon(idx, last);
        }
    }

    pub fn drop_weapon(&mut self, idx: usize) {
        let now = self.time;
        let p = &mut self.players[idx];
        if !p.alive {
            return;
        }
        let w = match p.active {
            Slot::Primary => p.primary.take(),
            Slot::Secondary => p.secondary.take(),
            Slot::Melee => None,
        };
        if let Some(w) = w {
            let (f, _, _) = angle_vectors(p.angles);
            self.dropped.push(DroppedWeapon {
                weapon: w,
                pos: p.pm.eye() - vec3(0.0, 0.0, 10.0),
                vel: p.pm.velocity + f * 300.0 + vec3(0.0, 0.0, 80.0),
                yaw: p.angles.y,
                time: now,
                resting: false,
            });
            let next = if p.primary.is_some() {
                Slot::Primary
            } else if p.secondary.is_some() {
                Slot::Secondary
            } else {
                Slot::Melee
            };
            p.active = Slot::Melee;
            self.switch_weapon(idx, next);
            if next == Slot::Melee {
                let p = &mut self.players[idx];
                p.active = Slot::Melee;
                p.next_attack = now + 0.5;
            }
        }
    }

    // ------------------------------------------------------------------
    // Simulation

    /// Advance the simulation by one tick. `local_cmd` is the human input.
    pub fn step(&mut self, local_cmd: Option<UserCmd>) {
        self.time += TICK as f64;
        self.tick += 1;

        // Phase transitions.
        match self.phase {
            Phase::Freeze if self.time >= self.phase_end => {
                self.phase = Phase::Live;
                self.phase_end = self.time + ROUND_TIME;
            }
            Phase::Over if self.time >= self.phase_end => {
                self.start_round();
            }
            _ => {}
        }

        // Bot thinking.
        for i in 0..self.players.len() {
            if self.players[i].bot.is_some() {
                let mut brain = self.players[i].bot.take().unwrap();
                let cmd = bot::think(self, i, &mut brain);
                let actions = std::mem::take(&mut brain.actions);
                self.players[i].bot = Some(brain);
                for a in actions {
                    match a {
                        bot::BotAction::Buy(id) => {
                            self.buy(i, id);
                        }
                        bot::BotAction::Switch(slot) => self.switch_weapon(i, slot),
                    }
                }
                self.run_player(i, cmd);
            } else if Some(i) == self.local {
                if let Some(cmd) = local_cmd {
                    self.run_player(i, cmd);
                } else {
                    self.run_player(
                        i,
                        UserCmd { msec: TICK_MSEC, viewangles: self.players[i].angles, ..Default::default() },
                    );
                }
            }
        }

        self.update_dropped();

        // Deathmatch respawns.
        if self.settings.mode == Mode::Deathmatch {
            for i in 0..self.players.len() {
                if let Some(t) = self.players[i].respawn_at {
                    if self.time >= t {
                        let mut taken: Vec<Vec3> =
                            self.players.iter().filter(|p| p.alive).map(|p| p.pm.origin).collect();
                        self.spawn_player(i, false, &mut taken);
                    }
                }
            }
        }

        self.check_round_end();
        self.killfeed.retain(|k| self.time - k.time < 6.0);
        if let Some((_, t)) = &self.center_msg {
            if self.time > *t {
                self.center_msg = None;
            }
        }
    }

    fn run_player(&mut self, i: usize, mut cmd: UserCmd) {
        cmd.msec = TICK_MSEC;
        {
            let p = &mut self.players[i];
            p.prev_origin = p.pm.origin;
            if !p.alive {
                p.prev_buttons = cmd.buttons;
                return;
            }
            p.angles = cmd.viewangles;
            p.angles.x = p.angles.x.clamp(-89.0, 89.0);
            cmd.viewangles = p.angles;
            if self.phase == Phase::Freeze {
                cmd.forwardmove = 0.0;
                cmd.sidemove = 0.0;
                cmd.buttons &= !(IN_JUMP | IN_ATTACK | IN_ATTACK2);
            }
            p.pm.maxspeed = p.maxspeed();
        }

        // Movement.
        let before_onground;
        {
            let p = &mut self.players[i];
            before_onground = p.pm.onground;
            let mut pm = PlayerMove::new(&self.map.world, &self.vars, &mut p.pm, cmd);
            pm.run();
        }

        // Movement side effects: sounds, fall damage, board visual.
        let (jumped, landed, speed2d, onground) = {
            let p = &self.players[i];
            (p.pm.jumped, p.pm.landed_speed, horizontal(p.pm.velocity).length(), p.pm.onground)
        };
        if jumped {
            self.events.push(Event::Jump { player: i });
        }
        if landed > 0.0 && !before_onground {
            self.events.push(Event::Land { player: i, speed: landed });
            if self.settings.fall_damage && landed > PLAYER_MAX_SAFE_FALL_SPEED {
                let dmg = (landed - PLAYER_MAX_SAFE_FALL_SPEED) * DAMAGE_FOR_FALL_SPEED * 1.25;
                self.damage(i, None, dmg, HitGroup::Chest, 1.0, None, self.players[i].pm.origin);
                if !self.players[i].alive {
                    return;
                }
            }
        }
        {
            let p = &mut self.players[i];
            if onground && speed2d >= 150.0 {
                p.footstep_dist += speed2d * TICK;
                if p.footstep_dist > 110.0 {
                    p.footstep_dist = 0.0;
                    self.events.push(Event::Footstep { player: i });
                }
            }
            let target = if p.pm.is_surfing() { 1.0 } else { 0.0 };
            let rate = if target > p.board { 12.0 } else { 3.5 };
            p.board = crate::util::approach(p.board, target, rate * TICK);
            if p.pm.is_surfing() {
                p.board_normal = (p.board_normal * 0.8 + p.pm.surf_normal * 0.2).normalize();
            }
        }

        // Triggers.
        let teleport = {
            let p = &self.players[i];
            self.map.teleports.iter().any(|z| z.touches(p.pm.origin, p.pm.mins(), p.pm.maxs()))
        };
        if teleport {
            let team = self.players[i].team;
            let taken: Vec<Vec3> = self.players.iter().filter(|p| p.alive).map(|p| p.pm.origin).collect();
            let seed = self.rng.range_u32(0, 1000) as usize;
            let sp = self.map.random_spawn(team, &taken, seed);
            let p = &mut self.players[i];
            let ducking = p.pm.ducking;
            p.pm = PmState::new(sp.pos);
            let _ = ducking;
            p.prev_origin = sp.pos;
            p.angles.y = sp.yaw;
            p.board = 0.0;
            self.events.push(Event::Teleport { player: i });
            if let Some(b) = p.bot.as_mut() {
                b.on_teleport();
            }
        }

        self.pickup_weapons(i);
        self.weapon_frame(i, cmd);
        self.players[i].prev_buttons = cmd.buttons;
    }

    fn weapon_frame(&mut self, i: usize, cmd: UserCmd) {
        let now = self.time;
        let pressed = cmd.buttons & !self.players[i].prev_buttons;

        // Finish reloads.
        {
            let p = &mut self.players[i];
            if let Some(end) = p.reload_end {
                if now >= end {
                    let active = p.active;
                    let w = match active {
                        Slot::Primary => p.primary.as_mut(),
                        Slot::Secondary => p.secondary.as_mut(),
                        Slot::Melee => None,
                    };
                    let mut again = false;
                    if let Some(w) = w {
                        let def = w.def();
                        if w.id == WeaponId::M3 {
                            // one shell at a time
                            if w.clip < def.clip && w.reserve > 0 {
                                w.clip += 1;
                                w.reserve -= 1;
                            }
                            again = w.clip < def.clip && w.reserve > 0;
                        } else {
                            let need = def.clip - w.clip;
                            let take = need.min(w.reserve);
                            w.clip += take;
                            w.reserve -= take;
                            w.shots_fired = 0;
                            w.accuracy = match w.id {
                                WeaponId::Usp => 0.92,
                                WeaponId::Ak47 => 0.2,
                                _ => 0.0,
                            };
                        }
                    }
                    if again {
                        p.reload_end = Some(now + WeaponId::M3.def().reload_time as f64);
                    } else {
                        p.reload_end = None;
                    }
                }
            }
            if let Some(z) = p.resume_zoom {
                if now >= p.next_attack && p.reload_end.is_none() {
                    p.zoom = z;
                    p.resume_zoom = None;
                }
            }
        }

        // Weapon switching commands are handled by the caller for the human
        // player; bots send them through the brain before this point.

        if self.phase == Phase::Freeze {
            return;
        }

        let attack = cmd.buttons & IN_ATTACK != 0;
        let attack2_pressed = pressed & IN_ATTACK2 != 0;

        // Secondary attack: scope / knife stab.
        if attack2_pressed && now >= self.players[i].next_attack {
            let id = self.players[i].active_id();
            let def = id.def();
            if !def.zoom_fov.is_empty() && !self.players[i].reloading() {
                let p = &mut self.players[i];
                p.zoom = (p.zoom + 1) % (def.zoom_fov.len() as u8 + 1);
                p.resume_zoom = None;
                self.events.push(Event::Zoom { player: i });
            } else if id == WeaponId::Knife {
                self.knife_attack(i, true);
            }
        }

        let semi_block = {
            let p = &self.players[i];
            let def = p.active_id().def();
            !def.automatic && p.prev_buttons & IN_ATTACK != 0
        };

        if attack && now >= self.players[i].next_attack && !semi_block {
            let id = self.players[i].active_id();
            if id == WeaponId::Knife {
                self.knife_attack(i, false);
            } else {
                let (clip, reloading, is_m3) = {
                    let p = &self.players[i];
                    let w = p.weapon().unwrap();
                    (w.clip, p.reloading(), w.id == WeaponId::M3)
                };
                if reloading && is_m3 && clip > 0 {
                    self.players[i].reload_end = None; // interrupt shotgun reload
                }
                if clip == 0 {
                    if pressed & IN_ATTACK != 0 {
                        self.events.push(Event::DryFire { player: i });
                    }
                    self.players[i].next_attack = now + 0.2;
                } else if !self.players[i].reloading() {
                    self.fire(i);
                }
            }
        } else if !attack {
            if let Some(w) = self.players[i].weapon_mut() {
                idle_decay(w, now);
            }
        }

        // Reloading.
        let want_reload = cmd.buttons & IN_RELOAD != 0;
        let (can_reload, empty) = {
            let p = &self.players[i];
            match p.weapon() {
                Some(w) => (w.clip < w.def().clip && w.reserve > 0 && !p.reloading(), w.clip == 0),
                None => (false, false),
            }
        };
        if can_reload && now >= self.players[i].next_attack && (want_reload || (empty && !attack)) {
            let p = &mut self.players[i];
            let id = p.active_id();
            let t = if id == WeaponId::M3 { 0.55 } else { id.def().reload_time as f64 };
            p.reload_end = Some(now + t);
            if p.zoom > 0 {
                p.zoom = 0;
            }
            p.resume_zoom = None;
            self.events.push(Event::Reload { player: i });
        }
    }

    fn shooter_state(&self, i: usize) -> ShooterState {
        let p = &self.players[i];
        ShooterState {
            on_ground: p.pm.onground || (self.settings.ramp_accuracy && p.pm.is_surfing()),
            ducking: p.pm.ducking,
            speed2d: horizontal(p.pm.velocity).length(),
            zoomed: p.zoom > 0,
        }
    }

    fn fire(&mut self, i: usize) {
        let now = self.time;
        let st = self.shooter_state(i);
        let (spread, id, angles, src) = {
            let p = &mut self.players[i];
            let angles = p.angles + p.pm.punchangle;
            let src = p.pm.eye();
            let w = p.weapon_mut().unwrap();
            let spread = compute_spread(w, st, now);
            w.clip -= 1;
            w.last_fire = now;
            (spread, w.id, angles, src)
        };
        let def = id.def();
        let (fwd, right, up) = angle_vectors(angles);
        for _ in 0..def.pellets.max(1) {
            let x = self.rng.range(-0.5, 0.5) + self.rng.range(-0.5, 0.5);
            let y = self.rng.range(-0.5, 0.5) + self.rng.range(-0.5, 0.5);
            let dir = (fwd + right * (x * spread) + up * (y * spread)).normalize();
            self.fire_bullet(i, src, dir, id);
        }
        {
            let p = &mut self.players[i];
            let mut punch = p.pm.punchangle;
            let w = p.weapon_mut().unwrap();
            apply_recoil(w, st, &mut punch, &mut self.rng);
            p.pm.punchangle = punch;
            p.next_attack = now + def.cycle as f64;
            p.last_fire = now;
            if !def.zoom_fov.is_empty() && p.zoom > 0 {
                p.resume_zoom = Some(p.zoom);
                p.zoom = 0;
            }
        }
        self.events.push(Event::Shot { player: i, weapon: id, pos: src });
    }

    /// Ray against player hitboxes. Returns (distance, hit group).
    pub fn ray_player(&self, target: usize, start: Vec3, dir: Vec3, max: f32) -> Option<(f32, HitGroup)> {
        let p = &self.players[target];
        let o = p.pm.origin;
        let duck = p.pm.ducking;
        // coarse reject
        let to = o - start;
        let t_closest = to.dot(dir);
        if t_closest < -50.0 || t_closest > max + 50.0 {
            return None;
        }
        if (start + dir * t_closest).distance(o) > 60.0 {
            return None;
        }
        let mut best: Option<(f32, HitGroup)> = None;
        let mut consider = |t: Option<f32>, g: HitGroup| {
            if let Some(t) = t {
                if t >= 0.0 && t <= max && best.is_none_or(|b| t < b.0) {
                    best = Some((t, g));
                }
            }
        };
        for (center, radius, g) in hitboxes_spheres(o, duck) {
            consider(ray_sphere(start, dir, center, radius), g);
        }
        for (mins, maxs, g) in hitboxes_boxes(o, duck) {
            consider(ray_box(start, dir, mins, maxs), g);
        }
        best
    }

    fn fire_bullet(&mut self, i: usize, src: Vec3, dir: Vec3, id: WeaponId) {
        let def = id.def();
        let range = if id == WeaponId::M3 { 3000.0 } else { 8192.0 };
        let wtr = self.map.world.trace_ray(src, src + dir * range);
        let wall_dist = wtr.fraction * range;
        let mut hit: Option<(usize, f32, HitGroup)> = None;
        for j in 0..self.players.len() {
            if j == i || !self.players[j].alive {
                continue;
            }
            if let Some((t, g)) = self.ray_player(j, src, dir, wall_dist) {
                if hit.is_none_or(|h| t < h.1) {
                    hit = Some((j, t, g));
                }
            }
        }
        let muzzle = src + dir * 20.0 + vec3(0.0, 0.0, -6.0);
        match hit {
            Some((j, t, g)) => {
                let dmg = if id == WeaponId::M3 {
                    (1.0 - t / range) * def.damage
                } else {
                    def.damage * def.range_modifier.powf(t / 500.0)
                };
                let pos = src + dir * t;
                self.events.push(Event::Tracer { start: muzzle, end: pos });
                let same_team = self.players[j].team == self.players[i].team;
                if !same_team || self.settings.friendly_fire {
                    self.damage(j, Some(i), dmg, g, def.armor_ratio, Some(id), pos);
                }
            }
            None => {
                let end = src + dir * wall_dist;
                self.events.push(Event::Tracer { start: muzzle, end });
                if wtr.hit() {
                    self.events.push(Event::Impact { pos: end, normal: wtr.normal });
                }
            }
        }
    }

    fn knife_attack(&mut self, i: usize, stab: bool) {
        let now = self.time;
        let (src, fwd, yaw) = {
            let p = &self.players[i];
            let (f, _, _) = angle_vectors(p.angles);
            (p.pm.eye(), f, p.angles.y)
        };
        let range = if stab { 32.0 } else { 48.0 };
        let wtr = self.map.world.trace_ray(src, src + fwd * range);
        let max = wtr.fraction * range;
        let mut hit: Option<(usize, f32, HitGroup)> = None;
        // A line first, then a slightly fatter check like UTIL_TraceHull.
        for j in 0..self.players.len() {
            if j == i || !self.players[j].alive {
                continue;
            }
            let mut r = self.ray_player(j, src, fwd, max);
            if r.is_none() {
                let to = self.players[j].pm.origin - src;
                let d = to.length();
                if d < range + 20.0 && horizontal(to).normalize_or_zero().dot(horizontal(fwd).normalize_or_zero()) > 0.8
                {
                    r = Some((d, HitGroup::Chest));
                }
            }
            if let Some((t, g)) = r {
                if hit.is_none_or(|h| t < h.1) {
                    hit = Some((j, t, g));
                }
            }
        }
        let mut did_hit = false;
        if let Some((j, t, g)) = hit {
            let victim_yaw = self.players[j].angles.y;
            let (vf, _, _) = angle_vectors(vec3(0.0, victim_yaw, 0.0));
            let (af, _, _) = angle_vectors(vec3(0.0, yaw, 0.0));
            let backstab = vf.dot(af) > 0.8;
            let mut dmg = if stab { 65.0 } else { 15.0 };
            if backstab {
                dmg *= 3.0;
            }
            let pos = src + fwd * t;
            if self.players[j].team != self.players[i].team || self.settings.friendly_fire {
                self.damage(j, Some(i), dmg, g, 1.0, Some(WeaponId::Knife), pos);
            }
            did_hit = true;
        } else if wtr.hit() {
            did_hit = true;
            self.events.push(Event::Impact { pos: wtr.endpos, normal: wtr.normal });
        }
        let p = &mut self.players[i];
        p.next_attack = now
            + if stab {
                1.1
            } else if did_hit {
                0.4
            } else {
                0.35
            };
        p.last_fire = now;
        self.events.push(Event::KnifeSwing { player: i, hit: did_hit });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn damage(
        &mut self,
        victim: usize,
        attacker: Option<usize>,
        amount: f32,
        group: HitGroup,
        armor_ratio: f32,
        weapon: Option<WeaponId>,
        pos: Vec3,
    ) {
        if !self.players[victim].alive {
            return;
        }
        let now = self.time;
        let dmg = amount * group.multiplier();
        let (hp_dmg, armor_dmg) = {
            let p = &self.players[victim];
            if weapon.is_some() {
                armor_absorb(dmg, p.armor, p.helmet, group, armor_ratio)
            } else {
                (dmg, 0.0)
            }
        };
        let headshot = group == HitGroup::Head;
        let attacker_pos = attacker.map(|a| self.players[a].pm.origin);
        {
            let p = &mut self.players[victim];
            p.health -= hp_dmg;
            p.armor = (p.armor - armor_dmg).max(0.0);
            p.last_hurt = now;
            p.last_attacker = attacker;
            if let Some(a) = attacker_pos {
                p.hurt_from = a;
            }
            // CS 1.6 tagging slows you down, but only on the ground so
            // surfers keep their speed like on surf servers.
            if p.pm.onground {
                p.pm.velocity *= 0.5;
            }
            // flinch
            p.pm.punchangle.x -= (hp_dmg * 0.1).min(4.0);
        }
        if let Some(a) = attacker {
            self.events.push(Event::Hit { victim, attacker: a, pos, headshot });
            if let Some(b) = self.players[victim].bot.as_mut() {
                b.on_hurt(a);
            }
        }
        if self.players[victim].health <= 0.0 {
            self.kill(victim, attacker, weapon, headshot);
        }
    }

    fn kill(&mut self, victim: usize, attacker: Option<usize>, weapon: Option<WeaponId>, headshot: bool) {
        let now = self.time;
        {
            let p = &mut self.players[victim];
            p.alive = false;
            p.health = 0.0;
            p.deaths += 1;
            p.death_time = now;
            p.zoom = 0;
            p.pm.dead = true;
            if self.settings.mode == Mode::Deathmatch {
                p.respawn_at = Some(now + DM_RESPAWN_DELAY);
            }
        }
        // Drop the best weapon.
        let drop = {
            let p = &mut self.players[victim];
            let w = p.primary.take().or_else(|| p.secondary.take());
            w.map(|w| (w, p.pm.origin, p.pm.velocity, p.angles.y))
        };
        if let Some((w, pos, vel, yaw)) = drop {
            self.dropped.push(DroppedWeapon {
                weapon: w,
                pos,
                vel: vel * 0.6 + vec3(0.0, 0.0, 120.0),
                yaw,
                time: now,
                resting: false,
            });
        }
        {
            let p = &mut self.players[victim];
            p.primary = None;
            p.secondary = None;
        }
        let victim_team = self.players[victim].team;
        if let Some(a) = attacker {
            if a != victim {
                if self.players[a].team == victim_team {
                    self.players[a].kills -= 1;
                } else {
                    self.players[a].kills += 1;
                }
            }
        }
        let killer = attacker.map(|a| (self.players[a].name.clone(), self.players[a].team));
        self.killfeed.push(KillFeed {
            killer,
            victim: (self.players[victim].name.clone(), victim_team),
            weapon: weapon.map(|w| w.def().kill_icon).unwrap_or("worldspawn"),
            headshot,
            time: now,
        });
        if self.killfeed.len() > 5 {
            self.killfeed.remove(0);
        }
        self.events.push(Event::Kill { victim, attacker, weapon, headshot });
    }

    fn update_dropped(&mut self) {
        let now = self.time;
        let world = &self.map.world;
        let mins = vec3(-6.0, -6.0, -4.0);
        let maxs = vec3(6.0, 6.0, 4.0);
        for d in self.dropped.iter_mut() {
            if d.resting {
                continue;
            }
            d.vel.z -= 800.0 * TICK;
            let end = d.pos + d.vel * TICK;
            let tr = world.trace(d.pos, end, mins, maxs);
            d.pos = tr.endpos;
            if tr.hit() {
                if tr.normal.z > 0.7 {
                    d.vel = Vec3::ZERO;
                    d.resting = true;
                } else {
                    d.vel -= tr.normal * d.vel.dot(tr.normal);
                }
            }
        }
        let teleports = &self.map.teleports;
        self.dropped.retain(|d| now - d.time < 60.0 && !teleports.iter().any(|z| z.touches(d.pos, mins, maxs)));
    }

    fn pickup_weapons(&mut self, i: usize) {
        if !self.players[i].alive || self.dropped.is_empty() {
            return;
        }
        let now = self.time;
        let origin = self.players[i].pm.origin;
        let mut k = 0;
        while k < self.dropped.len() {
            let d = self.dropped[k];
            let close = (d.pos - origin).abs();
            if now - d.time > 0.5 && close.x < 32.0 && close.y < 32.0 && close.z < 50.0 {
                let slot = d.weapon.def().slot;
                let p = &mut self.players[i];
                let free = match slot {
                    Slot::Primary => p.primary.is_none(),
                    Slot::Secondary => p.secondary.is_none(),
                    Slot::Melee => false,
                };
                if free {
                    match slot {
                        Slot::Primary => p.primary = Some(d.weapon),
                        _ => p.secondary = Some(d.weapon),
                    }
                    self.dropped.swap_remove(k);
                    self.events.push(Event::Pickup { player: i });
                    let better = slot == Slot::Primary || self.players[i].active == Slot::Melee;
                    if better {
                        self.switch_weapon(i, slot);
                    }
                    continue;
                }
            }
            k += 1;
        }
    }

    /// Line of sight between two points through the world.
    pub fn visible(&self, a: Vec3, b: Vec3) -> bool {
        !self.map.world.trace_ray(a, b).hit()
    }
}

/// Head (and for crouching players, the same) spheres: (center, radius).
pub fn hitboxes_spheres(o: Vec3, duck: bool) -> [(Vec3, f32, HitGroup); 1] {
    if duck {
        [(o + vec3(0.0, 0.0, 10.0), 7.0, HitGroup::Head)]
    } else {
        [(o + vec3(0.0, 0.0, 27.0), 7.0, HitGroup::Head)]
    }
}

pub fn hitboxes_boxes(o: Vec3, duck: bool) -> [(Vec3, Vec3, HitGroup); 3] {
    if duck {
        [
            (o + vec3(-10.0, -10.0, -4.0), o + vec3(10.0, 10.0, 4.0), HitGroup::Chest),
            (o + vec3(-9.0, -9.0, -10.0), o + vec3(9.0, 9.0, -4.0), HitGroup::Stomach),
            (o + vec3(-10.0, -10.0, -18.0), o + vec3(10.0, 10.0, -10.0), HitGroup::Legs),
        ]
    } else {
        [
            (o + vec3(-10.0, -10.0, 6.0), o + vec3(10.0, 10.0, 20.0), HitGroup::Chest),
            (o + vec3(-9.0, -9.0, -6.0), o + vec3(9.0, 9.0, 6.0), HitGroup::Stomach),
            (o + vec3(-9.0, -9.0, -36.0), o + vec3(9.0, 9.0, -6.0), HitGroup::Legs),
        ]
    }
}

fn ray_sphere(start: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let oc = start - center;
    let b = oc.dot(dir);
    let c = oc.dot(oc) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    if t >= 0.0 {
        Some(t)
    } else {
        None
    }
}

fn ray_box(start: Vec3, dir: Vec3, mins: Vec3, maxs: Vec3) -> Option<f32> {
    let mut tmin = 0.0f32;
    let mut tmax = f32::MAX;
    for a in 0..3 {
        if dir[a].abs() < 1e-8 {
            if start[a] < mins[a] || start[a] > maxs[a] {
                return None;
            }
        } else {
            let inv = 1.0 / dir[a];
            let mut t0 = (mins[a] - start[a]) * inv;
            let mut t1 = (maxs[a] - start[a]) * inv;
            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
            }
            tmin = tmin.max(t0);
            tmax = tmax.min(t1);
            if tmin > tmax {
                return None;
            }
        }
    }
    Some(tmin)
}
