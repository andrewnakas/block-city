//! The arena boss: a soulslike duel fought with the platformer moveset.
//!
//! The boss telegraphs three attacks: a wide sweep you jump over, a leaping
//! slam whose shockwave you hop, and a charge you sidestep. You hurt it with
//! punches, dives, ground pounds and stomps on its head. Hits fill its posture
//! meter; when posture breaks it kneels, and you can grab it (E), spin it and
//! throw it. Coins around the ring refill one wedge of your eight-wedge meter.
//!
//! Numbers are tuning defaults written for this game (docs/specs/arena.md).

use crate::core::audio::Sfx;
use crate::core::player::{JumpSim, Loco, PlayerState};
use crate::core::rig::{HAIR_BUZZED, Rig, SOLDIER};
use crate::core::scene::{self, World};
use crate::core::score::{Kind, MashEvent};
use crate::core::ui::Hud;
use crate::sim::jump::JAction;
use crate::world::arena::{RADIUS, build_arena};
use bevy::prelude::*;
use std::f32::consts::TAU;

const NAME: &str = "THE HOLLOW WARDEN";
const HP: f32 = 600.0;
const POSTURE: f32 = 100.0;
const SCALE: f32 = 2.4;
/// Top of the boss's head, for stomps.
const HEAD: f32 = 1.8 * SCALE;

#[derive(Clone, Copy, PartialEq, Debug)]
enum BState {
    Idle(f32),
    Sweep { t: f32, second: bool },
    LeapWind(f32),
    Leap { t: f32, from: Vec3, to: Vec3 },
    Charge { t: f32, dir: Vec3, hit: bool },
    Recover(f32),
    Stagger(f32),
    Grabbed { t: f32, angle: f32, spin: f32 },
    Thrown { t: f32, from: Vec3, to: Vec3, power: f32 },
    Dead(f32),
}

#[derive(Component)]
struct Boss {
    rig: Entity,
    hp: f32,
    posture: f32,
    state: BState,
    swing_hit: bool,
}

#[derive(Component)]
struct Shockwave {
    t: f32,
    hit: bool,
}

#[derive(Component)]
struct Coin {
    respawn: f32,
}

#[derive(Resource)]
struct BossAssets {
    ring: Handle<Mesh>,
    ring_mat: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
struct Duel {
    wins: u32,
    last_health: f32,
    prev_action: Option<JAction>,
}

pub struct BossPlugin;

impl Plugin for BossPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Duel>()
            .add_systems(Startup, (build_arena, setup).run_if(scene::is(World::Arena)))
            .add_systems(Update, (player_strikes, boss_ai, shockwaves, coins, hud).chain().run_if(scene::is(World::Arena)));
    }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let armour = mats.add(StandardMaterial { base_color: Color::srgb(0.2, 0.19, 0.22), metallic: 0.7, perceptual_roughness: 0.45, ..default() });
    let rig = commands.spawn((Rig::person(SOLDIER, Some(HAIR_BUZZED)).with_tint(armour), Transform::from_scale(Vec3::splat(SCALE)), Visibility::default())).id();
    commands
        .spawn((Boss { rig, hp: HP, posture: 0.0, state: BState::Idle(2.0), swing_hit: false }, Transform::from_xyz(0.0, 0.0, 10.0), Visibility::default()))
        .add_child(rig);
    commands.insert_resource(BossAssets {
        ring: meshes.add(Torus::new(0.85, 1.0)),
        ring_mat: mats.add(StandardMaterial { base_color: Color::srgb(1.0, 0.55, 0.2), emissive: LinearRgba::rgb(6.0, 2.4, 0.6), ..default() }),
    });
    let coin_mesh = meshes.add(Cylinder::new(0.45, 0.1));
    let coin_mat = mats.add(StandardMaterial { base_color: Color::srgb(1.0, 0.8, 0.15), metallic: 0.9, perceptual_roughness: 0.3, emissive: LinearRgba::rgb(0.6, 0.45, 0.05), ..default() });
    for i in 0..8 {
        let a = i as f32 / 8.0 * TAU;
        let p = Vec3::new(a.sin(), 0.0, a.cos()) * 15.0 + Vec3::Y * 1.0;
        commands.spawn((Coin { respawn: 0.0 }, Mesh3d(coin_mesh.clone()), MeshMaterial3d(coin_mat.clone()), Transform::from_translation(p).with_rotation(Quat::from_rotation_x(TAU / 4.0))));
    }
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Damage from the platformer moveset: punches, dives, ground pounds, head stomps.
#[allow(clippy::too_many_arguments)]
fn player_strikes(
    mut p: ResMut<PlayerState>,
    sim: Res<JumpSim>,
    mut duel: ResMut<Duel>,
    mut q: Query<(&mut Boss, &Transform)>,
    mut hud: ResMut<Hud>,
    mut out: MessageWriter<MashEvent>,
    mut sfx: MessageWriter<Sfx>,
) {
    let Ok((mut b, tf)) = q.single_mut() else { return };
    let action = sim.j.action;
    let prev = duel.prev_action.replace(action);
    if p.loco != Loco::Foot || matches!(b.state, BState::Dead(_) | BState::Grabbed { .. } | BState::Thrown { .. }) {
        return;
    }
    let to = flat(tf.translation - p.pos);
    let d = to.length();
    let facing = Vec3::new(p.facing.sin(), 0.0, p.facing.cos());
    let in_front = to.normalize_or_zero().dot(facing) > 0.35;
    let started = |a: fn(&JAction) -> bool| a(&action) && !prev.as_ref().is_some_and(a);

    let mut hit: Option<(&str, f32, f32)> = None;
    if started(|a| matches!(a, JAction::Punch { .. })) && d < 3.0 && in_front {
        hit = Some(("Punch", 16.0, 10.0));
    } else if started(|a| matches!(a, JAction::Dive)) && d < 3.2 && in_front {
        hit = Some(("Dive", 14.0, 8.0));
    } else if matches!(prev, Some(JAction::GroundPound)) && !matches!(action, JAction::GroundPound) && d < 3.4 {
        hit = Some(("Ground Pound", 34.0, 28.0));
    } else if p.vel.y < -1.5 && d < 1.6 && (p.pos.y - (tf.translation.y + HEAD)).abs() < 0.8 {
        hit = Some(("Stomp", 22.0, 22.0));
        p.impulse += Vec3::Y * 11.0;
    }
    if let Some((name, dmg, posture)) = hit {
        let stagger = matches!(b.state, BState::Stagger(_));
        b.hp -= dmg * if stagger { 1.5 } else { 1.0 };
        if !stagger {
            b.posture += posture;
        }
        hud.hitmarker = 0.3;
        out.write(MashEvent::new(Kind::Trick, name, (dmg * 5.0) as u32));
        sfx.write(Sfx::new("metal", 0.9));
    }
}

