//! Clean-room 3D platformer controller in the style of the N64 classic.
//!
//! Runs on a fixed 30 Hz frame clock in "units" (1 u = 1 cm) and units/frame,
//! because that is how the original's movement is documented by speedrunners
//! and measured from footage (docs/specs/jump.md). The app converts to metres.
//! Behaviour only — no decompiled code or data is used.

use super::collide::CollisionWorld;
use bevy::math::Vec3;
use std::f32::consts::{PI, TAU};

pub const FRAME: f32 = 1.0 / 30.0;
pub const UNIT: f32 = 0.01; // metres per unit

#[derive(Clone, Debug)]
pub struct JumpTuning {
    pub gravity: f32,
    pub terminal: f32,
    pub run_max: f32,
    pub run_accel: f32,
    pub walk_decel: f32,
    pub turn_rate: f32,
    pub single_vy: f32,
    pub double_vy: f32,
    pub triple_vy: f32,
    pub triple_min_speed: f32,
    pub chain_window: u32,
    pub backflip_vy: f32,
    pub backflip_fwd: f32,
    pub sideflip_vy: f32,
    pub sideflip_fwd: f32,
    pub longjump_vy: f32,
    pub longjump_mult: f32,
    pub longjump_cap: f32,
    pub longjump_gravity: f32,
    pub wallkick_vy: f32,
    pub wallkick_fwd: f32,
    pub wallkick_window: u32,
    pub wallkick_min_speed: f32,
    pub pound_hover: u32,
    pub pound_vy: f32,
    pub dive_vy: f32,
    pub dive_boost: f32,
    pub dive_cap: f32,
    pub air_accel: f32,
    pub air_max: f32,
    pub short_hop_cut: f32,
    pub radius: f32,
    pub height: f32,
}

