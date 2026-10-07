//! Collision interface shared by the movement sims.
//!
//! The sims never talk to Rapier directly: the app backs this trait with
//! Rapier scene queries, and tests back it with a tiny analytic world, so
//! movement feel can be unit-tested without a renderer or physics engine.

use bevy::math::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f32,
    pub point: Vec3,
    pub normal: Vec3,
}

#[derive(Clone, Copy, Debug)]
pub struct RailSample {
    pub point: Vec3,
    pub tangent: Vec3,
    pub dist: f32,
    pub rail: u32,
}

pub trait CollisionWorld {
    /// Nearest solid hit along `dir` (unit) within `max` metres.
    fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<Hit>;

    /// Pushes a sphere out of solids. Returns the corrected centre and the
    /// last contact normal, if any contact happened.
    fn resolve_sphere(&self, center: Vec3, radius: f32) -> (Vec3, Option<Vec3>);

    /// Closest grindable rail point within `max` metres.
    fn nearest_rail(&self, p: Vec3, max: f32) -> Option<RailSample>;
}

/// Grind rails live outside the physics engine: they are polylines that the
/// skate sim latches onto. Shared by the app and tests.
#[derive(Default, Clone, Debug)]
pub struct RailSet {
    pub rails: Vec<Vec<Vec3>>,
}

impl RailSet {
    pub fn add(&mut self, pts: Vec<Vec3>) -> u32 {
        self.rails.push(pts);
        (self.rails.len() - 1) as u32
    }

    pub fn nearest(&self, p: Vec3, max: f32) -> Option<RailSample> {
        let mut best: Option<RailSample> = None;
        for (ri, rail) in self.rails.iter().enumerate() {
            for seg in rail.windows(2) {
                let (a, b) = (seg[0], seg[1]);
                let ab = b - a;
                let len2 = ab.length_squared();
                if len2 < 1e-8 {
                    continue;
                }
                let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
                let cp = a + ab * t;
                let d = cp.distance(p);
                if d <= max && best.is_none_or(|bs| d < bs.dist) {
                    best = Some(RailSample { point: cp, tangent: ab / len2.sqrt(), dist: d, rail: ri as u32 });
                }
            }
        }
        best
    }
}

/// Axis-aligned test world: an infinite ground plane at y=0 plus boxes.
#[derive(Default)]
pub struct TestWorld {
    pub boxes: Vec<(Vec3, Vec3)>, // (min, max)
    pub rails: RailSet,
    pub ground: bool,
}

impl TestWorld {
    pub fn flat() -> Self {
        Self { ground: true, ..Default::default() }
    }

    pub fn with_box(mut self, min: Vec3, max: Vec3) -> Self {
        self.boxes.push((min, max));
        self
    }
}

impl CollisionWorld for TestWorld {
    fn raycast(&self, o: Vec3, d: Vec3, max: f32) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let mut consider = |h: Hit| {
            if h.t >= 0.0 && h.t <= max && best.is_none_or(|b| h.t < b.t) {
                best = Some(h);
            }
        };
        if self.ground && d.y < -1e-6 && o.y >= 0.0 {
            let t = -o.y / d.y;
            consider(Hit { t, point: o + d * t, normal: Vec3::Y });
        }
        for &(mn, mx) in &self.boxes {
            let (mut t0, mut t1) = (0.0f32, max);
            let mut n = Vec3::ZERO;
            let mut ok = true;
            for a in 0..3 {
                let (oa, da, lo, hi) = (o[a], d[a], mn[a], mx[a]);
                if da.abs() < 1e-9 {
                    if oa < lo || oa > hi {
                        ok = false;
                        break;
                    }
                    continue;
                }
                let (mut ta, mut tb) = ((lo - oa) / da, (hi - oa) / da);
                let mut sign = -1.0;
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                    sign = 1.0;
                }
                if ta > t0 {
                    t0 = ta;
                    n = Vec3::ZERO;
                    n[a] = sign;
                }
                t1 = t1.min(tb);
                if t0 > t1 {
                    ok = false;
                    break;
                }
            }
            if ok && n != Vec3::ZERO {
                consider(Hit { t: t0, point: o + d * t0, normal: n });
            }
        }
        best
    }

    fn resolve_sphere(&self, c: Vec3, r: f32) -> (Vec3, Option<Vec3>) {
        let mut p = c;
        let mut normal = None;
        for _ in 0..4 {
            let mut moved = false;
            if self.ground && p.y < r {
                p.y = r;
                normal = Some(Vec3::Y);
                moved = true;
            }
            for &(mn, mx) in &self.boxes {
                let q = p.clamp(mn, mx);
                let delta = p - q;
                let d2 = delta.length_squared();
                if d2 > r * r {
                    continue;
                }
                if d2 > 1e-10 {
                    let d = d2.sqrt();
                    let n = delta / d;
                    p += n * (r - d);
                    normal = Some(n);
                } else {
                    // Centre inside: exit along the least-penetration face.
                    let pens = [
                        (p.x - mn.x, -Vec3::X),
                        (mx.x - p.x, Vec3::X),
                        (p.y - mn.y, -Vec3::Y),
                        (mx.y - p.y, Vec3::Y),
                        (p.z - mn.z, -Vec3::Z),
                        (mx.z - p.z, Vec3::Z),
                    ];
                    let (pen, n) = pens.iter().copied().fold((f32::MAX, Vec3::Y), |a, b| if b.0 < a.0 { b } else { a });
                    p += n * (pen + r);
                    normal = Some(n);
                }
                moved = true;
            }
            if !moved {
                break;
            }
        }
        (p, normal)
    }

    fn nearest_rail(&self, p: Vec3, max: f32) -> Option<RailSample> {
        self.rails.nearest(p, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_ground_and_box_top() {
        let w = TestWorld::flat().with_box(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        let h = w.raycast(Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y, 10.0).unwrap();
        assert!((h.point.y - 1.0).abs() < 1e-4 && h.normal == Vec3::Y);
        let h = w.raycast(Vec3::new(3.0, 5.0, 0.0), Vec3::NEG_Y, 10.0).unwrap();
        assert!(h.point.y.abs() < 1e-4);
    }

    #[test]
    fn sphere_pushed_out_of_wall() {
        let w = TestWorld::flat().with_box(Vec3::new(0.0, 0.0, -5.0), Vec3::new(1.0, 3.0, 5.0));
        let (p, n) = w.resolve_sphere(Vec3::new(-0.2, 1.0, 0.0), 0.5);
        assert!((p.x + 0.5).abs() < 1e-4, "{p:?}");
        assert_eq!(n, Some(-Vec3::X));
    }

    #[test]
    fn rail_nearest() {
        let mut r = RailSet::default();
        r.add(vec![Vec3::new(0.0, 1.0, 0.0), Vec3::new(10.0, 1.0, 0.0)]);
        let s = r.nearest(Vec3::new(4.0, 1.3, 0.1), 1.0).unwrap();
        assert!((s.point.x - 4.0).abs() < 1e-4 && s.tangent == Vec3::X);
        assert!(r.nearest(Vec3::new(4.0, 5.0, 0.0), 1.0).is_none());
    }
}
