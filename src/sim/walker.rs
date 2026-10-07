//! On-foot shooter movement in the style of the 2009 military FPS: brisk
//! walk, sprint that blocks firing, short hop, crouch. Values follow the
//! engine's publicly documented console defaults converted from inches
//! (speed 190 in/s, gravity 800 in/s^2, jump height 39 in).

use super::collide::CollisionWorld;
use bevy::math::Vec3;

const IN: f32 = 0.0254;

#[derive(Clone, Debug)]
pub struct WalkTuning {
    pub walk: f32,
    pub sprint_mult: f32,
    pub crouch_mult: f32,
    pub ads_mult: f32,
    pub gravity: f32,
    pub jump_height: f32,
    pub ground_accel: f32,
    pub friction: f32,
    pub air_accel: f32,
    pub radius: f32,
    pub step: f32,
}

impl Default for WalkTuning {
    fn default() -> Self {
        Self {
            walk: 190.0 * IN,
            sprint_mult: 1.5,
            crouch_mult: 0.65,
            ads_mult: 0.6,
            gravity: 800.0 * IN,
            jump_height: 39.0 * IN,
            ground_accel: 9.0,
            friction: 5.5,
            air_accel: 1.2,
            radius: 0.35,
            step: 0.45,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WalkInput {
    /// World-space wish direction (XZ, length 0..1).
    pub wish: [f32; 2],
    pub jump: bool,
    pub sprint: bool,
    pub crouch: bool,
    pub ads: bool,
}

#[derive(Clone, Debug)]
pub struct Walker {
    pub t: WalkTuning,
    pub pos: Vec3,
    pub vel: Vec3,
    pub on_ground: bool,
    pub sprinting: bool,
    pub crouched: bool,
}

impl Walker {
    pub fn new(pos: Vec3) -> Self {
        Self { t: WalkTuning::default(), pos, vel: Vec3::ZERO, on_ground: false, sprinting: false, crouched: false }
    }

    pub fn enter(&mut self, pos: Vec3, vel: Vec3) {
        self.pos = pos;
        self.vel = vel;
        self.on_ground = false;
    }

    pub fn step(&mut self, dt: f32, inp: &WalkInput, w: &dyn CollisionWorld) {
        let t = &self.t;
        let wish = Vec3::new(inp.wish[0], 0.0, inp.wish[1]);
        let wish_len = wish.length().min(1.0);
        let wish_dir = wish.normalize_or_zero();
        self.crouched = inp.crouch;
        self.sprinting = inp.sprint && !inp.crouch && !inp.ads && wish_len > 0.5 && self.on_ground;
        let mut max = t.walk * wish_len;
        if self.sprinting {
            max *= t.sprint_mult;
        } else if inp.crouch {
            max *= t.crouch_mult;
        } else if inp.ads {
            max *= t.ads_mult;
        }

        let mut h = Vec3::new(self.vel.x, 0.0, self.vel.z);
        if self.on_ground {
            // Friction, then accelerate toward the wish speed (Quake-lineage model).
            let sp = h.length();
            if sp > 0.0 {
                let drop = sp.max(1.0) * t.friction * dt;
                h *= ((sp - drop) / sp).max(0.0);
            }
            let cur = h.dot(wish_dir);
            let add = (max - cur).max(0.0).min(t.ground_accel * max.max(t.walk) * dt);
            h += wish_dir * add;
            if inp.jump {
                self.vel.y = (2.0 * t.gravity * t.jump_height).sqrt();
                self.on_ground = false;
            }
        } else {
            let cur = h.dot(wish_dir);
            let add = (max - cur).max(0.0).min(t.air_accel * t.walk * dt);
            h += wish_dir * add;
        }
        self.vel.x = h.x;
        self.vel.z = h.z;
        if !self.on_ground {
            self.vel.y -= t.gravity * dt;
        }

        let r = t.radius;
        let center = self.pos + Vec3::Y * (r + t.step) + self.vel * dt;
        let (c, n) = w.resolve_sphere(center, r);
        if let Some(n) = n {
            let into = self.vel.dot(n);
            if into < 0.0 && n.y.abs() < 0.7 {
                self.vel -= n * into;
            }
            if n.y < -0.7 && self.vel.y > 0.0 {
                self.vel.y = 0.0;
            }
        }
        let mut p = c - Vec3::Y * (r + t.step);
        // Ground probe from the body; small ledges are stepped up automatically.
        let probe = if self.on_ground { t.step + 0.2 } else { t.step + 0.02 };
        match w.raycast(p + Vec3::Y * (t.step + 0.05), Vec3::NEG_Y, probe + 0.05) {
            Some(hit) if hit.normal.y > 0.65 && self.vel.y <= 0.1 => {
                p.y = hit.point.y;
                self.vel.y = 0.0;
                self.on_ground = true;
            }
            _ => self.on_ground = false,
        }
        self.pos = p;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::collide::TestWorld;

    #[test]
    fn walk_and_sprint_speeds() {
        let w = TestWorld::flat();
        let mut m = Walker::new(Vec3::new(0.0, 0.1, 0.0));
        let dt = 1.0 / 120.0;
        for _ in 0..240 {
            m.step(dt, &WalkInput { wish: [0.0, 1.0], ..Default::default() }, &w);
        }
        assert!((m.vel.z - 4.83).abs() < 0.1, "walk {}", m.vel.z);
        for _ in 0..240 {
            m.step(dt, &WalkInput { wish: [0.0, 1.0], sprint: true, ..Default::default() }, &w);
        }
        assert!((m.vel.z - 7.24).abs() < 0.15, "sprint {}", m.vel.z);
    }

    #[test]
    fn jump_height_is_about_one_metre() {
        let w = TestWorld::flat();
        let mut m = Walker::new(Vec3::new(0.0, 0.1, 0.0));
        let dt = 1.0 / 120.0;
        for _ in 0..30 {
            m.step(dt, &WalkInput::default(), &w);
        }
        m.step(dt, &WalkInput { jump: true, ..Default::default() }, &w);
        let mut top: f32 = 0.0;
        for _ in 0..120 {
            m.step(dt, &WalkInput::default(), &w);
            top = top.max(m.pos.y);
        }
        assert!((top - 0.99).abs() < 0.06, "apex {top}");
        assert!(m.on_ground);
    }

    #[test]
    fn steps_up_curb() {
        let w = TestWorld::flat().with_box(Vec3::new(-5.0, 0.0, 2.0), Vec3::new(5.0, 0.2, 10.0));
        let mut m = Walker::new(Vec3::new(0.0, 0.0, 0.0));
        for _ in 0..240 {
            m.step(1.0 / 120.0, &WalkInput { wish: [0.0, 1.0], ..Default::default() }, &w);
        }
        assert!(m.pos.z > 4.0 && (m.pos.y - 0.2).abs() < 0.02, "{:?}", m.pos);
    }
}
