//! 3D rendering: world brushes, sky, players with their hover boards,
//! trails, effects and the first person weapon model.

use std::collections::VecDeque;

use macroquad::miniquad::{
    BlendFactor, BlendState, BlendValue, Comparison, CullFace, Equation, MipmapFilterMode, PassAction,
    PipelineParams, TextureFormat, TextureKind, TextureParams, TextureWrap, UniformDesc, UniformType,
};
use macroquad::prelude::*;

use crate::game::{Event, Game, Player};
use crate::map::{Team, Tex};
use crate::pmove::angle_vectors;
use crate::util::{horizontal, Rng};
use crate::weapons::WeaponId;

const SUN: Vec3 = vec3(0.38, 0.22, 0.90);
const MAX_VERTS: usize = 15000;

// ---------------------------------------------------------------------------
// Shaders

const WORLD_VS: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
varying lowp vec4 v_color;
varying highp vec2 v_uv;
varying highp vec3 v_pos;
varying highp float v_flag;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    vec4 wp = Model * vec4(position, 1.0);
    v_pos = wp.xyz;
    gl_Position = Projection * wp;
    v_color = color0 / 255.0;
    v_uv = texcoord;
    v_flag = normal.w;
}
"#;

const WORLD_FS: &str = r#"#version 100
precision highp float;
varying lowp vec4 v_color;
varying highp vec2 v_uv;
varying highp vec3 v_pos;
varying highp float v_flag;
uniform sampler2D Texture;
uniform vec3 CamPos;
uniform vec3 FogColor;
uniform vec4 FogParams;
void main() {
    vec4 c;
    if (v_flag > 0.5) {
        float t = FogParams.z;
        vec4 a = texture2D(Texture, v_uv + vec2(t * 0.020, t * 0.013));
        vec4 b = texture2D(Texture, v_uv * 0.63 - vec2(t * 0.011, -t * 0.017));
        float w = a.r * b.r;
        c = vec4(v_color.rgb * (0.55 + 0.9 * w) + vec3(0.25) * pow(w, 6.0), 1.0);
    } else {
        c = v_color * texture2D(Texture, v_uv);
    }
    float d = distance(v_pos, CamPos);
    float f = clamp((d - FogParams.x) / (FogParams.y - FogParams.x), 0.0, 1.0);
    float h = clamp(1.0 - (v_pos.z - 120.0) / FogParams.w, 0.0, 1.0);
    f = max(f, h * h * 0.75 * clamp(d / 2500.0, 0.25, 1.0));
    gl_FragColor = vec4(mix(c.rgb, FogColor, f), c.a);
}
"#;

const FX_VS: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying lowp vec4 v_color;
varying mediump vec2 v_uv;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1.0);
    v_color = color0 / 255.0;
    v_uv = texcoord;
}
"#;

const FX_FS: &str = r#"#version 100
precision mediump float;
varying lowp vec4 v_color;
varying mediump vec2 v_uv;
uniform sampler2D Texture;
void main() {
    gl_FragColor = v_color * texture2D(Texture, v_uv);
}
"#;

// ---------------------------------------------------------------------------
// Procedural textures

fn make_texture(size: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Texture2D {
    let mut bytes = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            bytes.extend_from_slice(&f(x, y));
        }
    }
    let gl = unsafe { get_internal_gl() };
    let id = gl.quad_context.new_texture_from_data_and_format(
        &bytes,
        TextureParams {
            kind: TextureKind::Texture2D,
            format: TextureFormat::RGBA8,
            wrap: TextureWrap::Repeat,
            min_filter: FilterMode::Linear,
            mag_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Linear,
            width: size,
            height: size,
            allocate_mipmaps: true,
            sample_count: 1,
        },
    );
    gl.quad_context.texture_generate_mipmaps(id);
    Texture2D::from_miniquad_texture(id)
}

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

/// Tileable value noise.
fn vnoise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let fx = x - xi as f32;
    let fy = y - yi as f32;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let w = |a: i32| a.rem_euclid(period);
    let a = hash2(w(xi), w(yi), seed);
    let b = hash2(w(xi + 1), w(yi), seed);
    let c = hash2(w(xi), w(yi + 1), seed);
    let d = hash2(w(xi + 1), w(yi + 1), seed);
    let ab = a + (b - a) * sx;
    let cd = c + (d - c) * sx;
    ab + (cd - ab) * sy
}

fn fbm(x: f32, y: f32, size: f32, seed: u32) -> f32 {
    let mut v = 0.0;
    let mut amp = 0.5;
    let mut freq = 4.0;
    for o in 0..5 {
        let p = freq as i32;
        v += amp * vnoise(x / size * freq, y / size * freq, p, seed + o);
        amp *= 0.5;
        freq *= 2.0;
    }
    v
}

fn gray(v: f32) -> [u8; 4] {
    let c = (v.clamp(0.0, 1.0) * 255.0) as u8;
    [c, c, c, 255]
}

pub struct Textures {
    pub grid: Texture2D,
    pub crate_: Texture2D,
    pub metal: Texture2D,
    pub concrete: Texture2D,
    pub water: Texture2D,
    pub glow: Texture2D,
}

impl Textures {
    fn new() -> Textures {
        let grid = make_texture(256, |x, y| {
            let n = fbm(x as f32, y as f32, 256.0, 1) * 0.06;
            let minor = x % 32 == 0 || y % 32 == 0;
            let major = x < 2 || y < 2 || x > 253 || y > 253;
            let v = if major {
                0.58
            } else if minor {
                0.76
            } else {
                0.92
            };
            gray(v - n)
        });
        let crate_ = make_texture(128, |x, y| {
            let n = fbm(x as f32 * 2.0, y as f32 * 0.25, 128.0, 7) * 0.25;
            let border = x < 12 || y < 12 || x > 115 || y > 115;
            let plank = (y % 26) < 2;
            let d1 = (x as i32 - y as i32).abs() < 7;
            let d2 = (x as i32 + y as i32 - 127).abs() < 7;
            let v = if border {
                0.62 - n * 0.5
            } else if d1 || d2 {
                0.7 - n * 0.4
            } else if plank {
                0.45
            } else {
                0.85 - n
            };
            let edge = x == 12 || y == 12 || x == 115 || y == 115;
            gray(if edge { 0.35 } else { v })
        });
        let metal = make_texture(256, |x, y| {
            let n = fbm(x as f32, y as f32, 256.0, 3);
            let panel = x % 128 < 2 || y % 128 < 2;
            let rx = (x % 128) as i32;
            let ry = (y % 128) as i32;
            let rivet = [(10, 10), (117, 10), (10, 117), (117, 117)]
                .iter()
                .any(|(a, b)| (rx - a) * (rx - a) + (ry - b) * (ry - b) < 12);
            let v = if panel {
                0.5
            } else if rivet {
                0.95
            } else {
                0.72 + n * 0.18 + ((y % 128) as f32 / 128.0) * 0.06
            };
            gray(v)
        });
        let concrete = make_texture(256, |x, y| {
            let n = fbm(x as f32, y as f32, 256.0, 11);
            let seam = y % 128 < 2 || (x + if (y / 128) % 2 == 0 { 0 } else { 64 }) % 128 < 2;
            gray(if seam { 0.6 } else { 0.7 + n * 0.3 })
        });
        let water = make_texture(256, |x, y| {
            let n = fbm(x as f32, y as f32, 256.0, 21);
            let n2 = fbm(y as f32 + 50.0, x as f32, 256.0, 22);
            let v = (0.5 + 0.5 * ((n * 9.0 + n2 * 5.0).sin())).powf(1.5);
            gray(v)
        });
        let glow = make_texture(64, |x, y| {
            let dx = (x as f32 + 0.5) / 32.0 - 1.0;
            let dy = (y as f32 + 0.5) / 32.0 - 1.0;
            let r = (dx * dx + dy * dy).sqrt();
            let a = (1.0 - r).clamp(0.0, 1.0).powf(2.0);
            [255, 255, 255, (a * 255.0) as u8]
        });
        Textures {
            grid,
            crate_,
            metal,
            concrete,
            water,
            glow,
        }
    }

