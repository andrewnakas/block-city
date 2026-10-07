//! STREETS mode: traffic, pedestrians, carjacking, a five-star wanted level
//! and police that chase, ram and shoot. Cars are driven by the same arcade
//! sim whether you, traffic or a cop is behind the wheel.

use crate::core::camera::CamRig;
use crate::core::fx::{Explosion, FxAssets};
use crate::core::input::{Latch, Look};
use crate::core::modes::{ActiveModes, Mode, ModeToggled, mode_on};
use crate::core::phys::{G_DYNAMIC, Rails, PhysWorld};
use crate::core::player::{JumpSim, Loco, MoveSet, PlayerState, SkateSim, WalkSim, enter_loco};
use crate::core::score::{Kind, MashEvent, Score};
use crate::core::ui::Hud;
use crate::modes::warfare::{Owner, Projectile};
use crate::sim::car::{CarInput, CarSim};
use crate::world::city::{BLOCK, GRID, road_lines};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use bevy_rapier3d::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Driver {
    Player,
    Traffic,
    Police,
    Parked,
}

#[derive(Component)]
pub struct Car {
    pub sim: CarSim,
    pub driver: Driver,
    /// Lane-following target for traffic AI.
    target: Vec3,
    stuck: f32,
    siren: f32,
    wrecked: bool,
}

#[derive(Component)]
struct Wheel;
#[derive(Component)]
struct LightBar;
#[derive(Component)]
pub struct Ped {
    goal: Vec3,
    flee: f32,
    vel: Vec3,
}

#[derive(Resource, Default)]
pub struct Wanted {
    pub heat: f32,
    pub stars: u32,
    unseen: f32,
}

impl Wanted {
    pub fn crime(&mut self, heat: f32) {
        self.heat = (self.heat + heat).min(5.99);
        self.stars = self.stars.max(self.heat as u32);
        self.unseen = 0.0;
    }
}

#[derive(Resource)]
struct CarAssets {
    cube: Handle<Mesh>,
    wheel: Handle<Mesh>,
    paints: Vec<Handle<StandardMaterial>>,
    police: Handle<StandardMaterial>,
    glass: Handle<StandardMaterial>,
    tire: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
    blue: Handle<StandardMaterial>,
    wreck: Handle<StandardMaterial>,
    ped: Vec<Handle<StandardMaterial>>,
}

const TRAFFIC: usize = 22;
const PEDS: usize = 26;
const LANE: f32 = 3.5;

pub struct StreetsPlugin;

impl Plugin for StreetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Wanted>()
            .add_systems(Startup, setup)
            .add_systems(FixedUpdate, enter_exit.in_set(MoveSet::Arbitrate).after(crate::core::player::arbitrate).run_if(mode_on(Mode::Streets)))
            .add_systems(FixedUpdate, drive_cars.in_set(MoveSet::Locomotion).run_if(mode_on(Mode::Streets)))
            .add_systems(FixedUpdate, (peds, car_hits, police_fire).in_set(MoveSet::Abilities).run_if(mode_on(Mode::Streets)))
            .add_systems(Update, (toggle, populate.run_if(mode_on(Mode::Streets)), wanted_logic.run_if(mode_on(Mode::Streets)), explosions, visuals, hud.run_if(mode_on(Mode::Streets))))
            .add_systems(EguiPrimaryContextPass, minimap.run_if(mode_on(Mode::Streets)));
    }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let mut m = |c: Color, metal: f32| mats.add(StandardMaterial { base_color: c, metallic: metal, perceptual_roughness: 0.4, ..default() });
    let paints = [[200, 40, 40], [40, 90, 200], [230, 200, 60], [60, 160, 90], [230, 230, 235], [40, 40, 45], [150, 80, 170], [240, 140, 40]]
        .iter()
        .map(|c| m(Color::srgb_u8(c[0], c[1], c[2]), 0.3))
        .collect();
    let ped = [[200, 90, 90], [90, 120, 200], [220, 200, 120], [120, 180, 120], [60, 60, 70]].iter().map(|c| m(Color::srgb_u8(c[0], c[1], c[2]), 0.0)).collect();
    commands.insert_resource(CarAssets {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        wheel: meshes.add(Cylinder::new(0.36, 0.26)),
        paints,
        police: m(Color::srgb(0.08, 0.08, 0.1), 0.3),
        glass: m(Color::srgba(0.2, 0.3, 0.4, 1.0), 0.8),
        tire: m(Color::srgb(0.06, 0.06, 0.06), 0.0),
        red: mats.add(StandardMaterial { base_color: Color::srgb(1.0, 0.1, 0.1), emissive: LinearRgba::rgb(6.0, 0.2, 0.2), ..default() }),
        blue: mats.add(StandardMaterial { base_color: Color::srgb(0.1, 0.2, 1.0), emissive: LinearRgba::rgb(0.2, 0.6, 6.0), ..default() }),
        wreck: mats.add(StandardMaterial { base_color: Color::srgb(0.08, 0.07, 0.06), perceptual_roughness: 1.0, ..default() }),
        ped,
    });
}

