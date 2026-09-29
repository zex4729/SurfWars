//! Small helpers: a deterministic RNG and angle math.

use macroquad::math::{vec3, Vec3};

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    /// Inclusive range, like RANDOM_LONG.
    pub fn range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo + 1) as u64) as u32
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.range_u32(0, items.len() as u32 - 1) as usize]
    }
}

/// Wraps an angle to (-180, 180].
pub fn angle_norm(a: f32) -> f32 {
    let mut a = a % 360.0;
    if a > 180.0 {
        a -= 360.0;
    }
    if a <= -180.0 {
        a += 360.0;
    }
    a
}

/// Pitch / yaw (Quake convention) that look along `dir`.
pub fn vec_to_angles(dir: Vec3) -> (f32, f32) {
    let yaw = dir.y.atan2(dir.x).to_degrees();
    let pitch = -(dir.z.atan2((dir.x * dir.x + dir.y * dir.y).sqrt())).to_degrees();
    (pitch, yaw)
}

pub fn horizontal(v: Vec3) -> Vec3 {
    vec3(v.x, v.y, 0.0)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn approach(cur: f32, target: f32, step: f32) -> f32 {
    if cur < target {
        (cur + step).min(target)
    } else {
        (cur - step).max(target)
    }
}
