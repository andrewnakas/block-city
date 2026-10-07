//! Input gathering. Raw presses are latched until a fixed-timestep tick has
//! consumed them, so short taps are never lost between 120 Hz sim steps and
//! variable-rate frames.

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use std::collections::HashSet;

#[derive(Resource, Default)]
pub struct Latch {
    keys: HashSet<KeyCode>,
    mouse: HashSet<MouseButton>,
    pub scroll: f32,
}

impl Latch {
    pub fn key(&self, k: KeyCode) -> bool {
        self.keys.contains(&k)
    }
    pub fn mouse(&self, b: MouseButton) -> bool {
        self.mouse.contains(&b)
    }
}

/// Camera yaw/pitch plus the "UI has the mouse" flag.
#[derive(Resource)]
pub struct Look {
    pub yaw: f32,
    pub pitch: f32,
    pub captured: bool,
    pub sensitivity: f32,
}

impl Default for Look {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: -0.25, captured: false, sensitivity: 0.0022 }
    }
}

impl Look {
    /// Camera-relative movement vector from WASD in world XZ.
    pub fn wish(&self, keys: &ButtonInput<KeyCode>) -> [f32; 2] {
        let mut f = 0.0;
        let mut r = 0.0;
        if keys.pressed(KeyCode::KeyW) {
            f += 1.0;
        }
        if keys.pressed(KeyCode::KeyS) {
            f -= 1.0;
        }
        if keys.pressed(KeyCode::KeyD) {
            r += 1.0;
        }
        if keys.pressed(KeyCode::KeyA) {
            r -= 1.0;
        }
        let (s, c) = self.yaw.sin_cos();
        // Forward = (sin yaw, cos yaw); right = (-cos yaw, sin yaw) in this
        // engine's Y-up, camera-looks-down-+Z convention.
        let fx = s * f - c * r;
        let fz = c * f + s * r;
        let len = (fx * fx + fz * fz).sqrt();
        if len > 1.0 { [fx / len, fz / len] } else { [fx, fz] }
    }
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Latch>()
            .init_resource::<Look>()
            .add_systems(PreUpdate, latch_inputs.after(bevy::input::InputSystems))
            .add_systems(FixedPostUpdate, mark_consumed)
            .add_systems(Update, (mouse_look, cursor_capture));
    }
}

pub fn latch_inputs(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut latch: ResMut<Latch>,
) {
    latch.keys.extend(keys.get_just_pressed().copied());
    latch.mouse.extend(mouse.get_just_pressed().copied());
    latch.scroll += scroll.delta.y;
}

/// Presses fire in exactly one fixed step, even when a frame runs several.
fn mark_consumed(mut latch: ResMut<Latch>) {
    latch.keys.clear();
    latch.mouse.clear();
    latch.scroll = 0.0;
}

fn mouse_look(motion: Res<bevy::input::mouse::AccumulatedMouseMotion>, mut look: ResMut<Look>) {
    if !look.captured {
        return;
    }
    let d = motion.delta;
    look.yaw -= d.x * look.sensitivity;
    look.pitch = (look.pitch - d.y * look.sensitivity).clamp(-1.45, 1.35);
}

fn cursor_capture(
    mut cursor: Single<&mut CursorOptions>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut look: ResMut<Look>,
    panel: Res<crate::core::ui::PanelOpen>,
) {
    if panel.0 || keys.just_pressed(KeyCode::Escape) {
        cursor.visible = true;
        cursor.grab_mode = CursorGrabMode::None;
        look.captured = false;
        return;
    }
    if mouse.just_pressed(MouseButton::Left) && !look.captured {
        cursor.visible = false;
        cursor.grab_mode = CursorGrabMode::Locked;
        look.captured = true;
    }
}
