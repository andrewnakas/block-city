//! Arcade car in the style of the 2004 open-world crime game: heavy, quick to
//! top speed, handbrake slides, ramps send it flying. Four wheel probes give
//! suspension height and body tilt; two body spheres handle crashes.

use super::collide::CollisionWorld;
use bevy::math::{Quat, Vec3};

#[derive(Clone, Debug)]
pub struct CarTuning {
    pub accel: f32,
    pub top_speed: f32,
    pub reverse_speed: f32,
    pub brake: f32,
    pub drag: f32,
    pub roll_resist: f32,
    pub steer_max: f32,
    pub grip: f32,
    pub handbrake_grip: f32,
    pub gravity: f32,
    pub half_len: f32,
    pub half_width: f32,
    pub ride_height: f32,
}

impl Default for CarTuning {
    fn default() -> Self {
        Self {
            accel: 10.5,
            top_speed: 42.0,
            reverse_speed: 12.0,
            brake: 22.0,
            drag: 0.0012,
            roll_resist: 0.9,
            steer_max: 0.62,
            grip: 9.0,
            handbrake_grip: 1.4,
            gravity: 15.0,
            half_len: 2.1,
            half_width: 0.95,
            ride_height: 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CarInput {
    pub throttle: f32,
    pub steer: f32,
    pub handbrake: bool,
}

#[derive(Clone, Debug)]
pub struct CarSim {
    pub t: CarTuning,
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub on_ground: bool,
    pub health: f32,
    pub wheel_spin: f32,
    pub steer_vis: f32,
    /// Impact speed of the last crash this step (for damage / effects).
    pub last_impact: f32,
    pub air_time: f32,
}

fn fwd(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

impl CarSim {
    pub fn new(pos: Vec3, yaw: f32) -> Self {
        Self {
            t: CarTuning::default(),
            pos,
            vel: Vec3::ZERO,
            yaw,
            pitch: 0.0,
            roll: 0.0,
            on_ground: false,
            health: 100.0,
            wheel_spin: 0.0,
            steer_vis: 0.0,
            last_impact: 0.0,
            air_time: 0.0,
        }
    }

    pub fn forward_speed(&self) -> f32 {
        self.vel.dot(fwd(self.yaw))
    }

    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(-self.pitch) * Quat::from_rotation_z(self.roll)
    }

    pub fn step(&mut self, dt: f32, inp: &CarInput, w: &dyn CollisionWorld) {
        let t = self.t.clone();
        self.last_impact = 0.0;
        let f = fwd(self.yaw);
        let r = Vec3::new(f.z, 0.0, -f.x); // side axis (+steer turns toward it)

        // Wheel probes: front/back, left/right.
        let corners = [f * t.half_len * 0.8 + r * t.half_width, f * t.half_len * 0.8 - r * t.half_width, -f * t.half_len * 0.8 + r * t.half_width, -f * t.half_len * 0.8 - r * t.half_width];
        let mut heights = [None; 4];
        for (i, c) in corners.iter().enumerate() {
            let o = self.pos + *c + Vec3::Y * 1.2;
            if let Some(h) = w.raycast(o, Vec3::NEG_Y, 1.2 + t.ride_height + 0.35)
                && h.normal.y > 0.5
            {
                heights[i] = Some(h.point.y);
            }
        }
        let grounded = heights.iter().filter(|h| h.is_some()).count();
        let was_ground = self.on_ground;
        self.on_ground = grounded >= 2;

        if self.on_ground {
            self.air_time = 0.0;
            let hs: Vec<f32> = heights.iter().map(|h| h.unwrap_or(self.pos.y - t.ride_height)).collect();
            let front = (hs[0] + hs[1]) * 0.5;
            let back = (hs[2] + hs[3]) * 0.5;
            let left = (hs[0] + hs[2]) * 0.5;
            let right = (hs[1] + hs[3]) * 0.5;
            let target_pitch = ((front - back) / (t.half_len * 1.6)).atan();
            let target_roll = ((left - right) / (t.half_width * 2.0)).atan();
            self.pitch += (target_pitch - self.pitch) * (dt * 14.0).min(1.0);
            self.roll += (target_roll - self.roll) * (dt * 14.0).min(1.0);
            let ground_y = hs.iter().sum::<f32>() / 4.0 + t.ride_height;
            if !was_ground && self.vel.y < -6.0 {
                self.last_impact = -self.vel.y * 0.4;
            }
            // Spring the body to ride height; keep slope-aligned velocity.
            self.pos.y += (ground_y - self.pos.y) * (dt * 22.0).min(1.0);
            let slope_n = Vec3::new(-self.pitch.sin() * f.x, 1.0, -self.pitch.sin() * f.z).normalize();
            let into = self.vel.dot(slope_n);
            if into < 0.0 {
                self.vel -= slope_n * into;
            }
            // Gravity along the slope.
            self.vel += f * (-t.gravity * self.pitch.sin()) * dt;

            let mut speed = self.forward_speed();
            let mut lateral = self.vel.dot(r);
            if inp.throttle > 0.0 {
                if speed < -0.5 {
                    speed += t.brake * inp.throttle * dt;
                } else {
                    // Strong pull off the line, tapering to nothing at top speed.
                    let room = (1.0 - speed / t.top_speed).max(0.0);
                    speed += t.accel * inp.throttle * room.sqrt() * dt;
                }
            } else if inp.throttle < 0.0 {
                if speed > 0.5 {
                    speed += t.brake * inp.throttle * dt;
                } else {
                    speed = (speed + t.accel * 0.8 * inp.throttle * dt).max(-t.reverse_speed);
                }
            } else {
                speed -= speed.signum() * (t.roll_resist * dt).min(speed.abs());
            }
            if inp.handbrake {
                speed -= speed.signum() * (6.0 * dt).min(speed.abs());
            }
            speed -= speed * speed.abs() * t.drag * dt;

            // Steering: yaw rate from speed (bicycle-ish), less lock at speed.
            let lock = t.steer_max * (1.0 - (speed.abs() / t.top_speed) * 0.55);
            self.steer_vis += (inp.steer * lock - self.steer_vis) * (dt * 8.0).min(1.0);
            let yaw_rate = speed / (t.half_len * 2.4) * self.steer_vis.tan();
            self.yaw += yaw_rate * dt * if inp.handbrake { 1.5 } else { 1.0 };

            // Tyres kill sideways slip; the handbrake lets the rear step out.
            let grip = if inp.handbrake { t.handbrake_grip } else { t.grip };
            lateral -= lateral * (grip * dt).min(1.0);
            let nf = fwd(self.yaw);
            let nr = Vec3::new(nf.z, 0.0, -nf.x);
            // Travel follows the surface pitch so ramps launch the car.
            let tilted = Vec3::new(nf.x * self.pitch.cos(), self.pitch.sin(), nf.z * self.pitch.cos());
            self.vel = tilted * speed + nr * lateral;
        } else {
            self.air_time += dt;
            self.vel.y -= t.gravity * dt;
            // Nose drops slowly in the air.
            self.pitch += (-0.25 * self.vel.y.signum().min(0.0) - self.pitch * 0.2) * dt;
        }

        self.pos += self.vel * dt;

        // Body spheres: front and back.
        for k in [1.0f32, -1.0] {
            let c = self.pos + fwd(self.yaw) * (t.half_len - t.half_width) * k + Vec3::Y * 0.5;
            let (nc, n) = w.resolve_sphere(c, t.half_width * 0.9);
            if let Some(n) = n {
                let push = nc - c;
                self.pos += push;
                let into = self.vel.dot(n);
                if into < 0.0 && n.y < 0.6 {
                    self.last_impact = self.last_impact.max(-into);
                    self.vel -= n * into * 1.35;
                    self.vel *= 0.85;
                } else if into < 0.0 {
                    self.vel -= n * into;
                }
            }
        }
        if self.last_impact > 8.0 {
            self.health -= (self.last_impact - 8.0) * 3.0;
        }
        self.wheel_spin += self.forward_speed() * dt / 0.35;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::collide::TestWorld;

    fn drive(c: &mut CarSim, w: &TestWorld, inp: CarInput, secs: f32) {
        for _ in 0..((secs * 120.0) as usize) {
            c.step(1.0 / 120.0, &inp, w);
        }
    }

    #[test]
    fn accelerates_toward_top_speed() {
        let w = TestWorld::flat();
        let mut c = CarSim::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
        drive(&mut c, &w, CarInput::default(), 0.5);
        assert!(c.on_ground);
        drive(&mut c, &w, CarInput { throttle: 1.0, ..Default::default() }, 3.0);
        let s3 = c.forward_speed();
        assert!(s3 > 18.0 && s3 < 30.0, "0-3s {s3}");
        drive(&mut c, &w, CarInput { throttle: 1.0, ..Default::default() }, 20.0);
        assert!(c.forward_speed() > 36.0 && c.forward_speed() <= 42.0, "top {}", c.forward_speed());
        assert!((c.pos.y - 0.5).abs() < 0.05);
    }

    #[test]
    fn steering_turns_and_handbrake_slides() {
        let w = TestWorld::flat();
        let mut c = CarSim::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
        drive(&mut c, &w, CarInput { throttle: 1.0, ..Default::default() }, 3.0);
        drive(&mut c, &w, CarInput { throttle: 0.5, steer: 1.0, ..Default::default() }, 1.0);
        assert!(c.yaw > 0.5, "turned left {}", c.yaw);
        let r = Vec3::new(c.yaw.cos(), 0.0, -c.yaw.sin());
        let grip_lat = c.vel.dot(r).abs();
        drive(&mut c, &w, CarInput { throttle: 0.5, steer: 1.0, handbrake: true }, 0.4);
        let r = Vec3::new(c.yaw.cos(), 0.0, -c.yaw.sin());
        assert!(c.vel.dot(r).abs() > grip_lat, "handbrake slides more");
    }

    #[test]
    fn wall_crash_bounces_and_damages() {
        let w = TestWorld::flat().with_box(Vec3::new(-10.0, 0.0, 30.0), Vec3::new(10.0, 4.0, 31.0));
        let mut c = CarSim::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
        drive(&mut c, &w, CarInput { throttle: 1.0, ..Default::default() }, 4.0);
        assert!(c.pos.z < 30.0);
        assert!(c.health < 100.0, "health {}", c.health);
    }
}