    fn for_tex(&self, t: Tex) -> &Texture2D {
        match t {
            Tex::Grid | Tex::Clip => &self.grid,
            Tex::Crate => &self.crate_,
            Tex::Metal => &self.metal,
            Tex::Concrete => &self.concrete,
            Tex::Water => &self.water,
        }
    }
}

// ---------------------------------------------------------------------------
// Geometry batching

fn rgba(c: Vec3, a: f32) -> [u8; 4] {
    [
        (c.x.clamp(0.0, 1.0) * 255.0) as u8,
        (c.y.clamp(0.0, 1.0) * 255.0) as u8,
        (c.z.clamp(0.0, 1.0) * 255.0) as u8,
        (a.clamp(0.0, 1.0) * 255.0) as u8,
    ]
}

pub fn shade(n: Vec3) -> f32 {
    0.58 + 0.42 * n.dot(SUN.normalize()).max(0.0) + 0.08 * n.z
}

#[derive(Default)]
pub struct Batch {
    verts: Vec<Vertex>,
    idx: Vec<u16>,
    tex: Option<Texture2D>,
}

impl Batch {
    fn vert(&mut self, p: Vec3, uv: Vec2, c: [u8; 4]) -> u16 {
        self.verts.push(Vertex {
            position: p,
            uv,
            color: c,
            normal: Vec4::ZERO,
        });
        (self.verts.len() - 1) as u16
    }

    fn reserve(&mut self, n: usize, _tex: Option<&Texture2D>) {
        if self.verts.len() + n > MAX_VERTS {
            let t = self.tex.clone();
            self.flush(t.as_ref());
        }
    }

    pub fn quad(&mut self, p: [Vec3; 4], uv: [Vec2; 4], c: [u8; 4]) {
        let a = self.vert(p[0], uv[0], c);
        let b = self.vert(p[1], uv[1], c);
        let cc = self.vert(p[2], uv[2], c);
        let d = self.vert(p[3], uv[3], c);
        self.idx.extend_from_slice(&[a, b, cc, a, cc, d]);
    }

    /// Unit cube [-0.5, 0.5]^3 transformed by `m`, lit per face.
    pub fn cube(&mut self, m: &Mat4, color: Vec3) {
        self.cube_a(m, color, 1.0, true);
    }

    pub fn cube_a(&mut self, m: &Mat4, color: Vec3, alpha: f32, lit: bool) {
        self.reserve(24, None);
        const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
            ([1.0, 0.0, 0.0], [[0.5, -0.5, -0.5], [0.5, 0.5, -0.5], [0.5, 0.5, 0.5], [0.5, -0.5, 0.5]]),
            ([-1.0, 0.0, 0.0], [[-0.5, 0.5, -0.5], [-0.5, -0.5, -0.5], [-0.5, -0.5, 0.5], [-0.5, 0.5, 0.5]]),
            ([0.0, 1.0, 0.0], [[0.5, 0.5, -0.5], [-0.5, 0.5, -0.5], [-0.5, 0.5, 0.5], [0.5, 0.5, 0.5]]),
            ([0.0, -1.0, 0.0], [[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5]]),
            ([0.0, 0.0, 1.0], [[-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5]]),
            ([0.0, 0.0, -1.0], [[-0.5, 0.5, -0.5], [0.5, 0.5, -0.5], [0.5, -0.5, -0.5], [-0.5, -0.5, -0.5]]),
        ];
        for (n, corners) in FACES.iter() {
            let wn = m.transform_vector3(Vec3::from(*n)).normalize_or_zero();
            let s = if lit { shade(wn) } else { 1.0 };
            let c = rgba(color * s, alpha);
            let p = corners.map(|q| m.transform_point3(Vec3::from(q)));
            self.quad(p, [Vec2::ZERO; 4], c);
        }
    }

    /// Box from a center, three half axes and a color.
    pub fn obox(&mut self, center: Vec3, ax: Vec3, ay: Vec3, az: Vec3, color: Vec3) {
        let m = Mat4::from_cols(
            (ax * 2.0).extend(0.0),
            (ay * 2.0).extend(0.0),
            (az * 2.0).extend(0.0),
            center.extend(1.0),
        );
        self.cube(&m, color);
    }

    /// Camera facing sprite (uses the glow texture).
    pub fn sprite(&mut self, pos: Vec3, right: Vec3, up: Vec3, size: f32, c: [u8; 4]) {
        self.reserve(4, None);
        let r = right * size;
        let u = up * size;
        self.quad(
            [pos - r - u, pos + r - u, pos + r + u, pos - r + u],
            [vec2(0.0, 1.0), vec2(1.0, 1.0), vec2(1.0, 0.0), vec2(0.0, 0.0)],
            c,
        );
    }

    /// A glowing line segment facing the camera.
    pub fn beam(&mut self, a: Vec3, b: Vec3, cam: Vec3, width: f32, c: [u8; 4]) {
        self.reserve(4, None);
        let dir = b - a;
        let side = dir.cross(cam - a).normalize_or_zero() * width;
        self.quad(
            [a - side, a + side, b + side, b - side],
            [vec2(0.0, 0.5), vec2(1.0, 0.5), vec2(1.0, 0.5), vec2(0.0, 0.5)],
            c,
        );
    }

    pub fn flush(&mut self, tex: Option<&Texture2D>) {
        if self.idx.is_empty() {
            self.verts.clear();
            return;
        }
        let mesh = Mesh {
            vertices: std::mem::take(&mut self.verts),
            indices: std::mem::take(&mut self.idx),
            texture: tex.cloned(),
        };
        draw_mesh(&mesh);
        self.verts = mesh.vertices;
        self.idx = mesh.indices;
        self.verts.clear();
        self.idx.clear();
    }
}

