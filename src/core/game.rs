//! What this game is. Its modes, world and time of day are fixed.

use crate::core::modes::Mode;
use crate::core::scene::World;

pub const TITLE: &str = "Block City";
pub const TAGLINE: &str = "An open-world crime city with a block-sandbox layer: steal cars, dig, build and blow up the streets.";
pub const MODES: &[Mode] = &[Mode::Streets, Mode::Blocks];
pub const WORLD: World = World::City;
pub const NIGHT: bool = false;
/// Shown on the controls screen (Tab), after the movement basics.
pub const HELP: &[&str] = &[
    "E: steal a car.  W/S drive, A/D steer, Space handbrake.",
    "4: blocks.  LMB break, RMB place, scroll block type, T TNT.",
    "TNT carves the city's block layer and wrecks cars. Wall off a street to stop a chase, or dig in while your wanted level climbs.",
];

pub fn mask() -> u8 {
    MODES.iter().fold(0, |m, x| m | x.bit())
}
