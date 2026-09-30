//! Game simulation: players, weapons, damage, rounds and triggers.
//! Everything here runs at a fixed 100 Hz tick like a CS 1.6 server with
//! `fps_max 100` clients.

use macroquad::math::{vec3, Vec3};

use crate::bot::{self, BotBrain, Difficulty};
use crate::map::{Map, PickupDef, PickupKind, Push, Team, BOOST_ACCEL, PICKUP_HALF};
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
    /// Movement variables (sv_airaccelerate etc), editable in the menu.
    pub vars: MoveVars,
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
            vars: MoveVars::surf_server(),
            ramp_accuracy: true,
        }
    }
}

/// Things that happened during a tick, for sounds and effects.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum Event {
    Shot {
        player: usize,
        weapon: WeaponId,
        pos: Vec3,
        silenced: bool,
    },
    Laser {
        start: Vec3,
        end: Vec3,
        team: Team,
    },
    Explosion {
        pos: Vec3,
    },
    PickupTaken {
        player: usize,
        pos: Vec3,
        health: bool,
    },
    Tracer {
        start: Vec3,
        end: Vec3,
    },
    Impact {
        pos: Vec3,
        normal: Vec3,
    },
    Hit {
        victim: usize,
        attacker: usize,
        pos: Vec3,
        headshot: bool,
        /// Health taken (not more than the victim had).
        damage: f32,
    },
    Kill {
        victim: usize,
        attacker: Option<usize>,
        weapon: Option<WeaponId>,
        headshot: bool,
    },
    Jump {
        player: usize,
    },
    Land {
        player: usize,
        speed: f32,
    },
    Footstep {
        player: usize,
    },
    Reload {
        player: usize,
    },
    DryFire {
        player: usize,
    },
    Zoom {
        player: usize,
    },
    Deploy {
        player: usize,
    },
    KnifeSwing {
        player: usize,
        hit: bool,
    },
    Teleport {
        player: usize,
    },
    /// Thrown by a launch pad.
    Launch {
        player: usize,
        pos: Vec3,
    },
    /// Entered a booster.
    Boost {
        player: usize,
    },
    /// Picked up an attachment for the inventory.
    AttachmentTaken {
        player: usize,
        item: AttItem,
    },
    Pickup {
        player: usize,
    },
    RoundStart,
    RoundEnd {
        winner: Option<Team>,
    },
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

#[derive(Clone, Copy, Debug)]
pub struct Rocket {
    pub pos: Vec3,
    pub vel: Vec3,
    pub owner: usize,
    pub t0: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct PickupState {
    pub def: PickupDef,
    /// Game time when it is available again (0 = available).
    pub available_at: f64,
    /// Dropped by a dead player: gone after this time (or once taken).
    pub expires: Option<f64>,
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
    /// Last primary weapon bought; handed out again on respawn.
    pub last_buy: Option<WeaponId>,
    /// Attachments picked in the buy menu for each weapon type.
    pub loadout: Vec<(WeaponId, Attachments)>,
    /// Attachments carried but not mounted on a gun.
    pub inventory: Vec<AttItem>,
    /// Launch pads ignore the player until then.
    pub launch_ready: f64,
    pub in_boost: bool,
    /// Last attachment picked up and when, for the HUD.
    pub last_item: Option<(AttItem, f64)>,
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
            last_buy: None,
            loadout: Vec::new(),
            inventory: Vec::new(),
            launch_ready: 0.0,
            in_boost: false,
            last_item: None,
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
        match self.weapon() {
            Some(w) => w.zoom_fov(self.zoom),
            None => 90.0,
        }
    }

    pub fn maxspeed(&self) -> f32 {
        let def = self.active_id().def();
        let mods = self.weapon().map(|w| w.mods()).unwrap_or_default();
        let sniper = self.weapon().is_some_and(|w| w.is_sniper());
        let base = if self.zoom > 0 && sniper { def.zoom_maxspeed } else { def.maxspeed };
        let ads = if self.zoom > 0 && def.zoom_fov.is_empty() { mods.ads_speed } else { 0.0 };
        (base + mods.speed + ads).max(100.0)
    }

