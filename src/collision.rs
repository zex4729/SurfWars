//! Convex brush collision in the style of the Quake / GoldSrc engines.
//!
//! The world is a list of convex brushes, each described by a set of
//! half-spaces. Moving an axis aligned box through the world is done by
//! expanding every brush plane by the box extents (a Minkowski sum) and
//! tracing a point against the expanded brushes. Bevel planes are added to
//! every brush so the expansion stays tight around edges, just like the
//! bevels the Quake 3 map compiler adds for box traces.

use macroquad::math::{DVec3, Vec3};

use crate::map::Mat;

/// Same epsilon that GoldSrc uses to keep the player a hair away from planes.
pub const DIST_EPSILON: f32 = 0.03125;

#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub normal: Vec3,
    pub dist: f32,
}

#[derive(Clone, Debug)]
pub struct Face {
    /// Polygon vertices, counter clockwise when viewed from outside.
    pub verts: Vec<Vec3>,
    pub normal: Vec3,
}

#[derive(Clone, Debug)]
pub struct Brush {
    /// Collision planes: the real faces followed by the bevel planes.
    pub planes: Vec<Plane>,
    /// Render faces.
    pub faces: Vec<Face>,
    pub mins: Vec3,
    pub maxs: Vec3,
    pub mat: Mat,
}

#[derive(Clone, Copy, Debug)]
pub struct Trace {
    pub fraction: f32,
    pub endpos: Vec3,
    pub normal: Vec3,
    pub startsolid: bool,
    pub allsolid: bool,
    pub brush: Option<usize>,
}

impl Trace {
    pub fn hit(&self) -> bool {
        self.fraction < 1.0
    }
}

const BUILD_EPS: f64 = 1e-3;

fn intersect3(a: (DVec3, f64), b: (DVec3, f64), c: (DVec3, f64)) -> Option<DVec3> {
    let n23 = b.0.cross(c.0);
    let denom = a.0.dot(n23);
    if denom.abs() < 1e-9 {
        return None;
    }
    let p = (n23 * a.1 + c.0.cross(a.0) * b.1 + a.0.cross(b.0) * c.1) / denom;
    Some(p)
}

fn same_plane(a: (DVec3, f64), b: (DVec3, f64)) -> bool {
    a.0.dot(b.0) > 0.99999 && (a.1 - b.1).abs() < 0.01
}

