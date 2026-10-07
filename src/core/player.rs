//! The one shared avatar. Exactly one locomotion owns movement at a time;
//! every hand-off carries position and velocity into the next sim, which is
//! what makes live mode-swapping feel continuous.

use crate::core::input::{Latch, Look};
use crate::core::modes::{ActiveModes, Mode, ModeToggled};
use crate::core::phys::{G_PLAYER, PhysWorld, Rails};
use crate::sim::jump::{JumpInput, Jumper};
use crate::sim::skate::Skater;
use crate::sim::walker::{WalkInput, Walker};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Loco {
    Foot,
    Skate,
    Vehicle,
    Swing,
}

#[derive(Resource)]
pub struct PlayerState {
    pub pos: Vec3,
    pub vel: Vec3,
    /// Body facing (yaw). Forward = (sin, 0, cos).
    pub facing: f32,
    pub loco: Loco,
    /// Where to return after a swing or leaving a car.
    pub resume: Loco,
    pub health: f32,
    pub on_ground: bool,
    pub airborne_time: f32,
    pub body: Entity,
    pub respawn: Vec3,
    /// Knock-back from explosions, cars, bullets; applied by the active locomotion.
    pub impulse: Vec3,
}

#[derive(Component)]
pub struct PlayerBody;

#[derive(Resource)]
pub struct SkateSim(pub Skater);
#[derive(Resource)]
pub struct JumpSim {
    pub j: Jumper,
    pub pending: JumpInput,
}
#[derive(Resource)]
pub struct WalkSim(pub Walker);

/// Request a locomotion change; resolved in FixedUpdate before movement.
#[derive(Message, Clone, Copy)]
pub struct SetLoco(pub Loco);

pub const SPAWN: Vec3 = Vec3::new(0.0, 1.0, -30.0);

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SetLoco>()
            .init_resource::<crate::core::phys::Holes>()
            .insert_resource(SkateSim(Skater::new(SPAWN, 0.0)))
            .insert_resource(JumpSim { j: Jumper::new(SPAWN, 0.0), pending: JumpInput::default() })
            .insert_resource(WalkSim(Walker::new(SPAWN)))
            .add_systems(Startup, spawn_player)
            .add_systems(FixedUpdate, arbitrate.in_set(MoveSet::Arbitrate))
            .add_systems(FixedUpdate, foot_step.in_set(MoveSet::Locomotion).run_if(|p: Res<PlayerState>| p.loco == Loco::Foot))
            .add_systems(FixedUpdate, (fall_guard, sync_body).chain().in_set(MoveSet::Post))
            .configure_sets(FixedUpdate, (MoveSet::Arbitrate, MoveSet::Locomotion, MoveSet::Abilities, MoveSet::Post).chain());
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum MoveSet {
    /// Decide who drives the avatar this tick.
    Arbitrate,
    /// Exactly one locomotion system runs here.
    Locomotion,
    /// Weapons, building, portals, tethers: act on the moved player.
    Abilities,
    Post,
}

pub fn spawn_player(mut commands: Commands) {
    let body = commands
        .spawn((
            PlayerBody,
            Transform::from_translation(SPAWN),
            RigidBody::KinematicPositionBased,
            Collider::capsule_y(0.55, 0.32),
            CollisionGroups::new(G_PLAYER, Group::ALL),
        ))
        .id();
    commands.insert_resource(PlayerState {
        pos: SPAWN,
        vel: Vec3::ZERO,
        facing: 0.0,
        loco: Loco::Skate,
        resume: Loco::Foot,
        health: 100.0,
        on_ground: false,
        airborne_time: 0.0,
        body,
        respawn: SPAWN,
        impulse: Vec3::ZERO,
    });
}

/// Moves the player into `to`, seeding the target sim with current momentum.
pub fn enter_loco(
    to: Loco,
    p: &mut PlayerState,
    modes: &ActiveModes,
    skate: &mut SkateSim,
    jump: &mut JumpSim,
    walk: &mut WalkSim,
) {
    match to {
        Loco::Skate => skate.0.enter(p.pos, p.vel, p.facing),
        Loco::Foot => {
            if modes.on(Mode::Jump) {
                jump.j.enter(p.pos, p.vel, p.facing);
            } else {
                walk.0.enter(p.pos, p.vel);
            }
        }
        Loco::Vehicle | Loco::Swing => {}
    }
    if p.loco != to && matches!(p.loco, Loco::Foot | Loco::Skate) {
        p.resume = p.loco;
    }
    p.loco = to;
}

