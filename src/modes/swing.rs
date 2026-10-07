//! SWING mode: web-swing off any surface (pendulum rope constraint with
//! pumping and a release fling), and a tether that ties two things together
//! and yanks them (cars to lamp posts, cops to each other, you to a car).

use crate::core::camera::CamRig;
use crate::core::input::{Latch, Look};
use crate::core::modes::{ActiveModes, Mode, ModeToggled, mode_on};
use crate::core::phys::{G_DYNAMIC, G_NPC, G_WORLD, PhysWorld, Rails};
use crate::core::player::{JumpSim, Loco, MoveSet, PlayerState, SkateSim, WalkSim, enter_loco};
use crate::core::score::{Kind, MashEvent};
use crate::core::ui::Hud;
use crate::modes::streets::Car;
use crate::modes::warfare::Bot;
use crate::sim::collide::CollisionWorld;
use crate::world::build::{Anchor, Static};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

#[derive(Resource, Default)]
pub struct Web {
    pub anchor: Option<Vec3>,
    pub length: f32,
    /// Length the rope reels toward: keeps the arc's low point off the ground.
    pub target: f32,
    tether_first: Option<End>,
    pub tethers: Vec<(End, End, f32)>,
    attach_t: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum End {
    Point(Vec3),
    Entity(Entity, Vec3),
    Player,
}

#[derive(Component)]
struct RopeVis(usize);

const REACH: f32 = 90.0;
const GRAVITY: f32 = 20.0;

pub struct SwingPlugin;

impl Plugin for SwingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Web>()
            .add_systems(FixedUpdate, shoot_web.in_set(MoveSet::Arbitrate).after(crate::core::player::arbitrate).run_if(mode_on(Mode::Swing)))
            .add_systems(FixedUpdate, swing_step.in_set(MoveSet::Locomotion).run_if(|p: Res<PlayerState>| p.loco == Loco::Swing))
            .add_systems(FixedUpdate, (tether_input, tether_pull).chain().in_set(MoveSet::Abilities).run_if(mode_on(Mode::Swing)))
            .add_systems(Update, (draw_ropes, toggle, hud.run_if(mode_on(Mode::Swing))));
    }
}

fn toggle(mut t: MessageReader<ModeToggled>, mut web: ResMut<Web>) {
    for ev in t.read() {
        if ev.mode == Mode::Swing && !ev.on {
            *web = Web::default();
        }
    }
}

/// Aim assist the way swing games do it: take the crosshair if it hits
/// something high, otherwise fan rays forward-and-up and pick the best tall
/// surface (rooftops, building faces), falling back to nearby lamp posts.
fn find_anchor(ctx: &RapierContext, rig: &CamRig, body: Entity, anchors: &[Vec3], from: Vec3) -> Option<Vec3> {
    let filt = QueryFilter::new().exclude_sensors().exclude_rigid_body(body).groups(CollisionGroups::new(Group::ALL, G_WORLD));
    let head = from + Vec3::Y * 1.6;
    if let Some((_, t)) = ctx.cast_ray(rig.eye, rig.forward, REACH, true, filt) {
        let hit = rig.eye + rig.forward * t;
        if hit.y > from.y + 6.0 {
            return Some(hit);
        }
    }
    let flat = rig.forward.with_y(0.0).normalize_or(Vec3::Z);
    let mut best: Option<(f32, Vec3)> = None;
    for yaw in [-0.6f32, -0.3, 0.0, 0.3, 0.6] {
        let dir_flat = Quat::from_rotation_y(yaw) * flat;
        for pitch in [0.35f32, 0.6, 0.85, 1.1] {
            let dir = (dir_flat * pitch.cos() + Vec3::Y * pitch.sin()).normalize();
            let Some((_, t)) = ctx.cast_ray(head, dir, REACH, true, filt) else { continue };
            let hit = head + dir * t;
            let height = hit.y - from.y;
            if height < 6.0 {
                continue;
            }
            // Prefer high, ahead, and not too far.
            let score = height.min(40.0) - yaw.abs() * 12.0 - (t - 30.0).max(0.0) * 0.4;
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, hit));
            }
        }
    }
    if let Some((_, hit)) = best {
        return Some(hit);
    }
    anchors
        .iter()
        .copied()
        .filter(|a| a.distance(from) < 30.0 && (*a - from).normalize().dot(flat) > 0.3)
        .min_by(|a, b| a.distance(from).total_cmp(&b.distance(from)))
}

