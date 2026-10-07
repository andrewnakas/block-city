//! Minimal voxel engine: sparse 16^3 chunks, DDA ray picking, greedy face
//! meshing and greedy box merging for colliders. Pure data, unit-tested.

use bevy::math::{IVec3, Vec3};
use std::collections::{HashMap, HashSet};

pub const CS: i32 = 16;
pub const CS3: usize = (CS * CS * CS) as usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum Block {
    Air = 0,
    Grass,
    Dirt,
    Stone,
    Planks,
    Log,
    Leaves,
    Sand,
    Brick,
    Rail,
    Tnt,
    Gold,
}

impl Block {
    pub const PLACEABLE: [Block; 10] =
        [Block::Grass, Block::Dirt, Block::Stone, Block::Planks, Block::Log, Block::Leaves, Block::Sand, Block::Brick, Block::Rail, Block::Tnt];

    pub fn from_u8(v: u8) -> Block {
        match v {
            1 => Block::Grass,
            2 => Block::Dirt,
            3 => Block::Stone,
            4 => Block::Planks,
            5 => Block::Log,
            6 => Block::Leaves,
            7 => Block::Sand,
            8 => Block::Brick,
            9 => Block::Rail,
            10 => Block::Tnt,
            11 => Block::Gold,
            _ => Block::Air,
        }
    }
    pub fn solid(self) -> bool {
        self != Block::Air
    }
    pub fn name(self) -> &'static str {
        match self {
            Block::Air => "Air",
            Block::Grass => "Grass",
            Block::Dirt => "Dirt",
            Block::Stone => "Stone",
            Block::Planks => "Planks",
            Block::Log => "Log",
            Block::Leaves => "Leaves",
            Block::Sand => "Sand",
            Block::Brick => "Brick",
            Block::Rail => "Grind Rail",
            Block::Tnt => "TNT",
            Block::Gold => "Gold",
        }
    }
    /// Seconds to break with the pickaxe.
    pub fn hardness(self) -> f32 {
        match self {
            Block::Leaves => 0.1,
            Block::Sand | Block::Dirt | Block::Grass => 0.3,
            Block::Planks | Block::Log | Block::Tnt | Block::Rail => 0.45,
            Block::Stone | Block::Brick => 0.7,
            Block::Gold => 0.9,
            Block::Air => 0.0,
        }
    }
}

#[derive(Clone)]
pub struct Chunk {
    pub blocks: Box<[u8; CS3]>,
    pub solid_count: u32,
}

impl Default for Chunk {
    fn default() -> Self {
        Self { blocks: Box::new([0; CS3]), solid_count: 0 }
    }
}

fn idx(l: IVec3) -> usize {
    (l.x + l.z * CS + l.y * CS * CS) as usize
}

pub fn chunk_of(p: IVec3) -> IVec3 {
    IVec3::new(p.x.div_euclid(CS), p.y.div_euclid(CS), p.z.div_euclid(CS))
}

pub fn local_of(p: IVec3) -> IVec3 {
    IVec3::new(p.x.rem_euclid(CS), p.y.rem_euclid(CS), p.z.rem_euclid(CS))
}

#[derive(Default)]
pub struct Voxels {
    pub chunks: HashMap<IVec3, Chunk>,
    pub dirty: HashSet<IVec3>,
}

impl Voxels {
    pub fn get(&self, p: IVec3) -> Block {
        self.chunks.get(&chunk_of(p)).map(|c| Block::from_u8(c.blocks[idx(local_of(p))])).unwrap_or(Block::Air)
    }

