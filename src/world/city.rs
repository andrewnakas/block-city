//! Seeded procedural city that every mode shares: a road grid, a skate plaza
//! at the centre, a platforming course, a combat compound, a voxel quarry, a
//! mega ramp, and towers to swing between.

use super::build::{Anchor, Builder};
use crate::core::phys::Rails;
use crate::sim::collide::RailSet;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Friction;

pub const BLOCK: f32 = 70.0;
pub const ROAD: f32 = 14.0;
pub const GRID: i32 = 3;
pub const QUARRY: (i32, i32) = (-1, 0);

/// Block-local area half size (inside the roads).
pub const LOT: f32 = (BLOCK - ROAD) * 0.5;

pub fn block_center(i: i32, j: i32) -> Vec3 {
    Vec3::new(i as f32 * BLOCK, 0.0, j as f32 * BLOCK)
}

/// Road centre lines for traffic: x = const lines and z = const lines.
pub fn road_lines() -> Vec<f32> {
    (-GRID..=GRID + 1).map(|k| (k as f32 - 0.5) * BLOCK).collect()
}

const ASPHALT: [u8; 3] = [52, 54, 60];
const SIDEWALK: [u8; 3] = [150, 150, 146];
const CONCRETE: [u8; 3] = [178, 176, 170];
const WOOD: [u8; 3] = [168, 120, 72];
const PAINT_RED: [u8; 3] = [196, 64, 52];
const PAINT_BLUE: [u8; 3] = [52, 108, 196];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % 10_000) as f32 / 10_000.0
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

pub fn build_city(assets: Res<AssetServer>, mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut b = Builder::new(&assets, &mut commands, &mut meshes, &mut materials);
    let ext = (GRID as f32 + 0.5) * BLOCK + ROAD;

    // Ground slab (roads are the ground colour; lots are raised paint only).
    let ground = b.boxy(Vec3::new(0.0, -0.5, 0.0), Vec3::new(ext + 60.0, 0.5, ext + 60.0), ASPHALT);
    b.commands.entity(ground).insert(Friction::coefficient(0.9));
    // Perimeter walls keep cars and players in.
    for (c, h) in [
        (Vec3::new(0.0, 4.0, ext + 2.0), Vec3::new(ext + 4.0, 4.0, 1.0)),
        (Vec3::new(0.0, 4.0, -ext - 2.0), Vec3::new(ext + 4.0, 4.0, 1.0)),
        (Vec3::new(ext + 2.0, 4.0, 0.0), Vec3::new(1.0, 4.0, ext + 4.0)),
        (Vec3::new(-ext - 2.0, 4.0, 0.0), Vec3::new(1.0, 4.0, ext + 4.0)),
    ] {
        b.boxy(c, h, [90, 96, 110]);
    }

    // Lane markings.
    for x in road_lines() {
        for k in 0..((2.0 * ext / 6.0) as i32) {
            let z = -ext + k as f32 * 6.0;
            b.deco(Vec3::new(x, 0.005, z), Vec3::new(0.08, 0.005, 1.4), [230, 210, 120]);
            b.deco(Vec3::new(z, 0.005, x), Vec3::new(1.4, 0.005, 0.08), [230, 210, 120]);
        }
    }

    let mut rng = Rng(0x9E3779B97F4A7C15);
    for i in -GRID..=GRID {
        for j in -GRID..=GRID {
            let c = block_center(i, j);
            // Sidewalk paint around every lot (flush so boards and cars roll over).
            b.deco(c + Vec3::Y * 0.004, Vec3::new(LOT + 2.5, 0.004, LOT + 2.5), SIDEWALK);
            // Street lamps on the corners: swing anchors and tether posts.
            for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                b.lamp(c + Vec3::new(sx * (LOT + 1.8), 0.0, sz * (LOT + 1.8)));
            }
            match (i, j) {
                (0, 0) => plaza(&mut b, c),
                (1, 0) => platform_course(&mut b, c),
                (0, -1) => compound(&mut b, c, &mut rng),
                (0, 1) => mega_ramp(&mut b, c),
                QUARRY => {
                    b.deco(c + Vec3::Y * 0.01, Vec3::new(LOT, 0.01, LOT), [120, 96, 70]);
                }
                _ => downtown(&mut b, c, &mut rng, i, j),
            }
        }
    }

    let rails = b.finish();
    commands.insert_resource(Rails(RailSet { rails }));
}

