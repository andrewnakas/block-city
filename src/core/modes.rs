//! The eight mashable game modes and which are switched on.

use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mode {
    Skate,
    Jump,
    Warfare,
    Streets,
    Blocks,
    Portals,
    Swing,
    BulletTime,
}

impl Mode {
    pub const ALL: [Mode; 8] = [
        Mode::Skate,
        Mode::Jump,
        Mode::Warfare,
        Mode::Streets,
        Mode::Blocks,
        Mode::Portals,
        Mode::Swing,
        Mode::BulletTime,
    ];

    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Skate => "SKATE",
            Mode::Jump => "JUMP",
            Mode::Warfare => "WARFARE",
            Mode::Streets => "STREETS",
            Mode::Blocks => "BLOCKS",
            Mode::Portals => "PORTALS",
            Mode::Swing => "SWING",
            Mode::BulletTime => "BULLET-TIME",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Mode::Skate => "Push, pop, flip, grind. Land it clean.",
            Mode::Jump => "Triple jumps, long jumps, wall kicks, ground pounds.",
            Mode::Warfare => "Aim down sights, bots, killstreaks.",
            Mode::Streets => "Steal cars, cause chaos, lose the cops.",
            Mode::Blocks => "Break and place blocks anywhere.",
            Mode::Portals => "Two linked portals. Speed in, speed out.",
            Mode::Swing => "Web-swing off anything. Tether things together.",
            Mode::BulletTime => "Time moves only when you move.",
        }
    }

    pub fn controls(self) -> &'static str {
        match self {
            Mode::Skate => "W push  S brake  A/D carve/spin  Space hold+release ollie (Shift: nollie)\nJ kickflip  K heelflip  L shove-it  U 360 flip  H hardflip  Y grab  I/O manual  Shift+A/D powerslide  G hop on/off",
            Mode::Jump => "Space jump (chain on landing for double/triple)  Ctrl crouch / ground pound\nCtrl+Space long jump (running) or backflip (standing)  reverse+Space side flip  F punch/dive\nSpace off a wall: wall kick",
            Mode::Warfare => "LMB fire  RMB aim  R reload  Shift sprint  C crouch  1-3 weapons  5 UAV  6 care package  7 airstrike",
            Mode::Streets => "E enter/steal car  W/S drive  A/D steer  Space handbrake",
            Mode::Blocks => "4 pickaxe/blocks  LMB break  RMB place  scroll block type  T TNT",
            Mode::Portals => "8 portal gun  LMB blue  RMB orange",
            Mode::Swing => "Q hold to web-swing  release to fling  Z tether two objects",
            Mode::BulletTime => "Time scales with how much you move",
        }
    }

    pub fn color(self) -> [u8; 3] {
        match self {
            Mode::Skate => [255, 184, 28],
            Mode::Jump => [230, 57, 70],
            Mode::Warfare => [110, 140, 90],
            Mode::Streets => [240, 120, 200],
            Mode::Blocks => [94, 170, 80],
            Mode::Portals => [64, 160, 255],
            Mode::Swing => [200, 40, 60],
            Mode::BulletTime => [235, 235, 245],
        }
    }

    pub fn hotkey(self) -> KeyCode {
        [KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4, KeyCode::F5, KeyCode::F6, KeyCode::F7, KeyCode::F8][self as usize]
    }
}

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActiveModes(pub u8);

impl Default for ActiveModes {
    fn default() -> Self {
        ActiveModes(Mode::Skate.bit() | Mode::Warfare.bit())
    }
}

impl ActiveModes {
    pub fn on(&self, m: Mode) -> bool {
        self.0 & m.bit() != 0
    }
    pub fn set(&mut self, m: Mode, on: bool) {
        if on {
            self.0 |= m.bit();
        } else {
            self.0 &= !m.bit();
        }
    }
    pub fn count(&self) -> u32 {
        self.0.count_ones()
    }
}

/// Run condition: the mode is enabled.
pub fn mode_on(m: Mode) -> impl Fn(Res<ActiveModes>) -> bool + Clone {
    move |modes: Res<ActiveModes>| modes.on(m)
}