    pub fn set(&mut self, p: IVec3, b: Block) {
        let ck = chunk_of(p);
        let l = local_of(p);
        let c = self.chunks.entry(ck).or_default();
        let old = c.blocks[idx(l)];
        if old == b as u8 {
            return;
        }
        if old == 0 {
            c.solid_count += 1;
        }
        if b == Block::Air {
            c.solid_count -= 1;
        }
        c.blocks[idx(l)] = b as u8;
        self.dirty.insert(ck);
        // Faces on chunk borders change the neighbour's mesh too.
        for (axis, v) in [(0, l.x), (1, l.y), (2, l.z)] {
            let mut d = IVec3::ZERO;
            if v == 0 {
                d[axis] = -1;
                self.dirty.insert(ck + d);
            } else if v == CS - 1 {
                d[axis] = 1;
                self.dirty.insert(ck + d);
            }
        }
    }

    /// Amanatides-Woo DDA. Returns (hit cell, previous empty cell).
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<(IVec3, IVec3)> {
        let d = dir.normalize_or_zero();
        if d == Vec3::ZERO {
            return None;
        }
        let mut cell = origin.floor().as_ivec3();
        let step = IVec3::new(d.x.signum() as i32, d.y.signum() as i32, d.z.signum() as i32);
        let next_boundary = |o: f32, c: i32, s: i32| if s > 0 { (c + 1) as f32 - o } else { o - c as f32 };
        let inv = |v: f32| if v.abs() < 1e-9 { f32::INFINITY } else { 1.0 / v.abs() };
        let mut t_max = Vec3::new(
            next_boundary(origin.x, cell.x, step.x) * inv(d.x),
            next_boundary(origin.y, cell.y, step.y) * inv(d.y),
            next_boundary(origin.z, cell.z, step.z) * inv(d.z),
        );
        let t_delta = Vec3::new(inv(d.x), inv(d.y), inv(d.z));
        let mut prev = cell;
        let mut t = 0.0;
        while t <= max {
            if self.get(cell).solid() {
                return Some((cell, prev));
            }
            prev = cell;
            if t_max.x < t_max.y && t_max.x < t_max.z {
                cell.x += step.x;
                t = t_max.x;
                t_max.x += t_delta.x;
            } else if t_max.y < t_max.z {
                cell.y += step.y;
                t = t_max.y;
                t_max.y += t_delta.y;
            } else {
                cell.z += step.z;
                t = t_max.z;
                t_max.z += t_delta.z;
            }
        }
        None
    }

