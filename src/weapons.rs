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
    /// Energy rifle: hitscan beam, dead accurate, no bullet drop-off.
    Laser,
    /// Rocket launcher: slow projectile with splash damage and knockback.
    Rocket,
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
    /// Built in scope zoom levels (degrees). The Scout has one; everything
    /// else zooms through its sight attachment.
    pub zoom_fov: &'static [f32],
    /// The sight a new gun comes with (the AWP's 8x scope).
    pub default_sight: Sight,
    pub kill_icon: &'static str,
}

/// What the buy menu sells. Everything else only comes from map pickups.
pub const ALL_BUYABLE: [WeaponId; 3] = [WeaponId::Usp, WeaponId::M3, WeaponId::Mp5];

/// Every weapon that can be placed as a map pickup.
pub const ALL_PICKUP_WEAPONS: [WeaponId; 8] = [
    WeaponId::Usp,
    WeaponId::M3,
    WeaponId::Mp5,
    WeaponId::Ak47,
    WeaponId::Scout,
    WeaponId::Awp,
    WeaponId::Laser,
    WeaponId::Rocket,
];

pub const ROCKET_SPEED: f32 = 1100.0;
pub const ROCKET_RADIUS: f32 = 260.0;

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
            WeaponId::Laser => &LASER,
            WeaponId::Rocket => &ROCKET,
        }
    }

    /// Short name used in files (map pickups).
    pub fn key(self) -> &'static str {
        self.def().kill_icon
    }

    pub fn from_key(k: &str) -> Option<WeaponId> {
        ALL_PICKUP_WEAPONS.iter().copied().chain([WeaponId::Knife]).find(|w| w.key() == k)
    }
}

static LASER: WeaponDef = WeaponDef {
    name: "Laser Rifle",
    slot: Slot::Primary,
    clip: 40,
    reserve: 80,
    damage: 24.0,
    range_modifier: 1.0,
    cycle: 0.09,
    reload_time: 2.2,
    deploy_time: 1.0,
    maxspeed: 240.0,
    zoom_maxspeed: 240.0,
    armor_ratio: 1.5,
    pellets: 1,
    automatic: true,
    zoom_fov: &[],
    default_sight: Sight::Iron,
    kill_icon: "laser",
};

static ROCKET: WeaponDef = WeaponDef {
    name: "Rocket Launcher",
    slot: Slot::Primary,
    clip: 4,
    reserve: 12,
    damage: 125.0,
    range_modifier: 1.0,
    cycle: 0.8,
    reload_time: 3.2,
    deploy_time: 1.2,
    maxspeed: 220.0,
    zoom_maxspeed: 220.0,
    armor_ratio: 1.0,
    pellets: 0,
    automatic: true,
    zoom_fov: &[],
    default_sight: Sight::Iron,
    kill_icon: "rocket",
};