fn spawn_car(commands: &mut Commands, a: &CarAssets, pos: Vec3, yaw: f32, driver: Driver, paint: usize) -> Entity {
    let body = if driver == Driver::Police { a.police.clone() } else { a.paints[paint % a.paints.len()].clone() };
    let sim = CarSim::new(pos, yaw);
    let mut e = commands.spawn((
        Car { sim, driver, target: pos, stuck: 0.0, siren: 0.0, wrecked: false },
        Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
        Visibility::default(),
        RigidBody::KinematicPositionBased,
        Collider::cuboid(0.95, 0.55, 2.1),
        CollisionGroups::new(G_DYNAMIC, Group::ALL),
        Velocity::default(),
    ));
    e.with_children(|c| {
        c.spawn((Mesh3d(a.cube.clone()), MeshMaterial3d(body.clone()), Transform::from_xyz(0.0, 0.05, 0.0).with_scale(Vec3::new(1.9, 0.6, 4.3))));
        c.spawn((Mesh3d(a.cube.clone()), MeshMaterial3d(a.glass.clone()), Transform::from_xyz(0.0, 0.6, -0.25).with_scale(Vec3::new(1.7, 0.55, 2.1))));
        c.spawn((Mesh3d(a.cube.clone()), MeshMaterial3d(body.clone()), Transform::from_xyz(0.0, 0.9, -0.25).with_scale(Vec3::new(1.75, 0.06, 2.0))));
        for (x, z) in [(-0.95, 1.35), (0.95, 1.35), (-0.95, -1.35), (0.95, -1.35)] {
            c.spawn((Wheel, Mesh3d(a.wheel.clone()), MeshMaterial3d(a.tire.clone()), Transform::from_xyz(x, -0.15, z).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))));
        }
        if driver == Driver::Police {
            c.spawn((Mesh3d(a.cube.clone()), MeshMaterial3d(a.paints[4].clone()), Transform::from_xyz(0.0, 0.05, 0.0).with_scale(Vec3::new(1.92, 0.3, 1.6))));
            c.spawn((LightBar, Mesh3d(a.cube.clone()), MeshMaterial3d(a.red.clone()), Transform::from_xyz(-0.35, 1.0, -0.25).with_scale(Vec3::new(0.6, 0.15, 0.3))));
            c.spawn((LightBar, Mesh3d(a.cube.clone()), MeshMaterial3d(a.blue.clone()), Transform::from_xyz(0.35, 1.0, -0.25).with_scale(Vec3::new(0.6, 0.15, 0.3))));
        }
    });
    e.id()
}

/// Nearest lane point + heading for a position (right-hand traffic).
fn lane_target(pos: Vec3, yaw: f32) -> Vec3 {
    let f = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let lines = road_lines();
    let snap = |v: f32| lines.iter().copied().min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs())).unwrap_or(0.0);
    let ahead = pos + f * 14.0;
    // Travelling along Z: x is the road line; lane is offset to the driver's right.
    if f.z.abs() > f.x.abs() {
        let x = snap(pos.x) - f.z.signum() * LANE;
        Vec3::new(x, 0.0, ahead.z)
    } else {
        let z = snap(pos.z) + f.x.signum() * LANE;
        Vec3::new(ahead.x, 0.0, z)
    }
}

