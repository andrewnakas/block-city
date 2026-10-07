//! Realm raiders and block-game melee. Raiders roam the villages and the keep,
//! notice you, close in and swing. You hit back with the classic block-game
//! rules: short reach, an attack cooldown, knock-back (more when sprinting),
//! critical hits while falling, and a brief invulnerability after each hit.
//!
//! Values come from observed behaviour of block-sandbox combat (see
//! docs/specs/realm.md), not from any game's code.

use crate::core::audio::Sfx;
use crate::core::camera::CamRig;
use crate::core::input::Look;
use crate::core::player::PlayerState;
use crate::core::rig::{HAIR_PARTED, Rig, SOLDIER};
use crate::core::scene::{self, World};
use crate::core::score::{Kind, MashEvent};
use crate::core::ui::Hud;
use crate::modes::blocks::VoxelWorld;
use crate::world::realm::{KEEP, VILLAGES, surface};
use bevy::prelude::*;

/// Tuning, in game units (metres, seconds, health where the player has 100).
pub struct MeleeTuning {
    pub reach: f32,
    pub cone_deg: f32,
    pub cooldown: f32,
    pub damage: f32,
    pub crit_mult: f32,
    pub knockback: f32,
    pub sprint_knockback: f32,
    pub invuln: f32,
}

pub const MELEE: MeleeTuning = MeleeTuning { reach: 3.2, cone_deg: 28.0, cooldown: 0.6, damage: 7.0, crit_mult: 1.5, knockback: 5.5, sprint_knockback: 4.0, invuln: 0.5 };

const RAIDER_HP: f32 = 20.0;
const RAIDER_DMG: f32 = 15.0;
const MAX_RAIDERS: usize = 9;
const SIGHT: f32 = 20.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum RState {
    Wander,
    Chase,
    Windup(f32),
    Recover(f32),
}

#[derive(Component)]
pub struct Raider {
    rig: Entity,
    hp: f32,
    state: RState,
    home: Vec3,
    goal: Vec3,
    think: f32,
    vel: Vec3,
    knock: Vec3,
    invuln: f32,
}

#[derive(Component)]
struct Fallen(f32);

/// The raider under the crosshair within reach, if any. Block mining yields to it.
#[derive(Resource, Default)]
pub struct MeleeTarget(pub Option<Entity>);

#[derive(Resource, Default)]
struct MeleeState {
    cooldown: f32,
    felled: u32,
    since_hurt: f32,
    last_health: f32,
}

#[derive(Resource)]
struct RaiderLook(Handle<StandardMaterial>);

pub struct RaidersPlugin;

impl Plugin for RaidersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MeleeTarget>()
            .init_resource::<MeleeState>()
            .add_systems(Startup, setup.run_if(scene::is(World::Realm)))
            .add_systems(Update, (spawn, target, attack, think, fallen, hud).chain().run_if(scene::is(World::Realm)));
    }
}

fn setup(mut commands: Commands, mut mats: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(RaiderLook(mats.add(StandardMaterial { base_color: Color::srgb(0.42, 0.26, 0.16), perceptual_roughness: 0.9, ..default() })));
}

fn spawn_points() -> Vec<IVec2> {
    let mut v: Vec<IVec2> = VILLAGES.iter().flat_map(|c| [*c + IVec2::new(3, 0), *c + IVec2::new(-3, 2)]).collect();
    v.extend([KEEP + IVec2::new(-4, -4), KEEP + IVec2::new(4, 4), KEEP + IVec2::new(0, -14)]);
    v
}

fn spawn(mut commands: Commands, look: Option<Res<RaiderLook>>, raiders: Query<&Raider>, world: Res<VoxelWorld>, p: Res<PlayerState>, time: Res<Time>, mut timer: Local<f32>, mut next: Local<usize>) {
    let Some(look) = look else { return };
    *timer -= time.delta_secs();
    if raiders.iter().count() >= MAX_RAIDERS || *timer > 0.0 {
        return;
    }
    *timer = 4.0;
    let pts = spawn_points();
    // Never pop in right next to the player.
    for _ in 0..pts.len() {
        let xz = pts[*next % pts.len()];
        *next += 1;
        let Some(y) = surface(&world.v, xz.x, xz.y, 60) else { continue };
        let pos = Vec3::new(xz.x as f32 + 0.5, y as f32 + 1.0, xz.y as f32 + 0.5);
        if pos.distance(p.pos) < 16.0 {
            continue;
        }
        let rig = commands.spawn((Rig::person(SOLDIER, Some(HAIR_PARTED)).with_tint(look.0.clone()), Transform::from_xyz(0.0, 0.0, 0.0), Visibility::default())).id();
        commands
            .spawn((Raider { rig, hp: RAIDER_HP, state: RState::Wander, home: pos, goal: pos, think: 0.0, vel: Vec3::ZERO, knock: Vec3::ZERO, invuln: 0.0 }, Transform::from_translation(pos), Visibility::default()))
            .add_child(rig);
        return;
    }
}