// ---------------------------------------------------------------------------
// Effects

struct Tracer {
    a: Vec3,
    b: Vec3,
    t0: f64,
}

struct Particle {
    pos: Vec3,
    vel: Vec3,
    t0: f64,
    life: f32,
    size: f32,
    color: Vec3,
    gravity: f32,
}

struct Decal {
    pos: Vec3,
    normal: Vec3,
    t0: f64,
}

struct Ring {
    pos: Vec3,
    t0: f64,
    color: Vec3,
}

pub struct Renderer {
    tex: Textures,
    world: Vec<(Tex, Vec<Mesh>)>,
    world_mat: Material,
    fx_mat: Material,
    alpha_mat: Material,
    sky: Mesh,
    tracers: Vec<Tracer>,
    particles: Vec<Particle>,
    decals: VecDeque<Decal>,
    rings: Vec<Ring>,
    trails: Vec<VecDeque<(Vec3, f64)>>,
    solid: Batch,
    fx: Batch,
    decal: Batch,
    rng: Rng,
    time: f64,
}

pub struct View {
    pub pos: Vec3,
    pub angles: Vec3,
    pub fov: f32,
    /// Player whose eyes we are looking through (hidden, draws the view model).
    pub first_person: Option<usize>,
}

pub fn team_color(t: Team) -> Vec3 {
    match t {
        Team::T => vec3(1.0, 0.45, 0.15),
        Team::CT => vec3(0.2, 0.6, 1.0),
    }
}

impl Renderer {
    pub fn new(game: &Game) -> Renderer {
        let tex = Textures::new();
        let world_mat = load_material(
            ShaderSource::Glsl {
                vertex: WORLD_VS,
                fragment: WORLD_FS,
            },
            MaterialParams {
                pipeline_params: PipelineParams {
                    depth_test: Comparison::LessOrEqual,
                    depth_write: true,
                    cull_face: CullFace::Nothing,
                    color_blend: Some(BlendState::new(
                        Equation::Add,
                        BlendFactor::Value(BlendValue::SourceAlpha),
                        BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                    )),
                    ..Default::default()
                },
                uniforms: vec![
                    UniformDesc::new("CamPos", UniformType::Float3),
                    UniformDesc::new("FogColor", UniformType::Float3),
                    UniformDesc::new("FogParams", UniformType::Float4),
                ],
                textures: vec![],
            },
        )
        .expect("world shader");
        let fx_mat = load_material(
            ShaderSource::Glsl {
                vertex: FX_VS,
                fragment: FX_FS,
            },
            MaterialParams {
                pipeline_params: PipelineParams {
                    depth_test: Comparison::LessOrEqual,
                    depth_write: false,
                    cull_face: CullFace::Nothing,
                    color_blend: Some(BlendState::new(
                        Equation::Add,
                        BlendFactor::Value(BlendValue::SourceAlpha),
                        BlendFactor::One,
                    )),
                    ..Default::default()
                },
                uniforms: vec![],
                textures: vec![],
            },
        )
        .expect("fx shader");
        let alpha_mat = load_material(
            ShaderSource::Glsl {
                vertex: FX_VS,
                fragment: FX_FS,
            },
            MaterialParams {
                pipeline_params: PipelineParams {
                    depth_test: Comparison::LessOrEqual,
                    depth_write: false,
                    cull_face: CullFace::Nothing,
                    color_blend: Some(BlendState::new(
                        Equation::Add,
                        BlendFactor::Value(BlendValue::SourceAlpha),
                        BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                    )),
                    ..Default::default()
                },
                uniforms: vec![],
                textures: vec![],
            },
        )
        .expect("alpha shader");

        let mut r = Renderer {
            world: Vec::new(),
            sky: build_sky(game),
            tex,
            world_mat,
            fx_mat,
            alpha_mat,
            tracers: Vec::new(),
            particles: Vec::new(),
            decals: VecDeque::new(),
            rings: Vec::new(),
            trails: Vec::new(),
            solid: Batch::default(),
            fx: Batch::default(),
            decal: Batch::default(),
            rng: Rng::new(99),
            time: 0.0,
        };
        r.fx.tex = Some(r.tex.glow.clone());
        r.decal.tex = Some(r.tex.glow.clone());
        r.build_world(game);
        r
    }

    fn build_world(&mut self, game: &Game) {
        let mut groups: Vec<(Tex, Vec<Vertex>, Vec<u16>, Vec<Mesh>)> = Vec::new();
        for b in &game.map.world.brushes {
            if !b.mat.visible() {
                continue;
            }
            let gi = match groups.iter().position(|g| g.0 == b.mat.tex) {
                Some(i) => i,
                None => {
                    groups.push((b.mat.tex, Vec::new(), Vec::new(), Vec::new()));
                    groups.len() - 1
                }
            };
            let base = vec3(b.mat.color[0] as f32, b.mat.color[1] as f32, b.mat.color[2] as f32) / 255.0;
            for f in &b.faces {
                let n = f.normal;
                // Skip faces nobody can see (the underside of the floor etc).
                if b.mat.tex == Tex::Water && n.z < 0.9 {
                    continue;
                }
                let t = if n.z.abs() < 0.99 {
                    Vec3::Z.cross(n).normalize()
                } else {
                    Vec3::X
                };
                let bt = n.cross(t);
                let (scale, fit) = match b.mat.tex {
                    Tex::Crate => (1.0, true),
                    Tex::Water => (1.0 / 700.0, false),
                    _ => (1.0 / 256.0, false),
                };
                let (mut umin, mut vmin, mut umax, mut vmax) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for v in &f.verts {
                    umin = umin.min(v.dot(t));
                    umax = umax.max(v.dot(t));
                    vmin = vmin.min(v.dot(bt));
                    vmax = vmax.max(v.dot(bt));
                }
                let mut s = shade(n);
                // the two faces of a surf ramp read better with a little contrast
                if n.z > 0.2 && n.z < 0.8 {
                    s *= if n.x + n.y > 0.0 { 1.0 } else { 0.88 };
                }
                let flag = if b.mat.tex == Tex::Water { 1.0 } else { 0.0 };
                let g = &mut groups[gi];
                if g.1.len() + f.verts.len() > MAX_VERTS {
                    let verts = std::mem::take(&mut g.1);
                    let idx = std::mem::take(&mut g.2);
                    g.3.push(Mesh {
                        vertices: verts,
                        indices: idx,
                        texture: None,
                    });
                }
                let start = g.1.len() as u16;
                for v in &f.verts {
                    // Darken toward the bottom of the pit.
                    let depth = ((v.z - 100.0) / 2200.0).clamp(0.0, 1.0);
                    let c = base * s * (0.72 + 0.28 * depth);
                    let uv = if fit {
                        vec2(
                            (v.dot(t) - umin) / (umax - umin).max(1.0),
                            (v.dot(bt) - vmin) / (vmax - vmin).max(1.0),
                        )
                    } else {
                        vec2(v.dot(t) * scale, -v.dot(bt) * scale)
                    };
                    g.1.push(Vertex {
                        position: *v,
                        uv,
                        color: rgba(c, 1.0),
                        normal: vec4(n.x, n.y, n.z, flag),
                    });
                }
                for i in 1..(f.verts.len() - 1) {
                    g.2.extend_from_slice(&[start, start + i as u16, start + i as u16 + 1]);
                }
            }
        }
        for (tex, verts, idx, mut meshes) in groups {
            if !idx.is_empty() {
                meshes.push(Mesh {
                    vertices: verts,
                    indices: idx,
                    texture: None,
                });
            }
            let t = self.tex.for_tex(tex).clone();
            for m in meshes.iter_mut() {
                m.texture = Some(t.clone());
            }
            self.world.push((tex, meshes));
        }
    }

