//! Procedurally synthesized sound effects. Everything is generated at start
//! up, so the game ships without any asset files.

use macroquad::math::Vec3;

use crate::game::{Event, Game};
use crate::util::{horizontal, Rng};
use crate::weapons::WeaponId;

const RATE: u32 = 44100;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sfx {
    Usp,
    Mp5,
    M3,
    Ak47,
    Scout,
    Awp,
    KnifeSlash,
    KnifeHit,
    Reload,
    DryFire,
    Zoom,
    Step1,
    Step2,
    Land,
    Hit,
    Headshot,
    Death,
    Teleport,
    Pickup,
    RoundStart,
    Win,
    Deploy,
    Wind,
    Hum,
}

const ALL: [Sfx; 24] = [
    Sfx::Usp,
    Sfx::Mp5,
    Sfx::M3,
    Sfx::Ak47,
    Sfx::Scout,
    Sfx::Awp,
    Sfx::KnifeSlash,
    Sfx::KnifeHit,
    Sfx::Reload,
    Sfx::DryFire,
    Sfx::Zoom,
    Sfx::Step1,
    Sfx::Step2,
    Sfx::Land,
    Sfx::Hit,
    Sfx::Headshot,
    Sfx::Death,
    Sfx::Teleport,
    Sfx::Pickup,
    Sfx::RoundStart,
    Sfx::Win,
    Sfx::Deploy,
    Sfx::Wind,
    Sfx::Hum,
];

// ---------------------------------------------------------------------------
// Synthesis helpers

struct Lp {
    y: f32,
    a: f32,
}

impl Lp {
    fn new(cutoff: f32) -> Lp {
        Lp {
            y: 0.0,
            a: 1.0 - (-2.0 * std::f32::consts::PI * cutoff / RATE as f32).exp(),
        }
    }
    fn set(&mut self, cutoff: f32) {
        self.a = 1.0 - (-2.0 * std::f32::consts::PI * cutoff / RATE as f32).exp();
    }
    fn run(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }
}

fn buf(secs: f32) -> Vec<f32> {
    vec![0.0; (secs * RATE as f32) as usize]
}

fn normalize(v: &mut [f32], peak: f32) {
    let m = v.iter().fold(0.0f32, |a, b| a.max(b.abs()));
    if m > 0.0 {
        for s in v.iter_mut() {
            *s = *s / m * peak;
        }
    }
}

struct Gun {
    dur: f32,
    crack: f32,
    body_hz: f32,
    body_decay: f32,
    tail: f32,
    cutoff: f32,
    drive: f32,
}

fn gunshot(g: Gun, seed: u64) -> Vec<f32> {
    let mut rng = Rng::new(seed);
    let mut out = buf(g.dur);
    let mut lp = Lp::new(g.cutoff);
    let mut lp2 = Lp::new(g.cutoff * 0.35);
    let mut phase = 0.0f32;
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / RATE as f32;
        let w = rng.range(-1.0, 1.0);
        let crack = w * (-t / g.crack).exp();
        lp.set(g.cutoff * (0.25 + 0.75 * (-t / (g.tail * 0.6)).exp()));
        let body = lp.run(w) * (-t / g.tail).exp();
        let rumble = lp2.run(w) * (-t / (g.tail * 2.0)).exp();
        let f = g.body_hz * (1.0 + 1.5 * (-t / 0.02).exp());
        phase += f / RATE as f32 * std::f32::consts::TAU;
        let thump = phase.sin() * (-t / g.body_decay).exp();
        let x = crack * 0.7 + body * 2.2 + rumble * 2.5 + thump * 0.9;
        *s = (x * g.drive).tanh();
    }
    // fade the end
    let n = out.len();
    for i in 0..(n / 10) {
        out[n - 1 - i] *= i as f32 / (n / 10) as f32;
    }
    normalize(&mut out, 0.9);
    out
}

fn click(out: &mut [f32], at: f32, freq: f32, amp: f32, rng: &mut Rng) {
    let start = (at * RATE as f32) as usize;
    let len = (0.03 * RATE as f32) as usize;
    let mut prev = 0.0;
    for i in 0..len {
        if start + i >= out.len() {
            break;
        }
        let t = i as f32 / RATE as f32;
        let w = rng.range(-1.0, 1.0);
        let hp = w - prev;
        prev = w;
        let ring = (t * freq * std::f32::consts::TAU).sin() * (-t / 0.012).exp();
        out[start + i] += (hp * (-t / 0.003).exp() * 0.8 + ring * 0.5) * amp;
    }
}