    /// Greedy-meshed faces of one chunk, grouped by block type.
    /// Each quad: (block, 4 corners in world space, normal).
    pub fn mesh_chunk(&self, ck: IVec3) -> Vec<Quad> {
        let mut quads = vec![];
        let Some(chunk) = self.chunks.get(&ck) else { return quads };
        if chunk.solid_count == 0 {
            return quads;
        }
        let base = ck * CS;
        for axis in 0..3 {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            for dir in [-1i32, 1] {
                for slice in 0..CS {
                    // Mask of exposed faces in this slice.
                    let mut mask = [0u8; (CS * CS) as usize];
                    for a in 0..CS {
                        for b in 0..CS {
                            let mut l = IVec3::ZERO;
                            l[axis] = slice;
                            l[u] = a;
                            l[v] = b;
                            let here = chunk.blocks[idx(l)];
                            if here == 0 {
                                continue;
                            }
                            let mut n = base + l;
                            n[axis] += dir;
                            if !self.get(n).solid() || (self.get(n) == Block::Leaves && here != Block::Leaves as u8) {
                                mask[(a + b * CS) as usize] = here;
                            }
                        }
                    }
                    // Greedy merge rectangles of equal type.
                    for b in 0..CS {
                        let mut a = 0;
                        while a < CS {
                            let t = mask[(a + b * CS) as usize];
                            if t == 0 {
                                a += 1;
                                continue;
                            }
                            let mut w = 1;
                            while a + w < CS && mask[(a + w + b * CS) as usize] == t {
                                w += 1;
                            }
                            let mut h = 1;
                            'grow: while b + h < CS {
                                for k in 0..w {
                                    if mask[(a + k + (b + h) * CS) as usize] != t {
                                        break 'grow;
                                    }
                                }
                                h += 1;
                            }
                            for bb in 0..h {
                                for aa in 0..w {
                                    mask[(a + aa + (b + bb) * CS) as usize] = 0;
                                }
                            }
                            let mut o = Vec3::ZERO;
                            o[axis] = (base[axis] + slice + if dir > 0 { 1 } else { 0 }) as f32;
                            o[u] = (base[u] + a) as f32;
                            o[v] = (base[v] + b) as f32;
                            let mut du = Vec3::ZERO;
                            du[u] = w as f32;
                            let mut dv = Vec3::ZERO;
                            dv[v] = h as f32;
                            let mut normal = Vec3::ZERO;
                            normal[axis] = dir as f32;
                            quads.push(Quad { block: Block::from_u8(t), corners: [o, o + du, o + du + dv, o + dv], normal });
                            a += w;
                        }
                    }
                }
            }
        }
        quads
    }

    /// Greedy 3D box merge of solid cells: (min, max) world boxes for colliders.
    pub fn boxes_chunk(&self, ck: IVec3) -> Vec<(Vec3, Vec3)> {
        let mut out = vec![];
        let Some(chunk) = self.chunks.get(&ck) else { return out };
        let mut used = vec![false; CS3];
        let solid = |l: IVec3, used: &Vec<bool>| chunk.blocks[idx(l)] != 0 && chunk.blocks[idx(l)] != Block::Leaves as u8 && !used[idx(l)];
        for y in 0..CS {
            for z in 0..CS {
                for x in 0..CS {
                    let s = IVec3::new(x, y, z);
                    if !solid(s, &used) {
                        continue;
                    }
                    let mut w = 1;
                    while x + w < CS && solid(IVec3::new(x + w, y, z), &used) {
                        w += 1;
                    }
                    let mut d = 1;
                    'dz: while z + d < CS {
                        for k in 0..w {
                            if !solid(IVec3::new(x + k, y, z + d), &used) {
                                break 'dz;
                            }
                        }
                        d += 1;
                    }
                    let mut h = 1;
                    'hy: while y + h < CS {
                        for kz in 0..d {
                            for kx in 0..w {
                                if !solid(IVec3::new(x + kx, y + h, z + kz), &used) {
                                    break 'hy;
                                }
                            }
                        }
                        h += 1;
                    }
                    for ky in 0..h {
                        for kz in 0..d {
                            for kx in 0..w {
                                used[idx(IVec3::new(x + kx, y + ky, z + kz))] = true;
                            }
                        }
                    }
                    let mn = (ck * CS + s).as_vec3();
                    out.push((mn, mn + Vec3::new(w as f32, h as f32, d as f32)));
                }
            }
        }
        out
    }

    /// Straight runs of rail blocks become grind rails along their top edge.
    pub fn rail_runs(&self) -> Vec<Vec<Vec3>> {
        let mut cells: Vec<IVec3> = vec![];
        for (ck, c) in &self.chunks {
            for (i, b) in c.blocks.iter().enumerate() {
                if *b == Block::Rail as u8 {
                    let i = i as i32;
                    cells.push(*ck * CS + IVec3::new(i % CS, i / (CS * CS), (i / CS) % CS));
                }
            }
        }
        let set: HashSet<IVec3> = cells.iter().copied().collect();
        let mut taken: HashSet<IVec3> = HashSet::new();
        let mut runs = vec![];
        cells.sort_by_key(|c| (c.y, c.z, c.x));
        for &c in &cells {
            if taken.contains(&c) {
                continue;
            }
            // Prefer a run along X, then Z; a lone block is a 1 m rail along X.
            let mut best = (0usize, c, 1);
            for axis in [0usize, 2] {
                let mut step = IVec3::ZERO;
                step[axis] = 1;
                let mut start = c;
                while set.contains(&(start - step)) && !taken.contains(&(start - step)) {
                    start -= step;
                }
                let mut len = 1;
                while set.contains(&(start + step * len)) && !taken.contains(&(start + step * len)) {
                    len += 1;
                }
                if len > best.2 {
                    best = (axis, start, len);
                }
            }
            let (axis, start, len) = best;
            let mut step = IVec3::ZERO;
            step[axis] = 1;
            for k in 0..len {
                taken.insert(start + step * k);
            }
            let a = top_start(start, axis);
            let mut b = a;
            b[axis] += len as f32;
            runs.push(vec![a, b]);
        }
        runs
    }
}

