//! Static geometry builder: every solid piece gets a matching collider, so
//! what you see is exactly what every mode collides with.
//!
//! Visuals are batched: all static geometry of one colour inside one city
//! block becomes a single mesh. That keeps the city to a few hundred draw
//! calls, which matters on WebGL2 where there is no GPU-driven batching.

use crate::core::phys::G_WORLD;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use std::collections::HashMap;

#[derive(Component)]
pub struct Static;

/// Marks surfaces that portals may be placed on.
#[derive(Component)]
pub struct PortalSurface;

/// Lamp posts, poles and similar: preferred swing anchors / tether points.
#[derive(Component)]
pub struct Anchor;

const BATCH_CELL: f32 = 70.0;

#[derive(Default)]
struct Batch {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    tan: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl Batch {
    /// World-space planar UVs (metres) on the face plane, with a matching
    /// tangent frame so normal maps light correctly.
    fn push_face(&mut self, pts: &[Vec3], n: Vec3) {
        let t = if n.y.abs() < 0.9 { Vec3::Y.cross(n).normalize() } else { Vec3::X };
        let b = n.cross(t);
        for p in pts {
            self.pos.push(p.to_array());
            self.nrm.push(n.to_array());
            self.uv.push([p.dot(t), -p.dot(b)]);
            self.tan.push([t.x, t.y, t.z, 1.0]);
        }
    }

    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3) {
        let n = (b - a).cross(c - a).normalize_or_zero();
        let base = self.pos.len() as u32;
        self.push_face(&[a, b, c], n);
        self.idx.extend_from_slice(&[base, base + 1, base + 2]);
    }

    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
        // CCW from outside: a-b-c, a-c-d.
        let n = (b - a).cross(c - a).normalize_or_zero();
        let base = self.pos.len() as u32;
        self.push_face(&[a, b, c, d], n);
        self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn cuboid(&mut self, center: Vec3, half: Vec3, rot: Quat) {
        let p = |x: f32, y: f32, z: f32| center + rot * (Vec3::new(x, y, z) * half);
        // +X, -X, +Y, -Y, +Z, -Z faces, each CCW seen from outside.
        self.quad(p(1., -1., 1.), p(1., -1., -1.), p(1., 1., -1.), p(1., 1., 1.));
        self.quad(p(-1., -1., -1.), p(-1., -1., 1.), p(-1., 1., 1.), p(-1., 1., -1.));
        self.quad(p(-1., 1., 1.), p(1., 1., 1.), p(1., 1., -1.), p(-1., 1., -1.));
        self.quad(p(-1., -1., -1.), p(1., -1., -1.), p(1., -1., 1.), p(-1., -1., 1.));
        self.quad(p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.));
        self.quad(p(1., -1., -1.), p(-1., -1., -1.), p(-1., 1., -1.), p(1., 1., -1.));
    }
}

pub struct Builder<'w, 's, 'a> {
    pub assets: &'a AssetServer,
    pub commands: &'a mut Commands<'w, 's>,
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
    pub rails: Vec<Vec<Vec3>>,
    batches: HashMap<([u8; 3], i32, i32), Batch>,
    /// Night: lamp heads glow and some cast light.
    night: bool,
    glow: Option<(Handle<Mesh>, Handle<StandardMaterial>)>,
    lamps: u32,
}

impl<'w, 's, 'a> Builder<'w, 's, 'a> {
    pub fn new(assets: &'a AssetServer, commands: &'a mut Commands<'w, 's>, meshes: &'a mut Assets<Mesh>, materials: &'a mut Assets<StandardMaterial>) -> Self {
        Self { assets, commands, meshes, materials, rails: vec![], batches: HashMap::new(), night: crate::core::scene::night(), glow: None, lamps: 0 }
    }

    fn batch(&mut self, color: [u8; 3], at: Vec3) -> &mut Batch {
        let cell = ((at.x / BATCH_CELL).round() as i32, (at.z / BATCH_CELL).round() as i32);
        self.batches.entry((color, cell.0, cell.1)).or_default()
    }