/// Centre plaza: the skate park, also the spawn.
fn plaza(b: &mut Builder, c: Vec3) {
    b.deco(c + Vec3::Y * 0.008, Vec3::new(LOT, 0.008, LOT), [196, 192, 184]);

    // Funbox with four approach wedges.
    let fb = c + Vec3::new(0.0, 0.0, 4.0);
    b.boxy(fb + Vec3::Y * 0.5, Vec3::new(3.0, 0.5, 2.0), CONCRETE);
    b.wedge(fb + Vec3::new(0.0, 0.0, -3.5), 3.0, 1.0, 4.0, 0.0, CONCRETE);
    b.wedge(fb + Vec3::new(0.0, 0.0, 3.5), 3.0, 1.0, 4.0, std::f32::consts::PI, CONCRETE);
    b.wedge(fb + Vec3::new(-4.5, 0.0, 0.0), 3.0, 1.0, 4.0, std::f32::consts::FRAC_PI_2, CONCRETE);
    b.wedge(fb + Vec3::new(4.5, 0.0, 0.0), 3.0, 1.0, 4.0, -std::f32::consts::FRAC_PI_2, CONCRETE);
    // Rail across the funbox top.
    b.post_rail(fb + Vec3::new(-2.6, 1.0, 0.0), fb + Vec3::new(2.6, 1.0, 0.0), 0.35);

    // Ledges with grindable edges.
    for (k, z) in [(-1.0, -8.0), (1.0, -14.0)] {
        let p = c + Vec3::new(-12.0 * k, 0.0, z);
        b.boxy(p + Vec3::Y * 0.25, Vec3::new(5.0, 0.25, 0.4), [160, 160, 168]);
        b.rail(vec![p + Vec3::new(-4.9, 0.52, -0.4), p + Vec3::new(4.9, 0.52, -0.4)]);
        b.rail(vec![p + Vec3::new(-4.9, 0.52, 0.4), p + Vec3::new(4.9, 0.52, 0.4)]);
    }

    // Flat bars.
    b.post_rail(c + Vec3::new(10.0, 0.0, -20.0), c + Vec3::new(22.0, 0.0, -20.0), 0.4);
    b.post_rail(c + Vec3::new(-22.0, 0.0, 14.0), c + Vec3::new(-10.0, 0.0, 14.0), 0.55);

    // Stair set with handrails: raised deck you roll off.
    let s = c + Vec3::new(16.0, 0.0, 8.0);
    b.boxy(s + Vec3::new(0.0, 0.7, 6.0), Vec3::new(3.0, 0.7, 3.0), CONCRETE);
    b.wedge(s + Vec3::new(0.0, 0.0, 13.0), 4.0, 1.4, 6.0, std::f32::consts::PI, CONCRETE);
    for k in 0..5 {
        let h = 1.4 - 0.28 * (k as f32 + 1.0);
        b.boxy(s + Vec3::new(0.0, h * 0.5, 2.75 - k as f32 * 0.55), Vec3::new(3.0, h * 0.5, 0.28), CONCRETE);
    }
    b.rail(vec![s + Vec3::new(-2.6, 2.25, 3.0), s + Vec3::new(-2.6, 0.85, 0.2)]);
    b.rail(vec![s + Vec3::new(2.6, 2.25, 3.0), s + Vec3::new(2.6, 0.85, 0.2)]);

    // Kickers.
    b.wedge(c + Vec3::new(-6.0, 0.0, -20.0), 2.2, 0.55, 2.0, 0.0, WOOD);
    b.wedge(c + Vec3::new(6.0, 0.0, 16.0), 2.6, 0.8, 2.4, std::f32::consts::PI, WOOD);

    // Quarter pipes facing each other on the west side; bank on the east.
    b.quarter_pipe(c + Vec3::new(-14.0, 0.0, 20.0), 3.2, 8.0, 0.0, PAINT_BLUE);
    b.quarter_pipe(c + Vec3::new(-14.0, 0.0, -2.0), 3.2, 8.0, std::f32::consts::PI, PAINT_BLUE);
    b.wedge(c + Vec3::new(24.0, 0.0, -6.0), 4.0, 2.2, 14.0, -std::f32::consts::FRAC_PI_2, PAINT_RED);

    // Picnic tables / manual pads.
    for x in [-4.0, 4.0] {
        b.boxy(c + Vec3::new(x, 0.18, -26.0 + 0.0), Vec3::new(1.8, 0.18, 0.9), [140, 140, 150]);
    }
}