// ---------------------------------------------------------------------------
// Attachments

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Sight {
    #[default]
    Iron,
    RedDot,
    Holo,
    Acog,
    /// Small 2x scope; the only magnified sight that fits a pistol.
    Scope2x,
    /// The AWP's own 8x sniper scope. Built in, never an inventory item.
    Scope8x,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Muzzle {
    #[default]
    None,
    Suppressor,
    Compensator,
    LongBarrel,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Stock {
    #[default]
    Standard,
    Light,
    Heavy,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Grip {
    #[default]
    None,
    Vertical,
    Angled,
    Stubby,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Attachments {
    pub sight: Sight,
    pub muzzle: Muzzle,
    pub stock: Stock,
    pub grip: Grip,
}

/// Field of view (our 90 degree scale) for 2x, 4x and 8x magnification:
/// tan(fov / 2) = tan(45) / magnification.
pub const FOV_2X: f32 = 53.13;
pub const FOV_4X: f32 = 28.07;
pub const FOV_8X: f32 = 14.25;

/// Multipliers and offsets an attachment set applies to a weapon.
#[derive(Clone, Copy, Debug)]
pub struct Mods {
    /// All spread.
    pub spread: f32,
    /// Extra spread factor while airborne (not on ground or ramp).
    pub air_spread: f32,
    /// Extra spread factor while running (> 140 u/s on the ground).
    pub move_spread: f32,
    /// Spread factor while aiming down a sight.
    pub ads_spread: f32,
    pub recoil: f32,
    pub damage: f32,
    /// Added to the range modifier (less damage falloff over distance; for
    /// the shotgun it stretches the pellets' falloff range instead).
    pub range_bonus: f32,
    pub reload: f32,
    pub deploy: f32,
    /// Added to the movement speed.
    pub speed: f32,
    /// Added to the movement speed while aiming down the sight.
    pub ads_speed: f32,
    /// Zoom FOV levels the sight gives (degrees); empty for no zoom.
    pub zooms: &'static [f32],
    pub silenced: bool,
}

impl Default for Mods {
    fn default() -> Self {
        Mods {
            spread: 1.0,
            air_spread: 1.0,
            move_spread: 1.0,
            ads_spread: 1.0,
            recoil: 1.0,
            damage: 1.0,
            range_bonus: 0.0,
            reload: 1.0,
            deploy: 1.0,
            speed: 0.0,
            ads_speed: 0.0,
            zooms: &[],
            silenced: false,
        }
    }
}

impl Sight {
    pub const ALL: [Sight; 6] = [Sight::Iron, Sight::RedDot, Sight::Holo, Sight::Acog, Sight::Scope2x, Sight::Scope8x];
    pub fn name(self) -> &'static str {
        match self {
            Sight::Iron => "Iron sights",
            Sight::RedDot => "Red dot",
            Sight::Holo => "Holographic",
            Sight::Acog => "4x ACOG",
            Sight::Scope2x => "2x scope",
            Sight::Scope8x => "8x sniper scope",
        }
    }
    pub fn desc(self) -> &'static str {
        match self {
            Sight::Iron => "no zoom",
            Sight::RedDot => "MOUSE2 aim down sights, -15% spread aimed, +range dmg",
            Sight::Holo => "MOUSE2 aim down sights, -20% spread aimed, ++range dmg, slower draw",
            Sight::Acog => "4x zoom, -40% spread aimed, +++range dmg, slow aimed, not on pistols",
            Sight::Scope2x => "MOUSE2 2x aim, -25% spread aimed, ++range dmg, fits pistols",
            Sight::Scope8x => "8x zoom, the AWP's own scope",
        }
    }
}

impl Muzzle {
    pub const ALL: [Muzzle; 4] = [Muzzle::None, Muzzle::Suppressor, Muzzle::Compensator, Muzzle::LongBarrel];
    pub fn name(self) -> &'static str {
        match self {
            Muzzle::None => "No muzzle",
            Muzzle::Suppressor => "Suppressor",
            Muzzle::Compensator => "Compensator",
            Muzzle::LongBarrel => "Long barrel",
        }
    }
    pub fn desc(self) -> &'static str {
        match self {
            Muzzle::None => "-",
            Muzzle::Suppressor => "silent, no flash, -15% recoil, -8% dmg, +range dmg",
            Muzzle::Compensator => "-30% recoil, +8% spread, +range dmg",
            Muzzle::LongBarrel => "+8% damage, ++++range dmg, -8 speed",
        }
    }
}

impl Stock {
    pub const ALL: [Stock; 3] = [Stock::Standard, Stock::Light, Stock::Heavy];
    pub fn name(self) -> &'static str {
        match self {
            Stock::Standard => "Standard stock",
            Stock::Light => "Light stock",
            Stock::Heavy => "Heavy stock",
        }
    }
    pub fn desc(self) -> &'static str {
        match self {
            Stock::Standard => "-",
            Stock::Light => "+12 speed, -20% air spread, +12% recoil, +range dmg",
            Stock::Heavy => "-25% recoil, -12 speed, ++range dmg",
        }
    }
}

impl Grip {
    pub const ALL: [Grip; 4] = [Grip::None, Grip::Vertical, Grip::Angled, Grip::Stubby];
    pub fn name(self) -> &'static str {
        match self {
            Grip::None => "No grip",
            Grip::Vertical => "Vertical grip",
            Grip::Angled => "Angled grip",
            Grip::Stubby => "Stubby grip",
        }
    }
    pub fn desc(self) -> &'static str {
        match self {
            Grip::None => "-",
            Grip::Vertical => "-20% recoil, ++range dmg",
            Grip::Angled => "30% faster draw, 15% faster reload, +range dmg",
            Grip::Stubby => "-25% running spread, -8% recoil, +range dmg",
        }
    }
}