#[allow(clippy::too_many_arguments)]
fn boss_ai(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut p: ResMut<PlayerState>,
    mut q: Query<(&mut Boss, &mut Transform)>,
    mut rigs: Query<&mut Rig>,
    mut duel: ResMut<Duel>,
    mut commands: Commands,
    assets: Option<Res<BossAssets>>,
    mut hud: ResMut<Hud>,
    mut out: MessageWriter<MashEvent>,
    mut sfx: MessageWriter<Sfx>,
    mut boom: MessageWriter<crate::core::fx::Explosion>,
) {
    let Ok((mut b, mut tf)) = q.single_mut() else { return };
    let Some(assets) = assets else { return };
    let dt = time.delta_secs().min(0.05);

    // A respawned player (health jumped back to full) faces a fresh boss.
    if p.health > duel.last_health + 50.0 && !matches!(b.state, BState::Dead(_)) {
        b.hp = HP;
        b.posture = 0.0;
        b.state = BState::Idle(2.0);
        tf.translation = Vec3::new(0.0, 0.0, 10.0);
    }
    duel.last_health = p.health;

    let phase2 = b.hp < HP * 0.5;
    let speed = if phase2 { 1.35 } else { 1.0 };
    let to = flat(p.pos - tf.translation);
    let d = to.length();
    let dir = to.normalize_or_zero();
    let player_low = p.on_ground || p.pos.y - tf.translation.y < 1.0;

    if b.hp <= 0.0 && !matches!(b.state, BState::Dead(_)) {
        b.state = BState::Dead(10.0);
        duel.wins += 1;
        hud.banner = Some((format!("{NAME} DEFEATED"), 5.0));
        out.write(MashEvent::new(Kind::Kill { airborne: !p.on_ground, headshot: false }, "Warden", 5000));
    }
    if b.posture >= POSTURE && !matches!(b.state, BState::Stagger(_) | BState::Grabbed { .. } | BState::Thrown { .. } | BState::Dead(_)) {
        b.posture = 0.0;
        b.state = BState::Stagger(4.5);
        hud.center = Some(("POSTURE BROKEN: press E to grab".into(), 2.0));
        sfx.write(Sfx::new("metal", 1.0));
    }

    let anim;
    let mut anim_speed = 1.0;
    b.state = match b.state {
        BState::Idle(left) => {
            anim = "Walk_Loop";
            if d > 3.5 {
                tf.translation += dir * 2.2 * speed * dt;
            }
            if left - dt > 0.0 {
                BState::Idle(left - dt)
            } else if d < 6.5 {
                b.swing_hit = false;
                BState::Sweep { t: 0.0, second: false }
            } else if d < 20.0 {
                BState::LeapWind(0.0)
            } else {
                BState::Charge { t: 0.0, dir, hit: false }
            }
        }
        BState::Sweep { t, second } => {
            anim = "Sword_Attack";
            anim_speed = 0.8 * speed;
            let wind = 0.9 / speed;
            let t = t + dt;
            // The blade passes at the end of the wind-up: jump it.
            if t >= wind && t < wind + 0.25 && !b.swing_hit {
                let facing = tf.rotation.mul_vec3(Vec3::Z);
                if d < 7.5 && flat(facing).normalize_or_zero().dot(dir) > -0.2 && player_low {
                    b.swing_hit = true;
                    p.health -= 25.0;
                    p.impulse += dir * 9.0 + Vec3::Y * 5.0;
                    sfx.write(Sfx::new("metal", 1.0));
                }
            }
            if t > wind + 0.5 {
                if phase2 && !second {
                    b.swing_hit = false;
                    BState::Sweep { t: 0.15, second: true }
                } else {
                    BState::Recover(1.1 / speed)
                }
            } else {
                BState::Sweep { t, second }
            }
        }
        BState::LeapWind(t) => {
            anim = "Crouch_Idle_Loop";
            if t + dt > 0.6 / speed {
                BState::Leap { t: 0.0, from: tf.translation, to: flat(p.pos).clamp_length_max(RADIUS - 3.0) }
            } else {
                BState::LeapWind(t + dt)
            }
        }
        BState::Leap { t, from, to: land } => {
            anim = "Jump_Loop";
            let t = t + dt;
            let k = (t / 1.0).min(1.0);
            tf.translation = from.lerp(land, k) + Vec3::Y * (4.0 * k * (1.0 - k)) * 7.0;
            if k >= 1.0 {
                tf.translation.y = 0.0;
                // Direct hit, then a shockwave ring to hop.
                if flat(p.pos - land).length() < 3.5 && player_low {
                    p.health -= 30.0;
                    let away = flat(p.pos - land).normalize_or_zero();
                    p.impulse += away * 10.0 + Vec3::Y * 6.0;
                }
                commands.spawn((Shockwave { t: 0.0, hit: false }, Mesh3d(assets.ring.clone()), MeshMaterial3d(assets.ring_mat.clone()), Transform::from_translation(land + Vec3::Y * 0.3)));
                sfx.write(Sfx::new("explosion", 0.7));
                BState::Recover(1.2 / speed)
            } else {
                BState::Leap { t, from, to: land }
            }
        }
        BState::Charge { t, dir: cdir, hit } => {
            anim = "Sprint_Loop";
            let t = t + dt;
            tf.translation += cdir * 10.0 * speed * dt;
            let mut hit = hit;
            if !hit && d < 2.4 {
                hit = true;
                p.health -= 20.0;
                p.impulse += cdir * 12.0 + Vec3::Y * 5.0;
                sfx.write(Sfx::new("metal", 1.0));
            }
            if t > 1.6 || flat(tf.translation).length() > RADIUS - 2.0 { BState::Recover(1.0 / speed) } else { BState::Charge { t, dir: cdir, hit } }
        }
        BState::Recover(left) => {
            anim = "Idle_Loop";
            if left - dt > 0.0 { BState::Recover(left - dt) } else { BState::Idle(0.5 / speed) }
        }
        BState::Stagger(left) => {
            anim = "Fixing_Kneeling";
            if keys.just_pressed(KeyCode::KeyE) && d < 3.8 {
                hud.center = Some(("SPIN!".into(), 1.0));
                BState::Grabbed { t: 0.0, angle: (-dir).x.atan2(-dir.z), spin: 3.0 }
            } else if left - dt > 0.0 {
                BState::Stagger(left - dt)
            } else {
                BState::Idle(0.6)
            }
        }
        BState::Grabbed { t, angle, spin } => {
            anim = "Hit_Head";
            // Spin it around you; it speeds up, and you let go with E (or it slips after 2 s).
            let t = t + dt;
            let spin = (spin + 7.0 * dt).min(14.0);
            let angle = angle + spin * dt;
            let off = Vec3::new(angle.sin(), 0.0, angle.cos());
            tf.translation = flat(p.pos) + off * 3.2 + Vec3::Y * 0.6;
            if (keys.just_pressed(KeyCode::KeyE) && t > 0.4) || t > 2.0 {
                let fling = Vec3::new(off.z, 0.0, -off.x) * spin.signum() + off;
                let to = (flat(tf.translation) + fling.normalize() * 14.0).clamp_length_max(RADIUS + 1.0);
                BState::Thrown { t: 0.0, from: tf.translation, to, power: spin }
            } else {
                BState::Grabbed { t, angle, spin }
            }
        }
        BState::Thrown { t, from, to: land, power } => {
            anim = "Hit_Head";
            let t = t + dt;
            let k = (t / 0.9).min(1.0);
            tf.translation = from.lerp(land, k) + Vec3::Y * (4.0 * k * (1.0 - k)) * 4.0;
            if k >= 1.0 {
                tf.translation.y = 0.0;
                let dmg = 40.0 + power * 7.0;
                b.hp -= dmg;
                // Thrown into the outer wall: it explodes.
                if flat(land).length() > RADIUS - 1.0 {
                    b.hp -= 40.0;
                    boom.write(crate::core::fx::Explosion { pos: land + Vec3::Y, radius: 3.0, power: 0.6 });
                }
                hud.center = Some((format!("THROW  {:.0}", dmg), 1.2));
                out.write(MashEvent::new(Kind::Trick, "Warden Throw", (dmg * 10.0) as u32));
                sfx.write(Sfx::new("explosion", 0.9));
                BState::Recover(1.8)
            } else {
                BState::Thrown { t, from, to: land, power }
            }
        }
        BState::Dead(left) => {
            anim = "Death01";
            if left - dt > 0.0 {
                BState::Dead(left - dt)
            } else {
                b.hp = HP;
                b.posture = 0.0;
                tf.translation = Vec3::new(0.0, 0.0, 10.0);
                hud.banner = Some(("A new challenger stirs".into(), 3.0));
                BState::Idle(3.0)
            }
        }
    };
    // Face the player except mid-charge or mid-throw.
    if !matches!(b.state, BState::Charge { .. } | BState::Thrown { .. } | BState::Grabbed { .. } | BState::Dead(_) | BState::Stagger(_)) && d > 0.1 {
        let target = Quat::from_rotation_y(dir.x.atan2(dir.z));
        tf.rotation = tf.rotation.slerp(target, (dt * 4.0).min(1.0));
    }
    let r = flat(tf.translation);
    if r.length() > RADIUS && !matches!(b.state, BState::Thrown { .. } | BState::Grabbed { .. }) {
        let c = r.clamp_length_max(RADIUS);
        tf.translation.x = c.x;
        tf.translation.z = c.z;
    }
    b.posture = (b.posture - 4.0 * dt).max(0.0);
    if let Ok(mut rg) = rigs.get_mut(b.rig) {
        rg.want = anim;
        rg.speed = anim_speed;
    }
}

