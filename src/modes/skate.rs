//! SKATE mode: drives the skate sim when the player is on the board and
//! reports tricks to the shared score.

use crate::core::camera::CamRig;
use crate::core::input::{Latch, Look};
use crate::core::modes::{Mode, mode_on};
use crate::core::phys::{PhysWorld, Rails};
use crate::core::player::{Loco, MoveSet, PlayerState, SkateSim};
use crate::core::score::{Kind, MashEvent};
use crate::core::ui::Hud;
use crate::sim::skate::{FlipKind, SkEvent, SkState, SkateInput};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

pub struct SkatePlugin;

impl Plugin for SkatePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            step_skate.in_set(MoveSet::Locomotion).run_if(|p: Res<PlayerState>| p.loco == Loco::Skate),
        )
        .add_systems(Update, (skate_hud.run_if(mode_on(Mode::Skate)), follow_cam));
    }
}

#[allow(clippy::too_many_arguments)]
fn step_skate(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    latch: Res<Latch>,
    rapier: ReadRapierContext,
    rails: Res<Rails>,
    holes: Res<crate::core::phys::Holes>,
    mut p: ResMut<PlayerState>,
    mut sim: ResMut<SkateSim>,
    mut out: MessageWriter<MashEvent>,
    mut hud: ResMut<Hud>,
    mut rig: ResMut<CamRig>,
    mut sfx: MessageWriter<crate::core::audio::Sfx>,
) -> Result {
    let ctx = rapier.single()?;
    let w = PhysWorld { ctx: &ctx, rails: &rails.0, holes: &holes.0, exclude: Some(p.body) };
    let k = |c: KeyCode| keys.pressed(c);
    let shift = k(KeyCode::ShiftLeft) || k(KeyCode::ShiftRight);
    let mut steer = 0.0;
    if k(KeyCode::KeyA) {
        steer += 1.0;
    }
    if k(KeyCode::KeyD) {
        steer -= 1.0;
    }
    let flip = if latch.key(KeyCode::KeyJ) {
        Some(FlipKind::Kickflip)
    } else if latch.key(KeyCode::KeyK) {
        Some(FlipKind::Heelflip)
    } else if latch.key(KeyCode::KeyL) {
        Some(FlipKind::ShoveIt)
    } else if latch.key(KeyCode::KeyU) {
        Some(FlipKind::TreFlip)
    } else if latch.key(KeyCode::KeyH) {
        Some(FlipKind::Hardflip)
    } else {
        None
    };
    let inp = SkateInput {
        push: k(KeyCode::KeyW),
        brake: k(KeyCode::KeyS),
        steer,
        crouch: k(KeyCode::Space),
        nollie: shift,
        flip,
        grab: k(KeyCode::KeyY),
        manual: if k(KeyCode::KeyI) { 1 } else if k(KeyCode::KeyO) { -1 } else { 0 },
        powerslide: shift,
    };
    let s = &mut sim.0;
    let kick = std::mem::take(&mut p.impulse);
    if kick.length() > 0.5 {
        s.vel += kick;
        if kick.length() > 4.0 {
            s.force_bail();
        }
    }
    let was = s.state;
    s.step(time.delta_secs(), &inp, &w);
    if was != SkState::Air && s.state == SkState::Air && s.vel.y > 2.0 {
        sfx.write(crate::core::audio::Sfx { name: "pop", volume: 0.7, speed: 1.0 });
    }
    p.pos = s.pos;
    p.vel = s.vel;
    p.facing = s.board_yaw();
    p.on_ground = !matches!(s.state, SkState::Air | SkState::Bail);
    if s.state == SkState::Grind {
        p.on_ground = true;
    }

    for ev in s.events.drain(..) {
        match ev {
            SkEvent::Trick { name, points } => {
                out.write(MashEvent::new(Kind::Trick, name, points));
            }
            SkEvent::Bail => {
                sfx.write(crate::core::audio::Sfx { name: "land", volume: 0.9, speed: 0.8 });
                rig.shake = 0.5;
                hud.banner = Some(("BAIL".into(), 1.2));
                out.write(MashEvent::new(Kind::Bail, "", 0));
            }
            SkEvent::Banked(v) => {
                hud.banner = Some((format!("+{v}"), 1.4));
            }
            SkEvent::Landed { clean } => {
                sfx.write(crate::core::audio::Sfx { name: "land", volume: 0.75, speed: if clean { 1.0 } else { 0.85 } });
                if !clean {
                    hud.center = Some(("Sketchy!".into(), 0.8));
                }
            }
            SkEvent::GrindStart(_) => {
                sfx.write(crate::core::audio::Sfx { name: "grind", volume: 0.6, speed: 1.0 });
            }
        }
    }
    Ok(())
}

fn skate_hud(sim: Res<SkateSim>, p: Res<PlayerState>, mut hud: ResMut<Hud>) {
    if p.loco != Loco::Skate {
        if p.loco == Loco::Foot {
            hud.top.push("G: hop on the board".into());
        }
        return;
    }
    let s = &sim.0;
    if !s.combo.tricks.is_empty() {
        let tail: Vec<&str> = s.combo.tricks.iter().rev().take(4).map(|x| x.as_str()).collect();
        hud.top.push(format!("{} x{}   {}", s.combo.value, s.combo.multiplier, tail.into_iter().rev().collect::<Vec<_>>().join(" + ")));
    }
    if matches!(s.state, SkState::Grind | SkState::Manual) {
        let b = (s.manual_balance / 1.15 + 1.0) * 0.5;
        hud.bars.push(("balance".into(), b, if (b - 0.5).abs() > 0.35 { [230, 70, 60] } else { [120, 220, 120] }));
    }
    if s.crouch > 0.0 && s.state != SkState::Air {
        hud.bars.push(("pop".into(), s.crouch, [255, 190, 40]));
    }
}

/// When the mouse is idle the camera swings in behind the board or car.
fn follow_cam(
    time: Res<Time<bevy::time::Real>>,
    motion: Res<bevy::input::mouse::AccumulatedMouseMotion>,
    sim: Res<SkateSim>,
    p: Res<PlayerState>,
    mut look: ResMut<Look>,
    mut idle: Local<f32>,
) {
    if !matches!(p.loco, Loco::Skate | Loco::Vehicle) {
        return;
    }
    let dt = time.delta_secs();
    if motion.delta.length_squared() > 0.5 {
        *idle = 0.0;
        return;
    }
    *idle += dt;
    if *idle < 0.6 {
        return;
    }
    let v = Vec3::new(p.vel.x, 0.0, p.vel.z);
    if v.length() < 1.0 || (p.loco == Loco::Skate && sim.0.state == SkState::Bail) {
        return;
    }
    let want = v.x.atan2(v.z);
    let d = (want - look.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    look.yaw += d * (dt * 3.0).min(1.0);
    look.pitch += (-0.22 - look.pitch) * (dt * 2.0).min(1.0);
}
