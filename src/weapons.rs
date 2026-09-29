//! Weapon definitions. Numbers and accuracy / recoil formulas follow the
//! Counter-Strike 1.6 weapon code.

use macroquad::math::Vec3;

use crate::util::Rng;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum WeaponId {
    Knife,
    Usp,
    Mp5,
    M3,
    Ak47,
    Scout,
    Awp,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Primary,
    Secondary,
    Melee,
}

pub struct WeaponDef {
    pub name: &'static str,
    pub slot: Slot,
    pub clip: u32,
    pub reserve: u32,
    pub damage: f32,
    pub range_modifier: f32,
    pub cycle: f32,
    pub reload_time: f32,
    pub deploy_time: f32,
    pub maxspeed: f32,
    pub zoom_maxspeed: f32,
    pub armor_ratio: f32,
    pub pellets: u32,
    pub automatic: bool,
    /// Zoom field of view levels (degrees), empty if no scope.
    pub zoom_fov: &'static [f32],
    pub kill_icon: &'static str,
}

pub const ALL_BUYABLE: [WeaponId; 6] =
    [WeaponId::Usp, WeaponId::M3, WeaponId::Mp5, WeaponId::Ak47, WeaponId::Scout, WeaponId::Awp];

impl WeaponId {
    pub fn def(self) -> &'static WeaponDef {
        match self {
            WeaponId::Knife => &KNIFE,
            WeaponId::Usp => &USP,
            WeaponId::Mp5 => &MP5,
            WeaponId::M3 => &M3,
            WeaponId::Ak47 => &AK47,
            WeaponId::Scout => &SCOUT,
            WeaponId::Awp => &AWP,
        }
    }
}

static KNIFE: WeaponDef = WeaponDef {
    name: "Knife",
    slot: Slot::Melee,
    clip: 0,
    reserve: 0,
    damage: 15.0,
    range_modifier: 1.0,
    cycle: 0.4,
    reload_time: 0.0,
    deploy_time: 0.75,
    maxspeed: 250.0,
    zoom_maxspeed: 250.0,
    armor_ratio: 1.0,
    pellets: 0,
    automatic: true,
    zoom_fov: &[],
    kill_icon: "knife",
};

static USP: WeaponDef = WeaponDef {
    name: "USP .45",
    slot: Slot::Secondary,
    clip: 12,
    reserve: 100,
    damage: 34.0,
    range_modifier: 0.79,
    cycle: 0.225,
    reload_time: 2.7,
    deploy_time: 0.75,
    maxspeed: 250.0,
    zoom_maxspeed: 250.0,
    armor_ratio: 1.0,
    pellets: 1,
    automatic: false,
    zoom_fov: &[],
    kill_icon: "usp",
};

static MP5: WeaponDef = WeaponDef {
    name: "MP5 Navy",
    slot: Slot::Primary,
    clip: 30,
    reserve: 120,
    damage: 26.0,
    range_modifier: 0.84,
    cycle: 0.075,
    reload_time: 2.63,
    deploy_time: 1.0,
    maxspeed: 250.0,
    zoom_maxspeed: 250.0,
    armor_ratio: 1.0,
    pellets: 1,
    automatic: true,
    zoom_fov: &[],
    kill_icon: "mp5navy",
};

static M3: WeaponDef = WeaponDef {
    name: "M3 Super 90",
    slot: Slot::Primary,
    clip: 8,
    reserve: 32,
    damage: 20.0,
    range_modifier: 1.0,
    cycle: 0.875,
    reload_time: 0.45,
    deploy_time: 1.0,
    maxspeed: 230.0,
    zoom_maxspeed: 230.0,
    armor_ratio: 1.0,
    pellets: 9,
    automatic: true,
    zoom_fov: &[],
    kill_icon: "m3",
};

static AK47: WeaponDef = WeaponDef {
    name: "AK-47",
    slot: Slot::Primary,
    clip: 30,
    reserve: 90,
    damage: 36.0,
    range_modifier: 0.98,
    cycle: 0.0955,
    reload_time: 2.45,
    deploy_time: 1.0,
    maxspeed: 221.0,
    zoom_maxspeed: 221.0,
    armor_ratio: 1.55,
    pellets: 1,
    automatic: true,
    zoom_fov: &[],
    kill_icon: "ak47",
};

static SCOUT: WeaponDef = WeaponDef {
    name: "Schmidt Scout",
    slot: Slot::Primary,
    clip: 10,
    reserve: 90,
    damage: 75.0,
    range_modifier: 0.98,
    cycle: 1.25,
    reload_time: 2.0,
    deploy_time: 1.0,
    maxspeed: 260.0,
    zoom_maxspeed: 220.0,
    armor_ratio: 1.7,
    pellets: 1,
    automatic: true,
    zoom_fov: &[40.0, 15.0],
    kill_icon: "scout",
};

