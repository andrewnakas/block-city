//! Shared game core: modes, player, input, camera, scoring, UI.
pub mod audio;
pub mod avatar;
pub mod camera;
#[cfg(not(target_arch = "wasm32"))]
pub mod debug;
pub mod fx;
pub mod game;
pub mod hotbar;
pub mod input;
pub mod look;
pub mod modes;
pub mod phys;
pub mod player;
pub mod rig;
pub mod scene;
pub mod score;
pub mod ui;