fn populate(mut commands: Commands, a: Res<CarAssets>, cars: Query<&Car>, peds: Query<&Ped>, time: Res<Time>, mut n: Local<u32>, p: Res<PlayerState>) {
    let traffic = cars.iter().filter(|c| c.driver == Driver::Traffic && !c.wrecked).count();
    let lines = road_lines();
    let ext = (GRID as f32 + 0.5) * BLOCK;
    if traffic < TRAFFIC {
        *n += 1;
        let k = *n as f32;
        let along = ((k * 0.618).fract() - 0.5) * 2.0 * ext;
        let line = lines[(*n as usize * 7) % lines.len()];
        let (pos, yaw) = if *n % 2 == 0 {
            let dir = if *n % 4 == 0 { 0.0 } else { std::f32::consts::PI };
            (Vec3::new(line - dir.cos() * LANE, 0.6, along), dir)
        } else {
            let dir = if *n % 4 == 1 { std::f32::consts::FRAC_PI_2 } else { -std::f32::consts::FRAC_PI_2 };
            (Vec3::new(along, 0.6, line + dir.sin() * LANE), dir)
        };
        if pos.distance(p.pos) > 25.0 {
            spawn_car(&mut commands, &a, pos, yaw, Driver::Traffic, *n as usize);
        }
    }
    // A few parked cars by the plaza for easy jacking.
    if *n == 3 {
        for (k, x) in [-8.0f32, 0.0, 8.0].iter().enumerate() {
            spawn_car(&mut commands, &a, Vec3::new(*x, 0.6, -BLOCK * 0.5 + 4.0), std::f32::consts::PI, Driver::Parked, k + 2);
        }
    }
    if peds.iter().count() < PEDS {
        let t = time.elapsed_secs() + *n as f32;
        let line = lines[(*n as usize * 3) % lines.len()];
        let side = if *n % 2 == 0 { 9.0 } else { -9.0 };
        let along = ((t * 0.37).sin()) * ext;
        let pos = if *n % 3 == 0 { Vec3::new(line + side, 0.0, along) } else { Vec3::new(along, 0.0, line + side) };
        let mat = a.ped[*n as usize % a.ped.len()].clone();
        commands
            .spawn((Ped { goal: pos, flee: 0.0, vel: Vec3::ZERO }, Transform::from_translation(pos), Visibility::default()))
            .with_children(|c| {
                c.spawn((Mesh3d(a.cube.clone()), MeshMaterial3d(mat), Transform::from_xyz(0.0, 0.9, 0.0).with_scale(Vec3::new(0.45, 1.5, 0.3))));
                c.spawn((Mesh3d(a.cube.clone()), MeshMaterial3d(a.paints[7].clone()), Transform::from_xyz(0.0, 1.8, 0.0).with_scale(Vec3::splat(0.26))));
            });
    }
}

fn toggle(mut t: MessageReader<ModeToggled>, mut commands: Commands, cars: Query<(Entity, &Car)>, peds: Query<Entity, With<Ped>>, mut wanted: ResMut<Wanted>) {
    for ev in t.read() {
        if ev.mode == Mode::Streets && !ev.on {
            for (e, c) in cars.iter() {
                if c.driver != Driver::Player {
                    commands.entity(e).despawn();
                }
            }
            for e in peds.iter() {
                commands.entity(e).despawn();
            }
            *wanted = Wanted::default();
        }
    }
}