fn target(rig: Res<CamRig>, look: Res<Look>, raiders: Query<(Entity, &Transform), With<Raider>>, mut tgt: ResMut<MeleeTarget>) {
    tgt.0 = None;
    if !look.captured {
        return;
    }
    let mut best = f32::MAX;
    let cos = MELEE.cone_deg.to_radians().cos();
    for (e, tf) in raiders.iter() {
        let chest = tf.translation + Vec3::Y * 1.1;
        let to = chest - rig.eye;
        let d = to.length();
        // In third person the camera sits behind the player, so reach grows by the camera distance.
        let reach = MELEE.reach + if rig.first_person { 0.0 } else { rig.dist };
        if d < reach && to.normalize_or_zero().dot(rig.forward) > cos && d < best {
            best = d;
            tgt.0 = Some(e);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn attack(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    look: Res<Look>,
    tgt: Res<MeleeTarget>,
    p: Res<PlayerState>,
    mut st: ResMut<MeleeState>,
    mut raiders: Query<(&mut Raider, &Transform)>,
    mut rigs: Query<&mut Rig>,
    mut hud: ResMut<Hud>,
    mut sfx: MessageWriter<Sfx>,
) {
    st.cooldown -= time.delta_secs();
    if !look.captured || !mouse.just_pressed(MouseButton::Left) || st.cooldown > 0.0 {
        return;
    }
    let Some(e) = tgt.0 else { return };
    let Ok((mut r, tf)) = raiders.get_mut(e) else { return };
    st.cooldown = MELEE.cooldown;
    if r.invuln > 0.0 {
        return;
    }
    let falling = !p.on_ground && p.vel.y < -0.5;
    let dmg = MELEE.damage * if falling { MELEE.crit_mult } else { 1.0 };
    r.hp -= dmg;
    r.invuln = MELEE.invuln;
    let mut dir = tf.translation - p.pos;
    dir.y = 0.0;
    let sprinting = Vec2::new(p.vel.x, p.vel.z).length() > 5.5;
    r.knock = dir.normalize_or_zero() * (MELEE.knockback + if sprinting { MELEE.sprint_knockback } else { 0.0 }) + Vec3::Y * 4.0;
    r.state = RState::Recover(0.45);
    if let Ok(mut rg) = rigs.get_mut(r.rig) {
        rg.want = "Hit_Chest";
    }
    hud.hitmarker = 0.25;
    if falling {
        hud.center = Some(("CRITICAL".into(), 0.6));
    }
    sfx.write(Sfx::new("wood", 0.8));
}

#[allow(clippy::too_many_arguments)]
fn think(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    mut p: ResMut<PlayerState>,
    mut commands: Commands,
    mut raiders: Query<(Entity, &mut Raider, &mut Transform)>,
    mut rigs: Query<&mut Rig>,
    mut st: ResMut<MeleeState>,
    mut out: MessageWriter<MashEvent>,
    mut sfx: MessageWriter<Sfx>,
) {
    let dt = time.delta_secs().min(0.05);
    let t = time.elapsed_secs();
    for (e, mut r, mut tf) in raiders.iter_mut() {
        r.invuln = (r.invuln - dt).max(0.0);
        if r.hp <= 0.0 {
            if let Ok(mut rg) = rigs.get_mut(r.rig) {
                rg.want = "Death01";
            }
            commands.entity(e).remove::<Raider>().insert(Fallen(2.5));
            st.felled += 1;
            out.write(MashEvent::new(Kind::Kill { airborne: !p.on_ground, headshot: false }, "Raider", 100));
            continue;
        }
        let to_p = p.pos - tf.translation;
        let flat = Vec3::new(to_p.x, 0.0, to_p.z);
        let dist = flat.length();
        let alert = dist < SIGHT && (to_p.y).abs() < 6.0;

        r.state = match r.state {
            RState::Windup(left) => {
                let left = left - dt;
                if left <= 0.0 {
                    // The swing lands if you're still in front of it.
                    let facing = Vec3::new(tf.rotation.mul_vec3(Vec3::Z).x, 0.0, tf.rotation.mul_vec3(Vec3::Z).z);
                    if dist < 2.5 && facing.normalize_or_zero().dot(flat.normalize_or_zero()) > 0.3 {
                        p.health -= RAIDER_DMG;
                        p.impulse += flat.normalize_or_zero() * 6.0 + Vec3::Y * 3.5;
                        sfx.write(Sfx::new("wood", 1.0));
                    }
                    RState::Recover(0.8)
                } else {
                    RState::Windup(left)
                }
            }
            RState::Recover(left) => {
                if left - dt <= 0.0 { if alert { RState::Chase } else { RState::Wander } } else { RState::Recover(left - dt) }
            }
            _ if alert && dist < 1.8 => RState::Windup(0.5),
            _ if alert => RState::Chase,
            _ => RState::Wander,
        };

        r.think -= dt;
        if matches!(r.state, RState::Wander) && r.think <= 0.0 {
            r.think = 3.0 + (tf.translation.x * 0.37 + t).sin().abs() * 3.0;
            r.goal = r.home + Vec3::new((t + tf.translation.z).sin(), 0.0, (t * 1.3 + tf.translation.x).cos()) * 7.0;
        }
        let (want, speed) = match r.state {
            RState::Chase => (flat.normalize_or_zero(), 3.9),
            RState::Wander => {
                let g = r.goal - tf.translation;
                let g = Vec3::new(g.x, 0.0, g.z);
                (if g.length() > 0.6 { g.normalize() } else { Vec3::ZERO }, 1.6)
            }
            _ => (Vec3::ZERO, 0.0),
        };
        r.vel = r.vel.lerp(want * speed, (dt * 6.0).min(1.0));
        let step = r.vel * dt + Vec3::new(r.knock.x, 0.0, r.knock.z) * dt;
        let decay = (1.0 - dt * 5.0).max(0.0);
        r.knock.x *= decay;
        r.knock.z *= decay;

        // Walk on the blocks: step up one block, never through a two-high wall.
        let cur = tf.translation;
        let feet = cur.y.floor() as i32;
        let next = cur + step;
        let ground_next = surface(&world.v, next.x.floor() as i32, next.z.floor() as i32, feet).map(|y| y + 1);
        let mut pos = cur;
        match ground_next {
            Some(g) if g <= feet + 1 => pos = Vec3::new(next.x, cur.y, next.z),
            _ => {
                // Blocked: slide sideways next think.
                r.goal = cur + Vec3::new(-step.z, 0.0, step.x).normalize_or_zero() * 4.0;
            }
        }
        let ground = surface(&world.v, pos.x.floor() as i32, pos.z.floor() as i32, pos.y.floor() as i32 + 1).map(|y| y as f32 + 1.0).unwrap_or(0.0);
        // knock.y doubles as vertical speed: knocked up, then gravity, then the ground.
        r.knock.y -= 20.0 * dt;
        pos.y += r.knock.y * dt;
        if pos.y <= ground {
            pos.y = ground;
            r.knock.y = 0.0;
        }
        tf.translation = pos;

        let face = if alert { flat } else { r.vel };
        if face.length() > 0.05 {
            tf.rotation = Quat::from_rotation_y(face.x.atan2(face.z));
        }
        if let Ok(mut rg) = rigs.get_mut(r.rig) {
            rg.want = match r.state {
                RState::Windup(_) => "Sword_Attack",
                RState::Recover(_) if r.invuln > 0.0 => "Hit_Chest",
                _ if r.vel.length() > 2.5 => "Jog_Fwd_Loop",
                _ if r.vel.length() > 0.3 => "Walk_Loop",
                _ => "Sword_Idle",
            };
            rg.speed = if matches!(r.state, RState::Windup(_)) { 1.6 } else { 1.0 };
        }
    }
}

fn fallen(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut Fallen)>) {
    for (e, mut f) in q.iter_mut() {
        f.0 -= time.delta_secs();
        if f.0 <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

fn hud(time: Res<Time>, mut p: ResMut<PlayerState>, mut st: ResMut<MeleeState>, tgt: Res<MeleeTarget>, mut hud: ResMut<Hud>) {
    // Slow natural regeneration once you've been out of a fight for a few seconds.
    if p.health < st.last_health {
        st.since_hurt = 0.0;
    }
    st.since_hurt += time.delta_secs();
    if st.since_hurt > 4.0 && p.health < 100.0 {
        p.health = (p.health + 2.5 * time.delta_secs()).min(100.0);
    }
    st.last_health = p.health;
    hud.bars.push(("health".into(), (p.health / 100.0).clamp(0.0, 1.0), if p.health < 30.0 { [230, 60, 50] } else { [235, 70, 80] }));
    hud.top.push(format!("Raiders felled: {}", st.felled));
    if tgt.0.is_some() {
        hud.crosshair = true;
    }
}
