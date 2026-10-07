//! Controls screen (Tab) and the HUD. Modes publish their HUD text through `Hud`
//! each frame, so the UI never needs to know mode internals.

use crate::core::camera::CamRig;
use crate::core::input::Look;
use crate::core::modes::{ActiveModes, Mode};
use crate::core::player::{Loco, PlayerState};
use crate::core::score::Score;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

#[derive(Resource)]
pub struct PanelOpen(pub bool);

/// Per-frame HUD contributions from modes.
#[derive(Resource, Default)]
pub struct Hud {
    pub top: Vec<String>,
    pub center: Option<(String, f32)>,
    pub crosshair: bool,
    pub hitmarker: f32,
    pub bars: Vec<(String, f32, [u8; 3])>,
    pub hotbar: Vec<(String, bool)>,
    pub stars: Option<u32>,
    pub banner: Option<(String, f32)>,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PanelOpen(crate::core::modes::startup_panel_open()))
            .init_resource::<Hud>()
            .add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(First, clear_hud)
            .add_systems(Update, toggle_panel)
            .add_systems(EguiPrimaryContextPass, (hud_ui, panel_ui).chain());
    }
}

fn clear_hud(mut hud: ResMut<Hud>, time: Res<Time<bevy::time::Real>>) {
    let dt = time.delta_secs();
    hud.top.clear();
    hud.bars.clear();
    hud.hotbar.clear();
    hud.crosshair = false;
    hud.stars = None;
    hud.hitmarker = (hud.hitmarker - dt).max(0.0);
    if let Some((_, t)) = hud.banner.as_mut() {
        *t -= dt;
    }
    if hud.banner.as_ref().is_some_and(|b| b.1 <= 0.0) {
        hud.banner = None;
    }
    if let Some((_, t)) = hud.center.as_mut() {
        *t -= dt;
    }
    if hud.center.as_ref().is_some_and(|b| b.1 <= 0.0) {
        hud.center = None;
    }
}

fn toggle_panel(keys: Res<ButtonInput<KeyCode>>, mut open: ResMut<PanelOpen>, mut vt: ResMut<Time<Virtual>>) {
    if keys.just_pressed(KeyCode::Tab) {
        open.0 = !open.0;
    }
    // The world waits while you pick modes.
    if open.0 && !vt.is_paused() {
        vt.pause();
    } else if !open.0 && vt.is_paused() {
        vt.unpause();
    }
}