/// E: steal the nearest car (from foot, board or mid-swing) or get out.
#[allow(clippy::too_many_arguments)]
fn enter_exit(
    latch: Res<Latch>,
    modes: Res<ActiveModes>,
    mut p: ResMut<PlayerState>,
    mut cars: Query<(Entity, &mut Car)>,
    mut skate: ResMut<SkateSim>,
    mut jump: ResMut<JumpSim>,
    mut walk: ResMut<WalkSim>,
    mut wanted: ResMut<Wanted>,
    mut out: MessageWriter<MashEvent>,
    mut hud: ResMut<Hud>,
    a: Res<CarAssets>,
    mut commands: Commands,
) {
    if !latch.key(KeyCode::KeyE) {
        return;
    }
    if p.loco == Loco::Vehicle {
        let Some((_, mut car)) = cars.iter_mut().find(|(_, c)| c.driver == Driver::Player) else { return };
        car.driver = Driver::Parked;
        let side = Vec3::new(car.sim.yaw.cos(), 0.0, -car.sim.yaw.sin());
        p.pos = car.sim.pos + side * 1.8 + Vec3::Y * 0.3;
        p.vel = car.sim.vel;
        let back = if p.resume == Loco::Skate && modes.on(Mode::Skate) { Loco::Skate } else { Loco::Foot };
        enter_loco(back, &mut p, &modes, &mut skate, &mut jump, &mut walk);
        return;
    }
    let best = cars
        .iter_mut()
        .filter(|(_, c)| !c.wrecked && c.driver != Driver::Player)
        .map(|(e, c)| (e, c.sim.pos.distance(p.pos)))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((e, d)) = best else { return };
    // Reach is generous when you arrive fast (swinging / flying into the car).
    if d > 4.0 + p.vel.length() * 0.15 {
        return;
    }
    let Ok((_, mut car)) = cars.get_mut(e) else { return };
    let occupied = matches!(car.driver, Driver::Traffic | Driver::Police);
    if occupied {
        // The old driver gets dragged out and runs.
        let side = Vec3::new(car.sim.yaw.cos(), 0.0, -car.sim.yaw.sin());
        commands
            .spawn((Ped { goal: car.sim.pos + side * 30.0, flee: 8.0, vel: Vec3::ZERO }, Transform::from_translation(car.sim.pos + side * 2.0 - Vec3::Y * 0.6), Visibility::default()))
            .with_children(|c| {
                c.spawn((Mesh3d(a.cube.clone()), MeshMaterial3d(a.ped[0].clone()), Transform::from_xyz(0.0, 0.9, 0.0).with_scale(Vec3::new(0.45, 1.5, 0.3))));
            });
        wanted.crime(if car.driver == Driver::Police { 2.0 } else { 1.0 });
    }
    let airborne = !p.on_ground || p.loco == Loco::Swing;
    out.write(MashEvent::new(Kind::Carjack, if airborne { "Flying Carjack" } else { "Carjack" }, if occupied { 300 } else { 100 }));
    hud.banner = Some(("E to get out".into(), 1.5));
    // Arriving fast? The car inherits some of your momentum.
    let carry = p.vel.with_y(0.0) * 0.5;
    car.sim.vel += carry;
    car.driver = Driver::Player;
    if matches!(p.loco, Loco::Foot | Loco::Skate) {
        p.resume = p.loco;
    }
    p.loco = Loco::Vehicle;
}

