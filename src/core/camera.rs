//! One camera rig for every mode: low skate follow cam, platformer orbit,
//! chase cam in cars, first person for shooting and building.

use crate::core::input::Look;
use crate::core::modes::{ActiveModes, Mode};
use crate::core::phys::G_WORLD;
use crate::core::player::{Loco, PlayerState};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

#[derive(Component)]
pub struct MainCam;

#[derive(Resource)]
pub struct CamRig {
    pub first_person: bool,
    /// Player pressed V to override the automatic choice.
    pub manual: Option<bool>,
    pub dist: f32,
    pub fov: f32,
    pub ads: f32,
    pub shake: f32,
    pub eye: Vec3,
    pub forward: Vec3,
}

impl Default for CamRig {
    fn default() -> Self {
        Self { first_person: false, manual: None, dist: 4.0, fov: 70.0, ads: 0.0, shake: 0.0, eye: Vec3::ZERO, forward: Vec3::Z }
    }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CamRig>()
            .add_systems(Startup, spawn_camera)
            .add_systems(PostUpdate, follow.before(TransformSystems::Propagate));
    }
}

pub fn spawn_camera(mut commands: Commands) {
    let mut cam = commands.spawn((
        MainCam,
        // The HUD draws on this camera, never on the portal cameras.
        bevy_egui::PrimaryEguiContext,
        // Layer 1 holds portal surfaces, which only the main view draws.
        bevy::camera::visibility::RenderLayers::from_layers(&[0, 1]),
        Camera3d::default(),
        if cfg!(target_arch = "wasm32") { Msaa::Off } else { Msaa::Sample4 },
        Projection::Perspective(PerspectiveProjection { fov: 70f32.to_radians(), near: 0.05, far: 900.0, ..default() }),
        Transform::from_xyz(0.0, 3.0, -36.0),
        crate::core::look::main_camera_look(),
        DistanceFog {
            color: if crate::core::scene::night() { Color::srgb(0.03, 0.04, 0.08) } else { Color::srgb(0.66, 0.78, 0.92) },
            falloff: if crate::core::scene::night() { FogFalloff::Linear { start: 40.0, end: 260.0 } } else { FogFalloff::Linear { start: 140.0, end: 520.0 } },
            ..default()
        },
    ));
    crate::core::look::attach_high_quality(&mut cam);
}

/// Default view per locomotion + active modes.
pub fn wants_first_person(p: &PlayerState, modes: &ActiveModes) -> bool {
    p.loco == Loco::Foot && !modes.on(Mode::Jump) && (modes.on(Mode::Warfare) || modes.on(Mode::Blocks) || modes.on(Mode::Portals))
}

#[allow(clippy::too_many_arguments)]
pub fn follow(
    time: Res<Time<bevy::time::Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    look: Res<Look>,
    modes: Res<ActiveModes>,
    p: Res<PlayerState>,
    mut rig: ResMut<CamRig>,
    rapier: ReadRapierContext,
    mut cam: Query<(&mut Transform, &mut Projection), With<MainCam>>,
) {
    let Ok((mut tf, mut proj)) = cam.single_mut() else { return };
    let dt = time.delta_secs();
    if keys.just_pressed(KeyCode::KeyV) {
        let cur = rig.first_person;
        rig.manual = Some(!cur);
    }
    let auto = wants_first_person(&p, &modes);
    rig.first_person = rig.manual.unwrap_or(auto) && matches!(p.loco, Loco::Foot | Loco::Skate);

    let aiming = modes.on(Mode::Warfare) && mouse.pressed(MouseButton::Right);
    rig.ads += ((if aiming { 1.0 } else { 0.0 }) - rig.ads) * (dt * 14.0).min(1.0);

    let (sy, cy) = look.yaw.sin_cos();
    let (sp, cp) = look.pitch.sin_cos();
    let fwd = Vec3::new(sy * cp, sp, cy * cp);
    rig.forward = fwd;

    let (target, eye) = if rig.first_person {
        let eye_h = if p.loco == Loco::Skate { 1.55 } else { 1.62 };
        let e = p.pos + Vec3::Y * eye_h;
        (e + fwd, e)
    } else {
        let (height, want) = match p.loco {
            // Low, close and wide, riding just behind the board.
            Loco::Skate => (1.0, 2.25),
            Loco::Vehicle => (1.8, 8.5),
            Loco::Swing => (1.2, 6.0),
            Loco::Foot => {
                if modes.on(Mode::Jump) { (1.0, 7.0) } else { (1.6, 3.6) }
            }
        };
        // Over-the-shoulder when aiming in third person.
        let want = want * (1.0 - 0.55 * rig.ads);
        let right = Vec3::new(-cy, 0.0, sy);
        let pivot = p.pos + Vec3::Y * height + right * (-0.6 * rig.ads);
        let speed_pull = (p.vel.length() * 0.06).min(1.2);
        let mut dist = want + speed_pull;
        if let Ok(ctx) = rapier.single() {
            let filt = QueryFilter::new().exclude_sensors().groups(CollisionGroups::new(Group::ALL, G_WORLD));
            if let Some((_, toi)) = ctx.cast_ray(pivot, -fwd, dist + 0.3, true, filt) {
                dist = (toi - 0.3).max(0.6);
            }
        }
        rig.dist += (dist - rig.dist) * (dt * 10.0).min(1.0);
        if dist < rig.dist {
            rig.dist = dist;
        }
        (pivot, pivot - fwd * rig.dist)
    };

    let mut eye = eye;
    if rig.shake > 0.0 {
        let t = time.elapsed_secs();
        eye += Vec3::new((t * 53.0).sin(), (t * 61.0).cos(), (t * 47.0).sin()) * rig.shake * 0.15;
        rig.shake = (rig.shake - dt * 2.5).max(0.0);
    }
    rig.eye = eye;
    *tf = Transform::from_translation(eye).looking_at(target, Vec3::Y);

    let base_fov = match p.loco {
        Loco::Vehicle => 72.0 + (p.vel.length() * 0.6).min(18.0),
        Loco::Skate => 76.0 + (p.vel.length() * 0.6).min(10.0),
        _ => 70.0 + (p.vel.length() * 0.35).min(10.0),
    };
    rig.fov = base_fov * (1.0 - 0.32 * rig.ads);
    if let Projection::Perspective(pp) = proj.as_mut() {
        pp.fov = rig.fov.to_radians();
    }
}