fn shockwaves(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut Shockwave, &mut Transform)>, mut p: ResMut<PlayerState>) {
    for (e, mut s, mut tf) in q.iter_mut() {
        s.t += time.delta_secs();
        let r = 1.0 + s.t * 16.0;
        tf.scale = Vec3::new(r, 1.0, r);
        let d = flat(p.pos - tf.translation).length();
        let low = p.on_ground || p.pos.y - tf.translation.y < 0.8;
        if !s.hit && low && (d - r).abs() < 0.9 {
            s.hit = true;
            p.health -= 18.0;
            let away = flat(p.pos - tf.translation).normalize_or_zero();
            p.impulse += away * 7.0 + Vec3::Y * 5.0;
        }
        if s.t > 0.75 {
            commands.entity(e).despawn();
        }
    }
}

fn coins(time: Res<Time>, mut q: Query<(&mut Coin, &mut Transform, &mut Visibility)>, mut p: ResMut<PlayerState>, mut out: MessageWriter<MashEvent>) {
    for (mut c, mut tf, mut vis) in q.iter_mut() {
        tf.rotate_y(time.delta_secs() * 3.0);
        if c.respawn > 0.0 {
            c.respawn -= time.delta_secs();
            *vis = if c.respawn > 0.0 { Visibility::Hidden } else { Visibility::Visible };
            continue;
        }
        if tf.translation.distance(p.pos + Vec3::Y * 0.8) < 1.4 && p.health < 100.0 {
            p.health = (p.health + 12.5).min(100.0);
            c.respawn = 25.0;
            out.write(MashEvent::new(Kind::Platform, "Coin", 10));
        }
    }
}

fn hud(p: Res<PlayerState>, q: Query<&Boss>, duel: Res<Duel>, mut hud: ResMut<Hud>) {
    // Eight-wedge power meter: health shown in whole wedges.
    let wedges = (p.health / 12.5).ceil().clamp(0.0, 8.0);
    hud.bars.push((format!("power {}/8", wedges as u32), wedges / 8.0, if wedges <= 2.0 { [230, 60, 50] } else { [90, 170, 240] }));
    if let Ok(b) = q.single() {
        hud.bars.push((NAME.to_string(), (b.hp / HP).clamp(0.0, 1.0), [200, 50, 40]));
        hud.bars.push(("posture".into(), (b.posture / POSTURE).clamp(0.0, 1.0), [235, 190, 60]));
    }
    if duel.wins > 0 {
        hud.top.push(format!("Wardens defeated: {}", duel.wins));
    }
}