#[allow(clippy::too_many_arguments)]
fn drive_cars(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    rapier: ReadRapierContext,
    rails: Res<Rails>,
    holes: Res<crate::core::phys::Holes>,
    mut p: ResMut<PlayerState>,
    look: Res<Look>,
    wanted: Res<Wanted>,
    mut cars: Query<(Entity, &mut Car, &mut Transform)>,
) -> Result {
    let ctx = rapier.single()?;
    let dt = time.delta_secs();
    // Car positions for simple car-ahead braking.
    let others: Vec<(Entity, Vec3)> = cars.iter().map(|(e, c, _)| (e, c.sim.pos)).collect();
    for (e, mut car, mut tf) in cars.iter_mut() {
        if car.wrecked {
            continue;
        }
        let w = PhysWorld { ctx: &ctx, rails: &rails.0, holes: &holes.0, exclude: Some(e) };
        let inp = match car.driver {
            Driver::Player => {
                let kick = std::mem::take(&mut p.impulse);
                car.sim.vel += kick * 0.3;
                let k = |c: KeyCode| keys.pressed(c);
                CarInput {
                    throttle: if k(KeyCode::KeyW) { 1.0 } else if k(KeyCode::KeyS) { -1.0 } else { 0.0 },
                    steer: if k(KeyCode::KeyA) { 1.0 } else if k(KeyCode::KeyD) { -1.0 } else { 0.0 },
                    handbrake: k(KeyCode::Space),
                }
            }
            Driver::Parked => CarInput { throttle: 0.0, steer: 0.0, handbrake: true },
            Driver::Traffic | Driver::Police => {
                let chasing = car.driver == Driver::Police && wanted.stars > 0;
                let goal = if chasing { p.pos } else { car.target };
                if !chasing && car.sim.pos.with_y(0.0).distance(car.target.with_y(0.0)) < 6.0 || car.target == car.sim.pos {
                    // Pick the next lane target; turn at intersections now and then.
                    let mut yaw = car.sim.yaw;
                    let near_cross = road_lines().iter().any(|l| (car.sim.pos.x - l).abs() < 6.0) && road_lines().iter().any(|l| (car.sim.pos.z - l).abs() < 6.0);
                    if near_cross && (car.sim.pos.x * 0.13 + car.sim.pos.z * 0.07).sin() > 0.3 {
                        yaw += if (car.sim.pos.x * 0.31).cos() > 0.0 { std::f32::consts::FRAC_PI_2 } else { -std::f32::consts::FRAC_PI_2 };
                    }
                    let snapped = (yaw / std::f32::consts::FRAC_PI_2).round() * std::f32::consts::FRAC_PI_2;
                    car.target = lane_target(car.sim.pos, snapped);
                }
                let to = (goal - car.sim.pos).with_y(0.0);
                let want_yaw = to.x.atan2(to.z);
                let diff = (want_yaw - car.sim.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                let f = Vec3::new(car.sim.yaw.sin(), 0.0, car.sim.yaw.cos());
                let blocked = others.iter().any(|(oe, op)| *oe != e && {
                    let d = *op - car.sim.pos;
                    d.dot(f) > 0.0 && d.dot(f) < 12.0 && (d - f * d.dot(f)).length() < 2.2
                }) || ((p.pos - car.sim.pos).dot(f) > 0.0 && (p.pos - car.sim.pos).dot(f) < 10.0 && ((p.pos - car.sim.pos) - f * (p.pos - car.sim.pos).dot(f)).length() < 2.0 && !chasing);
                let cruise = if chasing { 40.0 } else { 14.0 };
                let speed = car.sim.forward_speed();
                let throttle = if blocked { -1.0 } else if speed < cruise * (1.0 - diff.abs().min(1.0) * 0.6) { 1.0 } else { 0.0 };
                if speed.abs() < 0.5 && throttle > 0.0 {
                    car.stuck += dt;
                } else {
                    car.stuck = 0.0;
                }
                if car.stuck > 3.0 {
                    car.sim.yaw += 0.8;
                    car.stuck = 0.0;
                }
                CarInput { throttle, steer: (diff * 2.0).clamp(-1.0, 1.0), handbrake: chasing && diff.abs() > 1.2 }
            }
        };
        car.sim.step(dt, &inp, &w);
        if car.driver == Driver::Police {
            car.siren += dt;
        }
        tf.translation = car.sim.pos;
        tf.rotation = car.sim.rotation();
        if car.driver == Driver::Player {
            p.pos = car.sim.pos + Vec3::Y * 0.2;
            p.vel = car.sim.vel;
            p.facing = car.sim.yaw;
            p.on_ground = car.sim.on_ground;
            if car.sim.health <= 0.0 {
                p.health = 0.0;
            }
        }
        let _ = &look;
    }
    Ok(())
}

/// Cars hitting people, skaters and each other.
#[allow(clippy::too_many_arguments)]
fn car_hits(
    mut p: ResMut<PlayerState>,
    mut cars: Query<(&mut Car, &Transform)>,
    mut peds: Query<(Entity, &mut Ped, &Transform), Without<Car>>,
    mut commands: Commands,
    mut wanted: ResMut<Wanted>,
    mut out: MessageWriter<MashEvent>,
) {
    let mut list: Vec<_> = cars.iter_mut().collect();
    for i in 0..list.len() {
        let (car, _) = &list[i];
        if car.wrecked {
            continue;
        }
        let speed = car.sim.vel.length();
        let pos = car.sim.pos;
        let vel = car.sim.vel;
        let player_car = car.driver == Driver::Player;
        // Player struck by a car (not their own).
        if !player_car && p.loco != Loco::Vehicle && speed > 4.0 && pos.distance(p.pos + Vec3::Y * 0.6) < 2.2 {
            p.impulse += vel * 0.9 + Vec3::Y * 4.0;
            p.health -= speed * 1.5;
        }
        for (pe, mut ped, pt) in peds.iter_mut() {
            if speed > 5.0 && pos.distance(pt.translation + Vec3::Y * 0.8) < 2.0 {
                commands.entity(pe).despawn();
                if player_car {
                    wanted.crime(0.6);
                    out.write(MashEvent::new(Kind::Crime, "Road Rage", 40));
                }
            } else if speed > 10.0 && pos.distance(pt.translation) < 9.0 {
                ped.flee = 3.0;
            }
        }
        // Car-car shoves.
        for j in (i + 1)..list.len() {
            let d = list[j].0.sim.pos - list[i].0.sim.pos;
            let dist = d.with_y(0.0).length();
            if dist < 3.6 && dist > 0.01 {
                let n = d.with_y(0.0) / dist;
                let rel = (list[i].0.sim.vel - list[j].0.sim.vel).dot(n);
                let push = n * (3.6 - dist) * 0.5;
                list[i].0.sim.pos -= push;
                list[j].0.sim.pos += push;
                if rel > 0.0 {
                    let imp = n * rel * 0.6;
                    list[i].0.sim.vel -= imp;
                    list[j].0.sim.vel += imp;
                    if rel > 8.0 {
                        list[i].0.sim.health -= rel * 2.0;
                        list[j].0.sim.health -= rel * 2.0;
                        if list[i].0.driver == Driver::Player || list[j].0.driver == Driver::Player {
                            let cop = list[i].0.driver == Driver::Police || list[j].0.driver == Driver::Police;
                            wanted.crime(if cop { 1.0 } else { 0.25 });
                        }
                    }
                }
            }
        }
    }
}

fn peds(time: Res<Time>, mut q: Query<(&mut Ped, &mut Transform)>, p: Res<PlayerState>) {
    let dt = time.delta_secs();
    for (mut ped, mut tf) in q.iter_mut() {
        ped.flee = (ped.flee - dt).max(0.0);
        let to = (ped.goal - tf.translation).with_y(0.0);
        if to.length() < 1.0 {
            let s = (tf.translation.x * 0.71 + tf.translation.z * 0.37).sin();
            ped.goal = tf.translation + Vec3::new(s * 20.0, 0.0, (s * 3.0).cos() * 20.0);
        }
        let speed = if ped.flee > 0.0 { 5.5 } else { 1.4 };
        let mut dir = to.normalize_or_zero();
        if ped.flee > 0.0 {
            let away = (tf.translation - p.pos).with_y(0.0).normalize_or_zero();
            dir = (dir * 0.3 + away).normalize_or_zero();
        }
        ped.vel = ped.vel.lerp(dir * speed, (dt * 4.0).min(1.0));
        tf.translation += ped.vel * dt;
        if ped.vel.length() > 0.1 {
            tf.rotation = Quat::from_rotation_y(ped.vel.x.atan2(ped.vel.z));
        }
    }
}

fn wanted_logic(
    time: Res<Time>,
    mut wanted: ResMut<Wanted>,
    mut commands: Commands,
    a: Res<CarAssets>,
    cars: Query<(Entity, &Car)>,
    mut p: ResMut<PlayerState>,
    mut hud: ResMut<Hud>,
    mut score: ResMut<Score>,
    mut spawn_cd: Local<f32>,
) {
    let dt = time.delta_secs();
    // Gunfire near people is a crime.
    let cops: Vec<Vec3> = cars.iter().filter(|(_, c)| c.driver == Driver::Police && !c.wrecked).map(|(_, c)| c.sim.pos).collect();
    if wanted.stars > 0 {
        let close = cops.iter().any(|c| c.distance(p.pos) < 70.0);
        if close {
            wanted.unseen = 0.0;
        } else {
            wanted.unseen += dt;
            if wanted.unseen > 10.0 {
                wanted.stars -= 1;
                wanted.heat = wanted.stars as f32;
                wanted.unseen = 0.0;
                if wanted.stars == 0 {
                    hud.banner = Some(("LOST THE COPS".into(), 2.0));
                }
            }
        }
        // Keep cops on you: stars x2 cruisers.
        *spawn_cd -= dt;
        if cops.len() < (wanted.stars * 2) as usize && *spawn_cd <= 0.0 {
            *spawn_cd = 3.0;
            let ang = time.elapsed_secs() * 1.7;
            let at = p.pos + Vec3::new(ang.sin(), 0.0, ang.cos()) * 70.0;
            let lines = road_lines();
            let snap = |v: f32| lines.iter().copied().min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs())).unwrap_or(0.0);
            let pos = Vec3::new(snap(at.x), 0.6, at.z);
            let to = p.pos - pos;
            spawn_car(&mut commands, &a, pos, to.x.atan2(to.z), Driver::Police, 0);
        }
    } else {
        // Despawn cops that are out of the chase.
        for (e, c) in cars.iter() {
            if c.driver == Driver::Police && c.sim.pos.distance(p.pos) > 120.0 {
                commands.entity(e).despawn();
            }
        }
    }
    if p.health <= 0.0 && wanted.stars > 0 {
        hud.banner = Some(("WASTED".into(), 2.5));
        *wanted = Wanted::default();
        score.kill_streak = 0;
        p.health = 0.0;
    }
}