    /// Feed game events into the effect systems.
    pub fn handle_events(&mut self, game: &Game, events: &[Event]) {
        let now = game.time;
        for e in events {
            match e {
                Event::Tracer { start, end } => {
                    if self.rng.chance(0.5) {
                        self.tracers.push(Tracer {
                            a: *start,
                            b: *end,
                            t0: now,
                        });
                    }
                }
                Event::Impact { pos, normal } => {
                    self.decals.push_back(Decal {
                        pos: *pos + *normal * 0.6,
                        normal: *normal,
                        t0: now,
                    });
                    if self.decals.len() > 150 {
                        self.decals.pop_front();
                    }
                    for _ in 0..6 {
                        let v = (*normal + vec3(self.rng.range(-0.7, 0.7), self.rng.range(-0.7, 0.7), self.rng.range(-0.3, 0.9)))
                            * self.rng.range(120.0, 320.0);
                        self.particles.push(Particle {
                            pos: *pos,
                            vel: v,
                            t0: now,
                            life: self.rng.range(0.15, 0.35),
                            size: 1.4,
                            color: vec3(1.0, 0.8, 0.4),
                            gravity: 800.0,
                        });
                    }
                }
                Event::Hit { pos, headshot, .. } => {
                    let n = if *headshot { 14 } else { 8 };
                    for _ in 0..n {
                        let v = vec3(self.rng.range(-1.0, 1.0), self.rng.range(-1.0, 1.0), self.rng.range(-0.2, 1.0))
                            * self.rng.range(60.0, 220.0);
                        self.particles.push(Particle {
                            pos: *pos,
                            vel: v,
                            t0: now,
                            life: self.rng.range(0.3, 0.6),
                            size: self.rng.range(1.5, 3.0),
                            color: vec3(0.55, 0.02, 0.02),
                            gravity: 600.0,
                        });
                    }
                }
                Event::Teleport { player } => {
                    let p = &game.players[*player];
                    self.rings.push(Ring {
                        pos: p.pm.feet(),
                        t0: now,
                        color: team_color(p.team),
                    });
                    if let Some(t) = self.trails.get_mut(*player) {
                        t.clear();
                    }
                }
                Event::RoundStart => {
                    self.decals.clear();
                    for t in self.trails.iter_mut() {
                        t.clear();
                    }
                }
                _ => {}
            }
        }
    }

