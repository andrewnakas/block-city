//! Automation harness for headless-ish verification (native only):
//!   GAMEMASH_MODES=skate,warfare   start with these modes, panel closed
//!   GAMEMASH_SCRIPT="0.5:W+;2:W-;2.2:Space+;2.5:Space-"  timed key presses
//!   GAMEMASH_SHOTS="/tmp/shot:3,6"  screenshots at those seconds, then exit
//!   GAMEMASH_LOOK="yaw,pitch"       initial camera angles
//!   GAMEMASH_SPAWN="x,y,z"          start position
//! Also prints a once-per-second state line for scripted assertions.

use crate::core::input::Look;
use crate::core::modes::{ActiveModes, Mode};
use crate::core::player::{JumpSim, PlayerState, SkateSim, WalkSim, enter_loco};
use crate::core::score::Score;
use crate::core::ui::PanelOpen;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

#[derive(Resource, Default)]
struct Harness {
    script: Vec<(f32, Input, bool, bool)>,
    shots: Vec<f32>,
    prefix: String,
    t: f32,
    next_log: f32,
    exit_at: Option<f32>,
}

#[derive(Clone, Copy)]
enum Input {
    Key(KeyCode),
    Mouse(MouseButton),
    /// "@yaw,pitch": point the camera.
    Look(f32, f32),
}

fn key(name: &str) -> Option<Input> {
    if let Some(rest) = name.strip_prefix('@') {
        let v: Vec<f32> = rest.split(',').filter_map(|x| x.parse().ok()).collect();
        return (v.len() == 2).then(|| Input::Look(v[0], v[1]));
    }
    match name {
        "LMB" => return Some(Input::Mouse(MouseButton::Left)),
        "RMB" => return Some(Input::Mouse(MouseButton::Right)),
        _ => {}
    }
    let k = match name {
        "W" => KeyCode::KeyW,
        "A" => KeyCode::KeyA,
        "S" => KeyCode::KeyS,
        "D" => KeyCode::KeyD,
        "E" => KeyCode::KeyE,
        "F" => KeyCode::KeyF,
        "G" => KeyCode::KeyG,
        "J" => KeyCode::KeyJ,
        "K" => KeyCode::KeyK,
        "L" => KeyCode::KeyL,
        "Q" => KeyCode::KeyQ,
        "T" => KeyCode::KeyT,
        "U" => KeyCode::KeyU,
        "V" => KeyCode::KeyV,
        "Z" => KeyCode::KeyZ,
        "Space" => KeyCode::Space,
        "Shift" => KeyCode::ShiftLeft,
        "Ctrl" => KeyCode::ControlLeft,
        "1" => KeyCode::Digit1,
        "3" => KeyCode::Digit3,
        "4" => KeyCode::Digit4,
        "8" => KeyCode::Digit8,
        _ => return None,
    };
    Some(Input::Key(k))
}

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("GAMEMASH_MODES").is_err() && std::env::var("GAMEMASH_SHOTS").is_err() {
            return;
        }
        let mut h = Harness::default();
        if let Ok(s) = std::env::var("GAMEMASH_SCRIPT") {
            for step in s.split(';').filter(|x| !x.is_empty()) {
                let mut it = step.split(':');
                let (Some(t), Some(k)) = (it.next(), it.next()) else { continue };
                let down = k.ends_with('+') || k.starts_with('@');
                let tap = k.ends_with('!') && !k.starts_with('@');
                let name = if k.starts_with('@') { k } else { k.trim_end_matches(['+', '-', '!']) };
                let (Some(code), Ok(t)) = (key(name), t.parse::<f32>()) else { continue };
                h.script.push((t, code, down || tap, false));
                if tap {
                    h.script.push((t + 0.08, code, false, false));
                }
            }
        }
        if let Ok(s) = std::env::var("GAMEMASH_SHOTS") {
            let mut parts = s.split(':');
            h.prefix = parts.next().unwrap_or("/tmp/gamemash").to_string();
            h.shots = parts.next().unwrap_or("3").split(',').filter_map(|x| x.parse().ok()).collect();
            h.exit_at = h.shots.iter().copied().reduce(f32::max).map(|t| t + 2.0);
        }
        app.insert_resource(h).add_systems(Startup, apply_env.after(crate::core::player::spawn_player)).add_systems(PreUpdate, drive.after(bevy::input::InputSystems).before(crate::core::input::latch_inputs)).add_systems(Update, log_state);
    }
}

