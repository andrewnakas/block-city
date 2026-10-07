//! Shared world events every mode reacts to (explosions), plus simple
//! flash/debris visuals.

use crate::core::camera::CamRig;
use crate::core::player::PlayerState;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

#[derive(Message, Clone, Copy, Debug)]
pub struct Explosion {
    pub pos: Vec3,
    pub radius: f32,
    pub power: f32,
}

#[derive(Component)]
pub struct Flash {
    pub age: f32,
    pub life: f32,
    pub grow: f32,
}

#[derive(Resource)]
pub struct FxAssets {
    pub sphere: Handle<Mesh>,
    pub fire: Handle<StandardMaterial>,
    pub tracer: Handle<StandardMaterial>,
    pub cube: Handle<Mesh>,
}

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Explosion>()
            .add_systems(Startup, setup)
            .add_systems(Update, (explode, tick_flash));
    }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(FxAssets {
        sphere: meshes.add(Sphere::new(1.0)),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        fire: mats.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.6, 0.15, 0.85),
            emissive: LinearRgba::rgb(12.0, 4.0, 0.6),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        tracer: mats.add(StandardMaterial { base_color: Color::srgb(1.0, 0.9, 0.5), emissive: LinearRgba::rgb(8.0, 6.0, 2.0), unlit: true, ..default() }),
    });
}

fn explode(
    mut ev: MessageReader<Explosion>,
    mut commands: Commands,
    fx: Res<FxAssets>,
    mut rig: ResMut<CamRig>,
    mut p: ResMut<PlayerState>,
    mut bodies: Query<(&Transform, &mut Velocity, &RigidBody)>,
    mut sfx: MessageWriter<crate::core::audio::Sfx>,
) {
    for e in ev.read() {
        sfx.write(crate::core::audio::Sfx::at("explosion", 1.0, p.pos.distance(e.pos)));
        commands.spawn((
            Flash { age: 0.0, life: 0.45, grow: e.radius * 1.4 },
            Mesh3d(fx.sphere.clone()),
            MeshMaterial3d(fx.fire.clone()),
            Transform::from_translation(e.pos).with_scale(Vec3::splat(0.3)),
        ));
        let d = p.pos.distance(e.pos);
        rig.shake = rig.shake.max((1.0 - d / (e.radius * 6.0)).clamp(0.0, 1.0));
        if d < e.radius * 1.5 {
            p.health -= e.power * (1.0 - d / (e.radius * 1.5)) * 60.0;
            let push = (p.pos - e.pos + Vec3::Y).normalize_or_zero() * e.power * 9.0;
            p.impulse += push;
        }
        for (tf, mut v, rb) in bodies.iter_mut() {
            if *rb != RigidBody::Dynamic {
                continue;
            }
            let off = tf.translation - e.pos;
            let dist = off.length();
            if dist < e.radius * 2.5 {
                let k = (1.0 - dist / (e.radius * 2.5)) * e.power * 14.0;
                v.linear += (off.normalize_or_zero() + Vec3::Y * 0.8) * k;
                v.angular += Vec3::new(off.z, 0.0, -off.x).normalize_or_zero() * k * 0.3;
            }
        }
    }
}

fn tick_flash(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut Flash, &mut Transform)>) {
    for (e, mut f, mut tf) in q.iter_mut() {
        f.age += time.delta_secs();
        let k = f.age / f.life;
        if k >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        tf.scale = Vec3::splat(0.3 + f.grow * k.sqrt());
    }
}