    /// The attachments this player wants on `id`.
    pub fn attachments_for(&self, id: WeaponId) -> Attachments {
        self.loadout.iter().find(|(w, _)| *w == id).map(|(_, a)| *a).unwrap_or_default()
    }

    fn slot_weapon_mut(&mut self, slot: Slot) -> Option<&mut Weapon> {
        match slot {
            Slot::Primary => self.primary.as_mut(),
            Slot::Secondary => self.secondary.as_mut(),
            Slot::Melee => None,
        }
    }

    pub fn slot_weapon(&self, slot: Slot) -> Option<&Weapon> {
        match slot {
            Slot::Primary => self.primary.as_ref(),
            Slot::Secondary => self.secondary.as_ref(),
            Slot::Melee => None,
        }
    }

    /// A human's gun leaving their hands: its attachments go back into the
    /// inventory. Bots keep theirs on the gun (loot).
    fn strip_slot(&mut self, slot: Slot) {
        if self.is_bot() {
            return;
        }
        if let Some(w) = self.slot_weapon_mut(slot) {
            let items = w.att.items();
            w.att = Weapon::default_att(w.id);
            self.inventory.extend(items);
        }
    }

    /// Mounts the parts last chosen for this gun type that are in the
    /// inventory onto the gun in `slot`.
    fn fit_preferred(&mut self, slot: Slot) {
        if self.is_bot() {
            return;
        }
        let Some(id) = self.slot_weapon(slot).map(|w| w.id) else { return };
        let pref = self.attachments_for(id);
        for cat in 0..4 {
            let Some(item) = pref.get(cat) else { continue };
            let free = self.slot_weapon(slot).is_some_and(|w| w.att.get(cat).is_none() && w.fits(item));
            if let Some(k) = self.inventory.iter().position(|x| *x == item).filter(|_| free) {
                self.inventory.remove(k);
                if let Some(w) = self.slot_weapon_mut(slot) {
                    w.set_part(cat, Some(item));
                }
            }
        }
    }

    pub fn set_attachments(&mut self, id: WeaponId, att: Attachments) {
        self.loadout.retain(|(w, _)| *w != id);
        self.loadout.push((id, att));
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
    pub pickups: Vec<PickupState>,
    pub rockets: Vec<Rocket>,
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
        Game::with_map(map, settings, seed)
    }

    pub fn with_map(map: Map, settings: Settings, seed: u64) -> Game {
        let vars = settings.vars;
        let pickups = map.pickups.iter().map(|d| PickupState { def: *d, available_at: 0.0, expires: None }).collect();
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
            pickups,
            rockets: Vec::new(),
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
        self.rockets.clear();
        for pk in self.pickups.iter_mut() {
            pk.available_at = 0.0;
        }
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
            // Like a CSDM gun menu: humans get their last purchase back.
            p.primary = if p.is_bot() { None } else { p.last_buy.map(Weapon::new) };
            p.secondary = Some(Weapon::new(WeaponId::Usp));
            p.fit_preferred(Slot::Primary);
            p.fit_preferred(Slot::Secondary);
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
        if !self.in_buyzone(idx) || !ALL_BUYABLE.contains(&id) {
            return false;
        }
        self.give(idx, id);
        true
    }

    /// Hands a weapon to a player (buying, tests) with their attachments.
    pub fn give(&mut self, idx: usize, id: WeaponId) {
        let slot = id.def().slot;
        let p = &mut self.players[idx];
        if slot == Slot::Melee {
            return;
        }
        p.strip_slot(slot);
        let w = if p.is_bot() { Weapon::with(id, p.attachments_for(id)) } else { Weapon::new(id) };
        match slot {
            Slot::Primary => {
                p.primary = Some(w);
                p.last_buy = Some(id);
            }
            _ => p.secondary = Some(w),
        }
        p.fit_preferred(slot);
        self.equip(idx, slot);
        self.events.push(Event::Pickup { player: idx });
    }