static AWP: WeaponDef = WeaponDef {
    name: "AWP",
    slot: Slot::Primary,
    clip: 10,
    reserve: 30,
    damage: 115.0,
    range_modifier: 0.99,
    cycle: 1.45,
    reload_time: 2.5,
    deploy_time: 1.25,
    maxspeed: 210.0,
    zoom_maxspeed: 150.0,
    armor_ratio: 1.95,
    pellets: 1,
    automatic: true,
    zoom_fov: &[40.0, 10.0],
    kill_icon: "awp",
};

/// A weapon owned by a player.
#[derive(Clone, Copy, Debug)]
pub struct Weapon {
    pub id: WeaponId,
    pub clip: u32,
    pub reserve: u32,
    pub accuracy: f32,
    pub shots_fired: u32,
    pub last_fire: f64,
    pub decrease_shots_at: f64,
    pub delay_fire: bool,
    pub direction: bool,
}

impl Weapon {
    pub fn new(id: WeaponId) -> Weapon {
        let d = id.def();
        Weapon {
            id,
            clip: d.clip,
            reserve: d.reserve,
            accuracy: Self::initial_accuracy(id),
            shots_fired: 0,
            last_fire: -10.0,
            decrease_shots_at: 0.0,
            delay_fire: false,
            direction: false,
        }
    }

    fn initial_accuracy(id: WeaponId) -> f32 {
        match id {
            WeaponId::Usp => 0.92,
            WeaponId::Ak47 => 0.2,
            _ => 0.0,
        }
    }

    pub fn def(&self) -> &'static WeaponDef {
        self.id.def()
    }
}

/// Movement state that the CS accuracy code looks at.
#[derive(Clone, Copy, Debug)]
pub struct ShooterState {
    pub on_ground: bool,
    pub ducking: bool,
    pub speed2d: f32,
    pub zoomed: bool,
}

/// Returns the spread for the next shot and updates the accuracy state the
/// same way `XXXPrimaryAttack` / `XXXFire` do in CS 1.6.
pub fn compute_spread(w: &mut Weapon, s: ShooterState, now: f64) -> f32 {
    match w.id {
        WeaponId::Knife => 0.0,
        WeaponId::Usp => {
            let spread = if !s.on_ground {
                1.2 * (1.0 - w.accuracy)
            } else if s.speed2d > 0.0 {
                0.225 * (1.0 - w.accuracy)
            } else if s.ducking {
                0.08 * (1.0 - w.accuracy)
            } else {
                0.1 * (1.0 - w.accuracy)
            };
            if w.last_fire > 0.0 {
                w.accuracy -= (0.3 - (now - w.last_fire) as f32) * 0.275;
                w.accuracy = w.accuracy.clamp(0.6, 0.92);
            }
            spread
        }
        WeaponId::Mp5 => {
            let spread = if !s.on_ground { 0.2 * w.accuracy } else { 0.04 * w.accuracy };
            w.delay_fire = true;
            w.shots_fired += 1;
            w.accuracy = ((w.shots_fired * w.shots_fired) as f32 / 220.1 + 0.45).min(0.75);
            spread
        }
        WeaponId::Ak47 => {
            let spread = if !s.on_ground {
                0.04 + 0.4 * w.accuracy
            } else if s.speed2d > 140.0 {
                0.04 + 0.07 * w.accuracy
            } else {
                0.0275 * w.accuracy
            };
            w.delay_fire = true;
            w.shots_fired += 1;
            let sf = w.shots_fired as f32;
            w.accuracy = (sf * sf * sf / 200.0 + 0.35).min(1.25);
            spread
        }
        WeaponId::Scout => {
            let mut spread = if !s.on_ground {
                0.2
            } else if s.speed2d > 170.0 {
                0.075
            } else if s.ducking {
                0.0
            } else {
                0.007
            };
            if !s.zoomed {
                spread += 0.025;
            }
            spread
        }
        WeaponId::Awp => {
            let mut spread = if !s.on_ground {
                0.85
            } else if s.speed2d > 140.0 {
                0.25
            } else if s.speed2d > 10.0 {
                0.1
            } else if s.ducking {
                0.0
            } else {
                0.001
            };
            if !s.zoomed {
                spread += 0.08;
            }
            spread
        }
        WeaponId::M3 => 0.0675,
    }
}

