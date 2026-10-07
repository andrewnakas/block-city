//! BLOCKS mode: a voxel layer over the whole city. Break and place anywhere,
//! rail blocks become real grind rails, TNT blows holes in everything.

use crate::core::camera::CamRig;
use crate::core::fx::Explosion;
use crate::core::hotbar::{Hotbar, Item};
use crate::core::input::{Latch, Look};
use crate::core::modes::{ActiveModes, Mode, ModeToggled, mode_on};
use crate::core::phys::{G_WORLD, Rails};
use crate::core::player::{MoveSet, PlayerState};
use crate::core::score::{Kind, MashEvent};
use crate::core::ui::Hud;
use crate::sim::voxel::{Block, Voxels, noise2};
use crate::world::build::PortalSurface;
use crate::core::scene::{self, World};
use crate::world::city::{LOT, QUARRY, block_center};
use crate::world::realm;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy_rapier3d::prelude::*;
use std::collections::HashMap;

#[derive(Resource, Default)]
pub struct VoxelWorld {
    pub v: Voxels,
    entities: HashMap<IVec3, Entity>,
    static_rails: Option<usize>,
    rails_dirty: bool,
}

#[derive(Resource)]
struct BlockMats(HashMap<u8, Handle<StandardMaterial>>);

#[derive(Resource, Default)]
pub struct BuildState {
    pub selected: usize,
    target: Option<(IVec3, IVec3)>,
    breaking: Option<(IVec3, f32)>,
}

#[derive(Component)]
struct VoxelChunk;
#[derive(Component)]
struct Highlight;
#[derive(Component)]
struct PrimedTnt {
    fuse: f32,
}

pub struct BlocksPlugin;

impl Plugin for BlocksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VoxelWorld>()
            .init_resource::<BuildState>()
            .add_systems(Startup, (setup, generate_quarry.run_if(scene::is(World::City))))
            .add_systems(
                Startup,
                (realm::build_realm_base, (realm::generate_realm, realm::place_realm_spawn).chain().after(crate::core::player::spawn_player)).run_if(scene::is(World::Realm)),
            )
            .add_systems(FixedUpdate, interact.in_set(MoveSet::Abilities).run_if(mode_on(Mode::Blocks)))
            .add_systems(Update, (show_hide, remesh, sync_rails, tnt_fuse, explosions_carve, hud_and_highlight));
    }
}

fn block_color(b: Block) -> [u8; 3] {
    match b {
        Block::Grass => [96, 168, 72],
        Block::Dirt => [134, 96, 64],
        Block::Stone => [128, 128, 132],
        Block::Planks => [178, 140, 84],
        Block::Log => [104, 80, 52],
        Block::Leaves => [64, 132, 56],
        Block::Sand => [222, 206, 150],
        Block::Brick => [160, 72, 60],
        Block::Rail => [190, 190, 200],
        Block::Tnt => [210, 52, 44],
        Block::Gold => [236, 196, 64],
        Block::Air => [0, 0, 0],
    }
}