    fn collider(&mut self, center: Vec3, rot: Quat, collider: Collider) -> Entity {
        self.commands
            .spawn((Static, PortalSurface, Transform { translation: center, rotation: rot, ..default() }, collider, CollisionGroups::new(G_WORLD, Group::ALL), RigidBody::Fixed))
            .id()
    }

    /// Solid box, `half` extents, optional rotation.
    pub fn solid(&mut self, center: Vec3, half: Vec3, rot: Quat, color: [u8; 3]) -> Entity {
        self.batch(color, center).cuboid(center, half, rot);
        self.collider(center, rot, Collider::cuboid(half.x, half.y, half.z))
    }

    pub fn boxy(&mut self, center: Vec3, half: Vec3, color: [u8; 3]) -> Entity {
        self.solid(center, half, Quat::IDENTITY, color)
    }

    /// Visual-only box (paint, decals, glow heads).
    pub fn deco(&mut self, center: Vec3, half: Vec3, color: [u8; 3]) {
        self.batch(color, center).cuboid(center, half, Quat::IDENTITY);
    }

    /// Wedge ramp: base centred at `base`, rising along its local +Z to `h`.
    pub fn wedge(&mut self, base: Vec3, len: f32, h: f32, width: f32, yaw: f32, color: [u8; 3]) -> Entity {
        let (hw, hl) = (width * 0.5, len * 0.5);
        let local = [
            Vec3::new(-hw, 0.0, -hl),
            Vec3::new(hw, 0.0, -hl),
            Vec3::new(-hw, 0.0, hl),
            Vec3::new(hw, 0.0, hl),
            Vec3::new(-hw, h, hl),
            Vec3::new(hw, h, hl),
        ];
        let rot = Quat::from_rotation_y(yaw);
        let w: Vec<Vec3> = local.iter().map(|p| base + rot * *p).collect();
        let b = self.batch(color, base);
        b.quad(w[0], w[1], w[3], w[2]); // bottom (faces down)
        b.quad(w[0], w[4], w[5], w[1]); // slope
        b.quad(w[2], w[3], w[5], w[4]); // back wall
        b.tri(w[0], w[2], w[4]); // side
        b.tri(w[1], w[5], w[3]); // side
        let collider = Collider::convex_hull(&local).expect("wedge hull");
        self.collider(base, rot, collider)
    }

    /// Curved quarter pipe built from tilted slabs; deck at the top.
    pub fn quarter_pipe(&mut self, base: Vec3, radius: f32, width: f32, yaw: f32, color: [u8; 3]) {
        let rot = Quat::from_rotation_y(yaw);
        let segs = 9;
        let max_ang = 82f32.to_radians();
        for i in 0..segs {
            let a0 = max_ang * i as f32 / segs as f32;
            let a1 = max_ang * (i + 1) as f32 / segs as f32;
            let p = |a: f32| Vec3::new(0.0, radius - radius * a.cos(), radius * a.sin());
            let (p0, p1) = (p(a0), p(a1));
            let mid = (p0 + p1) * 0.5;
            let len = p0.distance(p1);
            let am = (a0 + a1) * 0.5;
            // Slab tangent along the curve, thickness pushed behind the surface.
            let thick = 0.25;
            let normal_in = Vec3::new(0.0, am.cos(), -am.sin());
            let center = mid - normal_in * thick * 0.5;
            let r = rot * Quat::from_rotation_x(-am);
            self.solid(base + rot * center, Vec3::new(width * 0.5, thick * 0.5, len * 0.5 + 0.02), r, color);
        }
        // Back support and deck.
        let top = Vec3::new(0.0, radius - radius * max_ang.cos(), radius * max_ang.sin());
        self.solid(base + rot * Vec3::new(0.0, top.y * 0.5, top.z + 0.8), Vec3::new(width * 0.5, top.y * 0.5, 0.8), rot, [120, 120, 128]);
        // Coping rail along the lip.
        let lip = top + Vec3::new(0.0, 0.05, 0.05);
        self.rail(vec![base + rot * (lip + Vec3::new(-width * 0.5 + 0.1, 0.0, 0.0)), base + rot * (lip + Vec3::new(width * 0.5 - 0.1, 0.0, 0.0))]);
    }