fn thud(out: &mut [f32], at: f32, freq: f32, decay: f32, amp: f32, rng: &mut Rng) {
    let start = (at * RATE as f32) as usize;
    let len = (decay * 6.0 * RATE as f32) as usize;
    let mut lp = Lp::new(500.0);
    for i in 0..len {
        if start + i >= out.len() {
            break;
        }
        let t = i as f32 / RATE as f32;
        let w = lp.run(rng.range(-1.0, 1.0));
        let s = (t * freq * std::f32::consts::TAU).sin();
        out[start + i] += (s * 0.8 + w * 1.5) * (-t / decay).exp() * amp;
    }
}

fn synth(sfx: Sfx) -> Vec<f32> {
    let mut rng = Rng::new(sfx as u64 * 7919 + 13);
    match sfx {
        Sfx::Usp => gunshot(
            Gun { dur: 0.45, crack: 0.006, body_hz: 150.0, body_decay: 0.035, tail: 0.07, cutoff: 3800.0, drive: 1.6 },
            1,
        ),
        Sfx::Mp5 => gunshot(
            Gun { dur: 0.35, crack: 0.005, body_hz: 190.0, body_decay: 0.03, tail: 0.055, cutoff: 4200.0, drive: 1.4 },
            2,
        ),
        Sfx::M3 => gunshot(
            Gun { dur: 0.9, crack: 0.012, body_hz: 70.0, body_decay: 0.11, tail: 0.22, cutoff: 1700.0, drive: 2.6 },
            3,
        ),
        Sfx::Ak47 => gunshot(
            Gun { dur: 0.55, crack: 0.009, body_hz: 115.0, body_decay: 0.06, tail: 0.12, cutoff: 2600.0, drive: 2.2 },
            4,
        ),
        Sfx::Scout => gunshot(
            Gun { dur: 0.9, crack: 0.011, body_hz: 125.0, body_decay: 0.07, tail: 0.2, cutoff: 3000.0, drive: 2.0 },
            5,
        ),
        Sfx::Awp => gunshot(
            Gun { dur: 1.5, crack: 0.014, body_hz: 58.0, body_decay: 0.16, tail: 0.38, cutoff: 1900.0, drive: 3.2 },
            6,
        ),
        Sfx::KnifeSlash => {
            let mut out = buf(0.28);
            let mut lp = Lp::new(800.0);
            let n = out.len();
            for (i, s) in out.iter_mut().enumerate() {
                let k = i as f32 / n as f32;
                lp.set(600.0 + 5000.0 * (k * std::f32::consts::PI).sin());
                let w = rng.range(-1.0, 1.0);
                let hp = w - lp.run(w);
                *s = hp * (k * std::f32::consts::PI).sin().powi(2);
            }
            normalize(&mut out, 0.5);
            out
        }
        Sfx::KnifeHit => {
            let mut out = buf(0.3);
            thud(&mut out, 0.0, 110.0, 0.04, 1.0, &mut rng);
            click(&mut out, 0.0, 3200.0, 0.6, &mut rng);
            normalize(&mut out, 0.7);
            out
        }
        Sfx::Reload => {
            let mut out = buf(1.6);
            click(&mut out, 0.05, 2600.0, 0.9, &mut rng);
            click(&mut out, 0.1, 1800.0, 0.6, &mut rng);
            click(&mut out, 0.85, 2200.0, 1.0, &mut rng);
            click(&mut out, 0.9, 3000.0, 0.7, &mut rng);
            click(&mut out, 1.3, 1500.0, 1.0, &mut rng);
            click(&mut out, 1.4, 2800.0, 1.0, &mut rng);
            normalize(&mut out, 0.45);
            out
        }
        Sfx::DryFire => {
            let mut out = buf(0.08);
            click(&mut out, 0.0, 3500.0, 1.0, &mut rng);
            normalize(&mut out, 0.4);
            out
        }
        Sfx::Zoom => {
            let mut out = buf(0.08);
            click(&mut out, 0.0, 5000.0, 1.0, &mut rng);
            normalize(&mut out, 0.3);
            out
        }
        Sfx::Deploy => {
            let mut out = buf(0.4);
            click(&mut out, 0.02, 2000.0, 1.0, &mut rng);
            click(&mut out, 0.2, 2600.0, 0.8, &mut rng);
            normalize(&mut out, 0.3);
            out
        }
        Sfx::Pickup => {
            let mut out = buf(0.3);
            click(&mut out, 0.0, 1700.0, 1.0, &mut rng);
            click(&mut out, 0.12, 2500.0, 1.0, &mut rng);
            normalize(&mut out, 0.4);
            out
        }
        Sfx::Step1 | Sfx::Step2 => {
            let mut out = buf(0.15);
            let f = if sfx == Sfx::Step1 { 90.0 } else { 105.0 };
            thud(&mut out, 0.0, f, 0.018, 1.0, &mut rng);
            click(&mut out, 0.002, 1200.0, 0.25, &mut rng);
            normalize(&mut out, 0.35);
            out
        }
        Sfx::Land => {
            let mut out = buf(0.3);
            thud(&mut out, 0.0, 70.0, 0.035, 1.0, &mut rng);
            thud(&mut out, 0.03, 95.0, 0.025, 0.6, &mut rng);
            normalize(&mut out, 0.5);
            out
        }
        Sfx::Hit => {
            let mut out = buf(0.25);
            thud(&mut out, 0.0, 85.0, 0.03, 1.0, &mut rng);
            normalize(&mut out, 0.6);
            out
        }
        Sfx::Headshot => {
            let mut out = buf(0.45);
            for (i, s) in out.iter_mut().enumerate() {
                let t = i as f32 / RATE as f32;
                let tau = std::f32::consts::TAU;
                let ring = (t * 2350.0 * tau).sin() * 0.5
                    + (t * 3720.0 * tau).sin() * 0.3
                    + (t * 5230.0 * tau).sin() * 0.2;
                *s = ring * (-t / 0.09).exp();
            }
            thud(&mut out, 0.0, 90.0, 0.02, 0.8, &mut rng);
            normalize(&mut out, 0.55);
            out
        }
        Sfx::Death => {
            let mut out = buf(0.5);
            let mut phase = 0.0f32;
            let mut bp1 = Lp::new(900.0);
            let mut bp2 = Lp::new(300.0);
            let n = out.len();
            for (i, s) in out.iter_mut().enumerate() {
                let k = i as f32 / n as f32;
                let f = 170.0 - 80.0 * k;
                phase = (phase + f / RATE as f32).fract();
                let saw = phase * 2.0 - 1.0 + rng.range(-0.1, 0.1);
                let band = bp1.run(saw) - bp2.run(saw);
                *s = band * (k * std::f32::consts::PI).sin().powf(0.6);
            }
            normalize(&mut out, 0.45);
            out
        }
        Sfx::Teleport => {
            let mut out = buf(0.6);
            let n = out.len();
            let mut phase = 0.0f32;
            for (i, s) in out.iter_mut().enumerate() {
                let k = i as f32 / n as f32;
                phase += (300.0 + 1600.0 * k * k) / RATE as f32 * std::f32::consts::TAU;
                *s = (phase.sin() * 0.6 + rng.range(-0.2, 0.2)) * (k * std::f32::consts::PI).sin();
            }
            normalize(&mut out, 0.3);
            out
        }
        Sfx::RoundStart => {
            let mut out = buf(0.5);
            for (i, s) in out.iter_mut().enumerate() {
                let t = i as f32 / RATE as f32;
                let f = if t < 0.2 { 880.0 } else { 1320.0 };
                let env = if t < 0.2 { (-(t) / 0.15).exp() } else { (-(t - 0.2) / 0.2).exp() };
                *s = (t * f * std::f32::consts::TAU).sin().signum() * 0.3 * env;
            }
            let mut lp = Lp::new(3000.0);
            for s in out.iter_mut() {
                *s = lp.run(*s);
            }
            normalize(&mut out, 0.3);
            out
        }
        Sfx::Win => {
            let mut out = buf(1.1);
            let notes = [(0.0, 523.25), (0.18, 659.25), (0.36, 783.99), (0.54, 1046.5)];
            for (at, f) in notes {
                let start = (at * RATE as f32) as usize;
                for i in 0..(0.5 * RATE as f32) as usize {
                    if start + i >= out.len() {
                        break;
                    }
                    let t = i as f32 / RATE as f32;
                    let tau = std::f32::consts::TAU;
                    let v = (t * f * tau).sin() + 0.3 * (t * f * 2.0 * tau).sin();
                    out[start + i] += v * (-t / 0.25).exp();
                }
            }
            normalize(&mut out, 0.35);
            out
        }
        Sfx::Wind => {
            // Seamless loop: crossfade the end into the start.
            let len = 2.0;
            let mut raw = buf(len + 0.25);
            let mut lp = Lp::new(500.0);
            let mut lp2 = Lp::new(3.0);
            for s in raw.iter_mut() {
                let gust = 0.6 + 0.4 * lp2.run(rng.range(-1.0, 1.0)) * 20.0;
                *s = lp.run(rng.range(-1.0, 1.0)) * gust;
            }
            let n = (len * RATE as f32) as usize;
            let fade = raw.len() - n;
            let mut out = raw[..n].to_vec();
            for i in 0..fade {
                let k = i as f32 / fade as f32;
                out[i] = out[i] * k + raw[n + i] * (1.0 - k);
            }
            normalize(&mut out, 0.8);
            out
        }
        Sfx::Hum => {
            let mut out = buf(1.0);
            for (i, s) in out.iter_mut().enumerate() {
                let t = i as f32 / RATE as f32;
                let tau = std::f32::consts::TAU;
                *s = (t * 110.0 * tau).sin() * 0.5
                    + (t * 165.0 * tau).sin() * 0.25
                    + (t * 220.0 * tau).sin() * 0.2
                    + (t * 331.0 * tau).sin() * 0.05 * (t * 3.0 * tau).sin();
            }
            normalize(&mut out, 0.5);
            out
        }
    }
}

fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let data_len = samples.len() as u32 * 2;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let q = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        v.extend_from_slice(&q.to_le_bytes());
    }
    v
}

// ---------------------------------------------------------------------------
// Playback

#[cfg(feature = "audio")]
use macroquad::audio::{load_sound_from_bytes, play_sound, set_sound_volume, PlaySoundParams, Sound};

pub struct Audio {
    #[cfg(feature = "audio")]
    sounds: std::collections::HashMap<Sfx, Sound>,
    pub volume: f32,
    step_toggle: bool,
    enabled: bool,
}

impl Audio {
    pub async fn new(enabled: bool) -> Audio {
        #[allow(unused_mut)]
        let mut a = Audio {
            #[cfg(feature = "audio")]
            sounds: std::collections::HashMap::new(),
            volume: 0.7,
            step_toggle: false,
            enabled: enabled && cfg!(feature = "audio"),
        };
        #[cfg(feature = "audio")]
        if a.enabled {
            for sfx in ALL {
                let bytes = wav_bytes(&synth(sfx));
                if let Ok(s) = load_sound_from_bytes(&bytes).await {
                    a.sounds.insert(sfx, s);
                }
            }
            for sfx in [Sfx::Wind, Sfx::Hum] {
                if let Some(s) = a.sounds.get(&sfx) {
                    play_sound(s, PlaySoundParams { looped: true, volume: 0.0 });
                }
            }
        }
        let _ = ALL;
        a
    }