/// 16x16 pixel-art texture generated from a base colour (original art).
fn block_texture(b: Block) -> Image {
    let base = block_color(b);
    let mut data = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16u32 {
        for x in 0..16u32 {
            let n = noise2(x as f32 * 0.9, y as f32 * 0.9, b as u32 * 31 + 7);
            let mut k = 0.78 + n * 0.4;
            match b {
                Block::Planks if y % 4 == 0 => k *= 0.7,
                Block::Brick if y % 4 == 0 || (x + if (y / 4) % 2 == 0 { 0 } else { 4 }) % 8 == 0 => k = 1.25,
                Block::Log if x % 5 == 0 => k *= 0.75,
                Block::Tnt if (6..10).contains(&y) => {
                    data.extend_from_slice(&[235, 235, 225, 255]);
                    continue;
                }
                Block::Rail if y <= 2 || y >= 13 => k = 1.3,
                Block::Gold if n > 0.75 => k = 1.4,
                Block::Grass if y < 3 && n < 0.5 => {
                    data.extend_from_slice(&[120, 88, 58, 255]);
                    continue;
                }
                _ => {}
            }
            let c = |v: u8| ((v as f32 * k).min(255.0)) as u8;
            let a = if b == Block::Leaves && n < 0.25 { 0 } else { 255 };
            data.extend_from_slice(&[c(base[0]), c(base[1]), c(base[2]), a]);
        }
    }
    let mut img = Image::new(Extent3d { width: 16, height: 16, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        ..default()
    });
    img
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut mats: ResMut<Assets<StandardMaterial>>, mut meshes: ResMut<Assets<Mesh>>) {
    let mut map = HashMap::new();
    for b in Block::PLACEABLE.iter().chain([Block::Gold].iter()) {
        let tex = images.add(block_texture(*b));
        map.insert(
            *b as u8,
            mats.add(StandardMaterial {
                base_color_texture: Some(tex),
                perceptual_roughness: 0.95,
                alpha_mode: if *b == Block::Leaves { AlphaMode::Mask(0.5) } else { AlphaMode::Opaque },
                ..default()
            }),
        );
    }
    commands.insert_resource(BlockMats(map));
    commands.spawn((
        Highlight,
        Mesh3d(meshes.add(Cuboid::new(1.02, 1.02, 1.02))),
        MeshMaterial3d(mats.add(StandardMaterial { base_color: Color::srgba(1.0, 1.0, 1.0, 0.18), alpha_mode: AlphaMode::Blend, unlit: true, ..default() })),
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// Quarry terrain: rolling hills, a mined pit, trees, ore, and a rail line.
fn generate_quarry(mut world: ResMut<VoxelWorld>) {
    let c = block_center(QUARRY.0, QUARRY.1);
    let lot = LOT as i32 - 1;
    let v = &mut world.v;
    for x in -lot..lot {
        for z in -lot..lot {
            let wx = c.x as i32 + x;
            let wz = c.z as i32 + z;
            let edge = ((lot - x.abs().max(z.abs())) as f32 / 6.0).min(1.0);
            let hills = noise2(wx as f32 * 0.07, wz as f32 * 0.07, 3) * 9.0 + noise2(wx as f32 * 0.2, wz as f32 * 0.2, 9) * 3.0;
            let pit = ((x * x + (z - 6) * (z - 6)) as f32).sqrt() < 9.0;
            let h = if pit { 0 } else { (hills * edge) as i32 };
            for y in 0..=h {
                let b = if y == h {
                    if h < 2 { Block::Sand } else { Block::Grass }
                } else if y > h - 3 {
                    Block::Dirt
                } else if noise2(wx as f32 * 0.5, (wz + y * 13) as f32 * 0.5, 17) > 0.86 {
                    Block::Gold
                } else {
                    Block::Stone
                };
                v.set(IVec3::new(wx, y, wz), b);
            }
            // Trees on high ground.
            if h > 4 && !pit && noise2(wx as f32 * 1.7, wz as f32 * 1.7, 41) > 0.93 {
                for t in 1..5 {
                    v.set(IVec3::new(wx, h + t, wz), Block::Log);
                }
                for dx in -2..=2i32 {
                    for dz in -2..=2i32 {
                        for dy in 3..6 {
                            if dx.abs() + dz.abs() + (dy - 4i32).abs() < 4 && v.get(IVec3::new(wx + dx, h + dy, wz + dz)) == Block::Air {
                                v.set(IVec3::new(wx + dx, h + dy, wz + dz), Block::Leaves);
                            }
                        }
                    }
                }
            }
        }
    }
    // Showcase: a grindable rail line across the pit floor and a TNT stack.
    for x in -6..7 {
        v.set(IVec3::new(c.x as i32 + x, 0, c.z as i32 + 6), Block::Rail);
    }
    for y in 1..4 {
        v.set(IVec3::new(c.x as i32 + 4, y, c.z as i32 + 12), Block::Tnt);
    }
    world.rails_dirty = true;
}

fn show_hide(mut toggles: MessageReader<ModeToggled>, mut q: Query<&mut Visibility, With<VoxelChunk>>, mut cmds: Commands, chunks: Query<Entity, With<VoxelChunk>>) {
    for t in toggles.read() {
        if t.mode != Mode::Blocks {
            continue;
        }
        for mut v in q.iter_mut() {
            *v = if t.on { Visibility::Inherited } else { Visibility::Hidden };
        }
        // Colliders follow visibility so a disabled mode has no invisible walls.
        for e in chunks.iter() {
            if t.on {
                cmds.entity(e).remove::<ColliderDisabled>();
            } else {
                cmds.entity(e).insert(ColliderDisabled);
            }
        }
    }
}

fn remesh(mut commands: Commands, mut world: ResMut<VoxelWorld>, mats: Res<BlockMats>, mut meshes: ResMut<Assets<Mesh>>, modes: Res<ActiveModes>) {
    if world.v.dirty.is_empty() {
        return;
    }
    let dirty: Vec<IVec3> = world.v.dirty.drain().collect();
    let on = modes.on(Mode::Blocks);
    for ck in dirty {
        if let Some(e) = world.entities.remove(&ck) {
            commands.entity(e).despawn();
        }
        let quads = world.v.mesh_chunk(ck);
        if quads.is_empty() {
            continue;
        }
        // One mesh per block type so each gets its own repeating texture.
        let mut by_type: HashMap<u8, (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>)> = HashMap::new();
        for q in &quads {
            let e = by_type.entry(q.block as u8).or_default();
            let n0 = e.0.len() as u32;
            let axis = if q.normal.x != 0.0 { 0 } else if q.normal.y != 0.0 { 1 } else { 2 };
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            for c in q.corners {
                e.0.push(c.to_array());
                e.1.push(q.normal.to_array());
                e.2.push([c[u], -c[v]]);
            }
            // Corners run along +u then +v, so (u x v) = +axis: CCW faces +axis.
            if q.normal[axis] > 0.0 {
                e.3.extend_from_slice(&[n0, n0 + 1, n0 + 2, n0, n0 + 2, n0 + 3]);
            } else {
                e.3.extend_from_slice(&[n0, n0 + 2, n0 + 1, n0, n0 + 3, n0 + 2]);
            }
        }
        let boxes = world.v.boxes_chunk(ck);
        let mut parts = vec![];
        for (mn, mx) in &boxes {
            let half = (*mx - *mn) * 0.5;
            parts.push((*mn + half, Quat::IDENTITY, Collider::cuboid(half.x, half.y, half.z)));
        }
        let mut ec = commands.spawn((
            VoxelChunk,
            PortalSurface,
            Transform::default(),
            if on { Visibility::Inherited } else { Visibility::Hidden },
            RigidBody::Fixed,
            CollisionGroups::new(G_WORLD, Group::ALL),
        ));
        if !parts.is_empty() {
            ec.insert(Collider::compound(parts));
        }
        if !on {
            ec.insert(ColliderDisabled);
        }
        ec.with_children(|c| {
            for (t, (pos, nrm, uv, idx)) in by_type {
                let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
                    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
                    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
                    .with_inserted_indices(Indices::U32(idx));
                if let Some(m) = mats.0.get(&t) {
                    c.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(m.clone()), Transform::default()));
                }
            }
        });
        let id = ec.id();
        world.entities.insert(ck, id);
    }
    world.rails_dirty = true;
}

fn sync_rails(mut world: ResMut<VoxelWorld>, rails: Option<ResMut<Rails>>, modes: Res<ActiveModes>, mut last_on: Local<bool>) {
    let Some(mut rails) = rails else { return };
    let on = modes.on(Mode::Blocks);
    if !world.rails_dirty && *last_on == on {
        return;
    }
    *last_on = on;
    world.rails_dirty = false;
    let n = *world.static_rails.get_or_insert(rails.0.rails.len());
    rails.0.rails.truncate(n);
    if on {
        rails.0.rails.extend(world.v.rail_runs());
    }
}

#[allow(clippy::too_many_arguments)]
fn interact(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    latch: Res<Latch>,
    look: Res<Look>,
    bar: Res<Hotbar>,
    rig: Res<CamRig>,
    p: Res<PlayerState>,
    mut world: ResMut<VoxelWorld>,
    mut st: ResMut<BuildState>,
    mut out: MessageWriter<MashEvent>,
    mut commands: Commands,
    mats: Res<BlockMats>,
    fx: Res<crate::core::fx::FxAssets>,
    melee: Res<crate::modes::raiders::MeleeTarget>,
) {
    // Scroll / brackets cycle the block type.
    let n = Block::PLACEABLE.len() as i32;
    let mut sel = st.selected as i32;
    if latch.scroll > 0.0 || latch.key(KeyCode::BracketRight) {
        sel += 1;
    }
    if latch.scroll < 0.0 || latch.key(KeyCode::BracketLeft) {
        sel -= 1;
    }
    st.selected = sel.rem_euclid(n) as usize;

    // A raider in reach takes the click instead of the block behind it.
    if bar.item != Item::Blocks || !look.captured || melee.0.is_some() {
        st.target = None;
        st.breaking = None;
        return;
    }
    let reach = if rig.first_person { 7.0 } else { 7.0 + rig.dist };
    st.target = world.v.raycast(rig.eye, rig.forward, reach);
    let Some((hit, prev)) = st.target else {
        st.breaking = None;
        return;
    };

    if mouse.pressed(MouseButton::Left) {
        let b = world.v.get(hit);
        let prog = match st.breaking {
            Some((c, t)) if c == hit => t + time.delta_secs(),
            _ => 0.0,
        };
        if prog >= b.hardness() {
            if b == Block::Tnt {
                prime_tnt(&mut commands, &mats, &fx, hit);
            }
            world.v.set(hit, Block::Air);
            out.write(MashEvent::new(Kind::Block, "", 5));
            st.breaking = None;
        } else {
            st.breaking = Some((hit, prog));
        }
    } else {
        st.breaking = None;
    }

    let place = latch.mouse(MouseButton::Right) || latch.key(KeyCode::KeyT);
    if place {
        let b = if latch.key(KeyCode::KeyT) { Block::Tnt } else { Block::PLACEABLE[st.selected] };
        // Don't entomb the player.
        let pc = p.pos.floor().as_ivec3();
        let blocked = prev == pc || prev == pc + IVec3::Y;
        if !blocked {
            world.v.set(prev, b);
            out.write(MashEvent::new(Kind::Block, "", 2));
            if latch.key(KeyCode::KeyT) {
                world.v.set(prev, Block::Air);
                prime_tnt(&mut commands, &mats, &fx, prev);
            }
        }
    }
}

fn prime_tnt(commands: &mut Commands, mats: &BlockMats, fx: &crate::core::fx::FxAssets, cell: IVec3) {
    commands.spawn((
        PrimedTnt { fuse: 2.5 },
        Mesh3d(fx.cube.clone()),
        MeshMaterial3d(mats.0[&(Block::Tnt as u8)].clone()),
        Transform::from_translation(cell.as_vec3() + Vec3::splat(0.5)),
    ));
}

fn tnt_fuse(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut PrimedTnt, &mut Transform)>, mut boom: MessageWriter<Explosion>) {
    for (e, mut t, mut tf) in q.iter_mut() {
        t.fuse -= time.delta_secs();
        let pulse = 1.0 + 0.12 * ((t.fuse * 18.0).sin()).abs();
        tf.scale = Vec3::splat(pulse);
        if t.fuse <= 0.0 {
            boom.write(Explosion { pos: tf.translation, radius: 3.5, power: 1.0 });
            commands.entity(e).despawn();
        }
    }
}

