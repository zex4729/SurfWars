//! Skybox scenery: city skylines, mountain ranges, forests and desert mesas
//! drawn on a ring around the camera, like a GoldSrc 3D skybox. The meshes
//! are built around the origin and drawn with a model matrix that follows
//! the camera, so the scenery always sits on the horizon however far you fly.
//! Colours fade toward the sky's horizon colour with distance.

use macroquad::prelude::*;

use crate::map::{Backdrop, Map};
use crate::util::Rng;

/// Everything stays inside the sky dome (radius 30000).
const FAR: f32 = 27000.0;
/// Height of the ground ring relative to the camera; the scenery rises from it.
const BASE: f32 = -3200.0;

struct Builder {
    meshes: Vec<Mesh>,
    verts: Vec<Vertex>,
    idx: Vec<u16>,
    texture: Option<Texture2D>,
}

fn col(c: Vec3) -> [u8; 4] {
    [(c.x.clamp(0.0, 1.0) * 255.0) as u8, (c.y.clamp(0.0, 1.0) * 255.0) as u8, (c.z.clamp(0.0, 1.0) * 255.0) as u8, 255]
}

impl Builder {
    fn new(texture: Option<Texture2D>) -> Builder {
        Builder { meshes: Vec::new(), verts: Vec::new(), idx: Vec::new(), texture }
    }

    fn reserve(&mut self, n: usize) {
        if self.verts.len() + n > 60000 {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if !self.idx.is_empty() {
            self.meshes.push(Mesh {
                vertices: std::mem::take(&mut self.verts),
                indices: std::mem::take(&mut self.idx),
                texture: self.texture.clone(),
            });
        }
        self.verts.clear();
        self.idx.clear();
    }

    fn v(&mut self, p: Vec3, uv: Vec2, c: Vec3) -> u16 {
        self.verts.push(Vertex { position: p, uv, color: col(c), normal: Vec4::ZERO });
        (self.verts.len() - 1) as u16
    }

    /// Quad a b c d (in order) with per corner colours.
    fn quad(&mut self, p: [Vec3; 4], uv: [Vec2; 4], c: [Vec3; 4]) {
        self.reserve(4);
        let i: Vec<u16> = (0..4).map(|k| self.v(p[k], uv[k], c[k])).collect();
        self.idx.extend_from_slice(&[i[0], i[1], i[2], i[0], i[2], i[3]]);
    }

    fn tri(&mut self, p: [Vec3; 3], c: [Vec3; 3]) {
        self.reserve(3);
        let i: Vec<u16> = (0..3).map(|k| self.v(p[k], Vec2::ZERO, c[k])).collect();
        self.idx.extend_from_slice(&i);
    }

    fn finish(mut self) -> Vec<Mesh> {
        self.flush();
        self.meshes
    }
}

/// Smooth 1D periodic noise over an angle, 0..1.
struct Ridge {
    waves: Vec<(f32, f32, f32)>,
}

impl Ridge {
    fn new(rng: &mut Rng, octaves: usize, base_freq: f32) -> Ridge {
        let mut waves = Vec::new();
        let mut f = base_freq;
        let mut a = 1.0;
        for _ in 0..octaves {
            waves.push((f.round().max(1.0), rng.range(0.0, std::f32::consts::TAU), a));
            f *= 2.1;
            a *= 0.5;
        }
        Ridge { waves }
    }