    pub fn draw(&mut self, game: &Game, view: &View, alpha: f32, show_viewmodel: bool) {
        self.time = game.time;
        let (fwd, right, up) = angle_vectors(view.angles);
        let cam = Camera3D {
            position: view.pos,
            target: view.pos + fwd,
            up,
            fovy: vfov(view.fov),
            z_near: 2.0,
            z_far: 40000.0,
            ..Default::default()
        };
        clear_background(Color::new(
            game.map.fog_color[0],
            game.map.fog_color[1],
            game.map.fog_color[2],
            1.0,
        ));
        set_camera(&cam);

        // Sky dome follows the camera.
        {
            let mut sky = Mesh {
                vertices: self.sky.vertices.clone(),
                indices: self.sky.indices.clone(),
                texture: None,
            };
            for v in sky.vertices.iter_mut() {
                v.position += view.pos;
            }
            gl_use_default_material();
            draw_mesh(&sky);
            // sun
            gl_use_material(&self.fx_mat);
            let sun_pos = view.pos + SUN.normalize() * 30000.0;
            let (_, sr, su) = angle_vectors(view.angles);
            self.fx.sprite(sun_pos, sr, su, 2600.0, [255, 250, 225, 255]);
            self.fx.sprite(sun_pos, sr, su, 7000.0, [255, 235, 190, 90]);
            self.fx.flush(Some(&self.tex.glow));
        }

        // World.
        let fog = game.map.fog_color;
        self.world_mat.set_uniform("CamPos", view.pos);
        self.world_mat.set_uniform("FogColor", vec3(fog[0], fog[1], fog[2]));
        self.world_mat
            .set_uniform("FogParams", vec4(game.map.fog_start, game.map.fog_end, game.time as f32, 900.0));
        gl_use_material(&self.world_mat);
        for (_, meshes) in &self.world {
            for m in meshes {
                draw_mesh(m);
            }
        }

        // Players and dropped weapons.
        for (i, p) in game.players.iter().enumerate() {
            if !p.alive && game.time - p.death_time > 3.0 {
                continue;
            }
            if view.first_person == Some(i) {
                continue;
            }
            let pos = p.prev_origin.lerp(p.pm.origin, alpha);
            draw_player(&mut self.solid, p, pos, game.time);
        }
        for d in &game.dropped {
            let m = Mat4::from_translation(d.pos) * Mat4::from_rotation_z(d.yaw.to_radians());
            draw_weapon_model(&mut self.solid, d.weapon.id, &m);
        }
        self.solid.flush(None);

        // Decals (alpha blended).
        gl_use_material(&self.alpha_mat);
        for d in &self.decals {
            let age = (game.time - d.t0) as f32;
            let a = (1.0 - (age - 12.0).max(0.0) / 3.0).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let t = if d.normal.z.abs() < 0.9 { Vec3::Z.cross(d.normal).normalize() } else { Vec3::X };
            let b = d.normal.cross(t);
            self.decal.sprite(d.pos, t, b, 2.6, [20, 18, 16, (a * 230.0) as u8]);
        }
        self.decal.flush(Some(&self.tex.glow));
        while self.decals.front().map_or(false, |d| game.time - d.t0 > 15.0) {
            self.decals.pop_front();
        }

        gl_use_material(&self.world_mat);
        // Boards (also our own when surfing, you can see it below you).
        for (i, p) in game.players.iter().enumerate() {
            if !p.alive || p.board < 0.02 {
                continue;
            }
            let pos = p.prev_origin.lerp(p.pm.origin, alpha);
            draw_board(&mut self.solid, &mut self.fx, p, pos, game.time, view.pos, view.first_person == Some(i));
        }
        self.solid.flush(None);

        // Additive effects.
        gl_use_material(&self.fx_mat);
        self.update_trails(game, alpha);
        for (i, trail) in self.trails.iter().enumerate() {
            let Some(p) = game.players.get(i) else { continue };
            let c = team_color(p.team);
            let pts: Vec<&(Vec3, f64)> = trail.iter().collect();
            for w in pts.windows(2) {
                let age = (game.time - w[0].1) as f32;
                let a = (1.0 - age / 0.8).clamp(0.0, 1.0);
                if a <= 0.01 {
                    continue;
                }
                self.fx.beam(w[0].0, w[1].0, view.pos, 3.0 + age * 6.0, rgba(c, a * 0.55));
            }
        }
        let now = game.time;
        self.tracers.retain(|t| now - t.t0 < 0.12);
        for t in &self.tracers {
            let k = ((now - t.t0) / 0.12) as f32;
            let dir = t.b - t.a;
            let len = dir.length();
            let d = dir / len.max(1.0);
            let head = t.a + d * (len * (k + 0.25)).min(len);
            let tail = t.a + d * (len * k).min(len);
            self.fx.beam(tail, head, view.pos, 0.7, [255, 230, 150, 200]);
        }
        let dt = get_frame_time().min(0.05);
        self.particles.retain(|p| (now - p.t0) < p.life as f64);
        for p in self.particles.iter_mut() {
            p.vel.z -= p.gravity * dt;
            p.pos += p.vel * dt;
        }
        for p in &self.particles {
            let k = ((now - p.t0) as f32 / p.life).clamp(0.0, 1.0);
            let a = 1.0 - k;
            self.fx.sprite(p.pos, right, up, p.size * (1.0 + k), rgba(p.color, a));
        }
        self.rings.retain(|r| now - r.t0 < 0.7);
        for r in &self.rings {
            let k = ((now - r.t0) / 0.7) as f32;
            let rad = 20.0 + k * 70.0;
            let segs = 20;
            for s in 0..segs {
                let a0 = s as f32 / segs as f32 * std::f32::consts::TAU;
                let a1 = (s + 1) as f32 / segs as f32 * std::f32::consts::TAU;
                let p0 = r.pos + vec3(a0.cos(), a0.sin(), 0.0) * rad + vec3(0.0, 0.0, k * 40.0);
                let p1 = r.pos + vec3(a1.cos(), a1.sin(), 0.0) * rad + vec3(0.0, 0.0, k * 40.0);
                self.fx.beam(p0, p1, view.pos, 3.0, rgba(r.color, 1.0 - k));
            }
        }
        // Muzzle flashes on other players.
        for (i, p) in game.players.iter().enumerate() {
            if !p.alive || view.first_person == Some(i) {
                continue;
            }
            if game.time - p.last_fire < 0.05 && p.active_id() != WeaponId::Knife {
                let pos = p.prev_origin.lerp(p.pm.origin, alpha);
                let (f, _, _) = angle_vectors(p.angles);
                let tip = muzzle_world(p, pos) + f * 4.0;
                self.fx.sprite(tip, right, up, 9.0, [255, 200, 90, 255]);
            }
        }
        self.fx.flush(Some(&self.tex.glow));

        // First person weapon, drawn over everything.
        if show_viewmodel {
            if let Some(i) = view.first_person {
                let p = &game.players[i];
                if p.alive && p.zoom == 0 {
                    clear_depth();
                    gl_use_material(&self.world_mat);
                    draw_viewmodel(&mut self.solid, &mut self.fx, p, view, game.time, &self.tex.glow, &self.fx_mat);
                }
            }
        }
        gl_use_default_material();
        set_default_camera();
    }

    fn update_trails(&mut self, game: &Game, alpha: f32) {
        while self.trails.len() < game.players.len() {
            self.trails.push(VecDeque::new());
        }
        for (i, p) in game.players.iter().enumerate() {
            let t = &mut self.trails[i];
            while t.front().map_or(false, |(_, t0)| game.time - t0 > 0.8) {
                t.pop_front();
            }
            if p.alive && p.board > 0.5 && horizontal(p.pm.velocity).length() > 300.0 {
                let pos = p.prev_origin.lerp(p.pm.origin, alpha);
                let (tail, _) = board_frame(p, pos);
                let tail = tail - board_forward(p) * 26.0;
                if t.back().map_or(true, |(q, _)| q.distance(tail) > 12.0) {
                    t.push_back((tail, game.time));
                }
            }
        }
    }
}

fn vfov(hfov_deg: f32) -> f32 {
    // CS uses a horizontal FOV of 90 at 4:3. Keep that feel on wide screens
    // by deriving the vertical FOV from the 4:3 horizontal one.
    let h = (hfov_deg.to_radians() * 0.5).tan();
    2.0 * (h * 0.75).atan()
}

fn clear_depth() {
    unsafe {
        let mut gl = get_internal_gl();
        gl.flush();
        gl.quad_context.begin_default_pass(PassAction::Clear {
            color: None,
            depth: Some(1.0),
            stencil: None,
        });
        gl.quad_context.end_render_pass();
    }
}

