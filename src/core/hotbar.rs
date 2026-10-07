//! One hotbar decides what the mouse does, so shooting, mining, portals and
//! grappling never fight over the same click.

use crate::core::modes::{ActiveModes, Mode};
use crate::core::ui::Hud;
use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    Rifle,
    Pistol,
    Launcher,
    Blocks,
    PortalGun,
    Fists,
}

impl Item {
    pub fn slot_key(self) -> KeyCode {
        match self {
            Item::Rifle => KeyCode::Digit1,
            Item::Pistol => KeyCode::Digit2,
            Item::Launcher => KeyCode::Digit3,
            Item::Blocks => KeyCode::Digit4,
            Item::PortalGun => KeyCode::Digit8,
            Item::Fists => KeyCode::Digit9,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Item::Rifle => "1 Rifle",
            Item::Pistol => "2 Pistol",
            Item::Launcher => "3 Launcher",
            Item::Blocks => "4 Blocks",
            Item::PortalGun => "8 Portal Gun",
            Item::Fists => "9 Hands",
        }
    }
    fn mode(self) -> Option<Mode> {
        match self {
            Item::Rifle | Item::Pistol | Item::Launcher => Some(Mode::Warfare),
            Item::Blocks => Some(Mode::Blocks),
            Item::PortalGun => Some(Mode::Portals),
            Item::Fists => None,
        }
    }
}

const ORDER: [Item; 6] = [Item::Rifle, Item::Pistol, Item::Launcher, Item::Blocks, Item::PortalGun, Item::Fists];

#[derive(Resource)]
pub struct Hotbar {
    pub item: Item,
}

impl Default for Hotbar {
    fn default() -> Self {
        Self { item: Item::Rifle }
    }
}

pub fn available(modes: &ActiveModes) -> Vec<Item> {
    ORDER.iter().copied().filter(|i| i.mode().is_none_or(|m| modes.on(m))).collect()
}

pub struct HotbarPlugin;

impl Plugin for HotbarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hotbar>().add_systems(Update, select);
    }
}

fn select(keys: Res<ButtonInput<KeyCode>>, modes: Res<ActiveModes>, mut bar: ResMut<Hotbar>, mut hud: ResMut<Hud>) {
    let items = available(&modes);
    for it in &items {
        if keys.just_pressed(it.slot_key()) {
            bar.item = *it;
        }
    }
    if !items.contains(&bar.item) {
        bar.item = items[0];
    }
    if items.len() > 1 {
        hud.hotbar = items.iter().map(|i| (i.label().to_string(), *i == bar.item)).collect();
    }
}