/// Parses a comma list like "skate,warfare" into a mode mask. Unknown names are ignored;
/// returns None when nothing matched.
pub fn parse_modes(list: &str) -> Option<u8> {
    let mut mask = 0;
    for name in list.split(',').map(str::trim) {
        for m in Mode::ALL {
            if m.name().eq_ignore_ascii_case(name) || (m == Mode::BulletTime && name.eq_ignore_ascii_case("bullettime")) {
                mask |= m.bit();
            }
        }
    }
    (mask != 0).then_some(mask)
}

/// A query parameter from the page URL (web only).
#[cfg(target_arch = "wasm32")]
pub fn url_param(key: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search).ok()?.get(key)
}

/// Modes requested at launch: `?modes=` on the web, `GAMEMASH_MODES` natively.
pub fn startup_modes() -> Option<u8> {
    #[cfg(target_arch = "wasm32")]
    let list = url_param("modes");
    #[cfg(not(target_arch = "wasm32"))]
    let list = std::env::var("GAMEMASH_MODES").ok();
    parse_modes(&list?)
}

/// Whether the mode panel opens at launch. Embeds pass `?panel=0` to drop straight into play.
pub fn startup_panel_open() -> bool {
    #[cfg(target_arch = "wasm32")]
    return url_param("panel").as_deref() != Some("0");
    #[cfg(not(target_arch = "wasm32"))]
    true
}

pub struct Preset {
    pub name: &'static str,
    pub modes: &'static [Mode],
}

pub const PRESETS: &[Preset] = &[
    Preset { name: "Kickflip Warfare", modes: &[Mode::Skate, Mode::Warfare] },
    Preset { name: "Block Ops", modes: &[Mode::Warfare, Mode::Blocks, Mode::Skate] },
    Preset { name: "Plumber Portals", modes: &[Mode::Jump, Mode::Portals] },
    Preset { name: "Getaway", modes: &[Mode::Streets, Mode::Swing, Mode::BulletTime] },
    Preset { name: "Web Skater", modes: &[Mode::Skate, Mode::Swing, Mode::Portals] },
    Preset { name: "Slow-Mo Shootout", modes: &[Mode::Warfare, Mode::BulletTime, Mode::Jump] },
    Preset { name: "EVERYTHING", modes: &Mode::ALL },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mode_lists() {
        assert_eq!(parse_modes("skate,warfare"), Some(Mode::Skate.bit() | Mode::Warfare.bit()));
        assert_eq!(parse_modes(" SKATE , bullettime"), Some(Mode::Skate.bit() | Mode::BulletTime.bit()));
        assert_eq!(parse_modes("bullet-time"), Some(Mode::BulletTime.bit()));
        assert_eq!(parse_modes("nope"), None);
        assert_eq!(parse_modes(""), None);
    }
}

#[derive(Message, Clone, Copy, Debug)]
pub struct ModeToggled {
    pub mode: Mode,
    pub on: bool,
}

pub struct ModesPlugin;

impl Plugin for ModesPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ActiveModes(crate::core::game::mask()))
            .add_message::<ModeToggled>()
            .add_systems(Startup, log_start_modes)
            .add_systems(Update, (lock_modes, announce_changes).chain());
    }
}

/// One line at launch so embeds and CI can confirm which modes a URL preset turned on.
fn log_start_modes(modes: Res<ActiveModes>) {
    let names: Vec<_> = Mode::ALL.iter().filter(|m| modes.on(**m)).map(|m| m.name()).collect();
    info!("start modes: {}", names.join(", "));
}

/// This game's modes are fixed.
fn lock_modes(mut modes: ResMut<ActiveModes>) {
    let want = crate::core::game::mask();
    if modes.0 != want {
        modes.0 = want;
    }
}

fn announce_changes(modes: Res<ActiveModes>, mut prev: Local<Option<u8>>, mut out: MessageWriter<ModeToggled>) {
    let before = prev.unwrap_or(0);
    if before != modes.0 || prev.is_none() {
        for m in Mode::ALL {
            let (a, b) = (before & m.bit() != 0, modes.on(m));
            if a != b {
                out.write(ModeToggled { mode: m, on: b });
            }
        }
        *prev = Some(modes.0);
    }
}