impl Brush {
    /// Builds a brush from outward facing half-spaces `normal . p <= dist`.
    pub fn from_planes(input: &[(DVec3, f64)], mat: Mat) -> Option<Brush> {
        let mut planes: Vec<(DVec3, f64)> = Vec::new();
        for &(n, d) in input {
            let len = n.length();
            if len < 1e-9 {
                continue;
            }
            let p = (n / len, d / len);
            if !planes.iter().any(|&q| same_plane(p, q)) {
                planes.push(p);
            }
        }

        // Enumerate the polyhedron vertices.
        let mut verts: Vec<DVec3> = Vec::new();
        let n = planes.len();
        for i in 0..n {
            for j in (i + 1)..n {
                for k in (j + 1)..n {
                    if let Some(p) = intersect3(planes[i], planes[j], planes[k]) {
                        if planes.iter().all(|&(pn, pd)| pn.dot(p) - pd <= BUILD_EPS)
                            && !verts.iter().any(|v| v.distance_squared(p) < 1e-4)
                        {
                            verts.push(p);
                        }
                    }
                }
            }
        }
        if verts.len() < 4 {
            return None;
        }

        // Build the faces and drop redundant planes.
        let mut faces_d: Vec<(DVec3, f64, Vec<DVec3>)> = Vec::new();
        for &(pn, pd) in &planes {
            let mut on: Vec<DVec3> = verts
                .iter()
                .copied()
                .filter(|v| (pn.dot(*v) - pd).abs() < BUILD_EPS * 4.0)
                .collect();
            if on.len() < 3 {
                continue;
            }
            let center = on.iter().fold(DVec3::ZERO, |a, b| a + *b) / on.len() as f64;
            let t = if pn.z.abs() < 0.99 {
                DVec3::Z.cross(pn).normalize()
            } else {
                DVec3::X.cross(pn).normalize()
            };
            let b = pn.cross(t);
            on.sort_by(|a, c| {
                let da = *a - center;
                let dc = *c - center;
                let aa = da.dot(b).atan2(da.dot(t));
                let ac = dc.dot(b).atan2(dc.dot(t));
                aa.partial_cmp(&ac).unwrap()
            });
            faces_d.push((pn, pd, on));
        }

        let mut mins = DVec3::splat(f64::MAX);
        let mut maxs = DVec3::splat(f64::MIN);
        for v in &verts {
            mins = mins.min(*v);
            maxs = maxs.max(*v);
        }

        let mut coll: Vec<(DVec3, f64)> = faces_d.iter().map(|f| (f.0, f.1)).collect();

        // Axial bevels.
        let axes = [DVec3::X, DVec3::Y, DVec3::Z];
        for (i, ax) in axes.iter().enumerate() {
            let pos = (*ax, maxs[i]);
            let neg = (-*ax, -mins[i]);
            if !coll.iter().any(|&q| same_plane(pos, q)) {
                coll.push(pos);
            }
            if !coll.iter().any(|&q| same_plane(neg, q)) {
                coll.push(neg);
            }
        }

        // Edge bevels: planes through an edge that are built from the edge
        // direction and a coordinate axis, and that touch the brush only
        // along that edge.
        for (_, _, fv) in &faces_d {
            for i in 0..fv.len() {
                let v0 = fv[i];
                let v1 = fv[(i + 1) % fv.len()];
                let e = v1 - v0;
                if e.length() < 1e-6 {
                    continue;
                }
                let e = e.normalize();
                for ax in axes {
                    let c = e.cross(ax);
                    if c.length() < 1e-4 {
                        continue;
                    }
                    let c = c.normalize();
                    for s in [1.0, -1.0] {
                        let bn = c * s;
                        let bd = bn.dot(v0);
                        if verts.iter().all(|v| bn.dot(*v) <= bd + BUILD_EPS * 4.0)
                            && !coll.iter().any(|&q| same_plane((bn, bd), q))
                        {
                            coll.push((bn, bd));
                        }
                    }
                }
            }
        }

        let to_f = |v: DVec3| Vec3::new(v.x as f32, v.y as f32, v.z as f32);
        Some(Brush {
            planes: coll
                .iter()
                .map(|&(n, d)| Plane {
                    normal: to_f(n),
                    dist: d as f32,
                })
                .collect(),
            faces: faces_d
                .iter()
                .map(|(n, _, v)| Face {
                    verts: v.iter().map(|p| to_f(*p)).collect(),
                    normal: to_f(*n),
                })
                .collect(),
            mins: to_f(mins),
            maxs: to_f(maxs),
            mat,
        })
    }

    /// Axis aligned box brush.
    pub fn cuboid(mins: Vec3, maxs: Vec3, mat: Mat) -> Brush {
        let planes = [
            (DVec3::X, maxs.x as f64),
            (-DVec3::X, -mins.x as f64),
            (DVec3::Y, maxs.y as f64),
            (-DVec3::Y, -mins.y as f64),
            (DVec3::Z, maxs.z as f64),
            (-DVec3::Z, -mins.z as f64),
        ];
        Brush::from_planes(&planes, mat).expect("degenerate box brush")
    }

    /// Convex hull of a small point set (brute force, fine for < 20 points).
    pub fn hull(points: &[Vec3], mat: Mat) -> Brush {
        let pts: Vec<DVec3> = points
            .iter()
            .map(|p| DVec3::new(p.x as f64, p.y as f64, p.z as f64))
            .collect();
        let mut planes: Vec<(DVec3, f64)> = Vec::new();
        let n = pts.len();
        for i in 0..n {
            for j in (i + 1)..n {
                for k in (j + 1)..n {
                    let nrm = (pts[j] - pts[i]).cross(pts[k] - pts[i]);
                    if nrm.length() < 1e-6 {
                        continue;
                    }
                    let nrm = nrm.normalize();
                    for s in [1.0, -1.0] {
                        let pn = nrm * s;
                        let pd = pn.dot(pts[i]);
                        if pts.iter().all(|p| pn.dot(*p) <= pd + 1e-3)
                            && !planes.iter().any(|&q| same_plane((pn, pd), q))
                        {
                            planes.push((pn, pd));
                        }
                    }
                }
            }
        }
        Brush::from_planes(&planes, mat).expect("degenerate hull brush")
    }

    /// Point containment test (used for render culling helpers and tests).
    #[allow(dead_code)]
    pub fn contains(&self, p: Vec3) -> bool {
        self.planes.iter().all(|pl| pl.normal.dot(p) <= pl.dist)
    }
}