/// CS `CBasePlayerWeapon::KickBack`.
#[allow(clippy::too_many_arguments)]
fn kick_back(
    w: &mut Weapon,
    punch: &mut Vec3,
    rng: &mut Rng,
    up_base: f32,
    lateral_base: f32,
    up_modifier: f32,
    lateral_modifier: f32,
    up_max: f32,
    lateral_max: f32,
    direction_change: u32,
) {
    let (front, side) = if w.shots_fired == 1 {
        (up_base, lateral_base)
    } else {
        let s = w.shots_fired as f32;
        (s * up_modifier + up_base, s * lateral_modifier + lateral_base)
    };
    punch.x -= front;
    if punch.x < -up_max {
        punch.x = -up_max;
    }
    if w.direction {
        punch.y += side;
        if punch.y > lateral_max {
            punch.y = lateral_max;
        }
    } else {
        punch.y -= side;
        if punch.y < -lateral_max {
            punch.y = -lateral_max;
        }
    }
    if rng.range_u32(0, direction_change) == 0 {
        w.direction = !w.direction;
    }
}

/// Applies the view punch after a shot.
pub fn apply_recoil(w: &mut Weapon, s: ShooterState, punch: &mut Vec3, rng: &mut Rng) {
    match w.id {
        WeaponId::Knife => {}
        WeaponId::Usp | WeaponId::Scout | WeaponId::Awp => punch.x -= 2.0,
        WeaponId::M3 => {
            if s.on_ground {
                punch.x -= rng.range_u32(4, 6) as f32;
            } else {
                punch.x -= rng.range_u32(8, 11) as f32;
            }
        }
        WeaponId::Mp5 => {
            if !s.on_ground {
                kick_back(w, punch, rng, 0.9, 0.475, 0.35, 0.0425, 5.0, 3.0, 6);
            } else if s.speed2d > 0.0 {
                kick_back(w, punch, rng, 0.5, 0.275, 0.2, 0.03, 3.0, 2.0, 10);
            } else if s.ducking {
                kick_back(w, punch, rng, 0.225, 0.15, 0.1, 0.015, 2.0, 1.0, 10);
            } else {
                kick_back(w, punch, rng, 0.25, 0.175, 0.125, 0.02, 2.25, 1.25, 10);
            }
        }
        WeaponId::Ak47 => {
            if s.speed2d > 0.0 {
                kick_back(w, punch, rng, 1.5, 0.45, 0.225, 0.05, 6.5, 2.5, 7);
            } else if !s.on_ground {
                kick_back(w, punch, rng, 2.0, 1.0, 0.5, 0.35, 9.0, 6.0, 5);
            } else if s.ducking {
                kick_back(w, punch, rng, 0.9, 0.35, 0.15, 0.025, 5.5, 1.5, 9);
            } else {
                kick_back(w, punch, rng, 1.0, 0.375, 0.175, 0.0375, 5.75, 1.75, 8);
            }
        }
    }
}

/// Called every tick while the attack button is not held (CS ItemPostFrame).
pub fn idle_decay(w: &mut Weapon, now: f64) {
    match w.id {
        WeaponId::Usp => {
            w.shots_fired = 0;
        }
        WeaponId::Mp5 | WeaponId::Ak47 => {
            if w.delay_fire {
                w.delay_fire = false;
                if w.shots_fired > 15 {
                    w.shots_fired = 15;
                }
                w.decrease_shots_at = now + 0.4;
            }
            if w.shots_fired > 0 && now > w.decrease_shots_at {
                w.decrease_shots_at = now + 0.0225;
                w.shots_fired -= 1;
            }
        }
        _ => {
            w.shots_fired = 0;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitGroup {
    Head,
    Chest,
    Stomach,
    Legs,
}

impl HitGroup {
    pub fn multiplier(self) -> f32 {
        match self {
            HitGroup::Head => 4.0,
            HitGroup::Chest => 1.0,
            HitGroup::Stomach => 1.25,
            HitGroup::Legs => 0.75,
        }
    }
}

/// CS armor absorption. Returns (health damage, armor damage).
pub fn armor_absorb(damage: f32, armor: f32, helmet: bool, group: HitGroup, armor_ratio: f32) -> (f32, f32) {
    let armored = match group {
        HitGroup::Head => helmet,
        HitGroup::Legs => false,
        _ => true,
    };
    if armor <= 0.0 || !armored {
        return (damage, 0.0);
    }
    let ratio = 0.5 * armor_ratio;
    let bonus = 0.5;
    let mut new = damage * ratio;
    let mut armor_dmg = (damage - new) * bonus;
    if armor_dmg > armor {
        armor_dmg = armor;
        let absorbed = armor_dmg * (1.0 / bonus);
        new = damage - absorbed;
    }
    (new.max(0.0), armor_dmg)
}