/// Floating platforms, a wall-kick shaft and a pole to climb toward.
fn platform_course(b: &mut Builder, c: Vec3) {
    b.deco(c + Vec3::Y * 0.008, Vec3::new(LOT, 0.008, LOT), [110, 168, 96]);
    let steps = [
        (Vec3::new(-20.0, 1.2, -20.0), 2.5),
        (Vec3::new(-14.0, 2.6, -18.0), 2.0),
        (Vec3::new(-8.0, 4.2, -14.0), 2.0),
        (Vec3::new(-2.0, 6.0, -10.0), 1.8),
        (Vec3::new(4.0, 7.6, -6.0), 1.6),
        (Vec3::new(10.0, 9.0, -2.0), 2.2),
        (Vec3::new(16.0, 11.5, 4.0), 2.0),
    ];
    for (k, (p, r)) in steps.iter().enumerate() {
        let color = if k % 2 == 0 { [236, 196, 64] } else { [86, 160, 230] };
        b.boxy(c + *p, Vec3::new(*r, 0.3, *r), color);
    }
    // Wall-kick shaft: two tall parallel walls.
    let s = c + Vec3::new(-14.0, 0.0, 14.0);
    b.boxy(s + Vec3::new(-2.6, 9.0, 0.0), Vec3::new(0.5, 9.0, 4.0), [200, 120, 80]);
    b.boxy(s + Vec3::new(2.6, 9.0, 0.0), Vec3::new(0.5, 9.0, 4.0), [200, 120, 80]);
    b.boxy(s + Vec3::new(-5.6, 18.4, 0.0), Vec3::new(3.0, 0.4, 4.0), [236, 196, 64]);
    // Long-jump gap.
    b.boxy(c + Vec3::new(10.0, 1.5, 18.0), Vec3::new(4.0, 1.5, 4.0), [150, 110, 200]);
    b.boxy(c + Vec3::new(10.0, 1.5, 4.0), Vec3::new(4.0, 1.5, 3.0), [150, 110, 200]);
    // Summit tower.
    b.boxy(c + Vec3::new(20.0, 7.0, 18.0), Vec3::new(3.0, 7.0, 3.0), [180, 180, 190]);
    let top = b.boxy(c + Vec3::new(20.0, 15.0, 18.0), Vec3::new(0.15, 1.0, 0.15), [255, 215, 0]);
    b.commands.entity(top).insert(Anchor);
}

/// Combat compound: walls, crates and towers give cover for bot fights.
fn compound(b: &mut Builder, c: Vec3, rng: &mut Rng) {
    b.deco(c + Vec3::Y * 0.008, Vec3::new(LOT, 0.008, LOT), [150, 140, 110]);
    for side in [-1.0, 1.0] {
        b.boxy(c + Vec3::new(side * (LOT - 1.0), 1.5, -8.0), Vec3::new(0.5, 1.5, 14.0), [120, 116, 100]);
    }
    for _ in 0..22 {
        let p = c + Vec3::new(rng.range(-LOT + 3.0, LOT - 3.0), 0.0, rng.range(-LOT + 3.0, LOT - 3.0));
        let s = rng.range(0.6, 1.3);
        b.boxy(p + Vec3::Y * s, Vec3::splat(s), [126, 108, 70]);
    }
    for (x, z) in [(-18.0, -18.0), (18.0, 18.0)] {
        let p = c + Vec3::new(x, 0.0, z);
        for (dx, dz) in [(-1.6, -1.6), (1.6, -1.6), (-1.6, 1.6), (1.6, 1.6)] {
            b.boxy(p + Vec3::new(dx, 3.0, dz), Vec3::new(0.15, 3.0, 0.15), [90, 80, 60]);
        }
        b.boxy(p + Vec3::Y * 6.1, Vec3::new(2.0, 0.15, 2.0), [110, 96, 70]);
        b.wedge(p + Vec3::new(0.0, 0.0, -6.5), 9.0, 6.0, 1.6, 0.0, [110, 96, 70]);
    }
}