    /// Grindable rail drawn as a slim bar; physics is via the rail set.
    pub fn rail(&mut self, pts: Vec<Vec3>) {
        for s in pts.windows(2) {
            let (a, b) = (s[0], s[1]);
            let rot = Quat::from_rotation_arc(Vec3::Z, (b - a).normalize());
            self.batch([200, 200, 210], a).cuboid((a + b) * 0.5, Vec3::new(0.035, 0.035, a.distance(b) * 0.5), rot);
        }
        self.rails.push(pts);
    }

    /// Rail on posts at `height` from `a` to `b` (ground level points).
    pub fn post_rail(&mut self, a: Vec3, b: Vec3, height: f32) {
        let up = Vec3::Y * height;
        let n = ((a.distance(b) / 3.0).ceil() as usize).max(1);
        for i in 0..=n {
            let p = a.lerp(b, i as f32 / n as f32);
            self.boxy(p + Vec3::Y * height * 0.5, Vec3::new(0.04, height * 0.5, 0.04), [70, 70, 76]);
        }
        self.rail(vec![a + up, b + up]);
    }

    pub fn lamp(&mut self, p: Vec3) {
        let e = self.boxy(p + Vec3::Y * 3.5, Vec3::new(0.09, 3.5, 0.09), [60, 64, 70]);
        self.commands.entity(e).insert(Anchor);
        let head = p + Vec3::new(0.0, 7.05, 0.0);
        if !self.night {
            self.deco(head, Vec3::new(0.25, 0.08, 0.25), [255, 240, 200]);
            return;
        }
        let (mesh, mat) = match &self.glow {
            Some(g) => g.clone(),
            None => {
                let g = (
                    self.meshes.add(Cuboid::new(0.5, 0.16, 0.5)),
                    self.materials.add(StandardMaterial {
                        base_color: Color::srgb(1.0, 0.86, 0.6),
                        // ~4x white on screen so bloom catches it.
                        emissive: LinearRgba::rgb(1.0, 0.8, 0.5) * crate::core::look::scene_units(4.0),
                        ..default()
                    }),
                );
                self.glow = Some(g.clone());
                g
            }
        };
        self.commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::from_translation(head)));
        // Every fourth lamp casts real light: enough pools to read the streets, cheap enough for WebGL2.
        self.lamps += 1;
        if self.lamps % 4 == 0 {
            self.commands.spawn((
                // Bevy's 1e6 lm default suits ev100 9.7; scale to the night exposure.
                PointLight { color: Color::srgb(1.0, 0.8, 0.55), intensity: 400_000.0 * 2f32.powf(crate::core::look::camera_ev100() - 9.7), range: 24.0, shadow_maps_enabled: false, ..default() },
                Transform::from_translation(head - Vec3::Y * 0.3),
            ));
        }
    }

    /// Spawns one mesh per (colour, block) batch.
    pub fn finish(mut self) -> Vec<Vec<Vec3>> {
        let mut mats: HashMap<[u8; 3], Handle<StandardMaterial>> = HashMap::new();
        for ((color, _, _), b) in std::mem::take(&mut self.batches) {
            let mat = mats.entry(color).or_insert_with(|| super::materials::make(self.assets, self.materials, color)).clone();
            let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, b.pos)
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, b.nrm)
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, b.uv)
                .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, b.tan)
                .with_inserted_indices(Indices::U32(b.idx));
            self.commands.spawn((Mesh3d(self.meshes.add(mesh)), MeshMaterial3d(mat), Transform::default()));
        }
        std::mem::take(&mut self.rails)
    }
}