impl Default for JumpTuning {
    fn default() -> Self {
        // Community-documented values (per-frame at 30 fps); see docs/specs/jump.md.
        Self {
            gravity: 4.0,
            terminal: -75.0,
            run_max: 32.0,
            run_accel: 1.1,
            walk_decel: 2.0,
            turn_rate: 11.25f32.to_radians(),
            single_vy: 42.0,
            double_vy: 52.0,
            triple_vy: 69.0,
            triple_min_speed: 20.0,
            chain_window: 5,
            backflip_vy: 62.0,
            backflip_fwd: -16.0,
            sideflip_vy: 62.0,
            sideflip_fwd: 8.0,
            longjump_vy: 30.0,
            longjump_mult: 1.5,
            longjump_cap: 48.0,
            longjump_gravity: 2.0,
            wallkick_vy: 52.0,
            wallkick_fwd: 24.0,
            wallkick_window: 5,
            wallkick_min_speed: 12.0,
            pound_hover: 10,
            pound_vy: -50.0,
            dive_vy: 20.0,
            dive_boost: 15.0,
            dive_cap: 48.0,
            air_accel: 1.5,
            air_max: 32.0,
            short_hop_cut: 0.25,
            radius: 37.0,
            height: 160.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JAction {
    Idle,
    Walk,
    Skid,
    Crouch,
    CrouchSlide,
    JumpLand { kind: u8, frames: u32 },
    Jump { kind: u8 },
    Backflip,
    Sideflip,
    LongJump,
    WallKick,
    WallContact { frames: u32 },
    GroundPoundHover { frames: u32 },
    GroundPound,
    Dive,
    BellySlide,
    Freefall,
    Punch { frames: u32 },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct JumpInput {
    /// Desired move direction in world XZ (length 0..1 = stick magnitude).
    pub stick: [f32; 2],
    pub a_held: bool,
    pub a_pressed: bool,
    pub b_pressed: bool,
    pub z_held: bool,
    pub z_pressed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum JEvent {
    Jump(&'static str),
    Land,
    WallKick,
    GroundPoundImpact,
    Dive,
}

#[derive(Clone, Debug)]
pub struct Jumper {
    pub t: JumpTuning,
    pub action: JAction,
    /// Position in metres (feet).
    pub pos: Vec3,
    /// Horizontal speed along facing, units/frame (can be negative).
    pub fwd: f32,
    /// Vertical speed, units/frame.
    pub vy: f32,
    pub facing: f32,
    /// Extra lateral velocity (side flip / wall kick redirect), u/f.
    pub slide: Vec3,
    pub on_ground: bool,
    pub events: Vec<JEvent>,
    wall_normal: Vec3,
    accum: f32,
    prev_pos: Vec3,
    pub anim_t: f32,
}

fn dir(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

fn wrap_pi(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

impl Jumper {
    pub fn new(pos: Vec3, facing: f32) -> Self {
        Self {
            t: JumpTuning::default(),
            action: JAction::Freefall,
            pos,
            fwd: 0.0,
            vy: 0.0,
            facing,
            slide: Vec3::ZERO,
            on_ground: false,
            events: vec![],
            wall_normal: Vec3::ZERO,
            accum: 0.0,
            prev_pos: pos,
            anim_t: 0.0,
        }
    }

    /// World velocity in m/s.
    pub fn velocity(&self) -> Vec3 {
        (dir(self.facing) * self.fwd + self.slide + Vec3::Y * self.vy) * UNIT / FRAME
    }

    /// Take over from another locomotion mode, keeping momentum.
    pub fn enter(&mut self, pos: Vec3, vel_ms: Vec3, facing: f32) {
        let v = vel_ms * FRAME / UNIT;
        let h = Vec3::new(v.x, 0.0, v.z);
        self.pos = pos;
        self.prev_pos = pos;
        self.facing = if h.length() > 1.0 { h.x.atan2(h.z) } else { facing };
        self.fwd = h.length().min(self.t.dive_cap);
        self.vy = v.y;
        self.slide = Vec3::ZERO;
        self.action = JAction::Freefall;
        self.on_ground = false;
    }

    /// Interpolated render position.
    pub fn render_pos(&self) -> Vec3 {
        self.prev_pos.lerp(self.pos, (self.accum / FRAME).clamp(0.0, 1.0))
    }

    /// Advance by real time; runs whole 30 Hz frames and returns how many ran.
    /// Edge inputs are consumed by the first frame, so callers should keep
    /// latching them until this returns > 0.
    pub fn update(&mut self, dt: f32, inp: &JumpInput, w: &dyn CollisionWorld) -> u32 {
        self.accum += dt;
        let mut ran = 0;
        let mut first = true;
        while self.accum >= FRAME {
            self.accum -= FRAME;
            let mut i = *inp;
            if !first {
                i.a_pressed = false;
                i.b_pressed = false;
                i.z_pressed = false;
            }
            first = false;
            self.prev_pos = self.pos;
            self.frame(&i, w);
            ran += 1;
        }
        ran
    }

    fn stick(&self, inp: &JumpInput) -> (f32, f32) {
        let mag = (inp.stick[0] * inp.stick[0] + inp.stick[1] * inp.stick[1]).sqrt().min(1.0);
        (mag, inp.stick[0].atan2(inp.stick[1]))
    }

    pub fn frame(&mut self, inp: &JumpInput, w: &dyn CollisionWorld) {
        let t = self.t.clone();
        let (mag, want) = self.stick(inp);
        self.anim_t += FRAME;
        use JAction::*;

        if self.on_ground {
            // ---- ground actions ----
            match self.action {
                Punch { frames } => {
                    self.fwd *= 0.8;
                    self.action = if frames >= 8 { Idle } else { Punch { frames: frames + 1 } };
                }
                BellySlide => {
                    self.fwd = (self.fwd - 1.0).max(0.0);
                    if inp.a_pressed || inp.b_pressed || self.fwd <= 0.0 {
                        // Rollout.
                        self.action = Jump { kind: 1 };
                        self.vy = t.single_vy * 0.7;
                        self.on_ground = false;
                    }
                }
                CrouchSlide if inp.a_pressed => {
                    self.long_jump();
                }
                Crouch if inp.a_pressed => {
                    self.action = Backflip;
                    self.vy = t.backflip_vy;
                    self.fwd = t.backflip_fwd;
                    self.on_ground = false;
                    self.events.push(JEvent::Jump("Backflip"));
                }
                Skid if inp.a_pressed => {
                    self.action = Sideflip;
                    self.facing = want;
                    self.fwd = t.sideflip_fwd;
                    self.vy = t.sideflip_vy;
                    self.on_ground = false;
                    self.events.push(JEvent::Jump("Side Flip"));
                }
                _ if inp.a_pressed => {
                    // Jump chaining: land-and-jump inside the window escalates.
                    let next = match self.action {
                        JumpLand { kind: 1, frames } if frames <= t.chain_window => 2,
                        JumpLand { kind: 2, frames } if frames <= t.chain_window && self.fwd > t.triple_min_speed => 3,
                        _ => 1,
                    };
                    self.vy = match next {
                        2 => t.double_vy + self.fwd * 0.25,
                        3 => t.triple_vy,
                        _ => t.single_vy + self.fwd * 0.25,
                    };
                    self.action = Jump { kind: next };
                    self.on_ground = false;
                    self.events.push(JEvent::Jump(match next {
                        2 => "Double Jump",
                        3 => "Triple Jump",
                        _ => "Jump",
                    }));
                }
                _ if inp.b_pressed => {
                    if self.fwd >= 28.0 {
                        self.dive();
                    } else {
                        self.action = Punch { frames: 0 };
                    }
                }
                _ => {
                    let turn = wrap_pi(want - self.facing);
                    if inp.z_held {
                        self.action = if self.fwd > 10.0 { CrouchSlide } else { Crouch };
                        self.fwd = (self.fwd - if self.fwd > 10.0 { 0.5 } else { 2.0 }).max(0.0);
                    } else if mag > 0.05 && turn.abs() > 1.8 && self.fwd >= 16.0 {
                        // Hard reverse at speed: turnaround skid (side-flip window).
                        self.action = Skid;
                        self.fwd = (self.fwd - 3.0).max(0.0);
                        if self.fwd <= 0.0 {
                            self.facing = want;
                            self.action = Walk;
                        }
                    } else if self.action == Skid && self.fwd > 0.0 {
                        self.fwd = (self.fwd - 3.0).max(0.0);
                        if self.fwd <= 0.0 {
                            self.facing = want;
                        }
                    } else if mag > 0.05 {
                        self.facing += turn.clamp(-t.turn_rate, t.turn_rate);
                        let target = t.run_max * mag;
                        if self.fwd < target {
                            self.fwd = (self.fwd + t.run_accel).min(target);
                        } else {
                            self.fwd = (self.fwd - 1.0).max(target);
                        }
                        self.action = match self.action {
                            JumpLand { kind, frames } if frames <= t.chain_window => JumpLand { kind, frames: frames + 1 },
                            _ => Walk,
                        };
                    } else {
                        self.fwd = (self.fwd - t.walk_decel).max(0.0);
                        self.action = match self.action {
                            JumpLand { kind, frames } if frames <= t.chain_window => JumpLand { kind, frames: frames + 1 },
                            _ => Idle,
                        };
                    }
                }
            }
            self.slide *= 0.0;
            if self.on_ground {
                self.vy = 0.0;
            }
        } else {
            // ---- air actions ----
            match self.action {
                GroundPoundHover { frames } => {
                    self.fwd = 0.0;
                    self.slide = Vec3::ZERO;
                    self.vy = 0.0;
                    self.action = if frames >= t.pound_hover { GroundPound } else { GroundPoundHover { frames: frames + 1 } };
                }
                GroundPound => {
                    self.vy = t.pound_vy;
                }
                WallContact { frames } => {
                    if inp.a_pressed {
                        self.action = WallKick;
                        self.facing = self.wall_normal.x.atan2(self.wall_normal.z);
                        self.fwd = t.wallkick_fwd;
                        self.vy = t.wallkick_vy;
                        self.events.push(JEvent::WallKick);
                    } else if frames >= t.wallkick_window {
                        self.action = Freefall;
                    } else {
                        self.action = WallContact { frames: frames + 1 };
                        self.vy = (self.vy - t.gravity).max(t.terminal);
                    }
                }
                _ => {
                    if inp.z_pressed && !matches!(self.action, LongJump | Dive) {
                        self.action = GroundPoundHover { frames: 0 };
                        self.events.push(JEvent::Jump("Ground Pound"));
                    } else if inp.b_pressed && !matches!(self.action, Dive | LongJump) {
                        self.dive();
                    } else {
                        // Limited air control: speed nudges toward the stick.
                        if mag > 0.05 {
                            let along = (want - self.facing).cos();
                            self.fwd = (self.fwd + along * mag * t.air_accel * 0.35).clamp(-t.air_max, t.air_max.max(self.fwd));
                            let turn = wrap_pi(want - self.facing);
                            self.facing += turn.clamp(-0.035, 0.035);
                        }
                        let g = if self.action == LongJump { t.longjump_gravity } else { t.gravity };
                        // Variable height: letting go of jump while rising cuts the arc.
                        if matches!(self.action, Jump { .. }) && !inp.a_held && self.vy > 20.0 {
                            self.vy *= t.short_hop_cut;
                        }
                        self.vy = (self.vy - g).max(t.terminal);
                    }
                }
            }
        }

        self.integrate(w);
    }

    fn long_jump(&mut self) {
        self.action = JAction::LongJump;
        self.vy = self.t.longjump_vy;
        self.fwd = (self.fwd * self.t.longjump_mult).min(self.t.longjump_cap);
        self.on_ground = false;
        self.events.push(JEvent::Jump("Long Jump"));
    }

    fn dive(&mut self) {
        self.action = JAction::Dive;
        if self.on_ground {
            self.vy = self.t.dive_vy;
            self.on_ground = false;
        }
        self.fwd = (self.fwd + self.t.dive_boost).min(self.t.dive_cap);
        self.events.push(JEvent::Dive);
    }

    /// Moves in quarter-steps with sphere collision (keeps fast falls from tunnelling).
    fn integrate(&mut self, w: &dyn CollisionWorld) {
        let t = &self.t;
        let r = t.radius * UNIT;
        let vel = (dir(self.facing) * self.fwd + self.slide + Vec3::Y * self.vy) * UNIT;
        let mut hit_wall: Option<Vec3> = None;
        let mut landed = false;
        for _ in 0..4 {
            let mut p = self.pos + vel * 0.25;
            // Body sphere at mid height resolves walls and ceilings.
            let mid = p + Vec3::Y * (t.height * 0.5 * UNIT);
            let (mid2, n) = w.resolve_sphere(mid, r);
            if let Some(n) = n {
                if n.y < 0.4 && n.y > -0.4 {
                    hit_wall = Some(Vec3::new(n.x, 0.0, n.z).normalize_or_zero());
                }
                if n.y < -0.5 && self.vy > 0.0 {
                    self.vy = 0.0;
                }
            }
            p += mid2 - mid;
            // Floor probe.
            let probe = if self.on_ground { 0.25 } else { 0.02 + (-vel.y * 0.25).max(0.0) };
            match w.raycast(p + Vec3::Y * 0.5, Vec3::NEG_Y, 0.5 + probe) {
                Some(h) if h.normal.y > 0.6 && (self.vy <= 0.0 || self.on_ground) => {
                    p.y = h.point.y;
                    landed = true;
                }
                _ => {}
            }
            self.pos = p;
        }

        use JAction::*;
        if landed {
            if !self.on_ground {
                self.on_ground = true;
                self.vy = 0.0;
                self.slide = Vec3::ZERO;
                self.events.push(JEvent::Land);
                self.action = match self.action {
                    Jump { kind } => JumpLand { kind, frames: 0 },
                    GroundPound => {
                        self.events.push(JEvent::GroundPoundImpact);
                        self.fwd = 0.0;
                        Idle
                    }
                    Dive => BellySlide,
                    LongJump | Backflip | Sideflip | WallKick | Freefall => {
                        if self.fwd < 0.0 {
                            self.fwd = 0.0;
                        }
                        Walk
                    }
                    a => a,
                };
            }
        } else if self.on_ground {
            self.on_ground = false;
            self.action = Freefall;
        }

        if let Some(n) = hit_wall {
            let into = -dir(self.facing).dot(n) * self.fwd;
            if !self.on_ground
                && into > t.wallkick_min_speed
                && !matches!(self.action, GroundPound | GroundPoundHover { .. } | WallContact { .. })
            {
                self.wall_normal = n;
                self.fwd = 0.0;
                self.action = WallContact { frames: 0 };
            } else if into > 0.0 {
                // Slide along the wall: drop the into-wall component.
                let v = dir(self.facing) * self.fwd;
                let along = v - n * v.dot(n);
                if self.on_ground && along.length() > 0.5 {
                    self.fwd = along.length();
                    self.facing = along.x.atan2(along.z);
                } else {
                    self.fwd = 0.0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::collide::TestWorld;

    fn grounded() -> (Jumper, TestWorld) {
        let w = TestWorld::flat();
        let mut j = Jumper::new(Vec3::new(0.0, 0.3, 0.0), 0.0);
        for _ in 0..30 {
            j.frame(&JumpInput::default(), &w);
        }
        assert!(j.on_ground);
        (j, w)
    }

    fn run(j: &mut Jumper, w: &TestWorld, frames: usize) {
        let inp = JumpInput { stick: [0.0, 1.0], ..Default::default() };
        for _ in 0..frames {
            j.frame(&inp, w);
        }
    }

    fn jump_apex(j: &mut Jumper, w: &TestWorld) -> f32 {
        let base = j.pos.y;
        let mut inp = JumpInput { stick: [0.0, 1.0], a_held: true, a_pressed: true, ..Default::default() };
        let mut top = base;
        for _ in 0..90 {
            j.frame(&inp, w);
            inp.a_pressed = false;
            top = top.max(j.pos.y);
            if j.on_ground {
                break;
            }
        }
        top - base
    }

    #[test]
    fn single_jump_apex_matches_spec() {
        let (mut j, w) = grounded();
        // Standing single jump: 42 u/f launch, 4 u/f^2 gravity applied from the
        // next frame -> 42+38+...+2 = 242 u apex.
        j.frame(&JumpInput { a_pressed: true, a_held: true, ..Default::default() }, &w);
        let mut top = j.pos.y;
        for _ in 0..60 {
            j.frame(&JumpInput { a_held: true, ..Default::default() }, &w);
            top = top.max(j.pos.y);
        }
        assert!((top - 2.42).abs() < 0.03, "apex {top}");
    }

    #[test]
    fn run_reaches_cap_and_triple_jump_chains() {
        let (mut j, w) = grounded();
        run(&mut j, &w, 60);
        assert!((j.fwd - 32.0).abs() < 0.01, "run speed {}", j.fwd);
        let h1 = jump_apex(&mut j, &w);
        let h2 = jump_apex(&mut j, &w);
        let h3 = jump_apex(&mut j, &w);
        assert!(h2 > h1 && h3 > h2, "{h1} {h2} {h3}");
        assert!(j.events.contains(&JEvent::Jump("Triple Jump")));
    }

    #[test]
    fn late_jump_does_not_chain() {
        let (mut j, w) = grounded();
        run(&mut j, &w, 60);
        jump_apex(&mut j, &w);
        run(&mut j, &w, 10); // past the chain window
        j.events.clear();
        jump_apex(&mut j, &w);
        assert_eq!(j.events.first(), Some(&JEvent::Jump("Jump")));
    }

    #[test]
    fn long_jump_is_fast_and_low() {
        let (mut j, w) = grounded();
        run(&mut j, &w, 60);
        j.frame(&JumpInput { stick: [0.0, 1.0], z_held: true, z_pressed: true, ..Default::default() }, &w);
        j.frame(&JumpInput { stick: [0.0, 1.0], z_held: true, a_pressed: true, a_held: true, ..Default::default() }, &w);
        assert_eq!(j.action, JAction::LongJump);
        assert!(j.fwd > 40.0, "{}", j.fwd);
        let z0 = j.pos.z;
        let mut top: f32 = 0.0;
        for _ in 0..60 {
            j.frame(&JumpInput { stick: [0.0, 1.0], a_held: true, ..Default::default() }, &w);
            top = top.max(j.pos.y);
            if j.on_ground {
                break;
            }
        }
        assert!(top < 2.5 && j.pos.z - z0 > 6.0, "top {top}, dist {}", j.pos.z - z0);
    }

    #[test]
    fn backflip_goes_up_and_back() {
        let (mut j, w) = grounded();
        j.frame(&JumpInput { z_held: true, z_pressed: true, ..Default::default() }, &w);
        j.frame(&JumpInput { z_held: true, a_pressed: true, a_held: true, ..Default::default() }, &w);
        assert_eq!(j.action, JAction::Backflip);
        let mut top: f32 = 0.0;
        for _ in 0..90 {
            j.frame(&JumpInput { a_held: true, ..Default::default() }, &w);
            top = top.max(j.pos.y);
        }
        assert!(top > 3.5 && j.pos.z < -0.5, "top {top} z {}", j.pos.z);
    }

    #[test]
    fn wall_kick_bounces_off_wall() {
        let w = TestWorld::flat().with_box(Vec3::new(-10.0, 0.0, 5.0), Vec3::new(10.0, 20.0, 6.0));
        let mut j = Jumper::new(Vec3::new(0.0, 0.3, 0.0), 0.0);
        for _ in 0..10 {
            j.frame(&JumpInput::default(), &w);
        }
        run(&mut j, &w, 20);
        jump_apex_until_wall(&mut j, &w);
        assert!(matches!(j.action, JAction::WallContact { .. }), "{:?}", j.action);
        j.frame(&JumpInput { a_pressed: true, a_held: true, ..Default::default() }, &w);
        assert_eq!(j.action, JAction::WallKick);
        assert!(j.facing.cos() < -0.9, "faces away from wall");
    }

    fn jump_apex_until_wall(j: &mut Jumper, w: &TestWorld) {
        let mut inp = JumpInput { stick: [0.0, 1.0], a_held: true, a_pressed: true, ..Default::default() };
        for _ in 0..60 {
            j.frame(&inp, w);
            inp.a_pressed = false;
            if matches!(j.action, JAction::WallContact { .. }) {
                return;
            }
        }
    }

    #[test]
    fn ground_pound_hovers_then_slams() {
        let (mut j, w) = grounded();
        jump_apex_partial(&mut j, &w, 8);
        j.frame(&JumpInput { z_pressed: true, z_held: true, ..Default::default() }, &w);
        let y = j.pos.y;
        for _ in 0..5 {
            j.frame(&JumpInput::default(), &w);
        }
        assert!((j.pos.y - y).abs() < 0.01, "hover holds height");
        for _ in 0..40 {
            j.frame(&JumpInput::default(), &w);
        }
        assert!(j.events.contains(&JEvent::GroundPoundImpact));
    }

    fn jump_apex_partial(j: &mut Jumper, w: &TestWorld, frames: usize) {
        let mut inp = JumpInput { a_held: true, a_pressed: true, ..Default::default() };
        for _ in 0..frames {
            j.frame(&inp, w);
            inp.a_pressed = false;
        }
    }
}