    /// Mounts `item` (None: the default part) in category `cat` of the gun
    /// in `slot`, taking it from the inventory and putting back what was
    /// mounted before. Works anywhere, like a backpack.
    pub fn mount(&mut self, idx: usize, slot: Slot, cat: usize, item: Option<AttItem>) -> bool {
        let p = &mut self.players[idx];
        let Some(w) = p.slot_weapon(slot) else { return false };
        let cur = w.att.get(cat);
        if cur == item || item.is_some_and(|it| it.category() != cat || !w.fits(it)) {
            return false;
        }
        if let Some(it) = item {
            let Some(k) = p.inventory.iter().position(|x| *x == it) else { return false };
            p.inventory.remove(k);
        }
        let Some(w) = p.slot_weapon_mut(slot) else { return false };
        let old = w.set_part(cat, item);
        let (id, att, levels) = (w.id, w.att, w.zoom_levels());
        p.inventory.extend(old);
        p.set_attachments(id, att);
        if p.active == slot && p.zoom > levels {
            p.zoom = 0;
        }
        self.events.push(Event::Pickup { player: idx });
        true
    }

    /// Takes out the weapon in `slot`, playing the deploy even if it is
    /// already the active slot (after buying a new gun).
    fn equip(&mut self, idx: usize, slot: Slot) {
        let now = self.time;
        let p = &mut self.players[idx];
        if p.active != slot {
            p.last_active = p.active;
        }
        p.active = slot;
        p.zoom = 0;
        p.resume_zoom = None;
        p.reload_end = None;
        let deploy = p.active_id().def().deploy_time * p.weapon().map_or(1.0, |w| w.mods().deploy);
        p.next_attack = now + deploy as f64;
        p.deploy_time = now;
        self.events.push(Event::Deploy { player: idx });
    }