fn police_fire(time: Res<Time>, wanted: Res<Wanted>, cars: Query<&Car>, p: Res<PlayerState>, fx: Res<FxAssets>, mut commands: Commands, mut cd: Local<f32>) {
    if wanted.stars < 3 {
        return;
    }
    *cd -= time.delta_secs();
    if *cd > 0.0 {
        return;
    }
    *cd = 1.4 - wanted.stars as f32 * 0.15;
    for car in cars.iter().filter(|c| c.driver == Driver::Police && !c.wrecked) {
        let from = car.sim.pos + Vec3::Y * 1.2;
        let to = p.pos + Vec3::Y * 1.0 - from;
        if to.length() > 45.0 {
            continue;
        }
        let t = time.elapsed_secs();
        let dir = (to.normalize() + Vec3::new((t * 13.0).sin(), (t * 7.0).sin(), (t * 11.0).cos()) * 0.06).normalize();
        commands.spawn((
            Projectile { vel: dir * 200.0, owner: Owner::Bot, damage: 6.0, life: 1.0, rocket: false, portaled: false },
            Mesh3d(fx.cube.clone()),
            MeshMaterial3d(fx.tracer.clone()),
            Transform::from_translation(from + dir * 2.5).with_scale(Vec3::new(0.03, 0.03, 0.7)),
        ));
    }
}