/// Mega ramp: tall roll-in, big gap, landing, and a quarter pipe at the end.
fn mega_ramp(b: &mut Builder, c: Vec3) {
    b.deco(c + Vec3::Y * 0.008, Vec3::new(LOT, 0.008, LOT), [190, 186, 176]);
    let x = c.x;
    let z0 = c.z - 26.0;
    // Tower + roll-in (descends toward +z).
    b.boxy(Vec3::new(x, 8.0, z0 + 1.5), Vec3::new(3.0, 8.0, 1.5), [200, 200, 205]);
    b.wedge(Vec3::new(x, 0.0, z0 + 3.0 + 9.0), 18.0, 16.0, 6.0, std::f32::consts::PI, WOOD);
    // Kicker over the gap.
    b.wedge(Vec3::new(x, 0.0, z0 + 25.0), 5.0, 2.4, 6.0, 0.0, WOOD);
    // Landing ramp (descends).
    b.boxy(Vec3::new(x, 2.0, z0 + 36.0), Vec3::new(3.0, 2.0, 1.0), WOOD);
    b.wedge(Vec3::new(x, 0.0, z0 + 42.0), 10.0, 4.0, 6.0, std::f32::consts::PI, WOOD);
    b.quarter_pipe(Vec3::new(x, 0.0, z0 + 50.0), 4.0, 8.0, 0.0, PAINT_RED);
    // Stairs up the tower for people on foot.
    for k in 0..16 {
        let h = 0.5 * (k as f32 + 1.0);
        b.boxy(Vec3::new(x - 4.0, h, z0 - 14.0 + k as f32), Vec3::new(1.0, h, 0.5), [150, 150, 150]);
    }
}

/// Generic city block: towers with rooftops, some ramps up to the roof.
fn downtown(b: &mut Builder, c: Vec3, rng: &mut Rng, i: i32, j: i32) {
    let dist = (i.abs() + j.abs()) as f32;
    let n = 1 + (rng.next() * 3.0) as i32;
    let slots: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)];
    for k in 0..n.min(4) {
        let (sx, sz) = slots[((k + i * 3 + j * 7).rem_euclid(4)) as usize];
        let w = rng.range(7.0, 11.0);
        let d = rng.range(7.0, 11.0);
        let h = rng.range(6.0, 14.0) + dist * rng.range(2.0, 7.0);
        let p = c + Vec3::new(sx * (LOT - w - 2.0), h, sz * (LOT - d - 2.0));
        let tone = 120 + (rng.next() * 80.0) as u8;
        b.boxy(p, Vec3::new(w, h, d), [tone, tone - 10, tone + 12]);
        // Window bands.
        let floors = (h / 2.0) as i32;
        for f in 1..floors.min(30) {
            b.deco(p + Vec3::new(0.0, -h + f as f32 * 4.0, 0.0), Vec3::new(w + 0.03, 0.35, d + 0.03), [70, 90, 120]);
        }
        // Rooftop ledge rail: grind the city skyline.
        let top = p.y + h;
        b.rail(vec![
            Vec3::new(p.x - w + 0.3, top + 0.05, p.z - d + 0.3),
            Vec3::new(p.x + w - 0.3, top + 0.05, p.z - d + 0.3),
        ]);
    }
    // Low building with a ramp to its roof for boards and cars.
    if rng.next() < 0.4 {
        let base = c + Vec3::new(0.0, 0.0, 6.0);
        b.boxy(base + Vec3::Y * 3.0, Vec3::new(6.0, 3.0, 6.0), [160, 140, 120]);
        b.wedge(base + Vec3::new(0.0, 0.0, -13.0), 14.0, 6.0, 5.0, 0.0, [130, 130, 136]);
    }
}