fn top_start(c: IVec3, axis: usize) -> Vec3 {
    let mut p = c.as_vec3() + Vec3::new(0.5, 1.02, 0.5);
    p[axis] -= 0.5;
    p
}

#[derive(Clone, Debug)]
pub struct Quad {
    pub block: Block,
    pub corners: [Vec3; 4],
    pub normal: Vec3,
}

/// Tiny deterministic value noise for terrain.
pub fn noise2(x: f32, z: f32, seed: u32) -> f32 {
    let h = |ix: i32, iz: i32| {
        let mut n = (ix as u32).wrapping_mul(374761393) ^ (iz as u32).wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
        n = (n ^ (n >> 13)).wrapping_mul(1274126177);
        (n ^ (n >> 16)) as f32 / u32::MAX as f32
    };
    let (ix, iz) = (x.floor() as i32, z.floor() as i32);
    let (fx, fz) = (x - ix as f32, z - iz as f32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (a, b, c, d) = (h(ix, iz), h(ix + 1, iz), h(ix, iz + 1), h(ix + 1, iz + 1));
    let top = a + (b - a) * s(fx);
    let bot = c + (d - c) * s(fx);
    top + (bot - top) * s(fz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_negative_coords() {
        let mut v = Voxels::default();
        v.set(IVec3::new(-1, 0, -17), Block::Stone);
        assert_eq!(v.get(IVec3::new(-1, 0, -17)), Block::Stone);
        assert_eq!(v.get(IVec3::new(-1, 1, -17)), Block::Air);
    }

    #[test]
    fn dda_hits_block_and_reports_face_cell() {
        let mut v = Voxels::default();
        v.set(IVec3::new(5, 0, 0), Block::Dirt);
        let (hit, prev) = v.raycast(Vec3::new(0.5, 0.5, 0.5), Vec3::X, 10.0).unwrap();
        assert_eq!(hit, IVec3::new(5, 0, 0));
        assert_eq!(prev, IVec3::new(4, 0, 0));
        assert!(v.raycast(Vec3::new(0.5, 0.5, 0.5), Vec3::X, 3.0).is_none());
    }

    #[test]
    fn greedy_mesh_flat_slab_is_six_quads() {
        let mut v = Voxels::default();
        for x in 0..4 {
            for z in 0..4 {
                v.set(IVec3::new(x, 0, z), Block::Stone);
            }
        }
        let q = v.mesh_chunk(IVec3::ZERO);
        assert_eq!(q.len(), 6, "{q:?}");
        let b = v.boxes_chunk(IVec3::ZERO);
        assert_eq!(b, vec![(Vec3::ZERO, Vec3::new(4.0, 1.0, 4.0))]);
    }

    #[test]
    fn hidden_faces_between_chunks_are_culled() {
        let mut v = Voxels::default();
        v.set(IVec3::new(15, 0, 0), Block::Stone);
        v.set(IVec3::new(16, 0, 0), Block::Stone);
        let a = v.mesh_chunk(IVec3::ZERO);
        assert_eq!(a.len(), 5);
        assert!(v.dirty.contains(&IVec3::new(1, 0, 0)));
    }

    #[test]
    fn rail_blocks_form_one_rail() {
        let mut v = Voxels::default();
        for x in 0..5 {
            v.set(IVec3::new(x, 0, 0), Block::Rail);
        }
        let r = v.rail_runs();
        assert_eq!(r.len(), 1, "{r:?}");
        assert!((r[0][1].x - r[0][0].x - 5.0).abs() < 1e-5);
    }
}