pub struct CollisionWorld {
    pub brushes: Vec<Brush>,
}

impl CollisionWorld {
    pub fn new(brushes: Vec<Brush>) -> CollisionWorld {
        CollisionWorld { brushes }
    }

    /// Sweeps the box `[mins, maxs]` from `start` to `end`.
    pub fn trace(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3) -> Trace {
        let mut tr = Trace {
            fraction: 1.0,
            endpos: end,
            normal: Vec3::ZERO,
            startsolid: false,
            allsolid: false,
            brush: None,
        };
        let smin = start.min(end) + mins - Vec3::splat(1.0);
        let smax = start.max(end) + maxs + Vec3::splat(1.0);
        for (i, b) in self.brushes.iter().enumerate() {
            if !b.mat.solid() {
                continue;
            }
            if b.maxs.x < smin.x
                || b.mins.x > smax.x
                || b.maxs.y < smin.y
                || b.mins.y > smax.y
                || b.maxs.z < smin.z
                || b.mins.z > smax.z
            {
                continue;
            }
            trace_brush(b, i, start, end, mins, maxs, &mut tr);
            if tr.allsolid {
                break;
            }
        }
        if tr.fraction == 1.0 {
            tr.endpos = end;
        } else {
            tr.endpos = start + (end - start) * tr.fraction;
        }
        tr
    }

    /// Infinitely thin ray, used for bullets and line of sight checks.
    pub fn trace_ray(&self, start: Vec3, end: Vec3) -> Trace {
        self.trace(start, end, Vec3::ZERO, Vec3::ZERO)
    }

    /// True if a box at `origin` overlaps any solid brush.
    pub fn box_stuck(&self, origin: Vec3, mins: Vec3, maxs: Vec3) -> bool {
        self.trace(origin, origin, mins, maxs).startsolid
    }
}

fn trace_brush(
    b: &Brush,
    index: usize,
    start: Vec3,
    end: Vec3,
    mins: Vec3,
    maxs: Vec3,
    tr: &mut Trace,
) {
    // Entry / exit parameters along the move. The blocking plane is chosen
    // by the exact crossing point; the epsilon back-off is applied after.
    // (Choosing by the backed-off value lets a plane that is nearly parallel
    // to the move win over the real one, which turns ramp seams into walls.)
    let mut enter_raw = -1.0f32;
    let mut enter_frac = 0.0f32;
    let mut leave_raw = 1.0f32;
    let mut clip_normal = Vec3::ZERO;
    let mut startout = false;
    let mut getout = false;

    for p in &b.planes {
        let n = p.normal;
        // Offset the plane by the box corner that is deepest along -normal.
        let ofs = (if n.x < 0.0 { maxs.x } else { mins.x }) * n.x
            + (if n.y < 0.0 { maxs.y } else { mins.y }) * n.y
            + (if n.z < 0.0 { maxs.z } else { mins.z }) * n.z;
        let dist = p.dist - ofs;
        let d1 = start.dot(n) - dist;
        let d2 = end.dot(n) - dist;

        // GoldSrc hull semantics: a point on or in front of a plane is in
        // empty space, and a plane only blocks when the move crosses it.
        // (Quake 3 also blocks moves that merely come within the epsilon,
        // which makes a player gliding along a ramp stick to it.)
        if d2 >= 0.0 {
            getout = true;
        }
        if d1 >= 0.0 {
            startout = true;
        }
        if d1 >= 0.0 && d2 >= 0.0 {
            return; // the whole move is in front of this plane
        }
        if d1 < 0.0 && d2 < 0.0 {
            continue;
        }
        let raw = d1 / (d1 - d2);
        if d1 >= 0.0 {
            // entering: back off by the epsilon like SV_RecursiveHullCheck
            if raw > enter_raw {
                enter_raw = raw;
                enter_frac = ((d1 - DIST_EPSILON) / (d1 - d2)).max(0.0);
                clip_normal = n;
            }
        } else if raw < leave_raw {
            leave_raw = raw;
        }
    }

    if !startout {
        tr.startsolid = true;
        if !getout {
            tr.allsolid = true;
            tr.fraction = 0.0;
            tr.brush = Some(index);
        }
        return;
    }

    if enter_raw > -1.0 && enter_raw < leave_raw && enter_frac < tr.fraction {
        tr.fraction = enter_frac;
        tr.normal = clip_normal;
        tr.brush = Some(index);
    }
}
