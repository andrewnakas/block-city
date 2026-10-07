//! The realm: rolling voxel countryside for the block-builder-in-an-RPG
//! tribute. Hills, a river, three villages of timber huts, a stone keep on the
//! central hill, trees and ore. Everything is blocks, so all of it can be dug,
//! built on or blown up.

use super::build::Builder;
use crate::core::modes::ActiveModes;
use crate::core::player::{JumpSim, PlayerState, SkateSim, WalkSim};
use crate::modes::blocks::VoxelWorld;
use crate::sim::voxel::{Block, Voxels, noise2};
use bevy::prelude::*;

/// Half-width of the realm in blocks.
pub const R: i32 = 64;
pub const KEEP: IVec2 = IVec2::new(0, 22);
pub const VILLAGES: [IVec2; 3] = [IVec2::new(-38, -26), IVec2::new(36, -30), IVec2::new(-28, 40)];
const SPAWN_XZ: IVec2 = IVec2::new(0, -48);

fn river_dist(x: f32, z: f32) -> f32 {
    let centre = 22.0 * (x * 0.035).sin() - 4.0;
    (z - centre).abs()
}

/// Natural ground height before any building is placed.
pub fn natural_height(x: i32, z: i32) -> i32 {
    let (fx, fz) = (x as f32, z as f32);
    let hills = noise2(fx * 0.028, fz * 0.028, 5) * 14.0 + noise2(fx * 0.09, fz * 0.09, 11) * 3.0;
    // The keep sits on a raised hill.
    let keep = (1.0 - (Vec2::new(fx, fz) - KEEP.as_vec2()).length() / 26.0).max(0.0) * 9.0;
    let mut h = 4.0 + hills + keep;
    // The river carves a valley.
    let rd = river_dist(fx, fz);
    if rd < 9.0 {
        h = h.min(2.0 + rd * 0.6);
    }
    // Low shore at the edge of the map.
    let edge = (R - x.abs().max(z.abs())) as f32 / 8.0;
    if edge < 1.0 {
        h = 1.0 + (h - 1.0) * edge.max(0.0);
    }
    h.max(1.0) as i32
}

/// Height after villages are levelled.
fn ground_height(x: i32, z: i32) -> i32 {
    for v in VILLAGES {
        let d = (IVec2::new(x, z) - v).as_vec2().length();
        if d < 13.0 {
            let base = natural_height(v.x, v.y);
            // Blend the levelled pad into the surrounding hills.
            let k = ((d - 9.0) / 4.0).clamp(0.0, 1.0);
            return (base as f32 + (natural_height(x, z) - base) as f32 * k).round() as i32;
        }
    }
    natural_height(x, z)
}

/// Top solid block at (x, z), scanning down from `from_y`. Used by NPCs to walk on dug/built terrain.
pub fn surface(v: &Voxels, x: i32, z: i32, from_y: i32) -> Option<i32> {
    (0..=from_y).rev().find(|y| v.get(IVec3::new(x, *y, z)).solid())
}

fn hut(v: &mut Voxels, c: IVec3, door_dir: i32) {
    // 5x5 timber hut: log corners, plank walls, a door gap and a stepped brick roof.
    for x in -2..=2 {
        for z in -2..=2 {
            v.set(c + IVec3::new(x, 0, z), Block::Planks);
            let edge = x.abs() == 2 || z.abs() == 2;
            for y in 1..=3 {
                let corner = x.abs() == 2 && z.abs() == 2;
                let door = match door_dir {
                    0 => z == -2 && x == 0,
                    1 => x == 2 && z == 0,
                    2 => z == 2 && x == 0,
                    _ => x == -2 && z == 0,
                } && y <= 2;
                if edge && !door {
                    v.set(c + IVec3::new(x, y, z), if corner { Block::Log } else { Block::Planks });
                } else if !edge {
                    v.set(c + IVec3::new(x, y, z), Block::Air);
                }
            }
        }
    }
    for (y, r) in [(4, 3i32), (5, 2), (6, 1)] {
        for x in -r..=r {
            for z in -r..=r {
                if x.abs() == r || z.abs() == r || y == 6 {
                    v.set(c + IVec3::new(x, y, z), Block::Brick);
                }
            }
        }
    }
}

fn keep(v: &mut Voxels, c: IVec3) {
    // Curtain wall with battlements, a gate, and a tall central tower.
    let r = 9;
    for x in -r..=r {
        for z in -r..=r {
            v.set(c + IVec3::new(x, 0, z), Block::Stone);
            let wall = x.abs() == r || z.abs() == r;
            let gate = z == -r && x.abs() <= 1;
            for y in 1..=8 {
                let p = c + IVec3::new(x, y, z);
                if wall && !(gate && y <= 4) {
                    v.set(p, Block::Stone);
                } else if !wall {
                    v.set(p, Block::Air);
                }
            }
            if wall && (x + z).rem_euclid(2) == 0 {
                v.set(c + IVec3::new(x, 9, z), Block::Stone);
            }
            // Wall walk.
            if (x.abs() == r - 1 || z.abs() == r - 1) && !(z == -(r - 1) && x.abs() <= 1) {
                v.set(c + IVec3::new(x, 7, z), Block::Planks);
            }
        }
    }
    for x in -2..=2i32 {
        for z in -2..=2i32 {
            let shell = x.abs() == 2 || z.abs() == 2;
            for y in 1..=16 {
                let door = z == -2 && x == 0 && y <= 2;
                if shell && !door {
                    v.set(c + IVec3::new(x, y, z), Block::Stone);
                }
            }
            if (x + z).rem_euclid(2) == 0 && shell {
                v.set(c + IVec3::new(x, 17, z), Block::Stone);
            }
        }
    }
    // A stair of planks up the inside of the tower and a gold cache on top.
    for y in 1..=15 {
        let (x, z) = [(-1, -1), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0)][(y as usize) % 8];
        v.set(c + IVec3::new(x, y, z), Block::Planks);
    }
    v.set(c + IVec3::new(0, 16, 0), Block::Gold);
}

