//! Player avatar: rigged human (see `rig.rs`) plus a skateboard. Pose and
//! animation clip are driven entirely by whichever sim owns the player.

use crate::core::camera::CamRig;
use crate::core::hotbar::{Hotbar, Item};
use crate::core::modes::{ActiveModes, Mode};
use crate::core::player::{JumpSim, Loco, PlayerState, SkateSim};
use crate::core::rig::Rig;
use crate::sim::jump::JAction;
use crate::sim::skate::SkState;
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

#[derive(Component)]
pub struct AvatarRoot;
#[derive(Component)]
pub struct BodyPivot;
#[derive(Component)]
pub struct Board;
#[derive(Component)]
pub struct PlayerRig;

pub struct AvatarPlugin;

impl Plugin for AvatarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_avatar).add_systems(Update, pose_avatar);
    }
}

fn spawn_avatar(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>, assets: Res<AssetServer>) {
    commands.spawn((AvatarRoot, Transform::default(), Visibility::default())).with_children(|r| {
        r.spawn((BodyPivot, Transform::from_xyz(0.0, 0.9, 0.0), Visibility::default())).with_children(|b| {
            // Rigged human; feet at the pivot's -0.9.
            b.spawn((PlayerRig, Rig::person(crate::core::rig::SKATER, Some(crate::core::rig::HAIR_PARTED)), Transform::from_xyz(0.0, -0.9, 0.0), Visibility::default()));
        });
    });

    // Skateboard: maple deck with grip tape, metal trucks, urethane wheels.
    let wood = crate::world::materials::make(&assets, &mut mats, [168, 120, 72]);
    let grip = mats.add(StandardMaterial { base_color: Color::srgb(0.05, 0.05, 0.055), perceptual_roughness: 0.98, ..default() });
    let truck = mats.add(StandardMaterial { base_color: Color::srgb(0.75, 0.76, 0.78), metallic: 1.0, perceptual_roughness: 0.35, ..default() });
    let wheel = mats.add(StandardMaterial { base_color: Color::srgb(0.93, 0.91, 0.84), perceptual_roughness: 0.55, ..default() });
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let wheel_mesh = meshes.add(Cylinder::new(0.027, 0.032));
    commands.spawn((Board, Transform::default(), Visibility::default())).with_children(|b| {
        let s = |b: &mut ChildSpawnerCommands, mesh: &Handle<Mesh>, pos: Vec3, size: Vec3, rot: Quat, m: &Handle<StandardMaterial>| {
            b.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(m.clone()), Transform { translation: pos, rotation: rot, scale: size }));
        };
        let id = Quat::IDENTITY;
        s(b, &cube, Vec3::new(0.0, 0.1, 0.0), Vec3::new(0.205, 0.012, 0.62), id, &wood);
        s(b, &cube, Vec3::new(0.0, 0.107, 0.0), Vec3::new(0.2, 0.003, 0.6), id, &grip);
        // Kicked nose and tail.
        s(b, &cube, Vec3::new(0.0, 0.125, 0.36), Vec3::new(0.2, 0.012, 0.14), Quat::from_rotation_x(-0.35), &wood);
        s(b, &cube, Vec3::new(0.0, 0.125, -0.36), Vec3::new(0.2, 0.012, 0.14), Quat::from_rotation_x(0.35), &wood);
        for z in [0.22f32, -0.22] {
            s(b, &cube, Vec3::new(0.0, 0.075, z), Vec3::new(0.16, 0.02, 0.035), id, &truck);
            for x in [-0.085f32, 0.085] {
                s(b, &wheel_mesh, Vec3::new(x, 0.03, z), Vec3::ONE, Quat::from_rotation_z(PI / 2.0), &wheel);
            }
        }
    });
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn pose_avatar(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    p: Res<PlayerState>,
    cam: Res<CamRig>,
    modes: Res<ActiveModes>,
    bar: Res<Hotbar>,
    skate: Res<SkateSim>,
    jump: Res<JumpSim>,
    mut root: Query<(&mut Transform, &mut Visibility), (With<AvatarRoot>, Without<Board>, Without<BodyPivot>)>,
    mut pivot: Query<&mut Transform, (With<BodyPivot>, Without<AvatarRoot>, Without<Board>)>,
    mut board: Query<(&mut Transform, &mut Visibility), (With<Board>, Without<AvatarRoot>, Without<BodyPivot>)>,
    mut rig: Query<&mut Rig, With<PlayerRig>>,
) {
    let Ok((mut rt, mut rv)) = root.single_mut() else { return };
    let Ok(mut pv) = pivot.single_mut() else { return };
    let Ok((mut bt, mut bv)) = board.single_mut() else { return };
    let Ok(mut rig) = rig.single_mut() else { return };

    let hidden = p.loco == Loco::Vehicle || cam.first_person;
    *rv = if hidden { Visibility::Hidden } else { Visibility::Visible };
    let on_board = p.loco == Loco::Skate || (p.loco == Loco::Swing && p.resume == Loco::Skate);
    *bv = if on_board { Visibility::Visible } else { Visibility::Hidden };

    let speed = p.vel.with_y(0.0).length();
    let mut body_rot = Quat::from_rotation_y(p.facing);
    let mut drop = 0.0;
    rig.speed = 1.0;
    let armed = modes.on(Mode::Warfare) && matches!(bar.item, Item::Rifle | Item::Pistol | Item::Launcher);

    match p.loco {
        Loco::Skate => {
            let s = &skate.0;
            bt.translation = s.pos;
            bt.rotation = s.board_rotation();
            rt.translation = s.pos + s.board_rotation() * Vec3::Y * 0.11;
            // Pushing faces forward; everything else rides sideways (regular stance).
            let pushing = keys.pressed(KeyCode::KeyW) && s.state == SkState::Rolling && s.speed() < s.t.push_cap - 0.3 && s.crouch < 0.05;
            body_rot = Quat::from_rotation_y(s.board_yaw() + if pushing { 0.0 } else { PI / 2.0 });
            rig.want = match s.state {
                SkState::Bail => "Death01",
                SkState::Air => "Jump_Loop",
                SkState::Grind | SkState::Powerslide => "Crouch_Idle_Loop",
                _ if s.crouch > 0.15 => "Crouch_Idle_Loop",
                _ if pushing => "Push_Loop",
                _ => "Idle_Loop",
            };
            if s.state == SkState::Grind {
                // Lean into the grind with the board.
                body_rot *= Quat::from_rotation_z(-s.manual_balance * 0.25);
            }
        }
        Loco::Foot if modes.on(Mode::Jump) => {
            let j = &jump.j;
            rt.translation = p.pos;
            let ph = (j.anim_t * 2.2).fract();
            rig.want = match j.action {
                JAction::Punch { .. } => "Punch_Cross",
                JAction::Crouch | JAction::CrouchSlide => "Crouch_Idle_Loop",
                _ if !j.on_ground => "Jump_Loop",
                _ if speed > 6.5 => "Sprint_Loop",
                _ if speed > 2.6 => "Jog_Fwd_Loop",
                _ if speed > 0.3 => "Walk_Loop",
                _ => "Idle_Loop",
            };
            match j.action {
                JAction::Backflip => body_rot *= Quat::from_rotation_x(-ph * TAU),
                JAction::Jump { kind: 3 } | JAction::Sideflip => body_rot *= Quat::from_rotation_x(ph * TAU),
                JAction::GroundPoundHover { frames } => body_rot *= Quat::from_rotation_x(frames as f32 / 10.0 * TAU),
                JAction::Dive | JAction::BellySlide => {
                    body_rot *= Quat::from_rotation_x(PI / 2.0 - 0.2);
                    drop = 0.5;
                }
                JAction::LongJump => body_rot *= Quat::from_rotation_x(0.9),
                _ => {}
            }
            rig.speed = match rig.want {
                "Walk_Loop" => (speed / 1.5).clamp(0.6, 1.6),
                "Jog_Fwd_Loop" => (speed / 3.8).clamp(0.7, 1.5),
                "Sprint_Loop" => (speed / 7.0).clamp(0.8, 1.6),
                _ => 1.0,
            };
        }
        Loco::Foot => {
            rt.translation = p.pos;
            let crouch = keys.pressed(KeyCode::KeyC) || keys.pressed(KeyCode::ControlLeft);
            let aiming = armed && mouse.pressed(MouseButton::Right);
            rig.want = if !p.on_ground {
                "Jump_Loop"
            } else if crouch {
                if speed > 0.3 { "Crouch_Fwd_Loop" } else { "Crouch_Idle_Loop" }
            } else if aiming {
                "Pistol_Aim_Neutral"
            } else if speed > 6.0 {
                "Sprint_Loop"
            } else if speed > 2.6 {
                "Jog_Fwd_Loop"
            } else if speed > 0.3 {
                "Walk_Loop"
            } else if armed {
                "Pistol_Idle_Loop"
            } else {
                "Idle_Loop"
            };
            rig.speed = match rig.want {
                "Walk_Loop" | "Crouch_Fwd_Loop" => (speed / 1.5).clamp(0.6, 1.8),
                "Jog_Fwd_Loop" => (speed / 3.8).clamp(0.7, 1.5),
                "Sprint_Loop" => (speed / 7.0).clamp(0.8, 1.6),
                _ => 1.0,
            };
        }
        Loco::Swing => {
            rt.translation = p.pos;
            rig.want = "Jump_Loop";
        }
        Loco::Vehicle => {
            rt.translation = p.pos;
            rig.want = "Driving_Loop";
        }
    }
    rt.rotation = Quat::IDENTITY;
    pv.translation = Vec3::Y * (0.9 - drop);
    pv.rotation = body_rot;
}
