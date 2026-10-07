//! Backs the sims' `CollisionWorld` with Rapier scene queries.

use crate::sim::collide::{CollisionWorld, Hit, RailSample, RailSet};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

#[derive(Resource, Default)]
pub struct Rails(pub RailSet);

/// A hole cut through world geometry (a portal). Movement queries ignore
/// solids whose contact lies inside the hole, so you can pass through walls.
#[derive(Clone, Copy, Debug)]
pub struct Hole {
    pub center: Vec3,
    pub normal: Vec3,
    pub up: Vec3,
    pub half: Vec2,
}

impl Hole {
    pub fn contains(&self, p: Vec3) -> bool {
        let d = p - self.center;
        let depth = d.dot(self.normal);
        let right = self.up.cross(self.normal);
        (-3.0..=0.25).contains(&depth) && d.dot(right).abs() <= self.half.x && d.dot(self.up).abs() <= self.half.y
    }
}

#[derive(Resource, Default)]
pub struct Holes(pub Vec<Hole>);

/// Collision groups: the player's own body and trigger volumes are invisible
/// to movement queries.
pub const G_WORLD: Group = Group::GROUP_1;
pub const G_PLAYER: Group = Group::GROUP_2;
pub const G_DYNAMIC: Group = Group::GROUP_3;
pub const G_NPC: Group = Group::GROUP_4;

pub struct PhysWorld<'a, 'b> {
    pub ctx: &'a RapierContext<'b>,
    pub rails: &'a RailSet,
    pub holes: &'a [Hole],
    pub exclude: Option<Entity>,
}

impl PhysWorld<'_, '_> {
    fn filter(&self) -> QueryFilter<'_> {
        let mut f = QueryFilter::new().exclude_sensors().groups(CollisionGroups::new(Group::ALL, G_WORLD | G_DYNAMIC));
        if let Some(e) = self.exclude {
            f = f.exclude_rigid_body(e);
        }
        f
    }
}

impl CollisionWorld for PhysWorld<'_, '_> {
    fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<Hit> {
        let mut o = origin;
        let mut travelled = 0.0;
        for _ in 0..4 {
            let (_, i) = self.ctx.cast_ray_and_get_normal(o, dir, max - travelled, true, self.filter())?;
            if self.holes.iter().any(|h| h.contains(i.point)) {
                // Through the portal hole: keep going past this surface.
                travelled += i.time_of_impact + 0.02;
                o = i.point + dir * 0.02;
                if travelled >= max {
                    return None;
                }
                continue;
            }
            return Some(Hit { t: travelled + i.time_of_impact, point: i.point, normal: i.normal });
        }
        None
    }

    fn resolve_sphere(&self, center: Vec3, radius: f32) -> (Vec3, Option<Vec3>) {
        let mut p = center;
        let mut normal = None;
        for _ in 0..4 {
            let Some((_, proj)) = self.ctx.project_point(p, radius, false, self.filter()) else { break };
            if self.holes.iter().any(|h| h.contains(proj.point)) {
                break;
            }
            let delta = p - proj.point;
            let d = delta.length();
            let n = if proj.is_inside {
                // Centre is inside a solid: push out through the nearest face.
                if d < 1e-6 { Vec3::Y } else { -delta / d }
            } else {
                if d >= radius || d < 1e-6 {
                    break;
                }
                delta / d
            };
            let push = if proj.is_inside { d + radius } else { radius - d };
            p += n * (push + 1e-4);
            normal = Some(n);
        }
        (p, normal)
    }

    fn nearest_rail(&self, p: Vec3, max: f32) -> Option<RailSample> {
        self.rails.nearest(p, max)
    }
}