    pub fn switch_weapon(&mut self, idx: usize, slot: Slot) {
        let now = self.time;
        let p = &mut self.players[idx];
        if !p.alive || !p.has_slot(slot) || p.active == slot {
            return;
        }
        let _ = now;
        self.equip(idx, slot);
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
        let active = p.active;
        p.strip_slot(active);
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
                        bot::BotAction::Buy(id, att) => {
                            self.players[i].set_attachments(id, att);
                            self.buy(i, id);
                        }
                        bot::BotAction::Switch(slot) => self.switch_weapon(i, slot),
                        bot::BotAction::Suicide => {
                            if self.players[i].alive {
                                self.kill(i, None, None, false);
                            }
                        }
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
        let now = self.time;
        self.pickups.retain(|p| p.expires.is_none_or(|t| t > now));
        self.update_rockets();

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
        self.apply_boosters(i);
        let tele = {
            let p = &self.players[i];
            let (o, mins, maxs) = (p.pm.origin, p.pm.mins(), p.pm.maxs());
            self.map.teleports.iter().find(|t| t.zone.touches(o, mins, maxs)).map(|t| t.dest)
        };
        if let Some(crate::map::TeleDest::Point { pos, yaw }) = tele {
            let p = &mut self.players[i];
            p.pm = PmState::new(pos);
            p.prev_origin = pos;
            p.angles.y = yaw;
            p.board = 0.0;
            self.events.push(Event::Teleport { player: i });
            if let Some(b) = p.bot.as_mut() {
                b.on_teleport();
            }
        }
        let teleport = {
            let p = &self.players[i];
            self.map.in_kill_zone(p.pm.origin, p.pm.mins()) || tele == Some(crate::map::TeleDest::TeamSpawn)
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
        self.touch_pickups(i, cmd.buttons & IN_USE != 0 && self.players[i].prev_buttons & IN_USE == 0);
        self.weapon_frame(i, cmd);
        self.players[i].prev_buttons = cmd.buttons;
    }

    /// Booster and launch pad volumes.
    fn apply_boosters(&mut self, i: usize) {
        let now = self.time;
        let gravity = self.vars.gravity;
        let p = &mut self.players[i];
        let (origin, mins, maxs) = (p.pm.origin, p.pm.mins(), p.pm.maxs());
        let mut boosted = false;
        for b in &self.map.boosters {
            if !b.zone.touches(origin, mins, maxs) {
                continue;
            }
            match b.push {
                Push::Boost { dir, speed, two_way } => {
                    boosted = true;
                    let dir = if two_way && p.pm.velocity.dot(dir) < 0.0 { -dir } else { dir };
                    let along = p.pm.velocity.dot(dir);
                    if along < speed {
                        p.pm.velocity += dir * (BOOST_ACCEL * TICK).min(speed - along);
                    }
                }
                Push::Launch { .. } => {
                    if now < p.launch_ready {
                        continue;
                    }
                    if let Some(v) = b.launch_velocity(gravity) {
                        p.pm.velocity = v;
                        p.pm.onground = false;
                        p.launch_ready = now + 0.6;
                        self.events.push(Event::Launch { player: i, pos: b.pad() });
                    }
                }
            }
        }
        if boosted && !p.in_boost {
            self.events.push(Event::Boost { player: i });
        }
        p.in_boost = boosted;
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
                        let m = p.weapon().map_or(1.0, |w| w.mods().reload);
                        p.reload_end = Some(now + (WeaponId::M3.def().reload_time * m) as f64);
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
            let levels = self.players[i].weapon().map_or(0, |w| w.zoom_levels());
            if levels > 0 && !self.players[i].reloading() {
                let p = &mut self.players[i];
                p.zoom = (p.zoom + 1) % (levels + 1);
                p.resume_zoom = None;
                self.events.push(Event::Zoom { player: i });
            } else if id == WeaponId::Knife {
                self.knife_attack(i, true);
            }
        }

        // Pistols fire once per trigger press (CS counts m_iShotsFired,
        // which resets when the button is released).
        let semi_block = {
            let p = &self.players[i];
            let def = p.active_id().def();
            !def.automatic && p.weapon().is_some_and(|w| w.shots_fired > 0)
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
            let m = p.weapon().map_or(1.0, |w| w.mods().reload) as f64;
            let t = if id == WeaponId::M3 { 0.55 } else { id.def().reload_time as f64 };
            p.reload_end = Some(now + t * m);
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
        let zoom = self.players[i].zoom;
        let (spread, id, angles, src, mods) = {
            let p = &mut self.players[i];
            let angles = p.angles + p.pm.punchangle;
            let src = p.pm.eye();
            let w = p.weapon_mut().unwrap();
            let mods = w.mods();
            let mut spread = compute_spread(w, st, now) * mods.spread;
            if !st.on_ground {
                spread *= mods.air_spread;
            } else if st.speed2d > 140.0 {
                spread *= mods.move_spread;
            }
            if w.is_ads(zoom) {
                spread *= mods.ads_spread;
            }
            if !w.def().automatic {
                w.shots_fired += 1;
            }
            w.clip -= 1;
            w.last_fire = now;
            (spread, w.id, angles, src, mods)
        };
        let def = id.def();
        let (fwd, right, up) = angle_vectors(angles);
        if id == WeaponId::Rocket {
            let start = src + fwd * 16.0 + right * 6.0 - up * 4.0;
            self.rockets.push(Rocket { pos: start, vel: fwd * ROCKET_SPEED, owner: i, t0: now });
        } else {
            for _ in 0..def.pellets.max(1) {
                let x = self.rng.range(-0.5, 0.5) + self.rng.range(-0.5, 0.5);
                let y = self.rng.range(-0.5, 0.5) + self.rng.range(-0.5, 0.5);
                let dir = (fwd + right * (x * spread) + up * (y * spread)).normalize();
                self.fire_bullet(i, src, dir, id, mods);
            }
        }
        {
            let p = &mut self.players[i];
            let before = p.pm.punchangle;
            let mut punch = before;
            let w = p.weapon_mut().unwrap();
            apply_recoil(w, st, &mut punch, &mut self.rng);
            p.pm.punchangle = before + (punch - before) * mods.recoil;
            p.next_attack = now + def.cycle as f64;
            p.last_fire = now;
            // snipers unzoom while the bolt cycles
            if matches!(id, WeaponId::Scout | WeaponId::Awp) && p.zoom > 0 {
                p.resume_zoom = Some(p.zoom);
                p.zoom = 0;
            }
        }
        self.events.push(Event::Shot { player: i, weapon: id, pos: src, silenced: mods.silenced });
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

    fn fire_bullet(&mut self, i: usize, src: Vec3, dir: Vec3, id: WeaponId, mods: Mods) {
        let def = id.def();
        let team = self.players[i].team;
        let tracer = |g: &mut Game, a: Vec3, b: Vec3| {
            if id == WeaponId::Laser {
                g.events.push(Event::Laser { start: a, end: b, team });
            } else if !mods.silenced {
                g.events.push(Event::Tracer { start: a, end: b });
            }
        };
        let range = if id == WeaponId::M3 { 3000.0 * (1.0 + mods.range_bonus * 8.0) } else { 8192.0 };
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
                let dmg = mods.damage * def.damage * falloff(id, def.range_modifier, mods.range_bonus, t);
                let pos = src + dir * t;
                tracer(self, muzzle, pos);
                let same_team = self.players[j].team == self.players[i].team;
                if !same_team || self.settings.friendly_fire {
                    self.damage(j, Some(i), dmg, g, def.armor_ratio, Some(id), pos);
                }
            }
            None => {
                let end = src + dir * wall_dist;
                tracer(self, muzzle, end);
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
        let dealt = hp_dmg.min(self.players[victim].health).max(0.0);
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
            self.events.push(Event::Hit { victim, attacker: a, pos, headshot, damage: dealt });
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
        // Drop an ammo box on the ground below, or where they died if there
        // is no ground (surfers die in the air).
        {
            let pos = self.players[victim].pm.origin;
            let tr = self.map.world.trace_ray(pos, pos - vec3(0.0, 0.0, 3000.0));
            let at = if tr.hit() && tr.normal.z > 0.7 && tr.endpos.z > self.map.kill_z {
                tr.endpos + vec3(0.0, 0.0, 22.0)
            } else {
                pos
            };
            self.pickups.push(PickupState {
                def: PickupDef { pos: at, kind: PickupKind::Ammo },
                available_at: now + 0.3,
                expires: Some(now + 30.0),
            });
        }
        // Drop the best weapon.
        let drop = {
            let p = &mut self.players[victim];
            p.strip_slot(Slot::Primary);
            p.strip_slot(Slot::Secondary);
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
        let kill_z = self.map.kill_z;
        self.dropped.retain(|d| now - d.time < 60.0 && d.pos.z > kill_z);
        let _ = maxs;
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

    fn update_rockets(&mut self) {
        let now = self.time;
        let mut k = 0;
        while k < self.rockets.len() {
            let r = self.rockets[k];
            let end = r.pos + r.vel * TICK;
            let ext = vec3(3.0, 3.0, 3.0);
            let tr = self.map.world.trace(r.pos, end, -ext, ext);
            let mut hit_pos = if tr.hit() { Some(tr.endpos) } else { None };
            // direct hits on players (not the owner right after launch)
            for (j, p) in self.players.iter().enumerate() {
                if !p.alive || (j == r.owner && now - r.t0 < 0.3) {
                    continue;
                }
                let lo = p.pm.origin + p.pm.mins() - ext;
                let hi = p.pm.origin + p.pm.maxs() + ext;
                let seg = end - r.pos;
                // sample the segment; rockets are slow enough for this
                for s in 0..=4 {
                    let q = r.pos + seg * (s as f32 / 4.0);
                    if q.cmpge(lo).all() && q.cmple(hi).all() {
                        hit_pos = Some(q);
                        break;
                    }
                }
                if hit_pos.is_some() && hit_pos != Some(tr.endpos) {
                    break;
                }
            }
            if let Some(pos) = hit_pos {
                self.rockets.swap_remove(k);
                self.explode(pos - r.vel.normalize_or_zero() * 4.0, r.owner);
                continue;
            }
            if now - r.t0 > 8.0 || end.z < self.map.kill_z - 500.0 {
                self.rockets.swap_remove(k);
                continue;
            }
            self.rockets[k].pos = end;
            k += 1;
        }
    }

    /// Rocket splash: damage falls off with distance, walls block it, and
    /// everybody in range gets knocked back (rocket jumps).
    fn explode(&mut self, pos: Vec3, owner: usize) {
        self.events.push(Event::Explosion { pos });
        let max_dmg = WeaponId::Rocket.def().damage;
        let owner_team = self.players[owner].team;
        let owner_mods = self.players[owner].primary.map(|w| w.mods()).unwrap_or_default();
        for j in 0..self.players.len() {
            let p = &self.players[j];
            if !p.alive {
                continue;
            }
            let lo = p.pm.origin + p.pm.mins();
            let hi = p.pm.origin + p.pm.maxs();
            let closest = pos.clamp(lo, hi);
            let d = closest.distance(pos);
            if d > ROCKET_RADIUS {
                continue;
            }
            let center = p.pm.origin;
            if self.map.world.trace_ray(pos, center).hit() && self.map.world.trace_ray(pos, closest).hit() {
                continue;
            }
            let k = 1.0 - d / ROCKET_RADIUS;
            let full = max_dmg * k * owner_mods.damage;
            let dir = (center - pos).normalize_or(Vec3::Z) + vec3(0.0, 0.0, 0.25);
            {
                let p = &mut self.players[j];
                let push = (full * 7.0).min(950.0) * if j == owner { 1.2 } else { 1.0 };
                p.pm.velocity += dir.normalize() * push;
                p.pm.onground = false;
            }
            let same = self.players[j].team == owner_team;
            let dmg = if j == owner { full * 0.4 } else { full };
            if j == owner || !same || self.settings.friendly_fire {
                self.damage(
                    j,
                    if j == owner { None } else { Some(owner) },
                    dmg,
                    HitGroup::Chest,
                    1.0,
                    Some(WeaponId::Rocket),
                    closest,
                );
                if j == owner && !self.players[j].alive {
                    // self kill still credits the rocket in the feed
                }
            }
        }
    }

    /// Map pickups: health packs and weapon spawners.
    fn touch_pickups(&mut self, i: usize, use_pressed: bool) {
        if !self.players[i].alive || self.pickups.is_empty() {
            return;
        }
        let now = self.time;
        for k in 0..self.pickups.len() {
            let pk = self.pickups[k];
            if pk.available_at > now {
                continue;
            }
            let (origin, mins, maxs) = {
                let p = &self.players[i];
                (p.pm.origin, p.pm.mins(), p.pm.maxs())
            };
            let half = vec3(PICKUP_HALF, PICKUP_HALF, PICKUP_HALF);
            let zone = crate::map::Aabb::new(pk.def.pos - half, pk.def.pos + half);
            if !zone.touches(origin, mins, maxs) {
                continue;
            }
            let taken = match pk.def.kind {
                PickupKind::Ammo => {
                    // map boxes fill both guns up; a dropped box gives a
                    // magazine for each
                    let full = pk.expires.is_none();
                    let p = &mut self.players[i];
                    let mut got = false;
                    for w in [p.primary.as_mut(), p.secondary.as_mut()].into_iter().flatten() {
                        let max = w.def().reserve;
                        if w.reserve < max {
                            w.reserve = if full { max } else { (w.reserve + w.def().clip).min(max) };
                            got = true;
                        }
                    }
                    got
                }
                PickupKind::Health => {
                    let p = &mut self.players[i];
                    if p.health < 100.0 {
                        p.health = (p.health + 50.0).min(100.0);
                        true
                    } else {
                        false
                    }
                }
                PickupKind::Weapon(id) => {
                    let slot = id.def().slot;
                    let is_bot = self.players[i].is_bot();
                    let (free, current) = {
                        let p = &self.players[i];
                        match slot {
                            Slot::Primary => (p.primary.is_none(), p.primary.map(|w| w.id)),
                            _ => (p.secondary.is_none(), p.secondary.map(|w| w.id)),
                        }
                    };
                    let upgrade = is_bot && current.is_some_and(|c| weapon_tier(id) > weapon_tier(c));
                    if free || use_pressed || upgrade {
                        if !free {
                            // swap: drop what we hold in that slot
                            self.switch_weapon(i, slot);
                            if self.players[i].active == slot {
                                self.drop_weapon(i);
                            }
                        }
                        let att = if is_bot {
                            crate::weapons::Attachments::random(&mut self.rng)
                        } else {
                            Attachments::default()
                        };
                        let p = &mut self.players[i];
                        match slot {
                            Slot::Primary => p.primary = Some(Weapon::with(id, att)),
                            _ => p.secondary = Some(Weapon::with(id, att)),
                        }
                        p.fit_preferred(slot);
                        self.equip(i, slot);
                        true
                    } else {
                        false
                    }
                }
                PickupKind::Attachment(item) => {
                    let p = &mut self.players[i];
                    if p.is_bot() {
                        false
                    } else {
                        p.inventory.push(item);
                        p.last_item = Some((item, now));
                        // Straight onto the gun in hand if that spot is free.
                        let slot = p.active;
                        let free = p.slot_weapon(slot).is_some_and(|w| w.att.get(item.category()).is_none());
                        if free {
                            self.mount(i, slot, item.category(), Some(item));
                        }
                        self.events.push(Event::AttachmentTaken { player: i, item });
                        true
                    }
                }
            };
            if taken {
                self.pickups[k].available_at = now + pk.def.kind.respawn();
                if pk.expires.is_some() {
                    self.pickups[k].expires = Some(now);
                }
                let health = pk.def.kind == PickupKind::Health;
                self.events.push(Event::PickupTaken { player: i, pos: pk.def.pos, health });
            }
        }
    }

    /// Weapon spawner close enough to swap with USE, for the HUD hint.
    pub fn pickup_near(&self, i: usize) -> Option<WeaponId> {
        let p = &self.players[i];
        self.pickups.iter().find_map(|pk| match pk.def.kind {
            PickupKind::Weapon(id)
                if pk.available_at <= self.time
                    && pk.def.pos.distance(p.pm.origin) < 70.0
                    && p.primary.is_some_and(|w| w.id != id) =>
            {
                Some(id)
            }
            _ => None,
        })
    }

    /// Line of sight between two points through the world.
    pub fn visible(&self, a: Vec3, b: Vec3) -> bool {
        !self.map.world.trace_ray(a, b).hit()
    }
}

/// How much a bot prefers a weapon over another.
pub fn weapon_tier(id: WeaponId) -> u32 {
    match id {
        WeaponId::Knife => 0,
        WeaponId::Usp => 1,
        WeaponId::M3 | WeaponId::Mp5 => 2,
        WeaponId::Scout => 3,
        WeaponId::Ak47 => 4,
        WeaponId::Awp | WeaponId::Laser | WeaponId::Rocket => 5,
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
