//! Sound. One-shots go through the `Sfx` message; continuous sounds (board
//! roll, engine) are looping sinks whose volume and pitch follow the sims.
//! All recordings are CC0/CC-BY (see assets/CREDITS.md).

use crate::core::modes::{ActiveModes, Mode};
use crate::core::player::{Loco, PlayerState, SkateSim};
use crate::sim::skate::SkState;
use bevy::audio::{AudioSinkPlayback, PlaybackMode, Volume};
use bevy::prelude::*;
use std::collections::HashMap;

const FILES: &[(&str, &str)] = &[
    ("rifle", "sounds/rifle_shot.wav"),
    ("pistol", "sounds/pistol_shot.wav"),
    ("rifle_reload", "sounds/rifle_reload.wav"),
    ("pistol_reload", "sounds/pistol_reload.wav"),
    ("explosion", "sounds/explosion.mp3"),
    ("pop", "sounds/skate_pop.mp3"),
    ("land", "sounds/skate_land.mp3"),
    ("grind", "sounds/grind_hit.ogg"),
    ("metal", "sounds/metal_light.ogg"),
    ("wood", "sounds/wood_hit.ogg"),
    ("step0", "sounds/step_0.ogg"),
    ("step1", "sounds/step_1.ogg"),
    ("step2", "sounds/step_2.ogg"),
    ("step3", "sounds/step_3.ogg"),
    ("step4", "sounds/step_4.ogg"),
];

/// Play a one-shot: name from FILES, volume 0..1, playback speed (pitch).
#[derive(Message, Clone, Copy)]
pub struct Sfx {
    pub name: &'static str,
    pub volume: f32,
    pub speed: f32,
}

impl Sfx {
    pub fn new(name: &'static str, volume: f32) -> Self {
        Self { name, volume, speed: 1.0 }
    }
    /// Volume falloff for a sound heard from `dist` metres away.
    pub fn at(name: &'static str, volume: f32, dist: f32) -> Self {
        Self::new(name, volume / (1.0 + dist * dist * 0.004))
    }
}

#[derive(Resource)]
struct Sounds(HashMap<&'static str, Handle<AudioSource>>);

#[derive(Component)]
struct RollLoop;
#[derive(Component)]
struct EngineLoop;

pub struct AudioFxPlugin;

impl Plugin for AudioFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Sfx>().add_systems(Startup, load).add_systems(Update, (play, roll, engine, footsteps));
    }
}

fn load(mut commands: Commands, assets: Res<AssetServer>) {
    let map = FILES.iter().map(|(k, p)| (*k, assets.load(*p))).collect();
    commands.insert_resource(Sounds(map));
    let quiet = PlaybackSettings { mode: PlaybackMode::Loop, volume: Volume::Linear(0.0), ..default() };
    commands.spawn((RollLoop, AudioPlayer::new(assets.load("sounds/skate_roll.mp3")), quiet));
    commands.spawn((EngineLoop, AudioPlayer::new(assets.load("sounds/engine_loop.wav")), quiet));
}

fn play(mut commands: Commands, sounds: Res<Sounds>, mut ev: MessageReader<Sfx>) {
    for s in ev.read() {
        if s.volume < 0.01 {
            continue;
        }
        if let Some(h) = sounds.0.get(s.name) {
            commands.spawn((AudioPlayer::new(h.clone()), PlaybackSettings { speed: s.speed, ..PlaybackSettings::DESPAWN.with_volume(Volume::Linear(s.volume)) }));
        }
    }
}

/// Wheels on concrete: louder and higher with speed, silent in the air.
fn roll(time: Res<Time>, p: Res<PlayerState>, skate: Res<SkateSim>, mut q: Query<&mut AudioSink, With<RollLoop>>, mut level: Local<f32>) {
    let Ok(mut sink) = q.single_mut() else { return };
    let s = &skate.0;
    let rolling = p.loco == Loco::Skate && matches!(s.state, SkState::Rolling | SkState::Manual | SkState::Powerslide);
    let target = if rolling { (s.speed() / 8.0).clamp(0.0, 1.0) * if s.state == SkState::Powerslide { 1.3 } else { 0.8 } } else { 0.0 };
    *level += (target - *level) * (time.delta_secs() * 12.0).min(1.0);
    sink.set_volume(Volume::Linear(*level));
    sink.set_speed(0.8 + (s.speed() / 10.0).min(0.6));
}

fn engine(time: Res<Time>, p: Res<PlayerState>, modes: Res<ActiveModes>, mut q: Query<&mut AudioSink, With<EngineLoop>>, mut level: Local<f32>) {
    let Ok(mut sink) = q.single_mut() else { return };
    let driving = p.loco == Loco::Vehicle && modes.on(Mode::Streets);
    let target = if driving { 0.45 } else { 0.0 };
    *level += (target - *level) * (time.delta_secs() * 6.0).min(1.0);
    sink.set_volume(Volume::Linear(*level));
    sink.set_speed(0.7 + (p.vel.length() / 25.0).min(1.3));
}

fn footsteps(time: Res<Time>, p: Res<PlayerState>, mut out: MessageWriter<Sfx>, mut dist: Local<f32>, mut n: Local<usize>) {
    if p.loco != Loco::Foot || !p.on_ground {
        return;
    }
    let speed = p.vel.with_y(0.0).length();
    *dist += speed * time.delta_secs();
    let stride = 0.75 + speed * 0.06;
    if *dist > stride {
        *dist = 0.0;
        *n = (*n + 1) % 5;
        const STEPS: [&str; 5] = ["step0", "step1", "step2", "step3", "step4"];
        out.write(Sfx { name: STEPS[*n], volume: (0.15 + speed * 0.05).min(0.5), speed: 0.95 + (*n as f32) * 0.02 });
    }
}