/// Every explosion (TNT, rockets, airstrikes) carves the voxel layer and
/// chain-reacts other TNT.
fn explosions_carve(mut ev: MessageReader<Explosion>, mut world: ResMut<VoxelWorld>, mut commands: Commands, mats: Res<BlockMats>, fx: Res<crate::core::fx::FxAssets>, modes: Res<ActiveModes>) {
    if !modes.on(Mode::Blocks) {
        ev.clear();
        return;
    }
    for e in ev.read() {
        let r = e.radius.ceil() as i32;
        let c = e.pos.floor().as_ivec3();
        for x in -r..=r {
            for y in -r..=r {
                for z in -r..=r {
                    let p = c + IVec3::new(x, y, z);
                    if (p.as_vec3() + Vec3::splat(0.5)).distance(e.pos) <= e.radius {
                        let b = world.v.get(p);
                        if b == Block::Tnt {
                            prime_tnt(&mut commands, &mats, &fx, p);
                        }
                        if b != Block::Air {
                            world.v.set(p, Block::Air);
                        }
                    }
                }
            }
        }
    }
}

fn hud_and_highlight(
    st: Res<BuildState>,
    bar: Res<Hotbar>,
    modes: Res<ActiveModes>,
    mut hud: ResMut<Hud>,
    mut hl: Query<(&mut Transform, &mut Visibility), With<Highlight>>,
) {
    let Ok((mut tf, mut vis)) = hl.single_mut() else { return };
    let active = modes.on(Mode::Blocks) && bar.item == Item::Blocks;
    match (active, st.target) {
        (true, Some((hit, _))) => {
            *vis = Visibility::Visible;
            let crack = st.breaking.map(|(_, t)| t).unwrap_or(0.0);
            tf.translation = hit.as_vec3() + Vec3::splat(0.5);
            tf.scale = Vec3::splat(1.0 - crack * 0.15);
        }
        _ => *vis = Visibility::Hidden,
    }
    if active {
        hud.crosshair = true;
        hud.top.push(format!("Block: {}  (scroll to change, T = TNT)", Block::PLACEABLE[st.selected].name()));
    }
}
