//! The arena: a ruined colosseum for the platformer-versus-boss tribute. A flat
//! fighting floor ringed by broken pillars (wall-kick height), two low terraces
//! to jump between, and a crumbling outer wall.

use super::build::Builder;
use bevy::prelude::*;

pub const RADIUS: f32 = 28.0;

// These colours map to the concrete and dirt textures (see materials::surf_for).
const FLOOR: [u8; 3] = [178, 176, 170];
const STONE: [u8; 3] = [196, 192, 184];
const DARK: [u8; 3] = [120, 120, 128];
const MOSS: [u8; 3] = [110, 168, 96];

pub fn build_arena(assets: Res<AssetServer>, mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut b = Builder::new(&assets, &mut commands, &mut meshes, &mut materials);
    b.boxy(Vec3::new(0.0, -0.5, 0.0), Vec3::new(80.0, 0.5, 80.0), DARK);
    // Flagstone rings so the floor reads as a circle.
    for (r, c) in [(RADIUS, FLOOR), (18.0, STONE), (6.0, MOSS)] {
        let n = 24;
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let p = Vec3::new(a.sin(), 0.0, a.cos()) * (r - 2.0);
            b.deco(p + Vec3::Y * 0.01, Vec3::new(r * 0.14, 0.01, 2.2), c);
        }
    }
    // Pillars: some whole, some broken; tall ones are wall-kick walls.
    let n = 16;
    for i in 0..n {
        let a = (i as f32 + 0.5) / n as f32 * std::f32::consts::TAU;
        let p = Vec3::new(a.sin(), 0.0, a.cos()) * 22.0;
        let h = if i % 3 == 0 { 2.0 } else if i % 3 == 1 { 7.5 } else { 4.5 };
        b.boxy(p + Vec3::Y * h * 0.5, Vec3::new(1.1, h * 0.5, 1.1), STONE);
        b.deco(p + Vec3::Y * (h + 0.15), Vec3::new(1.4, 0.15, 1.4), DARK);
    }
    // Two terraces on opposite sides to fight from above.
    for z in [-1.0f32, 1.0] {
        b.boxy(Vec3::new(14.0 * z, 0.75, 14.0 * z), Vec3::new(4.0, 0.75, 4.0), STONE);
        b.boxy(Vec3::new(17.0 * z, 1.75, 17.0 * z), Vec3::new(2.5, 0.25, 2.5), STONE);
    }
    // Outer wall with gaps, then an invisible boundary.
    let n = 40;
    for i in 0..n {
        if i % 7 == 3 {
            continue;
        }
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let p = Vec3::new(a.sin(), 0.0, a.cos()) * (RADIUS + 3.0);
        let h = 5.0 + ((i * 37) % 5) as f32;
        b.solid(p + Vec3::Y * h * 0.5, Vec3::new(2.6, h * 0.5, 0.8), Quat::from_rotation_y(a), DARK);
    }
    for (c, h) in [
        (Vec3::new(0.0, 15.0, RADIUS + 6.0), Vec3::new(RADIUS + 8.0, 15.0, 1.0)),
        (Vec3::new(0.0, 15.0, -RADIUS - 6.0), Vec3::new(RADIUS + 8.0, 15.0, 1.0)),
        (Vec3::new(RADIUS + 6.0, 15.0, 0.0), Vec3::new(1.0, 15.0, RADIUS + 8.0)),
        (Vec3::new(-RADIUS - 6.0, 15.0, 0.0), Vec3::new(1.0, 15.0, RADIUS + 8.0)),
    ] {
        b.commands.spawn((
            crate::world::build::Static,
            Transform::from_translation(c),
            bevy_rapier3d::prelude::Collider::cuboid(h.x, h.y, h.z),
            bevy_rapier3d::prelude::RigidBody::Fixed,
            bevy_rapier3d::prelude::CollisionGroups::new(crate::core::phys::G_WORLD, bevy_rapier3d::prelude::Group::ALL),
        ));
    }
    b.finish();
}