    fn at(&self, angle: f32) -> f32 {
        let mut v = 0.0;
        let mut total = 0.0;
        for (f, ph, a) in &self.waves {
            // sharpened sines give ridge like peaks
            v += a * (1.0 - (angle * f + ph).sin().abs());
            total += a;
        }
        (v / total).clamp(0.0, 1.0)
    }
}

fn dir(a: f32) -> Vec3 {
    vec3(a.cos(), a.sin(), 0.0)
}

/// A flat ring of ground from `r0` to `r1`, so the scenery stands on
/// something rather than floating over the sky dome.
fn ground(b: &mut Builder, r0: f32, r1: f32, near: Vec3, far: Vec3) {
    let n = 64;
    for i in 0..n {
        let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
        let z = vec3(0.0, 0.0, BASE);
        b.quad(
            [dir(a0) * r0 + z, dir(a1) * r0 + z, dir(a1) * r1 + z, dir(a0) * r1 + z],
            [Vec2::ZERO; 4],
            [near, near, far, far],
        );
    }
}

/// A mountain range: a ring of peaks at radius `r`, `amp` high.
#[allow(clippy::too_many_arguments)]
fn range(
    b: &mut Builder,
    rng: &mut Rng,
    r: f32,
    amp: f32,
    freq: f32,
    foot: Vec3,
    rock: Vec3,
    peak: Vec3,
    snow: Option<Vec3>,
) {
    let ridge = Ridge::new(rng, 5, freq);
    let n = 540;
    let height = |a: f32| amp * (0.25 + 0.75 * ridge.at(a).powf(1.6));
    for i in 0..n {
        let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
        let (h0, h1) = (height(a0), height(a1));
        let p = |a: f32, z: f32| dir(a) * r + vec3(0.0, 0.0, z);
        let (m0, m1) = (BASE + (h0 - BASE) * 0.55, BASE + (h1 - BASE) * 0.55);
        b.quad([p(a0, BASE), p(a1, BASE), p(a1, m1), p(a0, m0)], [Vec2::ZERO; 4], [foot, foot, rock, rock]);
        // snow fades in above the snow line instead of switching per peak
        let top = |h: f32| match snow {
            Some(s) => {
                let k = ((h / amp - 0.5) / 0.3).clamp(0.0, 1.0);
                peak.lerp(s, k * k * (3.0 - 2.0 * k))
            }
            None => peak,
        };
        b.quad([p(a0, m0), p(a1, m1), p(a1, h1), p(a0, h0)], [Vec2::ZERO; 4], [rock, rock, top(h1), top(h0)]);
    }
}

pub fn build(map: &Map, windows: &Texture2D) -> Vec<Mesh> {
    let hor = Vec3::from(map.sky_horizon);
    let fog = Vec3::from(map.fog_color);
    // aerial perspective: blend toward the horizon colour
    let haze = |c: Vec3, k: f32| c.lerp(hor, k);
    let mut rng = Rng::new(0xb4c6 + map.name.len() as u64 * 31);
    match map.backdrop {
        Backdrop::None => Vec::new(),
        Backdrop::City => {
            let mut plain = Builder::new(None);
            range(
                &mut plain,
                &mut rng,
                FAR,
                2600.0,
                3.0,
                haze(vec3(0.3, 0.35, 0.42), 0.6),
                haze(vec3(0.4, 0.45, 0.52), 0.65),
                haze(vec3(0.55, 0.6, 0.68), 0.7),
                None,
            );
            ground(&mut plain, 12000.0, FAR, haze(vec3(0.16, 0.3, 0.4), 0.35), haze(fog, 0.5));
            let mut city = Builder::new(Some(windows.clone()));
            let density = Ridge::new(&mut rng, 3, 2.0);
            for (radius, count, hmax, k) in [(24000.0f32, 360, 6500.0f32, 0.5f32), (19500.0, 260, 4800.0, 0.3)] {
                for i in 0..count {
                    let a = (i as f32 + rng.range(0.0, 0.8)) / count as f32 * std::f32::consts::TAU;
                    if density.at(a) < 0.35 && rng.chance(0.8) {
                        continue;
                    }
                    let r = radius + rng.range(-1400.0, 1400.0);
                    let w = rng.range(450.0, 1300.0);
                    let d = w * rng.range(0.7, 1.3);
                    let tall = rng.range(0.0, 1.0).powf(2.2) * density.at(a).max(0.3);
                    let top = BASE + 800.0 + tall * (hmax - 800.0);
                    let tints =
                        [vec3(0.62, 0.66, 0.72), vec3(0.75, 0.7, 0.62), vec3(0.45, 0.5, 0.58), vec3(0.85, 0.85, 0.86)];
                    let tint = haze(tints[rng.range_u32(0, 4) as usize % 4], k);
                    let c = dir(a) * r;
                    let (fx, fy) = (dir(a + std::f32::consts::FRAC_PI_2), dir(a));
                    let corners = [c - fx * w - fy * d, c + fx * w - fy * d, c + fx * w + fy * d, c - fx * w + fy * d];
                    let scale = 1.0 / 1100.0;
                    for s in 0..4 {
                        let (p0, p1) = (corners[s], corners[(s + 1) % 4]);
                        let len = (p1 - p0).length();
                        let shade = [1.0, 0.8, 0.65, 0.85][s];
                        let cc = tint * shade;
                        b_quad_face(&mut city, p0, p1, top, len * scale, (top - BASE) * scale, cc);
                    }
                    let roof = haze(vec3(0.3, 0.32, 0.35), k);
                    let z = vec3(0.0, 0.0, top);
                    city.quad(
                        [corners[0] + z, corners[1] + z, corners[2] + z, corners[3] + z],
                        [Vec2::splat(0.02); 4],
                        [roof; 4],
                    );
                }
            }
            let mut v = plain.finish();
            v.extend(city.finish());
            v
        }
        Backdrop::Mountains => {
            let mut b = Builder::new(None);
            range(
                &mut b,
                &mut rng,
                FAR,
                7500.0,
                2.0,
                haze(vec3(0.25, 0.3, 0.35), 0.55),
                haze(vec3(0.42, 0.45, 0.5), 0.6),
                haze(vec3(0.55, 0.57, 0.6), 0.6),
                Some(haze(vec3(0.95, 0.96, 1.0), 0.25)),
            );
            range(
                &mut b,
                &mut rng,
                22000.0,
                4200.0,
                3.0,
                haze(vec3(0.18, 0.24, 0.2), 0.35),
                haze(vec3(0.3, 0.35, 0.32), 0.4),
                haze(vec3(0.42, 0.45, 0.45), 0.4),
                Some(haze(vec3(0.92, 0.93, 0.96), 0.2)),
            );
            ground(&mut b, 12000.0, FAR, haze(vec3(0.2, 0.28, 0.22), 0.35), haze(fog, 0.5));
            b.finish()
        }
        Backdrop::Forest => {
            let mut b = Builder::new(None);
            range(
                &mut b,
                &mut rng,
                FAR,
                5000.0,
                2.0,
                haze(vec3(0.25, 0.32, 0.3), 0.6),
                haze(vec3(0.35, 0.42, 0.4), 0.62),
                haze(vec3(0.5, 0.55, 0.55), 0.65),
                Some(haze(vec3(0.95, 0.96, 1.0), 0.35)),
            );
            // rolling hills covered in trees
            let hills = Ridge::new(&mut rng, 3, 4.0);
            let hr = 21000.0;
            let hill = |a: f32| BASE + 400.0 + 1600.0 * hills.at(a);
            let n = 360;
            let dark = haze(vec3(0.1, 0.22, 0.12), 0.35);
            let light = haze(vec3(0.2, 0.36, 0.18), 0.35);
            for i in 0..n {
                let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
                let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
                let p = |a: f32, z: f32| dir(a) * hr + vec3(0.0, 0.0, z);
                b.quad(
                    [p(a0, BASE), p(a1, BASE), p(a1, hill(a1)), p(a0, hill(a0))],
                    [Vec2::ZERO; 4],
                    [dark, dark, light, light],
                );
            }
            for _ in 0..2200 {
                let a = rng.range(0.0, std::f32::consts::TAU);
                let r = hr - rng.range(0.0, 2500.0);
                // trees in front of the hill face stand on its slope
                let base_z = hill(a) - (hr - r) * 0.6 - 60.0;
                let h = rng.range(350.0, 900.0);
                let w = h * rng.range(0.22, 0.32);
                let k = 0.25 + (r - (hr - 2500.0)) / 2500.0 * 0.15;
                let green = haze(vec3(0.06, rng.range(0.18, 0.26), 0.08), k);
                let tip = haze(vec3(0.14, 0.32, 0.12), k);
                let c = dir(a) * r;
                let apex = c + vec3(0.0, 0.0, base_z + h);
                let s = dir(a + std::f32::consts::FRAC_PI_2) * w;
                let fwd = dir(a) * w;
                let bz = vec3(0.0, 0.0, base_z);
                let pts = [c - s - fwd * 0.5 + bz, c + s - fwd * 0.5 + bz, c + fwd + bz];
                for k in 0..3 {
                    b.tri([pts[k], pts[(k + 1) % 3], apex], [green, green, tip]);
                }
            }
            ground(&mut b, 12000.0, FAR, haze(vec3(0.1, 0.2, 0.1), 0.35), haze(fog, 0.5));
            b.finish()
        }
        Backdrop::Mesas => {
            let mut b = Builder::new(None);
            range(
                &mut b,
                &mut rng,
                FAR,
                2400.0,
                2.0,
                haze(vec3(0.55, 0.35, 0.28), 0.6),
                haze(vec3(0.65, 0.42, 0.32), 0.62),
                haze(vec3(0.72, 0.5, 0.38), 0.65),
                None,
            );
            for _ in 0..70 {
                let a = rng.range(0.0, std::f32::consts::TAU);
                let r = rng.range(17000.0, 24500.0);
                let w = rng.range(1200.0, 3800.0);
                let h = rng.range(900.0, 3000.0);
                let k = 0.2 + (r - 17000.0) / 7500.0 * 0.35;
                let lower = haze(vec3(0.55, 0.26, 0.16), k);
                let band = haze(vec3(0.78, 0.45, 0.25), k);
                let cap = haze(vec3(0.7, 0.4, 0.24), k);
                let c = dir(a) * r;
                let sides = 9;
                let top = BASE + 700.0 + h;
                let mid = BASE + 700.0 + h * 0.62;
                let ring = |rad: f32, z: f32, j: usize| {
                    let t = j as f32 / sides as f32 * std::f32::consts::TAU;
                    c + vec3(t.cos() * rad, t.sin() * rad * 0.8, z)
                };
                for j in 0..sides {
                    let (a0, a1) = (j, (j + 1) % sides);
                    b.quad(
                        [ring(w, BASE, a0), ring(w, BASE, a1), ring(w * 0.8, mid, a1), ring(w * 0.8, mid, a0)],
                        [Vec2::ZERO; 4],
                        [lower, lower, lower, lower],
                    );
                    b.quad(
                        [
                            ring(w * 0.8, mid, a0),
                            ring(w * 0.8, mid, a1),
                            ring(w * 0.72, top, a1),
                            ring(w * 0.72, top, a0),
                        ],
                        [Vec2::ZERO; 4],
                        [band, band, cap, cap],
                    );
                    b.tri([ring(w * 0.72, top, a0), ring(w * 0.72, top, a1), c + vec3(0.0, 0.0, top)], [cap; 3]);
                }
            }
            ground(&mut b, 12000.0, FAR, haze(vec3(0.62, 0.4, 0.26), 0.35), haze(fog, 0.5));
            b.finish()
        }
    }
}

/// One wall of a building from `p0` to `p1` on the ground ring up to `top`,
/// with the window texture tiled `u` by `v` times.
fn b_quad_face(b: &mut Builder, p0: Vec3, p1: Vec3, top: f32, u: f32, v: f32, c: Vec3) {
    let z0 = vec3(0.0, 0.0, BASE);
    let z1 = vec3(0.0, 0.0, top);
    b.quad([p0 + z0, p1 + z0, p1 + z1, p0 + z1], [vec2(0.0, v), vec2(u, v), vec2(u, 0.0), vec2(0.0, 0.0)], [c; 4]);
}

/// Office windows: a dark facade with a grid of windows, some lit.
pub fn window_pixel(x: u32, y: u32) -> [u8; 4] {
    let cell = 8;
    let (cx, cy) = (x / cell, y / cell);
    let (ix, iy) = (x % cell, y % cell);
    let window = (1..=5).contains(&ix) && (2..=6).contains(&iy);
    if !window {
        return [150, 152, 158, 255];
    }
    // hash per window
    let h = (cx.wrapping_mul(73856093) ^ cy.wrapping_mul(19349663)).wrapping_mul(2654435761) >> 24;
    if h % 5 == 0 {
        [255, 230, 160, 255]
    } else {
        [60 + (h % 30) as u8, 72 + (h % 25) as u8, 90 + (h % 30) as u8, 255]
    }
}
