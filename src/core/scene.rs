//! Which world the modes play in, and the time of day.
//!
//! `?world=city|realm|arena` (web) or `GAMEMASH_WORLD` (native) picks the map;
//! `?time=night` / `GAMEMASH_TIME=night` turns the lights down. Each world can
//! pin modes it can't work without (the realm is made of blocks, the arena is
//! fought with the platformer moveset).

use crate::core::modes::{ActiveModes, Mode};
use crate::core::phys::Rails;
use crate::core::player::{JumpSim, Loco, PlayerState, SkateSim, WalkSim, enter_loco};
use bevy::prelude::*;

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum World {
    /// The procedural city every mode was designed around.
    City,
    /// Rolling voxel countryside with villages, a keep and raiders that fight back.
    Realm,
    /// A ruined colosseum with one boss.
    Arena,
}

impl World {
    /// Modes this world can't run without; they can't be switched off here.
    pub fn pinned(self) -> u8 {
        match self {
            World::City => 0,
            World::Realm => Mode::Blocks.bit(),
            World::Arena => Mode::Jump.bit(),
        }
    }
}

pub fn startup_world() -> World {
    crate::core::game::WORLD
}

/// Night lighting, read once at startup by the sky, sun, fog and lamps.
pub fn night() -> bool {
    crate::core::game::NIGHT
}

pub fn is(w: World) -> impl Fn(Res<World>) -> bool + Clone {
    move |cur: Res<World>| *cur == w
}

/// Where the player starts in each world (the realm sets its own once the terrain exists).
/// Placing the player also enters the locomotion that matches the starting modes.
pub fn spawn_point(w: World) -> Option<Vec3> {
    match w {
        World::City => Some(crate::core::player::SPAWN),
        World::Realm => None,
        World::Arena => Some(Vec3::new(0.0, 1.0, -22.0)),
    }
}

/// Moves the player to `pos` and restarts the active locomotion there.
pub fn place_player(pos: Vec3, facing: f32, p: &mut PlayerState, modes: &ActiveModes, skate: &mut SkateSim, jump: &mut JumpSim, walk: &mut WalkSim) {
    p.pos = pos;
    p.respawn = pos;
    p.vel = Vec3::ZERO;
    p.facing = facing;
    let loco = if modes.on(Mode::Skate) { Loco::Skate } else { Loco::Foot };
    enter_loco(loco, p, modes, skate, jump, walk);
}

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        let world = startup_world();
        app.insert_resource(world);
        if world != World::City {
            // The city owns the static grind rails; other worlds start with none.
            app.init_resource::<Rails>();
        }
        if night() {
            app.insert_resource(ClearColor(Color::srgb(0.03, 0.04, 0.08)));
        }
        app.add_systems(Startup, log_world).add_systems(Startup, place_at_spawn.after(crate::core::player::spawn_player));
    }
}

fn log_world(w: Res<World>) {
    info!("world: {:?}{}", *w, if night() { " (night)" } else { "" });
}

fn place_at_spawn(w: Res<World>, mut p: ResMut<PlayerState>, modes: Res<ActiveModes>, mut skate: ResMut<SkateSim>, mut jump: ResMut<JumpSim>, mut walk: ResMut<WalkSim>) {
    if let Some(pos) = spawn_point(*w) {
        place_player(pos, 0.0, &mut p, &modes, &mut skate, &mut jump, &mut walk);
    }
}
