//! Clean-room skateboarding sim.
//!
//! Written from observed behaviour only (see docs/specs/skate.md): pushes are
//! discrete kicks, ollies charge while crouched and pop on release, flips must
//! finish rotating before touchdown, landings are judged by board alignment,
//! grinds latch when the board comes down onto a rail. No original code or data.

use super::collide::{CollisionWorld, RailSample};
use bevy::math::{Quat, Vec3};
use std::f32::consts::{PI, TAU};

#[derive(Clone, Debug)]
pub struct SkateTuning {
    pub gravity: f32,
    pub push_impulse: f32,
    pub push_interval: f32,
    pub push_cap: f32,
    pub rolling_resistance: f32,
    pub air_drag: f32,
    pub brake_decel: f32,
    pub powerslide_decel: f32,
    pub carve_rate: f32,
    pub pop_min: f32,
    pub pop_max: f32,
    pub crouch_full: f32,
    pub air_spin_rate: f32,
    pub grind_snap: f32,
    pub grind_friction: f32,
    pub clean_land_deg: f32,
    pub bail_land_deg: f32,
    pub wall_bail_speed: f32,
    pub bail_time: f32,
    pub bank_delay: f32,
    pub radius: f32,
}

impl Default for SkateTuning {
    fn default() -> Self {
        // Initial values from public footage analysis; refined against the
        // black-box measurements in docs/specs/skate.md.
        Self {
            gravity: 19.6,
            push_impulse: 1.7,
            push_interval: 0.62,
            push_cap: 8.0,
            rolling_resistance: 0.28,
            air_drag: 0.004,
            brake_decel: 4.5,
            powerslide_decel: 7.5,
            carve_rate: 2.1,
            pop_min: 4.3,
            pop_max: 5.9,
            crouch_full: 0.32,
            air_spin_rate: 5.6,
            grind_snap: 0.42,
            grind_friction: 0.55,
            clean_land_deg: 24.0,
            bail_land_deg: 62.0,
            wall_bail_speed: 6.5,
            bail_time: 1.5,
            bank_delay: 1.0,
            radius: 0.28,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkState {
    Rolling,
    Manual,
    Powerslide,
    Air,
    Grind,
    Bail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlipKind {
    Kickflip,
    Heelflip,
    ShoveIt,
    TreFlip,
    Hardflip,
}

impl FlipKind {
    pub fn name(self) -> &'static str {
        match self {
            FlipKind::Kickflip => "Kickflip",
            FlipKind::Heelflip => "Heelflip",
            FlipKind::ShoveIt => "Pop Shove-it",
            FlipKind::TreFlip => "360 Flip",
            FlipKind::Hardflip => "Hardflip",
        }
    }
    fn points(self) -> u32 {
        match self {
            FlipKind::Kickflip | FlipKind::Heelflip => 150,
            FlipKind::ShoveIt => 120,
            FlipKind::TreFlip => 300,
            FlipKind::Hardflip => 260,
        }
    }
    fn duration(self) -> f32 {
        match self {
            FlipKind::ShoveIt => 0.36,
            FlipKind::Kickflip | FlipKind::Heelflip => 0.42,
            FlipKind::Hardflip => 0.48,
            FlipKind::TreFlip => 0.55,
        }
    }
    /// (board roll, board yaw) travelled over the whole trick, radians.
    fn rotation(self) -> (f32, f32) {
        match self {
            FlipKind::Kickflip => (TAU, 0.0),
            FlipKind::Heelflip => (-TAU, 0.0),
            FlipKind::ShoveIt => (0.0, PI),
            FlipKind::TreFlip => (TAU, TAU),
            FlipKind::Hardflip => (TAU, PI),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SkateInput {
    pub push: bool,
    pub brake: bool,
    /// Carve / spin: +1 left, -1 right.
    pub steer: f32,
    pub crouch: bool,
    pub nollie: bool,
    pub flip: Option<FlipKind>,
    pub grab: bool,
    /// +1 manual, -1 nose manual.
    pub manual: i8,
    pub powerslide: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SkEvent {
    Trick { name: String, points: u32 },
    Landed { clean: bool },
    Bail,
    GrindStart(&'static str),
    Banked(u32),
}

#[derive(Clone, Debug)]
pub struct Combo {
    pub tricks: Vec<String>,
    pub value: u32,
    pub multiplier: u32,
    pub banked: u64,
    bank_timer: f32,
}

impl Default for Combo {
    fn default() -> Self {
        Self { tricks: vec![], value: 0, multiplier: 0, banked: 0, bank_timer: 0.0 }
    }
}

impl Combo {
    fn add(&mut self, name: &str, base: u32, ev: &mut Vec<SkEvent>) {
        self.multiplier += 1;
        let pts = base * self.multiplier;
        self.value += pts;
        self.tricks.push(name.to_string());
        self.bank_timer = 0.0;
        ev.push(SkEvent::Trick { name: name.to_string(), points: pts });
    }
    fn tick_grind(&mut self, dt: f32) {
        // Grinds score continuously; the line stays alive.
        self.value += (40.0 * dt * self.multiplier.max(1) as f32).ceil() as u32;
        self.bank_timer = 0.0;
    }
    fn lose(&mut self) {
        self.tricks.clear();
        self.value = 0;
        self.multiplier = 0;
    }
    /// External bonus (kills, portals...) from the mashup synergy layer.
    pub fn add_bonus(&mut self, name: &str, base: u32, ev: &mut Vec<SkEvent>) {
        self.add(name, base, ev);
    }
}

#[derive(Clone, Debug)]
pub struct Skater {
    pub t: SkateTuning,
    pub state: SkState,
    /// Board contact point (bottom of the wheels).
    pub pos: Vec3,
    pub vel: Vec3,
    /// Board heading (yaw, radians). Forward = (sin h, 0, cos h).
    pub heading: f32,
    /// Rolling switch/fakie: travel is opposite the nose.
    pub fakie: bool,
    pub ground_normal: Vec3,
    pub crouch: f32,
    pub spin: f32,
    pub flip_roll: f32,
    pub flip_yaw: f32,
    pub grabbing: bool,
    pub manual_balance: f32,
    pub combo: Combo,
    pub events: Vec<SkEvent>,
    flip: Option<(FlipKind, f32)>,
    air_tricks: Vec<(String, u32)>,
    push_timer: f32,
    crouch_time: Option<f32>,
    grind: Option<(u32, f32, &'static str)>, // rail id, direction sign, name
    grind_time: f32,
    bail_timer: f32,
    air_time: f32,
    manual_dir: i8,
    wobble: f32,
}

pub fn forward(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

fn wrap_pi(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

impl Skater {
    pub fn new(pos: Vec3, heading: f32) -> Self {
        Self {
            t: SkateTuning::default(),
            state: SkState::Air,
            pos,
            vel: Vec3::ZERO,
            heading,
            fakie: false,
            ground_normal: Vec3::Y,
            crouch: 0.0,
            spin: 0.0,
            flip_roll: 0.0,
            flip_yaw: 0.0,
            grabbing: false,
            manual_balance: 0.0,
            combo: Combo::default(),
            events: vec![],
            flip: None,
            air_tricks: vec![],
            push_timer: 0.0,
            crouch_time: None,
            grind: None,
            grind_time: 0.0,
            bail_timer: 0.0,
            air_time: 0.0,
            manual_dir: 0,
            wobble: 0.0,
        }
    }

    pub fn speed(&self) -> f32 {
        self.vel.length()
    }

    /// Hand-off from another locomotion mode: keep momentum, face travel.
    pub fn enter(&mut self, pos: Vec3, vel: Vec3, facing: f32) {
        self.pos = pos;
        self.vel = vel;
        let h = Vec3::new(vel.x, 0.0, vel.z);
        self.heading = if h.length() > 0.5 { h.x.atan2(h.z) } else { facing };
        self.fakie = false;
        self.state = SkState::Air;
        self.spin = 0.0;
        self.flip = None;
        self.flip_roll = 0.0;
        self.flip_yaw = 0.0;
        self.air_tricks.clear();
        self.crouch_time = None;
        self.grind = None;
    }

    pub fn board_yaw(&self) -> f32 {
        self.heading + self.spin + self.flip_yaw
    }

    pub fn grind_name(&self) -> Option<&'static str> {
        self.grind.map(|g| g.2)
    }

    pub fn force_bail(&mut self) {
        if self.state != SkState::Bail {
            self.start_bail();
        }
    }

    pub fn step(&mut self, dt: f32, inp: &SkateInput, w: &dyn CollisionWorld) {
        self.wobble += dt;
        match self.state {
            SkState::Rolling | SkState::Manual | SkState::Powerslide => self.step_ground(dt, inp, w),
            SkState::Air => self.step_air(dt, inp, w),
            SkState::Grind => self.step_grind(dt, inp, w),
            SkState::Bail => self.step_bail(dt, w),
        }
        // Bank the combo after a stretch of clean rolling.
        if !self.combo.tricks.is_empty() {
            if matches!(self.state, SkState::Rolling | SkState::Powerslide) {
                self.combo.bank_timer += dt;
                if self.combo.bank_timer >= self.t.bank_delay {
                    let v = self.combo.value;
                    self.combo.banked += v as u64;
                    self.combo.lose();
                    self.events.push(SkEvent::Banked(v));
                }
            } else {
                self.combo.bank_timer = 0.0;
            }
        }
    }

    fn travel_dir(&self) -> Vec3 {
        let f = forward(self.heading);
        if self.fakie { -f } else { f }
    }

    fn step_ground(&mut self, dt: f32, inp: &SkateInput, w: &dyn CollisionWorld) {
        let t = self.t.clone();
        let n = self.ground_normal;
        let mut speed = self.vel.dot(self.travel_dir()).max(0.0);

        // Powerslide: board kicked sideways, heavy scrub, travel direction kept.
        let sliding = inp.powerslide && speed > 2.0 && inp.steer != 0.0;
        if sliding {
            if self.state != SkState::Powerslide {
                self.state = SkState::Powerslide;
            }
            speed = (speed - t.powerslide_decel * dt).max(0.0);
        } else if self.state == SkState::Powerslide {
            self.state = SkState::Rolling;
        }

        // Carving: slower turn-in at very low speed, tighter at mid speed.
        if !sliding {
            let rate = t.carve_rate * (0.55 + 0.45 * (speed / 6.0).min(1.0));
            self.heading += inp.steer * rate * dt;
        }

        // Pushing is a series of kicks, not continuous thrust.
        self.push_timer -= dt;
        let can_push = self.state == SkState::Rolling && self.crouch_time.is_none();
        if can_push && inp.push && self.push_timer <= 0.0 {
            let room = (t.push_cap - speed).max(0.0);
            speed += t.push_impulse.min(room);
            self.push_timer = t.push_interval;
        }
        if inp.brake && !sliding {
            speed = (speed - t.brake_decel * dt).max(0.0);
        }

        // Slope: gravity component along the surface in the travel direction.
        let dir = self.travel_dir();
        let along = (dir - n * dir.dot(n)).normalize_or_zero();
        let slope_acc = Vec3::new(0.0, -t.gravity, 0.0).dot(along);
        speed += slope_acc * dt;
        if speed < 0.0 {
            // Rolled back down: now going fakie.
            self.fakie = !self.fakie;
            speed = -speed;
        }
        speed = (speed - (t.rolling_resistance + t.air_drag * speed * speed) * dt).max(0.0);

        let dir = self.travel_dir();
        let along = (dir - n * dir.dot(n)).normalize_or_zero();
        self.vel = along * speed;

        // Manuals.
        if inp.manual != 0 && speed > 1.0 && self.state == SkState::Rolling {
            self.state = SkState::Manual;
            self.manual_dir = inp.manual;
            self.manual_balance = 0.0;
            let name = if inp.manual > 0 { "Manual" } else { "Nose Manual" };
            self.combo.add(name, 80, &mut self.events);
        }
        if self.state == SkState::Manual {
            if inp.manual == 0 {
                self.state = SkState::Rolling;
            } else {
                self.manual_balance += ((self.wobble * 2.3).sin() * 0.6 + inp.steer * -1.4) * dt;
                self.combo.tick_grind(dt * 0.5);
                if self.manual_balance.abs() > 1.0 {
                    self.start_bail();
                    return;
                }
            }
        }

        // Ollie: crouch while held, pop on release; strength scales with crouch.
        if inp.crouch {
            let ct = self.crouch_time.get_or_insert(0.0);
            *ct += dt;
            self.crouch = (*ct / t.crouch_full).min(1.0);
        } else if let Some(ct) = self.crouch_time.take() {
            let k = (ct / t.crouch_full).min(1.0);
            let pop = t.pop_min + (t.pop_max - t.pop_min) * k;
            self.crouch = 0.0;
            self.pop(pop, inp.nollie);
            return;
        } else {
            self.crouch = (self.crouch - dt * 5.0).max(0.0);
        }

        // Integrate along the surface, then re-seat on the ground.
        let r = t.radius;
        let center = self.pos + n * r + self.vel * dt;
        let (center, contact) = w.resolve_sphere(center, r);
        if let Some(cn) = contact {
            // Wall impact: hard hits bail, glancing ones scrub speed.
            if cn.y < 0.5 {
                let into = -self.vel.dot(cn);
                if into > t.wall_bail_speed {
                    self.pos = center - Vec3::Y * r;
                    self.start_bail();
                    return;
                }
                if into > 0.0 {
                    self.vel += cn * into;
                    let h = Vec3::new(self.vel.x, 0.0, self.vel.z);
                    if h.length() > 0.3 {
                        let yaw = h.x.atan2(h.z);
                        self.heading = if self.fakie { yaw + PI } else { yaw };
                    }
                }
            }
        }
        self.pos = center - Vec3::Y * r;

        match w.raycast(self.pos + Vec3::Y * 0.45, Vec3::NEG_Y, 0.45 + 0.3) {
            Some(h) if h.normal.y > 0.55 => {
                self.pos = h.point;
                self.ground_normal = self.ground_normal.lerp(h.normal, 0.35).normalize();
                // Leaving a ramp lip: surface drops away faster than gravity can hold.
            }
            _ => {
                if self.state == SkState::Manual {
                    self.state = SkState::Air;
                } else {
                    self.state = SkState::Air;
                }
                self.air_time = 0.0;
                self.spin = 0.0;
                self.air_tricks.clear();
            }
        }
        self.heading = wrap_pi(self.heading);
    }

    fn pop(&mut self, vy: f32, nollie: bool) {
        self.state = SkState::Air;
        self.air_time = 0.0;
        self.vel += self.ground_normal * vy;
        self.spin = 0.0;
        self.air_tricks.clear();
        let name = if nollie { "Nollie" } else if self.fakie { "Fakie Ollie" } else { "Ollie" };
        self.air_tricks.push((name.to_string(), 50));
        self.pos += Vec3::Y * 0.02;
    }

    fn step_air(&mut self, dt: f32, inp: &SkateInput, w: &dyn CollisionWorld) {
        let t = self.t.clone();
        self.air_time += dt;
        self.spin += inp.steer * t.air_spin_rate * dt;
        self.grabbing = inp.grab;
        self.crouch = if inp.grab { 0.8 } else { (self.crouch - dt * 4.0).max(0.25) };

        if let Some(kind) = inp.flip
            && self.flip.is_none()
        {
            self.flip = Some((kind, 0.0));
        }
        if let Some((kind, el)) = self.flip.as_mut() {
            let dur = kind.duration();
            let prev = (*el / dur).min(1.0);
            *el += dt;
            let now = (*el / dur).min(1.0);
            let ease = |x: f32| x * x * (3.0 - 2.0 * x);
            let (roll, yaw) = kind.rotation();
            self.flip_roll += roll * (ease(now) - ease(prev));
            self.flip_yaw += yaw * (ease(now) - ease(prev));
            if now >= 1.0 {
                let k = *kind;
                self.air_tricks.push((k.name().to_string(), k.points()));
                self.flip = None;
                self.flip_roll = 0.0;
                // A completed shove-it/hardflip leaves the board rotated.
                let (_, yaw) = k.rotation();
                self.heading += yaw;
                self.flip_yaw = 0.0;
            }
        }
        if inp.grab && !self.air_tricks.iter().any(|(n, _)| n == "Grab") {
            self.air_tricks.push(("Grab".into(), 100));
        }

        self.vel.y -= t.gravity * dt;
        self.vel -= self.vel * t.air_drag * self.vel.length() * dt;
        let r = t.radius;
        let center = self.pos + Vec3::Y * r + self.vel * dt;

        // Rails: latch when coming down onto one.
        if self.vel.y < 0.5
            && let Some(rail) = w.nearest_rail(center - Vec3::Y * r, t.grind_snap)
            && center.y - r >= rail.point.y - 0.15
        {
            self.enter_grind(rail);
            return;
        }

        let (center, contact) = w.resolve_sphere(center, r);
        self.pos = center - Vec3::Y * r;
        if let Some(cn) = contact {
            let into = self.vel.dot(cn);
            if into < 0.0 {
                self.vel -= cn * into;
            }
            if cn.y > 0.55 && self.vel.y <= 0.5 {
                self.land(cn);
            } else if cn.y < 0.3 && -into > t.wall_bail_speed {
                self.start_bail();
            }
        }
    }

    fn land(&mut self, normal: Vec3) {
        let t = &self.t;
        // Flip still rotating on touchdown = feet on a spinning board.
        let flip_unfinished = self.flip.map(|(k, el)| el / k.duration() < 0.82).unwrap_or(false);
        let travel = Vec3::new(self.vel.x, 0.0, self.vel.z);
        let board = self.heading + self.spin + self.flip_yaw;
        let (mis, fakie) = if travel.length() > 0.8 {
            let tyaw = travel.x.atan2(travel.z);
            let d = wrap_pi(board - tyaw).abs();
            if d <= PI / 2.0 { (d, false) } else { (PI - d, true) }
        } else {
            (wrap_pi(self.spin).abs().min((PI - wrap_pi(self.spin).abs()).abs()), self.fakie)
        };
        let deg = mis.to_degrees();
        let air_tricks = std::mem::take(&mut self.air_tricks);
        if flip_unfinished || deg > t.bail_land_deg || self.grabbing {
            for (n, p) in &air_tricks {
                self.combo.add(n, *p, &mut self.events);
            }
            self.start_bail();
            return;
        }
        // Spins count once they're within landing tolerance.
        let spun = self.spin.abs().to_degrees();
        let spin_name = if spun > 520.0 { Some(("540", 500)) } else if spun > 340.0 { Some(("360", 300)) } else if spun > 150.0 { Some(("180", 120)) } else { None };
        for (n, p) in &air_tricks {
            let name = match spin_name {
                Some((s, _)) if n != "Grab" && !n.contains("Ollie") && n != "Nollie" => format!("{s} {n}"),
                _ => n.clone(),
            };
            self.combo.add(&name, *p, &mut self.events);
        }
        if let Some((s, p)) = spin_name
            && air_tricks.iter().all(|(n, _)| n.contains("Ollie") || n == "Nollie" || n == "Grab")
        {
            self.combo.add(&format!("{s}"), p, &mut self.events);
        }
        let clean = deg <= t.clean_land_deg;
        if !clean {
            self.vel *= 0.6; // sketchy landing scrubs speed
        }
        self.heading = wrap_pi(board);
        self.fakie = fakie;
        self.spin = 0.0;
        self.flip = None;
        self.flip_roll = 0.0;
        self.flip_yaw = 0.0;
        self.ground_normal = normal;
        // Keep the horizontal momentum, project onto the surface.
        let into = self.vel.dot(normal);
        if into < 0.0 {
            self.vel -= normal * into;
        }
        self.state = SkState::Rolling;
        self.events.push(SkEvent::Landed { clean });
    }

    fn enter_grind(&mut self, rail: RailSample) {
        let along = self.vel.dot(rail.tangent);
        let sign = if along >= 0.0 { 1.0 } else { -1.0 };
        let speed = self.vel.length().max(2.5);
        let rail_yaw = (rail.tangent.x * sign).atan2(rail.tangent.z * sign);
        let board = self.heading + self.spin + self.flip_yaw;
        let rel = wrap_pi(board - rail_yaw).abs();
        let rel = rel.min(PI - rel);
        let name: &'static str = if rel > 55f32.to_radians() {
            "Boardslide"
        } else if rel > 25f32.to_radians() {
            "Crooked Grind"
        } else if self.vel.y < -3.0 {
            "5-0 Grind"
        } else {
            "50-50 Grind"
        };
        // Air tricks into the rail count.
        for (n, p) in std::mem::take(&mut self.air_tricks) {
            if !n.contains("Ollie") || self.combo.tricks.is_empty() {
                self.combo.add(&n, p, &mut self.events);
            }
        }
        self.flip = None;
        self.flip_roll = 0.0;
        self.flip_yaw = 0.0;
        self.spin = 0.0;
        self.heading = if rel > 55f32.to_radians() { rail_yaw + PI / 2.0 } else { rail_yaw };
        self.fakie = false;
        self.grind = Some((rail.rail, sign, name));
        self.grind_time = 0.0;
        self.vel = rail.tangent * sign * speed;
        self.pos = rail.point;
        self.manual_balance = 0.0;
        self.state = SkState::Grind;
        self.combo.add(name, 100, &mut self.events);
        self.events.push(SkEvent::GrindStart(name));
    }

    fn step_grind(&mut self, dt: f32, inp: &SkateInput, w: &dyn CollisionWorld) {
        let Some((rail_id, sign, _)) = self.grind else {
            self.state = SkState::Air;
            return;
        };
        self.grind_time += dt;
        let speed = (self.vel.length() - self.t.grind_friction * dt).max(0.8);
        let next = self.pos + self.vel * dt;
        match w.nearest_rail(next, 0.6) {
            Some(r) if r.rail == rail_id && r.point.distance(next) < 0.3 => {
                // Downhill rails speed you up.
                let tangent = r.tangent * sign;
                let s = speed - self.t.gravity * tangent.y * dt;
                self.vel = tangent * s.max(0.8);
                self.pos = r.point;
            }
            _ => {
                // Ran off the end.
                self.grind = None;
                self.state = SkState::Air;
                self.air_time = 0.0;
                self.air_tricks.clear();
                return;
            }
        }
        self.combo.tick_grind(dt);
        self.manual_balance += ((self.wobble * 1.9 + self.pos.x).sin() * 0.5 - inp.steer * 1.2) * dt;
        if self.manual_balance.abs() > 1.15 {
            self.grind = None;
            self.start_bail();
            return;
        }
        if inp.crouch {
            self.crouch_time.get_or_insert(0.0);
        } else if self.crouch_time.take().is_some() {
            self.grind = None;
            self.pop(self.t.pop_min, false);
        }
    }

    fn start_bail(&mut self) {
        self.state = SkState::Bail;
        self.bail_timer = self.t.bail_time;
        self.combo.lose();
        self.flip = None;
        self.flip_roll = 0.0;
        self.flip_yaw = 0.0;
        self.air_tricks.clear();
        self.crouch_time = None;
        self.events.push(SkEvent::Bail);
        let h = Vec3::new(self.vel.x, 0.0, self.vel.z);
        self.vel = h * 0.45 + Vec3::Y * 2.5;
    }

    fn step_bail(&mut self, dt: f32, w: &dyn CollisionWorld) {
        self.bail_timer -= dt;
        self.vel.y -= self.t.gravity * dt;
        let r = self.t.radius;
        let (c, n) = w.resolve_sphere(self.pos + Vec3::Y * r + self.vel * dt, r);
        self.pos = c - Vec3::Y * r;
        if let Some(n) = n {
            let into = self.vel.dot(n);
            if into < 0.0 {
                self.vel -= n * into * 1.3;
            }
            self.vel *= 1.0 - (3.0 * dt).min(1.0);
        }
        if self.bail_timer <= 0.0 {
            // Get back up where you fell, board under your feet.
            self.vel = Vec3::ZERO;
            self.spin = 0.0;
            self.fakie = false;
            self.state = SkState::Air;
            self.manual_balance = 0.0;
        }
    }

    /// Board orientation for rendering.
    pub fn board_rotation(&self) -> Quat {
        let up = self.ground_normal;
        let yaw = Quat::from_rotation_y(self.board_yaw());
        let tilt = Quat::from_rotation_arc(Vec3::Y, if matches!(self.state, SkState::Air) { Vec3::Y } else { up });
        let manual = match self.state {
            SkState::Manual => Quat::from_rotation_x(-0.18 * self.manual_dir as f32),
            _ => Quat::IDENTITY,
        };
        let slide = if self.state == SkState::Powerslide { Quat::from_rotation_y(PI / 2.0) } else { Quat::IDENTITY };
        tilt * yaw * slide * manual * Quat::from_rotation_z(self.flip_roll)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::collide::TestWorld;

    const DT: f32 = 1.0 / 120.0;

    fn settle(s: &mut Skater, w: &TestWorld) {
        for _ in 0..60 {
            s.step(DT, &SkateInput::default(), w);
        }
    }

    #[test]
    fn lands_from_spawn_and_rolls() {
        let w = TestWorld::flat();
        let mut s = Skater::new(Vec3::new(0.0, 0.5, 0.0), 0.0);
        settle(&mut s, &w);
        assert_eq!(s.state, SkState::Rolling);
        assert!(s.pos.y.abs() < 0.02, "{:?}", s.pos);
    }

    #[test]
    fn pushes_are_discrete_and_capped() {
        let w = TestWorld::flat();
        let mut s = Skater::new(Vec3::new(0.0, 0.0, 0.0), 0.0);
        settle(&mut s, &w);
        let inp = SkateInput { push: true, ..Default::default() };
        s.step(DT, &inp, &w);
        let after_one = s.speed();
        assert!((after_one - s.t.push_impulse).abs() < 0.05, "{after_one}");
        for _ in 0..(120 * 20) {
            s.step(DT, &inp, &w);
        }
        assert!(s.speed() <= s.t.push_cap + 0.01 && s.speed() > s.t.push_cap - 1.0, "{}", s.speed());
        assert!(s.pos.z > 50.0, "travelled forward along +z: {:?}", s.pos);
    }

    #[test]
    fn charged_ollie_pops_higher() {
        let w = TestWorld::flat();
        let apex = |hold: usize| {
            let mut s = Skater::new(Vec3::ZERO, 0.0);
            settle(&mut s, &w);
            for _ in 0..hold {
                s.step(DT, &SkateInput { crouch: true, ..Default::default() }, &w);
            }
            let mut top: f32 = 0.0;
            for _ in 0..120 {
                s.step(DT, &SkateInput::default(), &w);
                top = top.max(s.pos.y);
            }
            assert_eq!(s.state, SkState::Rolling);
            top
        };
        let tap = apex(2);
        let full = apex(60);
        assert!(tap > 0.4 && tap < 0.6, "tap ollie apex {tap}");
        assert!(full > 0.8 && full < 1.0, "charged ollie apex {full}");
    }

    #[test]
    fn kickflip_lands_clean_when_given_time() {
        let w = TestWorld::flat();
        let mut s = Skater::new(Vec3::ZERO, 0.0);
        settle(&mut s, &w);
        for _ in 0..40 {
            s.step(DT, &SkateInput { crouch: true, ..Default::default() }, &w);
        }
        s.step(DT, &SkateInput::default(), &w);
        s.step(DT, &SkateInput { flip: Some(FlipKind::Kickflip), ..Default::default() }, &w);
        for _ in 0..120 {
            s.step(DT, &SkateInput::default(), &w);
        }
        assert_eq!(s.state, SkState::Rolling);
        assert!(s.events.iter().any(|e| matches!(e, SkEvent::Trick { name, .. } if name == "Kickflip")), "{:?}", s.events);
        assert!(!s.events.contains(&SkEvent::Bail));
    }

    #[test]
    fn late_flip_bails() {
        let w = TestWorld::flat();
        let mut s = Skater::new(Vec3::ZERO, 0.0);
        settle(&mut s, &w);
        s.step(DT, &SkateInput { crouch: true, ..Default::default() }, &w);
        s.step(DT, &SkateInput::default(), &w);
        // Wait until just before touchdown, then flip.
        while s.vel.y > -3.5 {
            s.step(DT, &SkateInput::default(), &w);
        }
        s.step(DT, &SkateInput { flip: Some(FlipKind::TreFlip), ..Default::default() }, &w);
        for _ in 0..60 {
            s.step(DT, &SkateInput::default(), &w);
        }
        assert!(s.events.contains(&SkEvent::Bail), "{:?}", s.events);
    }

    #[test]
    fn grinds_rail_and_scores() {
        let mut w = TestWorld::flat();
        w.rails.add(vec![Vec3::new(0.0, 0.5, 2.0), Vec3::new(0.0, 0.5, 12.0)]);
        let mut s = Skater::new(Vec3::new(0.0, 0.0, -2.0), 0.0);
        settle(&mut s, &w);
        s.vel = Vec3::new(0.0, 0.0, 6.0);
        for _ in 0..40 {
            s.step(DT, &SkateInput { crouch: true, ..Default::default() }, &w);
        }
        let mut grinded = false;
        for _ in 0..240 {
            s.step(DT, &SkateInput::default(), &w);
            grinded |= s.state == SkState::Grind;
        }
        assert!(grinded, "{:?}", s.events);
        assert!(s.events.iter().any(|e| matches!(e, SkEvent::GrindStart("50-50 Grind"))));
    }

    #[test]
    fn hard_wall_hit_bails() {
        let w = TestWorld::flat().with_box(Vec3::new(-5.0, 0.0, 3.0), Vec3::new(5.0, 3.0, 4.0));
        let mut s = Skater::new(Vec3::ZERO, 0.0);
        settle(&mut s, &w);
        s.vel = Vec3::new(0.0, 0.0, 9.0);
        for _ in 0..120 {
            s.step(DT, &SkateInput::default(), &w);
        }
        assert!(s.events.contains(&SkEvent::Bail));
    }
}