pub const ATT_CATEGORIES: [&str; 4] = ["Sights", "Muzzles", "Stocks", "Grips"];

/// One attachment you can carry in the inventory. The defaults (iron
/// sights, no muzzle, standard stock, no grip) are not items: every gun
/// always has them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum AttItem {
    Sight(Sight),
    Muzzle(Muzzle),
    Stock(Stock),
    Grip(Grip),
}

impl AttItem {
    pub const ALL: [AttItem; 12] = [
        AttItem::Sight(Sight::RedDot),
        AttItem::Sight(Sight::Holo),
        AttItem::Sight(Sight::Scope2x),
        AttItem::Sight(Sight::Acog),
        AttItem::Muzzle(Muzzle::Suppressor),
        AttItem::Muzzle(Muzzle::Compensator),
        AttItem::Muzzle(Muzzle::LongBarrel),
        AttItem::Stock(Stock::Light),
        AttItem::Stock(Stock::Heavy),
        AttItem::Grip(Grip::Vertical),
        AttItem::Grip(Grip::Angled),
        AttItem::Grip(Grip::Stubby),
    ];

    /// 0 sight, 1 muzzle, 2 stock, 3 grip.
    pub fn category(self) -> usize {
        match self {
            AttItem::Sight(_) => 0,
            AttItem::Muzzle(_) => 1,
            AttItem::Stock(_) => 2,
            AttItem::Grip(_) => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AttItem::Sight(x) => x.name(),
            AttItem::Muzzle(x) => x.name(),
            AttItem::Stock(x) => x.name(),
            AttItem::Grip(x) => x.name(),
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            AttItem::Sight(x) => x.desc(),
            AttItem::Muzzle(x) => x.desc(),
            AttItem::Stock(x) => x.desc(),
            AttItem::Grip(x) => x.desc(),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            AttItem::Sight(Sight::RedDot) => "reddot",
            AttItem::Sight(Sight::Holo) => "holo",
            AttItem::Sight(Sight::Scope2x) => "scope2x",
            AttItem::Sight(_) => "acog",
            AttItem::Muzzle(Muzzle::Suppressor) => "suppressor",
            AttItem::Muzzle(Muzzle::Compensator) => "compensator",
            AttItem::Muzzle(_) => "longbarrel",
            AttItem::Stock(Stock::Light) => "lightstock",
            AttItem::Stock(_) => "heavystock",
            AttItem::Grip(Grip::Vertical) => "vgrip",
            AttItem::Grip(Grip::Angled) => "agrip",
            AttItem::Grip(_) => "sgrip",
        }
    }

    pub fn from_key(k: &str) -> Option<AttItem> {
        AttItem::ALL.iter().copied().find(|a| a.key() == k)
    }

    /// Rare items only show up on the sky platforms.
    pub fn rare(self) -> bool {
        matches!(self, AttItem::Sight(Sight::Acog | Sight::Holo) | AttItem::Muzzle(Muzzle::Suppressor))
    }

    /// Colour used for the pickup and inventory icons.
    pub fn color(self) -> [u8; 3] {
        match self.category() {
            0 => [255, 90, 80],
            1 => [110, 200, 255],
            2 => [255, 200, 90],
            _ => [140, 255, 140],
        }
    }
}

impl Attachments {
    pub fn random(rng: &mut Rng) -> Attachments {
        Attachments {
            sight: Sight::ALL[rng.range_u32(0, 4) as usize],
            muzzle: Muzzle::ALL[rng.range_u32(0, 3) as usize],
            stock: Stock::ALL[rng.range_u32(0, 2) as usize],
            grip: Grip::ALL[rng.range_u32(0, 3) as usize],
        }
    }

    /// The item mounted in category `cat`, or None for the default part.
    pub fn get(&self, cat: usize) -> Option<AttItem> {
        match cat {
            // built in sights (iron sights, the AWP scope) are not items
            0 => (!matches!(self.sight, Sight::Iron | Sight::Scope8x)).then_some(AttItem::Sight(self.sight)),
            1 => (self.muzzle != Muzzle::None).then_some(AttItem::Muzzle(self.muzzle)),
            2 => (self.stock != Stock::Standard).then_some(AttItem::Stock(self.stock)),
            _ => (self.grip != Grip::None).then_some(AttItem::Grip(self.grip)),
        }
    }