/// Explosions wreck cars (and wrecked cars explode in turn).
fn explosions(
    mut msgs: ResMut<Messages<Explosion>>,
    mut cursor: Local<bevy::ecs::message::MessageCursor<Explosion>>,
    mut cars: Query<(Entity, &mut Car)>,
    mut commands: Commands,
    a: Res<CarAssets>,
    children: Query<&Children>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut p: ResMut<PlayerState>,
    mut wanted: ResMut<Wanted>,
    modes: Res<ActiveModes>,
    mut out: MessageWriter<MashEvent>,
) {
    // Read and write the same channel: wrecked cars chain-explode.
    let blasts: Vec<Explosion> = cursor.read(&msgs).copied().collect();
    for (e, mut car) in cars.iter_mut() {
        if car.wrecked {
            continue;
        }
        for b in &blasts {
            if car.sim.pos.distance(b.pos) < b.radius * 1.6 {
                car.sim.health -= 70.0 * b.power;
            }
        }
        if car.sim.health <= 0.0 {
            car.wrecked = true;
            msgs.write(Explosion { pos: car.sim.pos, radius: 4.0, power: 1.0 });
            if car.driver == Driver::Player {
                p.health = 0.0;
                p.loco = Loco::Foot;
            }
            if car.driver == Driver::Police && modes.on(Mode::Streets) {
                wanted.crime(1.5);
                out.write(MashEvent::new(Kind::Crime, "Cop Car Down", 300));
            }
            // Wreck becomes a physics object you can knock around.
            let v = car.sim.vel + Vec3::Y * 6.0;
            commands.entity(e).insert((RigidBody::Dynamic, Velocity { linear: v, angular: Vec3::new(1.5, 0.5, 2.0) }, ColliderMassProperties::Density(0.4)));
            for c in children.iter_descendants(e) {
                if let Ok(mut m) = mats.get_mut(c) {
                    m.0 = a.wreck.clone();
                }
            }
        }
    }
}