fn build_sky(game: &Game) -> Mesh {
    let top = Vec3::from(game.map.sky_top);
    let hor = Vec3::from(game.map.sky_horizon);
    let fog = Vec3::from(game.map.fog_color);
    let r = 30000.0;
    let rings = 12;
    let segs = 24;
    let mut verts = Vec::new();
    let mut idx: Vec<u16> = Vec::new();
    for i in 0..=rings {
        let el = -0.35 + (i as f32 / rings as f32) * (std::f32::consts::FRAC_PI_2 + 0.35);
        for j in 0..=segs {
            let az = j as f32 / segs as f32 * std::f32::consts::TAU;
            let dir = vec3(el.cos() * az.cos(), el.cos() * az.sin(), el.sin());
            let t = (el / std::f32::consts::FRAC_PI_2).clamp(0.0, 1.0).powf(0.6);
            let c = if el < 0.0 { fog.lerp(hor, 1.0 + el / 0.35) } else { hor.lerp(top, t) };
            verts.push(Vertex {
                position: dir * r,
                uv: Vec2::ZERO,
                color: rgba(c, 1.0),
                normal: Vec4::ZERO,
            });
        }
    }
    for i in 0..rings {
        for j in 0..segs {
            let a = (i * (segs + 1) + j) as u16;
            let b = a + (segs + 1) as u16;
            idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    Mesh {
        vertices: verts,
        indices: idx,
        texture: None,
    }
}

// ---------------------------------------------------------------------------
// Player models

fn lean_rotation(p: &Player) -> Quat {
    let target = Vec3::Z.lerp(p.board_normal, 0.55 * p.board).normalize();
    Quat::from_rotation_arc(Vec3::Z, target)
}

fn board_forward(p: &Player) -> Vec3 {
    let n = p.board_normal;
    let v = p.pm.velocity - n * p.pm.velocity.dot(n);
    if v.length() > 40.0 {
        v.normalize()
    } else {
        let (f, _, _) = angle_vectors(vec3(0.0, p.angles.y, 0.0));
        (f - n * f.dot(n)).normalize_or_zero()
    }
}

/// Board center and up vector.
fn board_frame(p: &Player, pos: Vec3) -> (Vec3, Vec3) {
    let feet = pos + vec3(0.0, 0.0, p.pm.mins().z);
    let n = p.board_normal;
    let bob = (p.spawn_count as f32 + p.pm.origin.x * 0.01).sin() * 0.0;
    (feet - n * (4.5 + bob), n)
}

fn team_palette(t: Team) -> (Vec3, Vec3, Vec3, Vec3) {
    // (shirt, vest, pants, head gear)
    match t {
        Team::T => (
            vec3(0.62, 0.50, 0.34),
            vec3(0.45, 0.20, 0.16),
            vec3(0.33, 0.28, 0.22),
            vec3(0.18, 0.16, 0.15),
        ),
        Team::CT => (
            vec3(0.25, 0.32, 0.45),
            vec3(0.16, 0.20, 0.28),
            vec3(0.20, 0.23, 0.27),
            vec3(0.22, 0.26, 0.30),
        ),
    }
}

fn muzzle_world(p: &Player, pos: Vec3) -> Vec3 {
    let feet = pos + vec3(0.0, 0.0, p.pm.mins().z);
    let crouch = if p.pm.ducking { 1.0 } else { 0.0 };
    let yaw = Quat::from_rotation_z(p.angles.y.to_radians());
    let rot = lean_rotation(p) * yaw;
    feet + rot * vec3(26.0, -4.0, 50.0 - crouch * 16.0)
}

pub fn draw_player(b: &mut Batch, p: &Player, pos: Vec3, time: f64) {
    let feet = pos + vec3(0.0, 0.0, p.pm.mins().z);
    let (shirt, vest, pants, gear) = team_palette(p.team);
    let skin = vec3(0.78, 0.6, 0.48);
    let boots = vec3(0.12, 0.11, 0.1);

    let dead_t = if p.alive { 0.0 } else { ((time - p.death_time) as f32 / 0.5).clamp(0.0, 1.0) };
    let yaw = Quat::from_rotation_z(p.angles.y.to_radians());
    let mut rot = lean_rotation(p) * yaw;
    if dead_t > 0.0 {
        // topple over backwards
        rot = rot * Quat::from_rotation_y(-dead_t * 1.45);
    }
    let base = Mat4::from_rotation_translation(rot, feet);
    let part = |b: &mut Batch, center: Vec3, size: Vec3, extra: Quat, color: Vec3| {
        let m = base * Mat4::from_rotation_translation(extra, center) * Mat4::from_scale(size);
        b.cube(&m, color);
    };

    let speed = horizontal(p.pm.velocity).length();
    let surfing = p.board > 0.3;
    let onground = p.pm.onground;
    let crouch = if p.pm.ducking { 1.0 } else { 0.0 };
    let phase = (time as f32) * (speed / 250.0 * 9.0);
    let swing = if onground && speed > 20.0 && !surfing { phase.sin() * 0.55 * (speed / 250.0).min(1.0) } else { 0.0 };

    let leg_len = 34.0 - crouch * 17.0;
    let hip = leg_len;
    if surfing {
        // Surf stance: feet apart along the board, knees bent.
        let spread = 9.0;
        let bend = 0.25;
        part(b, vec3(spread, -1.0, leg_len * 0.5 - 2.0), vec3(7.0, 7.0, leg_len), Quat::from_rotation_y(bend), pants);
        part(b, vec3(-spread, 1.0, leg_len * 0.5 - 2.0), vec3(7.0, 7.0, leg_len), Quat::from_rotation_y(-bend), pants);
        part(b, vec3(spread + 2.0, -1.0, 2.0), vec3(10.0, 7.0, 4.0), Quat::IDENTITY, boots);
        part(b, vec3(-spread + 2.0, 1.0, 2.0), vec3(10.0, 7.0, 4.0), Quat::IDENTITY, boots);
    } else if !onground && !surfing {
        // Airborne: tuck the legs a little.
        for s in [-1.0f32, 1.0] {
            part(b, vec3(2.0, s * 5.0, hip - leg_len * 0.45), vec3(7.0, 7.0, leg_len * 0.9), Quat::from_rotation_y(0.35), pants);
            part(b, vec3(6.0, s * 5.0, hip - leg_len * 0.85), vec3(10.0, 7.0, 4.0), Quat::IDENTITY, boots);
        }
    } else {
        for s in [-1.0f32, 1.0] {
            let a = swing * s;
            let q = Quat::from_rotation_y(a);
            let knee = vec3(0.0, s * 5.0, hip) + q * vec3(0.0, 0.0, -leg_len * 0.5);
            part(b, knee, vec3(7.0, 7.0, leg_len), q, pants);
            let foot = vec3(0.0, s * 5.0, hip) + q * vec3(2.5, 0.0, -leg_len + 2.0);
            part(b, foot, vec3(10.0, 7.0, 4.0), q, boots);
        }
    }

    let torso_h = 26.0 - crouch * 4.0;
    let hunch = Quat::from_rotation_y(crouch * 0.35 + if surfing { 0.18 } else { 0.0 });
    let torso_c = vec3(0.0, 0.0, hip + torso_h * 0.5);
    part(b, torso_c, vec3(10.0, 18.0, torso_h), hunch, shirt);
    part(b, torso_c + vec3(0.6, 0.0, 1.0), vec3(11.0, 16.0, torso_h * 0.7), hunch, vest);
    let neck = torso_c + hunch * vec3(0.0, 0.0, torso_h * 0.5);
    let head_c = neck + vec3(0.0, 0.0, 6.0);
    let pitch_q = Quat::from_rotation_y(p.angles.x.to_radians() * 0.5);
    part(b, head_c, vec3(10.0, 9.0, 11.0), pitch_q, skin);
    match p.team {
        Team::T => {
            part(b, head_c + vec3(0.0, 0.0, 3.5), vec3(11.0, 10.0, 5.0), pitch_q, gear);
            part(b, head_c + vec3(-1.0, 0.0, -1.5), vec3(9.0, 10.0, 5.0), pitch_q, vec3(0.5, 0.12, 0.1));
        }
        Team::CT => {
            part(b, head_c + vec3(-0.5, 0.0, 4.0), vec3(12.0, 11.0, 5.0), pitch_q, gear);
            part(b, head_c + pitch_q * vec3(5.2, 0.0, 1.0), vec3(1.5, 8.0, 2.5), pitch_q, vec3(0.1, 0.12, 0.14));
        }
    }

    // Arms and weapon, aimed with the view pitch.
    let aim = Quat::from_rotation_y(p.angles.x.to_radians());
    let shoulder = neck + vec3(0.0, 0.0, -3.0);
    let gun_base = shoulder + aim * vec3(10.0, -4.0, -3.0);
    for s in [-1.0f32, 1.0] {
        let sh = shoulder + vec3(0.0, s * 10.0, 0.0);
        let hand = gun_base + aim * vec3(if s < 0.0 { 2.0 } else { 9.0 }, 0.0, -1.0);
        let mid = (sh + hand) * 0.5;
        let dir = (hand - sh).normalize_or_zero();
        let q = Quat::from_rotation_arc(Vec3::X, dir);
        part(b, mid, vec3((hand - sh).length(), 5.0, 5.0), q, shirt * 0.9);
        part(b, hand, vec3(4.0, 4.0, 4.0), Quat::IDENTITY, boots);
    }
    let wm = base * Mat4::from_rotation_translation(aim, gun_base);
    draw_weapon_model(b, p.active_id(), &wm);
}

/// Weapon model in local space: x forward, z up, origin at the grip.
pub fn draw_weapon_model(b: &mut Batch, id: WeaponId, m: &Mat4) {
    let black = vec3(0.12, 0.12, 0.13);
    let dark = vec3(0.22, 0.22, 0.24);
    let wood = vec3(0.45, 0.27, 0.13);
    let green = vec3(0.28, 0.36, 0.22);
    let steel = vec3(0.7, 0.72, 0.75);
    let mut part = |c: Vec3, s: Vec3, col: Vec3| {
        let mm = *m * Mat4::from_translation(c) * Mat4::from_scale(s);
        b.cube(&mm, col);
    };
    match id {
        WeaponId::Knife => {
            part(vec3(0.0, 0.0, 0.0), vec3(4.0, 1.2, 1.4), black);
            part(vec3(6.5, 0.0, 0.3), vec3(9.0, 0.4, 1.8), steel);
        }
        WeaponId::Usp => {
            part(vec3(3.0, 0.0, 1.8), vec3(8.0, 1.5, 1.8), dark);
            part(vec3(0.5, 0.0, -1.0), vec3(2.2, 1.3, 4.0), black);
        }
        WeaponId::Mp5 => {
            part(vec3(3.0, 0.0, 1.5), vec3(11.0, 2.0, 2.6), black);
            part(vec3(10.5, 0.0, 1.3), vec3(5.0, 2.2, 2.2), dark);
            part(vec3(14.0, 0.0, 1.6), vec3(3.0, 0.8, 0.8), black);
            part(vec3(4.5, 0.0, -2.5), vec3(2.2, 1.2, 5.5), black);
            part(vec3(-5.0, 0.0, 1.0), vec3(6.0, 1.0, 2.0), dark);
        }
        WeaponId::M3 => {
            part(vec3(2.0, 0.0, 1.5), vec3(8.0, 1.8, 2.6), black);
            part(vec3(14.0, 0.0, 2.2), vec3(20.0, 1.1, 1.1), dark);
            part(vec3(12.0, 0.0, 0.8), vec3(16.0, 1.3, 1.3), dark);
            part(vec3(10.0, 0.0, 0.8), vec3(6.0, 2.3, 2.3), black);
            part(vec3(-7.0, 0.0, 0.3), vec3(10.0, 1.6, 3.2), black);
        }
        WeaponId::Ak47 => {
            part(vec3(3.0, 0.0, 1.5), vec3(11.0, 1.8, 2.6), dark);
            part(vec3(11.0, 0.0, 1.2), vec3(6.0, 2.0, 2.0), wood);
            part(vec3(17.0, 0.0, 1.8), vec3(7.0, 0.8, 0.8), black);
            part(vec3(5.0, 0.0, -2.3), vec3(2.4, 1.3, 4.5), vec3(0.5, 0.3, 0.15));
            part(vec3(6.2, 0.0, -5.0), vec3(2.4, 1.3, 2.5), vec3(0.5, 0.3, 0.15));
            part(vec3(-6.0, 0.0, 0.4), vec3(10.0, 1.6, 2.8), wood);
        }
        WeaponId::Scout => {
            part(vec3(2.0, 0.0, 1.5), vec3(10.0, 1.8, 2.2), dark);
            part(vec3(16.0, 0.0, 1.9), vec3(18.0, 0.8, 0.8), black);
            part(vec3(3.0, 0.0, 4.0), vec3(9.0, 1.6, 1.6), black);
            part(vec3(-7.0, 0.0, 0.5), vec3(11.0, 1.6, 3.0), vec3(0.3, 0.33, 0.28));
        }
        WeaponId::Awp => {
            part(vec3(3.0, 0.0, 1.4), vec3(16.0, 2.6, 3.0), green);
            part(vec3(20.0, 0.0, 1.8), vec3(20.0, 1.1, 1.1), black);
            part(vec3(3.0, 0.0, 4.6), vec3(12.0, 2.0, 2.0), black);
            part(vec3(-9.0, 0.0, 0.2), vec3(12.0, 2.4, 4.0), green);
            part(vec3(4.0, 0.0, -2.0), vec3(2.4, 1.4, 3.2), black);
        }
    }
}

fn draw_board(b: &mut Batch, fx: &mut Batch, p: &Player, pos: Vec3, time: f64, cam: Vec3, own: bool) {
    let (center, n) = board_frame(p, pos);
    let f = board_forward(p);
    let s = n.cross(f).normalize_or_zero();
    let k = p.board;
    let len = 34.0 * (0.6 + 0.4 * k);
    let wid = 12.5 * (0.6 + 0.4 * k);
    let th = 1.8;
    let tc = team_color(p.team);
    let deck = vec3(0.1, 0.1, 0.12);
    // Outline of the deck: pointed nose, rounded tail.
    let outline: [(f32, f32); 8] = [
        (1.0, 0.0),
        (0.72, 0.62),
        (0.1, 1.0),
        (-0.75, 0.9),
        (-1.0, 0.0),
        (-0.75, -0.9),
        (0.1, -1.0),
        (0.72, -0.62),
    ];
    let pt = |u: f32, v: f32, h: f32| center + f * (u * len) + s * (v * wid) + n * h;
    let top_c = rgba(deck * shade(n), 1.0);
    let side_c = rgba(tc * 0.8, 1.0);
    let bot_c = rgba(tc * 1.2 + vec3(0.2, 0.2, 0.2), 1.0);
    b.reserve(40, None);
    let c0 = pt(0.0, 0.0, th);
    let cb = pt(0.0, 0.0, -th);
    for i in 0..outline.len() {
        let (u0, v0) = outline[i];
        let (u1, v1) = outline[(i + 1) % outline.len()];
        let a = pt(u0, v0, th);
        let c = pt(u1, v1, th);
        let ia = b.vert(c0, Vec2::ZERO, top_c);
        let ib = b.vert(a, Vec2::ZERO, top_c);
        let ic = b.vert(c, Vec2::ZERO, top_c);
        b.idx.extend_from_slice(&[ia, ib, ic]);
        let a2 = pt(u0, v0, -th);
        let c2 = pt(u1, v1, -th);
        let ja = b.vert(cb, Vec2::ZERO, bot_c);
        let jb = b.vert(c2, Vec2::ZERO, bot_c);
        let jc = b.vert(a2, Vec2::ZERO, bot_c);
        b.idx.extend_from_slice(&[ja, jb, jc]);
        b.quad([a2, c2, c, a], [Vec2::ZERO; 4], side_c);
    }
    // Stripe down the middle of the deck.
    let st = rgba(tc, 1.0);
    b.quad(
        [pt(0.85, -0.12, th + 0.1), pt(0.85, 0.12, th + 0.1), pt(-0.9, 0.12, th + 0.1), pt(-0.9, -0.12, th + 0.1)],
        [Vec2::ZERO; 4],
        st,
    );

    // Hover glow under the board and a thruster at the tail.
    let pulse = 0.8 + 0.2 * ((time * 14.0) as f32 + p.spawn_count as f32).sin();
    let under = center - n * 3.0;
    let ga = (k * pulse * if own { 0.5 } else { 0.8 }).clamp(0.0, 1.0);
    fx.reserve(8, None);
    fx.quad(
        [
            under + f * len * 1.5 + s * wid * 2.4,
            under - f * len * 1.5 + s * wid * 2.4,
            under - f * len * 1.5 - s * wid * 2.4,
            under + f * len * 1.5 - s * wid * 2.4,
        ],
        [vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0)],
        rgba(tc, ga),
    );
    let tail = center - f * len * 1.05;
    let right = (tail - cam).cross(n).normalize_or_zero();
    fx.sprite(tail, right, n, 7.0 * pulse, rgba(tc + vec3(0.3, 0.3, 0.3), ga));
}

// ---------------------------------------------------------------------------
// View model

#[allow(clippy::too_many_arguments)]
fn draw_viewmodel(b: &mut Batch, fx: &mut Batch, p: &Player, view: &View, time: f64, glow: &Texture2D, fx_mat: &Material) {
    let (f, r, u) = angle_vectors(view.angles);
    let l = -r;
    let id = p.active_id();
    let now = time;

    // Bob.
    let speed = horizontal(p.pm.velocity).length();
    let bob_amt = if p.pm.onground { (speed / 250.0).min(1.0) } else { 0.0 };
    let bt = (now as f32) * 11.0;
    let bob = vec3(0.0, (bt * 0.5).sin() * 0.6, bt.sin().abs() * -0.7) * bob_amt;

    // Firing kick.
    let tf = (now - p.last_fire) as f32;
    let kick = (-tf * 16.0).exp() * if tf >= 0.0 { 1.0 } else { 0.0 };
    let kick_amt = match id {
        WeaponId::Awp | WeaponId::Scout | WeaponId::M3 => 2.5,
        WeaponId::Knife => 0.0,
        _ => 1.0,
    };

    // Reload dip and deploy raise.
    let mut dip = 0.0;
    let mut roll = 0.0;
    if let Some(end) = p.reload_end {
        let total = if id == WeaponId::M3 { 0.5 } else { id.def().reload_time };
        let k = (1.0 - ((end - now) as f32 / total)).clamp(0.0, 1.0);
        let s = (k * std::f32::consts::PI).sin();
        dip = s * 7.0;
        roll = s * 0.6;
    }
    let td = (now - p.deploy_time) as f32;
    if td < 0.45 {
        dip += (1.0 - td / 0.45).powi(2) * 14.0;
    }

    // Knife swing.
    let mut swing = 0.0;
    if id == WeaponId::Knife && tf < 0.35 {
        swing = (tf / 0.35 * std::f32::consts::PI).sin();
    }

    let local = vec3(18.0 - kick * kick_amt * 2.0, -6.5, -7.0 - dip) + bob;
    let origin = view.pos + f * local.x + l * local.y + u * local.z;
    let basis = Mat4::from_cols(f.extend(0.0), l.extend(0.0), u.extend(0.0), origin.extend(1.0));
    let extra = Quat::from_rotation_y(-(kick * kick_amt * 0.12)) * Quat::from_rotation_x(roll)
        * Quat::from_rotation_z(swing * 0.9)
        * Quat::from_rotation_x(swing * 0.6);
    let m = basis * Mat4::from_quat(extra);

    let (shirt, _, _, _) = team_palette(p.team);
    let glove = vec3(0.12, 0.11, 0.1);
    // Arms reaching in from the bottom of the screen.
    let arm = |b: &mut Batch, from: Vec3, to: Vec3, color: Vec3, thick: f32| {
        let mid = (from + to) * 0.5;
        let dir = (to - from).normalize_or_zero();
        let q = Quat::from_rotation_arc(Vec3::X, dir);
        let mm = m * Mat4::from_rotation_translation(q, mid) * Mat4::from_scale(vec3((to - from).length(), thick, thick));
        b.cube(&mm, color);
    };
    let grip = vec3(0.0, 0.0, -0.5);
    let fore = match id {
        WeaponId::Usp | WeaponId::Knife => vec3(0.5, 1.2, -1.5),
        WeaponId::M3 => vec3(10.0, 0.5, -0.5),
        _ => vec3(9.0, 0.5, -0.5),
    };
    arm(b, vec3(-7.0, -2.5, -7.0), grip - vec3(1.0, 0.0, 0.8), shirt, 2.6);
    arm(b, grip - vec3(1.2, 0.0, 1.0), grip + vec3(0.3, 0.0, 0.2), glove, 2.2);
    if id != WeaponId::Knife {
        arm(b, vec3(-4.0, 8.0, -8.0), fore - vec3(0.8, -0.4, 0.6), shirt, 2.6);
        arm(b, fore - vec3(1.0, 0.0, 0.5), fore + vec3(0.6, 0.0, 0.1), glove, 2.2);
    }
    draw_weapon_model(b, id, &m);
    b.flush(None);

    // Muzzle flash.
    if tf < 0.045 && id != WeaponId::Knife {
        let tip_x = match id {
            WeaponId::Usp => 8.0,
            WeaponId::Mp5 => 16.0,
            WeaponId::M3 => 25.0,
            WeaponId::Ak47 => 21.0,
            WeaponId::Scout => 26.0,
            WeaponId::Awp => 31.0,
            WeaponId::Knife => 0.0,
        };
        let tip = m.transform_point3(vec3(tip_x, 0.0, 1.8));
        gl_use_material(fx_mat);
        fx.sprite(tip, r, u, 5.0, [255, 210, 110, 255]);
        fx.sprite(tip + f * 2.0, r, u, 3.0, [255, 255, 220, 255]);
        fx.flush(Some(glow));
    }
}