#[allow(clippy::too_many_arguments)]
fn shoot_web(
    keys: Res<ButtonInput<KeyCode>>,
    latch: Res<Latch>,
    look: Res<Look>,
    rig: Res<CamRig>,
    rapier: ReadRapierContext,
    anchors: Query<&GlobalTransform, With<Anchor>>,
    modes: Res<ActiveModes>,
    mut web: ResMut<Web>,
    mut p: ResMut<PlayerState>,
    mut skate: ResMut<SkateSim>,
    mut jump: ResMut<JumpSim>,
    mut walk: ResMut<WalkSim>,
    mut out: MessageWriter<MashEvent>,
) -> Result {
    let held = keys.pressed(KeyCode::KeyQ);
    if p.loco == Loco::Swing {
        let release = !held || latch.key(KeyCode::Space);
        if release {
            // Fling: keep momentum, a little extra pop if you jumped off.
            if latch.key(KeyCode::Space) {
                let boost = Vec3::Y * 4.0 + p.vel.with_y(0.0).normalize_or_zero() * 2.0;
                p.vel += boost;
            }
            web.anchor = None;
            let speed = p.vel.length();
            out.write(MashEvent::new(Kind::SwingRelease { speed }, if speed > 20.0 { "Swing Fling" } else { "" }, (speed * 4.0) as u32));
            let back = if p.resume == Loco::Skate && modes.on(Mode::Skate) { Loco::Skate } else { Loco::Foot };
            enter_loco(back, &mut p, &modes, &mut skate, &mut jump, &mut walk);
        }
        return Ok(());
    }
    if p.loco == Loco::Vehicle || !latch.key(KeyCode::KeyQ) || !look.captured {
        return Ok(());
    }
    let ctx = rapier.single()?;
    let pts: Vec<Vec3> = anchors.iter().map(|g| g.translation() + Vec3::Y * 3.4).collect();
    if let Some(a) = find_anchor(&ctx, &rig, p.body, &pts, p.pos) {
        let hand = p.pos + Vec3::Y * 1.6;
        web.anchor = Some(a);
        let dist = hand.distance(a);
        web.length = dist;
        web.target = (dist * 0.9).min(a.y - p.pos.y - 2.5).max(3.0);
        debug!("web anchor {a:?} dist {dist:.1} target {:.1}", web.target);
        web.attach_t = 0.0;
        // Yank off the ground toward the anchor so the swing starts in the air.
        let to = (a - hand).normalize_or_zero();
        p.vel += to * 7.0 + Vec3::Y * 4.0;
        if matches!(p.loco, Loco::Foot | Loco::Skate) {
            p.resume = p.loco;
        }
        p.loco = Loco::Swing;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn swing_step(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    rig: Res<CamRig>,
    rapier: ReadRapierContext,
    rails: Res<Rails>,
    holes: Res<crate::core::phys::Holes>,
    modes: Res<ActiveModes>,
    mut web: ResMut<Web>,
    mut p: ResMut<PlayerState>,
    mut skate: ResMut<SkateSim>,
    mut jump: ResMut<JumpSim>,
    mut walk: ResMut<WalkSim>,
) -> Result {
    let Some(anchor) = web.anchor else {
        enter_loco(Loco::Foot, &mut p, &modes, &mut skate, &mut jump, &mut walk);
        return Ok(());
    };
    let ctx = rapier.single()?;
    let w = PhysWorld { ctx: &ctx, rails: &rails.0, holes: &holes.0, exclude: Some(p.body) };
    let dt = time.delta_secs();
    web.attach_t += dt;
    let kick = std::mem::take(&mut p.impulse);
    p.vel += kick;

    // Pump along the camera direction, steer sideways, reel with W/S.
    let to_anchor = (anchor - (p.pos + Vec3::Y * 1.6)).normalize_or_zero();
    let mut pump = Vec3::ZERO;
    let fwd = rig.forward.with_y(0.0).normalize_or_zero();
    let right = Vec3::new(-fwd.z, 0.0, fwd.x);
    if keys.pressed(KeyCode::KeyW) {
        pump += fwd;
    }
    if keys.pressed(KeyCode::KeyA) {
        pump -= right;
    }
    if keys.pressed(KeyCode::KeyD) {
        pump += right;
    }
    let tangent_pump = pump - to_anchor * pump.dot(to_anchor);
    p.vel += tangent_pump.normalize_or_zero() * 9.0 * dt;
    if keys.pressed(KeyCode::ShiftLeft) {
        web.target = (web.target - 12.0 * dt).max(3.0);
    }
    if keys.pressed(KeyCode::KeyS) {
        web.target = (web.target + 8.0 * dt).min(REACH);
    }
    // Reel toward the target length (fast in, slow out).
    let reel = if web.length > web.target { -18.0 } else { 8.0 };
    web.length = if reel < 0.0 { (web.length + reel * dt).max(web.target) } else { (web.length + reel * dt).min(web.target) };
    p.vel.y -= GRAVITY * dt;
    p.vel *= 1.0 - 0.02 * dt;

    let mut hand = p.pos + Vec3::Y * 1.6 + p.vel * dt;
    // Rope constraint: stay within length; kill outward radial speed.
    let d = hand - anchor;
    let dist = d.length();
    if dist > web.length {
        let n = d / dist;
        hand = anchor + n * web.length;
        let radial = p.vel.dot(n);
        if radial > 0.0 {
            p.vel -= n * radial;
        }
    }
    let center = hand - Vec3::Y * 0.8;
    let (c2, hit) = w.resolve_sphere(center, 0.45);
    p.pos = c2 - Vec3::Y * 0.8;
    if let Some(n) = hit {
        let into = p.vel.dot(n);
        if into < 0.0 {
            p.vel -= n * into;
        }
        // Touching ground ends the swing.
        if n.y > 0.7 && web.attach_t > 0.6 && p.vel.y < 0.5 {
            web.anchor = None;
            let back = if p.resume == Loco::Skate && modes.on(Mode::Skate) { Loco::Skate } else { Loco::Foot };
            enter_loco(back, &mut p, &modes, &mut skate, &mut jump, &mut walk);
            return Ok(());
        }
    }
    p.on_ground = false;
    let h = p.vel.with_y(0.0);
    if h.length() > 0.5 {
        p.facing = h.x.atan2(h.z);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn tether_input(
    latch: Res<Latch>,
    look: Res<Look>,
    rig: Res<CamRig>,
    rapier: ReadRapierContext,
    p: Res<PlayerState>,
    mut web: ResMut<Web>,
    tfs: Query<&GlobalTransform>,
    statics: Query<(), With<Static>>,
    mut hud: ResMut<Hud>,
) -> Result {
    if !latch.key(KeyCode::KeyZ) || !look.captured {
        return Ok(());
    }
    let ctx = rapier.single()?;
    let filt = QueryFilter::new().exclude_sensors().exclude_rigid_body(p.body).groups(CollisionGroups::new(Group::ALL, G_WORLD | G_DYNAMIC | G_NPC));
    let Some((e, t)) = ctx.cast_ray(rig.eye, rig.forward, REACH, true, filt) else {
        // Aiming at nothing: tether yourself to the first point.
        if let Some(first) = web.tether_first.take() {
            web.tethers.push((first, End::Player, 2.0));
        }
        return Ok(());
    };
    let hit = rig.eye + rig.forward * t;
    // World geometry pins the tether in place; anything else gets dragged.
    let end = match tfs.get(e) {
        Ok(g) if statics.get(e).is_err() => End::Entity(e, g.affine().inverse().transform_point3(hit)),
        _ => End::Point(hit),
    };
    match web.tether_first.take() {
        None => {
            web.tether_first = Some(end);
            hud.center = Some(("Tether: pick the second point (Z)".into(), 2.0));
        }
        Some(first) => {
            let a = end_pos(&first, &tfs, &p);
            let b = end_pos(&end, &tfs, &p);
            let rest = (a.distance(b) * 0.3).max(1.5);
            web.tethers.push((first, end, rest));
            if web.tethers.len() > 6 {
                web.tethers.remove(0);
            }
        }
    }
    Ok(())
}

fn end_pos(e: &End, tfs: &Query<&GlobalTransform>, p: &PlayerState) -> Vec3 {
    match e {
        End::Point(v) => *v,
        End::Player => p.pos + Vec3::Y,
        End::Entity(ent, local) => tfs.get(*ent).map(|g| g.transform_point(*local)).unwrap_or(Vec3::ZERO),
    }
}

/// Tethers reel in: whatever can move gets yanked toward the other end.
fn tether_pull(
    time: Res<Time>,
    mut web: ResMut<Web>,
    tfs: Query<&GlobalTransform>,
    mut cars: Query<&mut Car>,
    mut bots: Query<&mut Transform, With<Bot>>,
    mut bodies: Query<(&mut Velocity, &RigidBody), Without<Car>>,
    mut p: ResMut<PlayerState>,
) {
    let dt = time.delta_secs();
    let tethers = web.tethers.clone();
    let mut keep = vec![];
    for (a, b, rest) in tethers {
        let pa = end_pos(&a, &tfs, &p);
        let pb = end_pos(&b, &tfs, &p);
        if pa == Vec3::ZERO || pb == Vec3::ZERO {
            continue;
        }
        keep.push((a, b, (rest - 3.0 * dt).max(1.0)));
        let d = pb - pa;
        let dist = d.length();
        if dist <= rest {
            continue;
        }
        let n = d / dist;
        let stretch = dist - rest;
        let movable = |e: &End, cars: &Query<&mut Car>, bodies: &Query<(&mut Velocity, &RigidBody), Without<Car>>, bots: &Query<&mut Transform, With<Bot>>| match e {
            End::Player => true,
            End::Entity(ent, _) => cars.get(*ent).is_ok() || bots.get(*ent).is_ok() || bodies.get(*ent).is_ok_and(|(_, rb)| *rb == RigidBody::Dynamic),
            End::Point(_) => false,
        };
        let (ma, mb) = (movable(&a, &cars, &bodies, &bots), movable(&b, &cars, &bodies, &bots));
        let share = if ma && mb { 0.5 } else { 1.0 };
        for (end, sign) in [(&a, 1.0f32), (&b, -1.0)] {
            let accel = n * sign * stretch * 14.0 * share * dt;
            match end {
                End::Player if (sign > 0.0 && ma) || (sign < 0.0 && mb) => p.impulse += accel,
                End::Entity(ent, _) => {
                    if let Ok(mut car) = cars.get_mut(*ent) {
                        car.sim.vel += accel;
                    } else if let Ok(mut bt) = bots.get_mut(*ent) {
                        bt.translation += accel * dt * 4.0;
                    } else if let Ok((mut v, rb)) = bodies.get_mut(*ent)
                        && *rb == RigidBody::Dynamic
                    {
                        v.linear += accel;
                    }
                }
                _ => {}
            }
        }
    }
    web.tethers = keep;
}

#[derive(Resource)]
struct RopeAssets(Handle<Mesh>, Handle<StandardMaterial>);

fn draw_ropes(
    mut commands: Commands,
    web: Res<Web>,
    p: Res<PlayerState>,
    tfs: Query<&GlobalTransform>,
    mut ropes: Query<(Entity, &RopeVis, &mut Transform)>,
    assets: Option<Res<RopeAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let Some(assets) = assets else {
        commands.insert_resource(RopeAssets(meshes.add(Cuboid::new(1.0, 1.0, 1.0)), mats.add(StandardMaterial { base_color: Color::srgb(0.95, 0.95, 0.98), unlit: true, ..default() })));
        return;
    };
    let mut segs: Vec<(Vec3, Vec3)> = vec![];
    if let (Some(a), Loco::Swing) = (web.anchor, p.loco) {
        segs.push((p.pos + Vec3::Y * 1.6, a));
    }
    for (a, b, _) in &web.tethers {
        segs.push((end_pos(a, &tfs, &p), end_pos(b, &tfs, &p)));
    }
    let mut used = vec![false; segs.len()];
    for (e, rv, mut tf) in ropes.iter_mut() {
        if let Some((a, b)) = segs.get(rv.0) {
            used[rv.0] = true;
            let len = a.distance(*b);
            *tf = Transform { translation: (*a + *b) * 0.5, rotation: Quat::from_rotation_arc(Vec3::Z, (*b - *a).normalize_or(Vec3::Z)), scale: Vec3::new(0.03, 0.03, len) };
        } else {
            commands.entity(e).despawn();
        }
    }
    for (i, u) in used.iter().enumerate() {
        if !u {
            commands.spawn((RopeVis(i), Mesh3d(assets.0.clone()), MeshMaterial3d(assets.1.clone()), Transform::from_scale(Vec3::ZERO)));
        }
    }
}

fn hud(web: Res<Web>, p: Res<PlayerState>, mut hud: ResMut<Hud>) {
    if p.loco == Loco::Swing {
        hud.top.push(format!("Swinging  rope {:.0}m   W pump  Shift reel in  Space fling", web.length));
    } else {
        hud.crosshair = true;
    }
    if !web.tethers.is_empty() {
        hud.top.push(format!("Tethers: {}", web.tethers.len()));
    }
}
