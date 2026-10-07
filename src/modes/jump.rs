//! JUMP mode extras: collectible shards scattered over the city (the course,
//! rooftops, the mega ramp), move names in the score feed, and ground pounds
//! that smash voxel blocks.

use crate::core::modes::{Mode, ModeToggled, mode_on};
use crate::core::player::{JumpSim, Loco, PlayerState};
use crate::core::score::{Kind, MashEvent};
use crate::core::ui::Hud;
use crate::modes::blocks::VoxelWorld;
use crate::sim::jump::JEvent;
use crate::sim::voxel::Block;
use crate::world::city::block_center;
use bevy::prelude::*;

#[derive(Component)]
struct Shard(usize);

#[derive(Resource, Default)]
pub struct Shards {
    pub got: Vec<bool>,
}

fn shard_spots() -> Vec<Vec3> {
    let course = block_center(1, 0);
    let mega = block_center(0, 1);
    vec![
        course + Vec3::new(16.0, 12.6, 4.0),
        course + Vec3::new(-14.0, 20.0, 14.0),
        course + Vec3::new(20.0, 16.8, 18.0),
        course + Vec3::new(10.0, 4.0, 11.0),
        mega + Vec3::new(0.0, 18.0, -24.5),
        mega + Vec3::new(0.0, 9.0, 4.0),
        block_center(0, 0) + Vec3::new(-14.0, 6.0, 20.0),
        block_center(-1, 0) + Vec3::new(0.0, 14.0, 0.0),
    ]
}

pub struct JumpPlugin;

impl Plugin for JumpPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shards>()
            .add_systems(Startup, spawn_shards)
            .add_systems(Update, (toggle, events.run_if(mode_on(Mode::Jump)), collect.run_if(mode_on(Mode::Jump)).run_if(crate::core::scene::is(crate::core::scene::World::City)), spin));
    }
}

fn spawn_shards(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>, mut shards: ResMut<Shards>) {
    let mesh = meshes.add(Tetrahedron::default());
    let mat = mats.add(StandardMaterial { base_color: Color::srgb(1.0, 0.85, 0.2), emissive: LinearRgba::rgb(3.0, 2.2, 0.4), metallic: 0.9, ..default() });
    let spots = shard_spots();
    shards.got = vec![false; spots.len()];
    for (i, p) in spots.into_iter().enumerate() {
        commands.spawn((Shard(i), Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::from_translation(p).with_scale(Vec3::splat(0.9)), Visibility::Hidden));
    }
}

fn toggle(mut t: MessageReader<ModeToggled>, mut q: Query<(&Shard, &mut Visibility)>, shards: Res<Shards>, world: Res<crate::core::scene::World>) {
    for ev in t.read() {
        // Shards are placed around the city's landmarks; other worlds have none.
        if ev.mode == Mode::Jump && *world == crate::core::scene::World::City {
            for (s, mut v) in q.iter_mut() {
                *v = if ev.on && !shards.got[s.0] { Visibility::Visible } else { Visibility::Hidden };
            }
        }
    }
}

fn spin(time: Res<Time>, mut q: Query<&mut Transform, With<Shard>>) {
    for mut t in q.iter_mut() {
        t.rotation = Quat::from_rotation_y(time.elapsed_secs() * 2.0) * Quat::from_rotation_x(0.6);
    }
}

fn collect(mut q: Query<(&Shard, &Transform, &mut Visibility)>, p: Res<PlayerState>, mut shards: ResMut<Shards>, mut out: MessageWriter<MashEvent>, mut hud: ResMut<Hud>) {
    for (s, t, mut v) in q.iter_mut() {
        if !shards.got[s.0] && t.translation.distance(p.pos + Vec3::Y * 0.8) < 1.5 {
            shards.got[s.0] = true;
            *v = Visibility::Hidden;
            let n = shards.got.iter().filter(|g| **g).count();
            out.write(MashEvent::new(Kind::Platform, "Shard", 1000));
            hud.banner = Some((format!("SHARD GET!  {n} / {}", shards.got.len()), 2.5));
        }
    }
    hud.top.push(format!("Shards {} / {}", shards.got.iter().filter(|g| **g).count(), shards.got.len()));
}

fn events(mut sim: ResMut<JumpSim>, p: Res<PlayerState>, mut out: MessageWriter<MashEvent>, voxels: Option<ResMut<VoxelWorld>>, modes: Res<crate::core::modes::ActiveModes>) {
    if p.loco != Loco::Foot {
        sim.j.events.clear();
        return;
    }
    let mut voxels = voxels;
    for ev in sim.j.events.drain(..) {
        match ev {
            JEvent::Jump(name) if name != "Jump" && name != "Ground Pound" => {
                out.write(MashEvent::new(Kind::Trick, name, match name {
                    "Triple Jump" => 300,
                    "Long Jump" | "Backflip" | "Side Flip" => 150,
                    _ => 60,
                }));
            }
            JEvent::WallKick => {
                out.write(MashEvent::new(Kind::Trick, "Wall Kick", 120));
            }
            JEvent::GroundPoundImpact => {
                out.write(MashEvent::new(Kind::Platform, "Ground Pound", 50));
                // Synergy: pounds smash the blocks under you.
                if modes.on(Mode::Blocks)
                    && let Some(v) = voxels.as_mut()
                {
                    let c = (p.pos - Vec3::Y * 0.5).floor().as_ivec3();
                    for dx in -1..=1 {
                        for dz in -1..=1 {
                            let cell = c + IVec3::new(dx, 0, dz);
                            if v.v.get(cell) != Block::Air {
                                v.v.set(cell, Block::Air);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