pub fn generate_realm(mut world: ResMut<VoxelWorld>) {
    let v = &mut world.v;
    for x in -R..R {
        for z in -R..R {
            let h = ground_height(x, z);
            let rd = river_dist(x as f32, z as f32);
            for y in 0..=h {
                let b = if y == h {
                    if rd < 6.0 || h <= 2 { Block::Sand } else { Block::Grass }
                } else if y > h - 3 {
                    if rd < 6.0 { Block::Sand } else { Block::Dirt }
                } else if noise2(x as f32 * 0.5, (z + y * 13) as f32 * 0.5, 23) > 0.88 {
                    Block::Gold
                } else {
                    Block::Stone
                };
                v.set(IVec3::new(x, y, z), b);
            }
            let near_build = VILLAGES.iter().any(|c| (IVec2::new(x, z) - *c).as_vec2().length() < 14.0) || (IVec2::new(x, z) - KEEP).as_vec2().length() < 14.0;
            if h > 4 && rd > 8.0 && !near_build && noise2(x as f32 * 1.7, z as f32 * 1.7, 43) > 0.9 {
                for t in 1..6 {
                    v.set(IVec3::new(x, h + t, z), Block::Log);
                }
                for dx in -2..=2i32 {
                    for dz in -2..=2i32 {
                        for dy in 4..7 {
                            if dx.abs() + dz.abs() + (dy - 5i32).abs() < 4 && v.get(IVec3::new(x + dx, h + dy, z + dz)) == Block::Air {
                                v.set(IVec3::new(x + dx, h + dy, z + dz), Block::Leaves);
                            }
                        }
                    }
                }
            }
        }
    }
    for (i, c) in VILLAGES.iter().enumerate() {
        let base = natural_height(c.x, c.y);
        for (k, off) in [IVec2::new(-6, -5), IVec2::new(5, -6), IVec2::new(-5, 6), IVec2::new(6, 5)].iter().enumerate() {
            hut(v, IVec3::new(c.x + off.x, base, c.y + off.y), ((i + k) % 4) as i32);
        }
        // Well in the square, with TNT stashed beside it.
        for (x, z) in [(-1, -1), (1, -1), (-1, 1), (1, 1), (0, -1), (0, 1), (-1, 0), (1, 0)] {
            v.set(IVec3::new(c.x + x, base + 1, c.y + z), Block::Stone);
        }
        v.set(IVec3::new(c.x + 2, base + 1, c.y + 2), Block::Tnt);
    }
    keep(v, IVec3::new(KEEP.x, natural_height(KEEP.x, KEEP.y), KEEP.y));
}

/// Bedrock slab under the realm and invisible walls at its edge.
pub fn build_realm_base(assets: Res<AssetServer>, mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut b = Builder::new(&assets, &mut commands, &mut meshes, &mut materials);
    let ext = R as f32 + 20.0;
    b.boxy(Vec3::new(0.0, -0.5, 0.0), Vec3::new(ext + 40.0, 0.5, ext + 40.0), [58, 62, 70]);
    let r = R as f32 + 0.5;
    for (c, h) in [
        (Vec3::new(0.0, 20.0, r + 1.0), Vec3::new(r + 2.0, 20.0, 1.0)),
        (Vec3::new(0.0, 20.0, -r - 1.0), Vec3::new(r + 2.0, 20.0, 1.0)),
        (Vec3::new(r + 1.0, 20.0, 0.0), Vec3::new(1.0, 20.0, r + 2.0)),
        (Vec3::new(-r - 1.0, 20.0, 0.0), Vec3::new(1.0, 20.0, r + 2.0)),
    ] {
        // Collider only: the realm fades into fog rather than ending at a wall.
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

pub fn place_realm_spawn(world: Res<VoxelWorld>, mut p: ResMut<PlayerState>, modes: Res<ActiveModes>, mut skate: ResMut<SkateSim>, mut jump: ResMut<JumpSim>, mut walk: ResMut<WalkSim>) {
    let y = surface(&world.v, SPAWN_XZ.x, SPAWN_XZ.y, 40).unwrap_or(4) + 1;
    let pos = Vec3::new(SPAWN_XZ.x as f32 + 0.5, y as f32 + 0.1, SPAWN_XZ.y as f32 + 0.5);
    crate::core::scene::place_player(pos, 0.0, &mut p, &modes, &mut skate, &mut jump, &mut walk);
}