fn apply_env(mut modes: ResMut<ActiveModes>, mut panel: ResMut<PanelOpen>, mut look: ResMut<Look>, mut p: ResMut<PlayerState>, mut skate: ResMut<SkateSim>, mut jump: ResMut<JumpSim>, mut walk: ResMut<WalkSim>) {
    if let Some(mask) = std::env::var("GAMEMASH_MODES").ok().as_deref().and_then(crate::core::modes::parse_modes) {
        modes.0 = mask;
    }
    panel.0 = false;
    look.captured = true;
    if let Ok(l) = std::env::var("GAMEMASH_LOOK") {
        let v: Vec<f32> = l.split(',').filter_map(|x| x.parse().ok()).collect();
        if v.len() == 2 {
            look.yaw = v[0];
            look.pitch = v[1];
        }
    }
    if let Ok(s) = std::env::var("GAMEMASH_SPAWN") {
        let v: Vec<f32> = s.split(',').filter_map(|x| x.parse().ok()).collect();
        if v.len() == 3 {
            p.pos = Vec3::new(v[0], v[1], v[2]);
            p.respawn = p.pos;
        }
    }
    let loco = if modes.on(Mode::Skate) { crate::core::player::Loco::Skate } else { crate::core::player::Loco::Foot };
    enter_loco(loco, &mut p, &modes, &mut skate, &mut jump, &mut walk);
}

fn drive(time: Res<Time<bevy::time::Real>>, mut h: ResMut<Harness>, mut keys: ResMut<ButtonInput<KeyCode>>, mut mouse: ResMut<ButtonInput<MouseButton>>, mut commands: Commands, mut exit: MessageWriter<AppExit>, mut look: ResMut<Look>, portal: Option<Res<crate::modes::portals::PortalAssets>>) {
    h.t += time.delta_secs();
    look.captured = true;
    let t = h.t;
    for step in h.script.iter_mut() {
        if !step.3 && t >= step.0 {
            step.3 = true;
            match (step.1, step.2) {
                (Input::Key(k), true) => keys.press(k),
                (Input::Key(k), false) => keys.release(k),
                (Input::Mouse(b), true) => mouse.press(b),
                (Input::Mouse(b), false) => mouse.release(b),
                (Input::Look(y, p), _) => {
                    look.yaw = y;
                    look.pitch = p;
                }
            }
        }
    }
    let prefix = h.prefix.clone();
    for (i, s) in h.shots.clone().iter().enumerate() {
        if *s > 0.0 && t >= *s {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(format!("{prefix}_{i}.png")));
            if let Some(pa) = portal.as_ref() {
                for (k, img) in pa.images.iter().enumerate() {
                    commands.spawn(Screenshot::image(img.clone())).observe(save_to_disk(format!("{prefix}_{i}_rt{k}.png")));
                }
            }
            h.shots[i] = -1.0;
        }
    }
    if h.exit_at.is_some_and(|e| t >= e) {
        exit.write(AppExit::Success);
    }
}

fn log_state(bots: Query<&Transform, With<crate::modes::warfare::Bot>>, diag: Res<bevy::diagnostic::DiagnosticsStore>, mut h: ResMut<Harness>, p: Res<PlayerState>, score: Res<Score>, skate: Res<SkateSim>) {
    if h.t >= h.next_log {
        h.next_log += 1.0;
        info!(
            "HARNESS bots={} fps={:.0} t={:.1} loco={:?} pos=({:.1},{:.1},{:.1}) speed={:.1} ground={} score={} skate={:?} hp={:.0}",
            bots.iter().count(),
            diag.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0),
            h.t, p.loco, p.pos.x, p.pos.y, p.pos.z, p.vel.length(), p.on_ground, score.total, skate.0.state, p.health
        );
    }
}
