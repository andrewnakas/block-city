//! BULLET-TIME mode: the world's clock follows you. Stand still and
//! everything (bullets, traffic, bots, your own fall) crawls; move and time
//! returns to full speed.

use crate::core::input::Look;
use crate::core::modes::{ActiveModes, Mode, ModeToggled};
use crate::core::player::{Loco, PlayerState};
use crate::core::ui::Hud;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

pub const MIN_SCALE: f32 = 0.05;

#[derive(Resource)]
pub struct TimeFlow {
    pub scale: f32,
    spike: f32,
}

impl Default for TimeFlow {
    fn default() -> Self {
        Self { scale: 1.0, spike: 0.0 }
    }
}

/// How much the player is "acting" this frame, 0..1.
pub fn activity(moving: bool, mouse: f32, action: bool, loco: Loco) -> f32 {
    let mut a: f32 = if moving { 1.0 } else { 0.0 };
    a = a.max((mouse / 40.0).min(0.35));
    if action {
        a = a.max(0.7);
    }
    if loco == Loco::Swing {
        a = a.max(0.25);
    }
    a
}

pub struct BulletTimePlugin;

impl Plugin for BulletTimePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimeFlow>()
            .add_systems(Update, (flow, restore))
            .add_systems(EguiPrimaryContextPass, overlay);
    }
}

#[allow(clippy::too_many_arguments)]
fn flow(
    real: Res<Time<bevy::time::Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    look: Res<Look>,
    modes: Res<ActiveModes>,
    p: Res<PlayerState>,
    mut tf: ResMut<TimeFlow>,
    mut vtime: ResMut<Time<Virtual>>,
    mut hud: ResMut<Hud>,
) {
    if !modes.on(Mode::BulletTime) {
        return;
    }
    let dt = real.delta_secs();
    let moving = [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD, KeyCode::Space].iter().any(|k| keys.pressed(*k));
    let action = mouse.just_pressed(MouseButton::Left) || keys.just_pressed(KeyCode::KeyE) || keys.just_pressed(KeyCode::KeyQ);
    if action {
        tf.spike = 0.12;
    }
    tf.spike = (tf.spike - dt).max(0.0);
    let a = if look.captured { activity(moving, motion.delta.length(), tf.spike > 0.0, p.loco) } else { 1.0 };
    let target = MIN_SCALE + (1.0 - MIN_SCALE) * a;
    // Speed up fast, slow down a touch slower (feels like the original).
    let rate = if target > tf.scale { 14.0 } else { 6.0 };
    tf.scale += (target - tf.scale) * (dt * rate).min(1.0);
    vtime.set_relative_speed(tf.scale);
    hud.bars.push(("time".into(), tf.scale, [235, 235, 245]));
}

fn restore(mut t: MessageReader<ModeToggled>, mut vtime: ResMut<Time<Virtual>>, mut tf: ResMut<TimeFlow>) {
    for ev in t.read() {
        if ev.mode == Mode::BulletTime && !ev.on {
            vtime.set_relative_speed(1.0);
            tf.scale = 1.0;
        }
    }
}

/// Cold tint while time is frozen.
fn overlay(mut contexts: EguiContexts, tf: Res<TimeFlow>, modes: Res<ActiveModes>) -> Result {
    if !modes.on(Mode::BulletTime) || tf.scale > 0.95 {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let a = ((1.0 - tf.scale) * 60.0) as u8;
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("bt")));
    painter.rect_filled(ctx.content_rect(), 0.0, egui::Color32::from_rgba_unmultiplied(200, 220, 255, a));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_still_freezes_moving_runs() {
        assert_eq!(activity(false, 0.0, false, Loco::Foot), 0.0);
        assert_eq!(activity(true, 0.0, false, Loco::Foot), 1.0);
        assert!(activity(false, 100.0, false, Loco::Foot) <= 0.35);
        assert!(activity(false, 0.0, true, Loco::Foot) >= 0.7);
    }
}