fn c32(c: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

#[allow(clippy::too_many_arguments)]
fn hud_ui(
    mut contexts: EguiContexts,
    hud: Res<Hud>,
    score: Res<Score>,
    modes: Res<ActiveModes>,
    p: Res<PlayerState>,
    rig: Res<CamRig>,
    look: Res<Look>,
    diag: Res<DiagnosticsStore>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let fps = diag.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);

    egui::Area::new(egui::Id::new("score")).anchor(egui::Align2::LEFT_TOP, [16.0, 12.0]).show(ctx, |ui| {
        ui.label(egui::RichText::new(format!("{}", score.total)).size(34.0).strong().color(egui::Color32::WHITE));
        ui.label(egui::RichText::new(format!("{:.0} fps   {:.1} m/s   {:?}", fps, p.vel.length(), p.loco)).size(13.0).color(egui::Color32::from_gray(220)));
        for line in &hud.top {
            ui.label(egui::RichText::new(line).size(16.0).color(egui::Color32::WHITE).strong());
        }
        for (name, frac, col) in &hud.bars {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(name).size(12.0).color(egui::Color32::WHITE));
                let (rect, _) = ui.allocate_exact_size(egui::vec2(140.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 3.0, egui::Color32::from_black_alpha(140));
                let mut fill = rect;
                fill.set_width(rect.width() * frac.clamp(0.0, 1.0));
                ui.painter().rect_filled(fill, 3.0, c32(*col));
            });
        }
        if let Some(stars) = hud.stars {
            let s: String = (0..5).map(|i| if i < stars { '★' } else { '☆' }).collect();
            ui.label(egui::RichText::new(s).size(26.0).color(egui::Color32::from_rgb(255, 214, 64)));
        }
    });

    // Score feed: synergies in gold.
    egui::Area::new(egui::Id::new("feed")).anchor(egui::Align2::RIGHT_TOP, [-16.0, 16.0]).show(ctx, |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
            for l in &score.feed {
                let a = ((3.5 - l.age).min(1.0) * 255.0) as u8;
                let col = if l.synergy { egui::Color32::from_rgba_unmultiplied(255, 205, 60, a) } else { egui::Color32::from_rgba_unmultiplied(255, 255, 255, a) };
                let size = if l.synergy { 22.0 } else { 17.0 };
                ui.label(egui::RichText::new(format!("{}  +{}", l.text, l.points)).size(size).strong().color(col));
            }
        });
    });

    if let Some((text, t)) = &hud.banner {
        egui::Area::new(egui::Id::new("banner")).anchor(egui::Align2::CENTER_TOP, [0.0, 90.0]).show(ctx, |ui| {
            let a = (t.min(1.0) * 255.0) as u8;
            ui.label(egui::RichText::new(text).size(30.0).strong().color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, a)));
        });
    }
    if let Some((text, _)) = &hud.center {
        egui::Area::new(egui::Id::new("center")).anchor(egui::Align2::CENTER_CENTER, [0.0, 60.0]).show(ctx, |ui| {
            ui.label(egui::RichText::new(text).size(18.0).color(egui::Color32::WHITE));
        });
    }

    // Crosshair + hitmarker.
    // Iron sights replace the crosshair while aiming.
    if (hud.crosshair || rig.first_person) && rig.ads < 0.5 {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("xhair")));
        let c = ctx.content_rect().center();
        let gap = 4.0 + 8.0 * (1.0 - rig.ads) + (p.vel.length() * 0.6).min(10.0);
        let col = egui::Color32::from_white_alpha(220);
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let a = c + egui::vec2(dx * gap, dy * gap);
            let b = c + egui::vec2(dx * (gap + 7.0), dy * (gap + 7.0));
            painter.line_segment([a, b], egui::Stroke::new(2.0, col));
        }
        if hud.hitmarker > 0.0 {
            let hc = egui::Color32::from_rgb(255, 80, 60);
            for (dx, dy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                painter.line_segment([c + egui::vec2(dx * 5.0, dy * 5.0), c + egui::vec2(dx * 12.0, dy * 12.0)], egui::Stroke::new(2.5, hc));
            }
        }
    }

    // Hotbar.
    if !hud.hotbar.is_empty() {
        egui::Area::new(egui::Id::new("hotbar")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -54.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (name, sel) in &hud.hotbar {
                    let frame = egui::Frame::new()
                        .fill(if *sel { egui::Color32::from_rgba_unmultiplied(255, 255, 255, 60) } else { egui::Color32::from_black_alpha(120) })
                        .stroke(egui::Stroke::new(if *sel { 2.0 } else { 1.0 }, egui::Color32::WHITE))
                        .inner_margin(6.0)
                        .corner_radius(4.0);
                    frame.show(ui, |ui| ui.label(egui::RichText::new(name).size(13.0).color(egui::Color32::WHITE)));
                }
            });
        });
    }

    egui::Area::new(egui::Id::new("chips")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -12.0]).show(ctx, |ui| {
        ui.label(egui::RichText::new("Tab: controls").size(12.0).color(egui::Color32::from_gray(200)));
    });
    let _ = &modes;

    if !look.captured {
        egui::Area::new(egui::Id::new("clickhint")).anchor(egui::Align2::CENTER_CENTER, [0.0, -40.0]).show(ctx, |ui| {
            ui.label(egui::RichText::new("Click to play").size(20.0).color(egui::Color32::WHITE));
        });
    }
    Ok(())
}

fn panel_ui(mut contexts: EguiContexts, mut open: ResMut<PanelOpen>, p: Res<PlayerState>) -> Result {
    if !open.0 {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect();
    let width = (screen.width() - 90.0).clamp(260.0, 640.0);
    let game = crate::core::game::TITLE.to_uppercase();
    egui::Window::new(game.as_str())
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([width, (screen.height() - 60.0).max(200.0)])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height((screen.height() - 120.0).max(160.0)).show(ui, |ui| {
                ui.label(egui::RichText::new(crate::core::game::TAGLINE).size(15.0));
                ui.add_space(8.0);
                ui.label(egui::RichText::new("WASD move.  Space jump.  Shift sprint.  Mouse look.  V first/third person.  Esc frees the mouse.").size(12.5));
                for m in crate::core::game::MODES {
                    ui.add(egui::Label::new(egui::RichText::new(m.controls()).size(12.0).color(egui::Color32::from_gray(200))).wrap());
                }
                ui.add_space(6.0);
                for line in crate::core::game::HELP {
                    ui.add(egui::Label::new(*line).wrap());
                }
                if p.loco == Loco::Vehicle {
                    ui.label("In a car: E to get out.");
                }
                ui.add_space(8.0);
                if ui.button(egui::RichText::new("PLAY  (Tab)").size(18.0).strong()).clicked() {
                    open.0 = false;
                }
                ui.label(egui::RichText::new("A clean-room tribute: original code and CC0 assets. No code, assets or names from the games that inspired it.").size(10.0).color(egui::Color32::from_gray(150)));
            });
        });
    Ok(())
}