/// Mode toggles and the board hop key decide who drives the avatar.
#[allow(clippy::too_many_arguments)]
pub fn arbitrate(
    mut toggles: MessageReader<ModeToggled>,
    mut requests: MessageReader<SetLoco>,
    modes: Res<ActiveModes>,
    latch: Res<Latch>,
    mut p: ResMut<PlayerState>,
    mut skate: ResMut<SkateSim>,
    mut jump: ResMut<JumpSim>,
    mut walk: ResMut<WalkSim>,
) {
    let mut want: Option<Loco> = None;
    for t in toggles.read() {
        match (t.mode, t.on) {
            (Mode::Skate, true) if p.loco == Loco::Foot => want = Some(Loco::Skate),
            (Mode::Skate, false) if p.loco == Loco::Skate => want = Some(Loco::Foot),
            // Switching foot style mid-air keeps momentum through the new sim.
            (Mode::Jump, _) if p.loco == Loco::Foot => want = Some(Loco::Foot),
            (Mode::Jump, true) if p.loco == Loco::Skate && !modes.on(Mode::Skate) => want = Some(Loco::Foot),
            (Mode::Streets, false) if p.loco == Loco::Vehicle => want = Some(p.resume),
            (Mode::Swing, false) if p.loco == Loco::Swing => want = Some(p.resume),
            _ => {}
        }
    }
    if latch.key(KeyCode::KeyG) && modes.on(Mode::Skate) {
        match p.loco {
            Loco::Foot => want = Some(Loco::Skate),
            Loco::Skate => want = Some(Loco::Foot),
            _ => {}
        }
    }
    for r in requests.read() {
        want = Some(r.0);
    }
    if let Some(mut to) = want {
        if to == Loco::Skate && !modes.on(Mode::Skate) {
            to = Loco::Foot;
        }
        enter_loco(to, &mut p, &modes, &mut skate, &mut jump, &mut walk);
    }
}

#[allow(clippy::too_many_arguments)]
fn foot_step(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    latch: Res<Latch>,
    look: Res<Look>,
    modes: Res<ActiveModes>,
    rapier: ReadRapierContext,
    rails: Res<Rails>,
    holes: Res<crate::core::phys::Holes>,
    mut p: ResMut<PlayerState>,
    mut jump: ResMut<JumpSim>,
    mut walk: ResMut<WalkSim>,
) -> Result {
    let ctx = rapier.single()?;
    let w = PhysWorld { ctx: &ctx, rails: &rails.0, holes: &holes.0, exclude: Some(p.body) };
    let dt = time.delta_secs();
    let wish = look.wish(&keys);
    let kick = std::mem::take(&mut p.impulse);
    if kick != Vec3::ZERO {
        if modes.on(Mode::Jump) {
            let v = jump.j.velocity() + kick;
            let (pos, facing) = (jump.j.pos, jump.j.facing);
            jump.j.enter(pos, v, facing);
        } else {
            walk.0.vel += kick;
            walk.0.on_ground = false;
        }
    }
    if modes.on(Mode::Jump) {
        let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::KeyC);
        let pend = &mut jump.pending;
        pend.stick = wish;
        pend.a_held = keys.pressed(KeyCode::Space);
        pend.z_held = ctrl;
        pend.a_pressed |= latch.key(KeyCode::Space);
        pend.b_pressed |= latch.key(KeyCode::KeyF);
        pend.z_pressed |= latch.key(KeyCode::ControlLeft) || latch.key(KeyCode::KeyC);
        let inp = *pend;
        let ran = jump.j.update(dt, &inp, &w);
        if ran > 0 {
            jump.pending = JumpInput::default();
        }
        p.pos = jump.j.render_pos();
        p.vel = jump.j.velocity();
        p.facing = jump.j.facing;
        p.on_ground = jump.j.on_ground;
    } else {
        let inp = WalkInput {
            wish,
            jump: latch.key(KeyCode::Space) || (keys.pressed(KeyCode::Space) && walk.0.on_ground),
            sprint: keys.pressed(KeyCode::ShiftLeft),
            crouch: keys.pressed(KeyCode::KeyC) || keys.pressed(KeyCode::ControlLeft),
            ads: mouse.pressed(MouseButton::Right) && modes.on(Mode::Warfare),
        };
        walk.0.step(dt, &inp, &w);
        p.pos = walk.0.pos;
        p.vel = walk.0.vel;
        p.on_ground = walk.0.on_ground;
        let h = Vec3::new(wish[0], 0.0, wish[1]);
        // On foot in first person the body faces where you look.
        p.facing = if h.length() > 0.1 { h.x.atan2(h.z) } else { look.yaw };
    }
    Ok(())
}

/// Falling out of the world respawns you at the last checkpoint.
fn fall_guard(
    mut p: ResMut<PlayerState>,
    modes: Res<ActiveModes>,
    time: Res<Time>,
    mut skate: ResMut<SkateSim>,
    mut jump: ResMut<JumpSim>,
    mut walk: ResMut<WalkSim>,
) {
    if p.on_ground {
        p.airborne_time = 0.0;
    } else {
        p.airborne_time += time.delta_secs();
    }
    if p.pos.y < -30.0 || p.health <= 0.0 {
        p.pos = p.respawn;
        p.vel = Vec3::ZERO;
        p.health = 100.0;
        let to = if p.loco == Loco::Skate { Loco::Skate } else { Loco::Foot };
        enter_loco(to, &mut p, &modes, &mut skate, &mut jump, &mut walk);
    }
}

fn sync_body(p: Res<PlayerState>, mut q: Query<&mut Transform, With<PlayerBody>>) {
    if let Ok(mut t) = q.get_mut(p.body) {
        t.translation = p.pos + Vec3::Y * 0.87;
    }
}