    #[allow(unused_variables)]
    fn play(&self, sfx: Sfx, vol: f32) {
        #[cfg(feature = "audio")]
        if self.enabled && vol > 0.01 {
            if let Some(s) = self.sounds.get(&sfx) {
                play_sound(s, PlaySoundParams { looped: false, volume: (vol * self.volume).min(1.0) });
            }
        }
    }

    #[allow(unused_variables)]
    fn set_loop(&self, sfx: Sfx, vol: f32) {
        #[cfg(feature = "audio")]
        if self.enabled {
            if let Some(s) = self.sounds.get(&sfx) {
                set_sound_volume(s, (vol * self.volume).clamp(0.0, 1.0));
            }
        }
    }

    fn at(&self, listener: Vec3, pos: Vec3, base: f32, range: f32) -> f32 {
        let d = listener.distance(pos);
        base * (1.0 - d / range).clamp(0.0, 1.0).powf(1.6)
    }

    /// Plays sounds for this frame's game events.
    pub fn handle_events(&mut self, game: &Game, events: &[Event], listener: Vec3, local: Option<usize>) {
        for e in events {
            match e {
                Event::Shot { player, weapon, pos } => {
                    let own = Some(*player) == local;
                    let (sfx, range, base) = match weapon {
                        WeaponId::Usp => (Sfx::Usp, 3500.0, 0.8),
                        WeaponId::Mp5 => (Sfx::Mp5, 3500.0, 0.7),
                        WeaponId::M3 => (Sfx::M3, 5000.0, 1.0),
                        WeaponId::Ak47 => (Sfx::Ak47, 5000.0, 0.9),
                        WeaponId::Scout => (Sfx::Scout, 7000.0, 0.95),
                        WeaponId::Awp => (Sfx::Awp, 9000.0, 1.0),
                        WeaponId::Knife => continue,
                    };
                    let v = if own { base } else { self.at(listener, *pos, base * 0.8, range) };
                    self.play(sfx, v);
                }
                Event::KnifeSwing { player, hit } => {
                    let pos = game.players[*player].pm.origin;
                    let v = self.at(listener, pos, 0.8, 900.0);
                    self.play(if *hit { Sfx::KnifeHit } else { Sfx::KnifeSlash }, v);
                }
                Event::Hit { victim, attacker, headshot, .. } => {
                    let pos = game.players[*victim].pm.origin;
                    let near = if Some(*attacker) == local || Some(*victim) == local { 1.0 } else { 0.0 };
                    let v = self.at(listener, pos, 0.8, 2500.0).max(near * 0.8);
                    self.play(if *headshot { Sfx::Headshot } else { Sfx::Hit }, v);
                }
                Event::Kill { victim, .. } => {
                    let pos = game.players[*victim].pm.origin;
                    self.play(Sfx::Death, self.at(listener, pos, 0.7, 2500.0));
                }
                Event::Footstep { player } => {
                    let pos = game.players[*player].pm.origin;
                    self.step_toggle = !self.step_toggle;
                    let sfx = if self.step_toggle { Sfx::Step1 } else { Sfx::Step2 };
                    self.play(sfx, self.at(listener, pos, 0.6, 1400.0));
                }
                Event::Jump { player } => {
                    let pos = game.players[*player].pm.origin;
                    self.play(Sfx::Step2, self.at(listener, pos, 0.4, 1000.0));
                }
                Event::Land { player, speed } => {
                    let pos = game.players[*player].pm.origin;
                    let v = (speed / 600.0).clamp(0.3, 1.0);
                    self.play(Sfx::Land, self.at(listener, pos, v, 1400.0));
                }
                Event::Reload { player } => {
                    if Some(*player) == local {
                        self.play(Sfx::Reload, 0.8);
                    }
                }
                Event::DryFire { player } => {
                    if Some(*player) == local {
                        self.play(Sfx::DryFire, 0.8);
                    }
                }
                Event::Zoom { player } => {
                    if Some(*player) == local {
                        self.play(Sfx::Zoom, 0.8);
                    }
                }
                Event::Deploy { player } => {
                    if Some(*player) == local {
                        self.play(Sfx::Deploy, 0.6);
                    }
                }
                Event::Pickup { player } => {
                    if Some(*player) == local {
                        self.play(Sfx::Pickup, 0.8);
                    }
                }
                Event::Teleport { player } => {
                    let pos = game.players[*player].pm.origin;
                    let v = if Some(*player) == local { 0.7 } else { self.at(listener, pos, 0.5, 1500.0) };
                    self.play(Sfx::Teleport, v);
                }
                Event::RoundStart => self.play(Sfx::RoundStart, 0.6),
                Event::RoundEnd { .. } => self.play(Sfx::Win, 0.6),
                _ => {}
            }
        }
    }

    /// Continuous sounds: wind rush with speed and the board hum.
    pub fn update_loops(&mut self, game: &Game, pov: Option<usize>, paused: bool) {
        let (wind, hum) = match pov.map(|i| &game.players[i]) {
            Some(p) if p.alive && !paused => {
                let speed = horizontal(p.pm.velocity).length();
                let w = ((speed - 350.0) / 1800.0).clamp(0.0, 1.0);
                (w * 0.9, p.board * 0.25)
            }
            _ => (0.0, 0.0),
        };
        self.set_loop(Sfx::Wind, wind);
        self.set_loop(Sfx::Hum, hum);
    }
}