fn visuals(time: Res<Time>, cars: Query<(&Car, &Children)>, mut wheels: Query<&mut Transform, With<Wheel>>, mut bars: Query<&mut Visibility, With<LightBar>>) {
    let t = time.elapsed_secs();
    for (car, kids) in cars.iter() {
        let mut i = 0;
        for k in kids.iter() {
            if let Ok(mut w) = wheels.get_mut(k) {
                let front = i < 2;
                let steer = if front { car.sim.steer_vis } else { 0.0 };
                w.rotation = Quat::from_rotation_y(steer) * Quat::from_rotation_x(car.sim.wheel_spin) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
                i += 1;
            }
            if let Ok(mut v) = bars.get_mut(k) {
                let on = (t * 6.0 + car.siren).sin() > 0.0;
                *v = if on { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
    }
}

fn hud(wanted: Res<Wanted>, p: Res<PlayerState>, cars: Query<&Car>, mut hud: ResMut<Hud>) {
    hud.stars = Some(wanted.stars);
    if p.loco == Loco::Vehicle {
        if let Some(c) = cars.iter().find(|c| c.driver == Driver::Player) {
            hud.top.push(format!("{:.0} km/h", c.sim.forward_speed().abs() * 3.6));
            hud.bars.push(("car".into(), c.sim.health / 100.0, [240, 160, 60]));
        }
    } else if cars.iter().any(|c| c.sim.pos.distance(p.pos) < 4.5 && c.driver != Driver::Player && !c.wrecked) {
        hud.center = Some(("E: steal car".into(), 0.1));
    }
}

fn minimap(mut contexts: EguiContexts, p: Res<PlayerState>, rig: Res<CamRig>, cars: Query<&Car>, peds: Query<&Transform, With<Ped>>) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::Area::new(egui::Id::new("minimap")).anchor(egui::Align2::LEFT_BOTTOM, [16.0, -40.0]).show(ctx, |ui| {
        let size = 170.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.circle_filled(rect.center(), size * 0.5, egui::Color32::from_black_alpha(150));
        let scale = size * 0.5 / 90.0;
        let yaw = rig.forward.x.atan2(rig.forward.z);
        let (s, c) = yaw.sin_cos();
        let to_map = |w: Vec3| {
            let d = w - p.pos;
            // Rotate so camera-forward is up.
            let x = -(d.x * c - d.z * s);
            let y = -(d.x * s + d.z * c);
            rect.center() + egui::vec2(x * scale, y * scale)
        };
        for l in road_lines() {
            let a = to_map(Vec3::new(l, 0.0, p.pos.z - 200.0));
            let b = to_map(Vec3::new(l, 0.0, p.pos.z + 200.0));
            painter.line_segment([a, b], egui::Stroke::new(5.0, egui::Color32::from_gray(90)));
            let a = to_map(Vec3::new(p.pos.x - 200.0, 0.0, l));
            let b = to_map(Vec3::new(p.pos.x + 200.0, 0.0, l));
            painter.line_segment([a, b], egui::Stroke::new(5.0, egui::Color32::from_gray(90)));
        }
        for car in cars.iter() {
            let col = match car.driver {
                Driver::Police => egui::Color32::from_rgb(80, 120, 255),
                Driver::Player => egui::Color32::WHITE,
                _ => egui::Color32::from_rgb(200, 200, 120),
            };
            let pt = to_map(car.sim.pos);
            if rect.contains(pt) {
                painter.circle_filled(pt, 3.0, col);
            }
        }
        for t in peds.iter() {
            let pt = to_map(t.translation);
            if rect.contains(pt) {
                painter.circle_filled(pt, 1.5, egui::Color32::from_rgb(120, 220, 120));
            }
        }
        let tip = rect.center() + egui::vec2(0.0, -7.0);
        painter.add(egui::Shape::convex_polygon(vec![tip, rect.center() + egui::vec2(5.0, 5.0), rect.center() + egui::vec2(-5.0, 5.0)], egui::Color32::from_rgb(255, 220, 60), egui::Stroke::NONE));
    });
    Ok(())
}