    /// Mounts `item` in category `cat` (None puts the default part back)
    /// and returns what was there before.
    pub fn set(&mut self, cat: usize, item: Option<AttItem>) -> Option<AttItem> {
        let old = self.get(cat);
        match (cat, item) {
            (_, Some(AttItem::Sight(x))) => self.sight = x,
            (_, Some(AttItem::Muzzle(x))) => self.muzzle = x,
            (_, Some(AttItem::Stock(x))) => self.stock = x,
            (_, Some(AttItem::Grip(x))) => self.grip = x,
            (0, None) => self.sight = Sight::Iron,
            (1, None) => self.muzzle = Muzzle::None,
            (2, None) => self.stock = Stock::Standard,
            (_, None) => self.grip = Grip::None,
        }
        old
    }

    /// All non-default parts.
    pub fn items(&self) -> Vec<AttItem> {
        (0..4).filter_map(|c| self.get(c)).collect()
    }

    pub fn mods(&self) -> Mods {
        let mut m = Mods::default();
        match self.sight {
            Sight::Iron => {}
            Sight::RedDot => {
                m.range_bonus += 0.02;
                m.zooms = &[80.0];
                m.ads_spread = 0.85;
            }
            Sight::Holo => {
                m.range_bonus += 0.03;
                m.zooms = &[72.0];
                m.ads_spread = 0.8;
                m.deploy *= 1.1;
            }
            Sight::Scope2x => {
                m.range_bonus += 0.025;
                m.zooms = &[FOV_2X];
                m.ads_spread = 0.75;
            }
            Sight::Scope8x => {
                m.range_bonus += 0.01;
                m.zooms = &[FOV_8X];
            }
            Sight::Acog => {
                m.range_bonus += 0.045;
                m.zooms = &[FOV_4X];
                m.ads_spread = 0.6;
                m.ads_speed -= 25.0;
            }
        }
        match self.muzzle {
            Muzzle::None => {}
            Muzzle::Suppressor => {
                m.range_bonus += 0.02;
                m.silenced = true;
                m.recoil *= 0.85;
                m.damage *= 0.92;
            }
            Muzzle::Compensator => {
                m.range_bonus += 0.015;
                m.recoil *= 0.7;
                m.spread *= 1.08;
            }
            Muzzle::LongBarrel => {
                m.damage *= 1.08;
                m.range_bonus += 0.06;
                m.speed -= 8.0;
            }
        }
        match self.stock {
            Stock::Standard => {}
            Stock::Light => {
                m.range_bonus += 0.01;
                m.speed += 12.0;
                m.air_spread *= 0.8;
                m.recoil *= 1.12;
            }
            Stock::Heavy => {
                m.range_bonus += 0.03;
                m.recoil *= 0.75;
                m.speed -= 12.0;
            }
        }
        match self.grip {
            Grip::None => {}
            Grip::Vertical => {
                m.recoil *= 0.8;
                m.range_bonus += 0.025;
            }
            Grip::Angled => {
                m.range_bonus += 0.01;
                m.deploy *= 0.7;
                m.reload *= 0.85;
            }
            Grip::Stubby => {
                m.range_bonus += 0.01;
                m.move_spread *= 0.75;
                m.recoil *= 0.92;
            }
        }
        m
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
    default_sight: Sight::Iron,
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
    default_sight: Sight::Iron,
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
    default_sight: Sight::Iron,
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
    default_sight: Sight::Iron,
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
    default_sight: Sight::Iron,
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
    default_sight: Sight::Iron,
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
    zoom_fov: &[],
    default_sight: Sight::Scope8x,
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
    pub att: Attachments,
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
            att: Weapon::default_att(id),
        }
    }

    /// A gun with these parts. Iron sights mean the gun's own sight (the
    /// AWP's scope), and parts that don't fit are left off.
    pub fn with(id: WeaponId, att: Attachments) -> Weapon {
        let mut w = Weapon::new(id);
        for cat in 0..4 {
            let item = match cat {
                0 if att.sight == Sight::Iron || att.sight == Sight::Scope8x => None,
                _ => att.get(cat),
            };
            if let Some(it) = item.filter(|it| w.fits(*it)) {
                w.att.set(cat, Some(it));
            }
        }
        w
    }

    /// The parts a new gun of this type comes with.
    pub fn default_att(id: WeaponId) -> Attachments {
        Attachments { sight: id.def().default_sight, ..Default::default() }
    }

    /// Whether an attachment can go on this gun: no long range scopes on the
    /// pistol, and the Scout keeps its own scope.
    pub fn fits(&self, item: AttItem) -> bool {
        match (self.id, item) {
            (WeaponId::Knife, _) => false,
            (WeaponId::Scout, AttItem::Sight(_)) => false,
            (_, AttItem::Sight(Sight::Scope8x)) => false,
            (_, AttItem::Sight(Sight::Acog)) => self.def().slot != Slot::Secondary,
            _ => true,
        }
    }

    /// Mounts `item` in category `cat` (None: the gun's default part) and
    /// returns the item that was there. Doesn't check `fits`.
    pub fn set_part(&mut self, cat: usize, item: Option<AttItem>) -> Option<AttItem> {
        let old = self.att.get(cat);
        match (cat, item) {
            (0, None) => self.att.sight = self.def().default_sight,
            _ => {
                self.att.set(cat, item);
            }
        }
        old
    }

    /// Name of the part a category falls back to.
    pub fn default_part_name(&self, cat: usize) -> &'static str {
        match cat {
            0 => self.def().default_sight.name(),
            1 => "No muzzle",
            2 => "Standard stock",
            _ => "No grip",
        }
    }

    /// Scout and AWP: CS sniper rules (zoomed speed, bolt unzoom).
    pub fn is_sniper(&self) -> bool {
        matches!(self.id, WeaponId::Scout | WeaponId::Awp)
    }

    pub fn mods(&self) -> Mods {
        if self.id == WeaponId::Knife {
            Mods::default()
        } else {
            self.att.mods()
        }
    }

    /// Damage per hit at `dist` units (before hit groups and armor).
    pub fn damage_at(&self, dist: f32) -> f32 {
        let d = self.def();
        let m = self.mods();
        m.damage * falloff(self.id, d.range_modifier, m.range_bonus, dist) * d.damage
    }

    /// Number of zoom levels: the built-in scope, or one from a sight.
    pub fn zoom_levels(&self) -> u8 {
        let d = self.def();
        if !d.zoom_fov.is_empty() {
            d.zoom_fov.len() as u8
        } else {
            self.mods().zooms.len() as u8
        }
    }

    pub fn zoom_fov(&self, level: u8) -> f32 {
        if level == 0 {
            return 90.0;
        }
        let d = self.def();
        if !d.zoom_fov.is_empty() {
            d.zoom_fov[(level as usize - 1).min(d.zoom_fov.len() - 1)]
        } else {
            let z = self.mods().zooms;
            z.get(level as usize - 1).copied().unwrap_or(90.0)
        }
    }

    /// True when aiming through an attachment sight (the ACOG as well):
    /// the sight's spread and speed modifiers apply.
    pub fn is_ads(&self, level: u8) -> bool {
        level > 0 && self.def().zoom_fov.is_empty()
    }

    /// True when the view shows a scope overlay: sniper scopes and the ACOG.
    /// Red dot and holographic sights aim down the sight instead.
    pub fn scope_view(&self, level: u8) -> bool {
        level > 0 && (!self.def().zoom_fov.is_empty() || matches!(self.att.sight, Sight::Acog | Sight::Scope8x))
    }

    /// Aiming down an open sight: the gun moves to the centre of the view.
    pub fn ads_view(&self, level: u8) -> bool {
        level > 0 && !self.scope_view(level)
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

/// CS damage falloff with distance; attachments add `bonus`. The range
/// modifier never goes over 1 (no damage gain with distance).
pub fn falloff(id: WeaponId, range_modifier: f32, bonus: f32, dist: f32) -> f32 {
    if id == WeaponId::M3 {
        let range = 3000.0 * (1.0 + bonus * 8.0);
        (1.0 - dist / range).max(0.0)
    } else {
        (range_modifier + bonus).min(1.0).powf(dist / 500.0)
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
        WeaponId::Laser => {
            if s.on_ground {
                0.002
            } else {
                0.012
            }
        }
        WeaponId::Rocket => 0.0,
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
        WeaponId::Laser => punch.x -= 0.25,
        WeaponId::Rocket => punch.x -= 3.0,
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
