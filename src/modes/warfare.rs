//! WARFARE mode: military-shooter gunplay. Aim down sights, recoil, bots
//! that patrol and flank, health regen, and killstreak rewards.
//! Every bullet is a real fast projectile (so bullet time and portals affect it).

use crate::core::camera::CamRig;
use crate::core::fx::{Explosion, FxAssets};
use crate::core::hotbar::{Hotbar, Item};
use crate::core::input::{Latch, Look};
use crate::core::modes::{ActiveModes, Mode, ModeToggled, mode_on};
use crate::core::phys::{G_NPC, G_PLAYER, G_WORLD};
use crate::core::player::{MoveSet, PlayerState};
use crate::core::score::{Kind, MashEvent, Score};
use crate::core::rig::Rig;
use crate::core::ui::Hud;
use crate::world::city::{LOT, block_center};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    Player,
    Bot,
}

#[derive(Component)]
pub struct Projectile {
    pub vel: Vec3,
    pub owner: Owner,
    pub damage: f32,
    pub life: f32,
    pub rocket: bool,
    /// Set by portals so a shot through a portal is credited.
    pub portaled: bool,
}

#[derive(Component)]
pub struct Bot {
    pub rig: Entity,
    pub hp: f32,
    pub goal: Vec3,
    pub fire_cd: f32,
    pub think: f32,
    pub vel: Vec3,
    pub seen_player: bool,
}

#[derive(Component)]
struct ViewModel;
/// A downed bot playing its death animation before it's removed.
#[derive(Component)]
struct Dying(f32);
#[derive(Component)]
struct ViewGun(Item);
#[derive(Component)]
struct CarePackage;

struct Weapon {
    dmg: f32,
    rpm: f32,
    auto: bool,
    mag: u32,
    reload: f32,
    hip_spread: f32,
    ads_spread: f32,
    kick: f32,
    speed: f32,
    rocket: bool,
}

fn weapon(i: Item) -> Weapon {
    match i {
        Item::Pistol => Weapon { dmg: 40.0, rpm: 400.0, auto: false, mag: 12, reload: 1.6, hip_spread: 2.5, ads_spread: 0.4, kick: 0.025, speed: 360.0, rocket: false },
        Item::Launcher => Weapon { dmg: 0.0, rpm: 60.0, auto: false, mag: 1, reload: 2.8, hip_spread: 1.0, ads_spread: 0.2, kick: 0.06, speed: 55.0, rocket: true },
        _ => Weapon { dmg: 30.0, rpm: 750.0, auto: true, mag: 30, reload: 2.2, hip_spread: 4.0, ads_spread: 0.35, kick: 0.012, speed: 420.0, rocket: false },
    }
}

#[derive(Resource)]
pub struct Arsenal {
    ammo: [u32; 3],
    reserve: [u32; 3],
    reloading: f32,
    cooldown: f32,
    recoil: f32,
    regen_wait: f32,
    pub streak_ready: [bool; 3],
    uav: f32,
    airstrike: Option<(Vec3, f32)>,
}

impl Default for Arsenal {
    fn default() -> Self {
        Self { ammo: [30, 12, 1], reserve: [180, 60, 6], reloading: 0.0, cooldown: 0.0, recoil: 0.0, regen_wait: 0.0, streak_ready: [false; 3], uav: 0.0, airstrike: None }
    }
}

fn slot(i: Item) -> Option<usize> {
    match i {
        Item::Rifle => Some(0),
        Item::Pistol => Some(1),
        Item::Launcher => Some(2),
        _ => None,
    }
}

#[derive(Resource)]
struct BotAssets {
    uniform: Handle<StandardMaterial>,
    mesh: Handle<Mesh>,
    body: Handle<StandardMaterial>,
    head: Handle<StandardMaterial>,
    gun: Handle<StandardMaterial>,
}

const MAX_BOTS: usize = 10;
/// Orientation/scale that put the gun models' barrels along +Z in metres.
const GUN_YAW: f32 = -std::f32::consts::FRAC_PI_2;
const GUN_SCALE: f32 = 1.0;

pub struct WarfarePlugin;

impl Plugin for WarfarePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Arsenal>()
            .add_systems(Startup, setup)
            .add_systems(FixedUpdate, (fire, bots_think).in_set(MoveSet::Abilities).run_if(mode_on(Mode::Warfare)))
            .add_systems(FixedUpdate, fly_projectiles.in_set(MoveSet::Post))
            .add_systems(Update, (bodies, toggle, spawn_bots.run_if(mode_on(Mode::Warfare)), streaks.run_if(mode_on(Mode::Warfare)), explosion_damage, care_pickup, hud, view_model, hide_gun_extras));
    }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>, assets: Res<AssetServer>) {
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let gunmat = mats.add(StandardMaterial { base_color: Color::srgb(0.12, 0.12, 0.14), metallic: 0.6, ..default() });
    commands.insert_resource(BotAssets {
        uniform: mats.add(StandardMaterial { base_color: Color::srgb(0.23, 0.25, 0.17), perceptual_roughness: 0.95, ..default() }),
        mesh: mesh.clone(),
        body: mats.add(StandardMaterial { base_color: Color::srgb(0.55, 0.18, 0.16), ..default() }),
        head: mats.add(StandardMaterial { base_color: Color::srgb(0.25, 0.22, 0.2), ..default() }),
        gun: gunmat.clone(),
    });
    // First-person guns (CC0 models), positioned every frame.
    let ak = assets.load(GltfAssetLabel::Scene(0).from_asset("models/ak.gltf"));
    let pistol = assets.load(GltfAssetLabel::Scene(0).from_asset("models/pistol.gltf"));
    let _ = gunmat;
    commands.spawn((ViewModel, Transform::default(), Visibility::Hidden)).with_children(|c| {
        c.spawn((ViewGun(Item::Rifle), WorldAssetRoot(ak.clone()), Transform::from_rotation(Quat::from_rotation_y(GUN_YAW)).with_scale(Vec3::splat(GUN_SCALE)), Visibility::Inherited));
        c.spawn((ViewGun(Item::Launcher), WorldAssetRoot(ak), Transform::from_rotation(Quat::from_rotation_y(GUN_YAW)).with_scale(Vec3::splat(GUN_SCALE * 1.2)), Visibility::Hidden));
        c.spawn((ViewGun(Item::Pistol), WorldAssetRoot(pistol), Transform::from_rotation(Quat::from_rotation_y(GUN_YAW)).with_scale(Vec3::splat(GUN_SCALE)), Visibility::Hidden));
    });
}

fn bodies(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut Dying)>) {
    for (e, mut d) in q.iter_mut() {
        d.0 -= time.delta_secs();
        if d.0 <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

fn toggle(mut t: MessageReader<ModeToggled>, mut commands: Commands, bots: Query<Entity, With<Bot>>, shots: Query<Entity, With<Projectile>>) {
    for ev in t.read() {
        if ev.mode == Mode::Warfare && !ev.on {
            for e in bots.iter().chain(shots.iter()) {
                commands.entity(e).despawn();
            }
        }
    }
}

fn aim_point(rig: &CamRig, ctx: &RapierContext, body: Entity) -> Vec3 {
    let f = QueryFilter::new().exclude_sensors().exclude_rigid_body(body);
    match ctx.cast_ray(rig.eye, rig.forward, 600.0, true, f) {
        Some((_, t)) => rig.eye + rig.forward * t,
        None => rig.eye + rig.forward * 600.0,
    }
}

#[allow(clippy::too_many_arguments)]
fn fire(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    latch: Res<Latch>,
    mut look: ResMut<Look>,
    bar: Res<Hotbar>,
    mut rig: ResMut<CamRig>,
    mut ars: ResMut<Arsenal>,
    mut p: ResMut<PlayerState>,
    rapier: ReadRapierContext,
    fx: Res<FxAssets>,
    mut commands: Commands,
    mut sfx: MessageWriter<crate::core::audio::Sfx>,
) -> Result {
    let dt = time.delta_secs();
    ars.cooldown -= dt;
    ars.recoil *= 1.0 - (dt * 8.0).min(1.0);
    // Health regen after a few seconds out of fire.
    ars.regen_wait -= dt;
    if ars.regen_wait <= 0.0 && p.health < 100.0 {
        p.health = (p.health + 35.0 * dt).min(100.0);
    }
    let Some(s) = slot(bar.item) else { return Ok(()) };
    let w = weapon(bar.item);
    if ars.reloading > 0.0 {
        ars.reloading -= dt;
        if ars.reloading <= 0.0 {
            let need = w.mag - ars.ammo[s];
            let take = need.min(ars.reserve[s]);
            ars.ammo[s] += take;
            ars.reserve[s] -= take;
        }
        return Ok(());
    }
    if latch.key(KeyCode::KeyR) && ars.ammo[s] < w.mag && ars.reserve[s] > 0 {
        ars.reloading = w.reload;
        sfx.write(crate::core::audio::Sfx::new(if bar.item == Item::Pistol { "pistol_reload" } else { "rifle_reload" }, 0.6));
        return Ok(());
    }
    let trigger = if w.auto { mouse.pressed(MouseButton::Left) } else { latch.mouse(MouseButton::Left) };
    if !trigger || ars.cooldown > 0.0 || !look.captured {
        return Ok(());
    }
    if ars.ammo[s] == 0 {
        if ars.reserve[s] > 0 {
            ars.reloading = w.reload;
        }
        return Ok(());
    }
    ars.ammo[s] -= 1;
    ars.cooldown = 60.0 / w.rpm;
    let shot = match bar.item {
        Item::Pistol => "pistol",
        Item::Launcher => "explosion",
        _ => "rifle",
    };
    sfx.write(crate::core::audio::Sfx { name: shot, volume: if w.rocket { 0.35 } else { 0.55 }, speed: 0.97 + (time.elapsed_secs() * 37.0).sin().abs() * 0.06 });
    let ctx = rapier.single()?;
    let target = aim_point(&rig, &ctx, p.body);
    // Spread tightens when aiming and loosens with movement.
    let spread = (w.hip_spread + (w.ads_spread - w.hip_spread) * rig.ads + p.vel.length() * 0.25 * (1.0 - rig.ads)).to_radians();
    let seed = time.elapsed_secs() * 1000.0;
    let jitter = Vec3::new((seed * 12.9898).sin(), (seed * 78.233).sin(), (seed * 37.719).sin()) * spread;
    let right = rig.forward.cross(Vec3::Y).normalize_or_zero();
    let muzzle = if rig.first_person { rig.eye + rig.forward * 0.6 + right * 0.18 - Vec3::Y * 0.12 } else { p.pos + Vec3::Y * 1.45 + right * 0.3 + rig.forward * 0.5 };
    let dir = ((target - muzzle).normalize_or_zero() + jitter * (target - muzzle).length().min(1.0)).normalize_or_zero();
    commands.spawn((
        Projectile { vel: dir * w.speed + p.vel, owner: Owner::Player, damage: w.dmg, life: 2.5, rocket: w.rocket, portaled: false },
        Mesh3d(fx.cube.clone()),
        MeshMaterial3d(if w.rocket { fx.fire.clone() } else { fx.tracer.clone() }),
        Transform::from_translation(muzzle).with_scale(if w.rocket { Vec3::splat(0.25) } else { Vec3::new(0.03, 0.03, 0.9) }),
    ));
    // Muzzle flash: a brief hot light that throws light on nearby walls.
    commands.spawn((
        crate::core::fx::Flash { age: 0.0, life: 0.05, grow: 0.0 },
        PointLight { intensity: if crate::core::look::HIGH_QUALITY { 4.0e6 } else { 2.0e5 }, range: 10.0, color: Color::srgb(1.0, 0.75, 0.4), shadow_maps_enabled: false, ..default() },
        Transform::from_translation(muzzle + dir * 0.3),
    ));
    ars.recoil += w.kick;
    look.pitch = (look.pitch + w.kick * (1.0 - 0.5 * rig.ads)).min(1.35);
    look.yaw += (seed * 3.1).sin() * w.kick * 0.3;
    rig.shake = rig.shake.max(if w.rocket { 0.25 } else { 0.04 });
    Ok(())
}

/// Swept projectiles: raycast each step so nothing tunnels at 400 m/s.
#[allow(clippy::too_many_arguments)]
fn fly_projectiles(
    time: Res<Time>,
    mut commands: Commands,
    rapier: ReadRapierContext,
    mut shots: Query<(Entity, &mut Projectile, &mut Transform)>,
    mut bots: Query<(&mut Bot, &Transform), Without<Projectile>>,
    mut rigs: Query<&mut Rig>,
    mut p: ResMut<PlayerState>,
    mut hud: ResMut<Hud>,
    mut out: MessageWriter<MashEvent>,
    mut boom: MessageWriter<Explosion>,
    mut ars: ResMut<Arsenal>,
    mut rig: ResMut<CamRig>,
) -> Result {
    let ctx = rapier.single()?;
    let dt = time.delta_secs();
    for (e, mut pr, mut tf) in shots.iter_mut() {
        pr.life -= dt;
        if pr.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        if pr.rocket {
            pr.vel.y -= 2.0 * dt;
        }
        let step = pr.vel * dt;
        let len = step.length();
        let groups = match pr.owner {
            Owner::Player => CollisionGroups::new(Group::ALL, G_WORLD | G_NPC | crate::core::phys::G_DYNAMIC),
            Owner::Bot => CollisionGroups::new(Group::ALL, G_WORLD | G_PLAYER | crate::core::phys::G_DYNAMIC),
        };
        let filt = QueryFilter::new().exclude_sensors().groups(groups);
        let hit = ctx.cast_ray(tf.translation, step / len.max(1e-6), len, true, filt);
        if let Some((ent, toi)) = hit {
            let at = tf.translation + step / len.max(1e-6) * toi;
            commands.entity(e).despawn();
            if pr.rocket {
                boom.write(Explosion { pos: at, radius: 4.0, power: 1.4 });
                continue;
            }
            if ent == p.body && pr.owner == Owner::Bot {
                p.health -= pr.damage;
                ars.regen_wait = 4.0;
                rig.shake = rig.shake.max(0.12);
                continue;
            }
            if let Ok((mut bot, btf)) = bots.get_mut(ent) {
                let head = at.y > btf.translation.y + 0.55;
                let dmg = if head { pr.damage * 2.0 } else { pr.damage };
                bot.hp -= dmg;
                bot.seen_player = true;
                hud.hitmarker = 0.18;
                if bot.hp <= 0.0 {
                    commands.entity(ent).remove::<(Bot, Collider)>().insert(Dying(3.0));
                    if let Ok(mut r) = rigs.get_mut(bot.rig) {
                        r.want = "Death01";
                    }
                    let label = if head { "Headshot" } else { "Kill" };
                    out.write(MashEvent::new(Kind::Kill { airborne: !p.on_ground, headshot: head }, label, if head { 150 } else { 100 }));
                    if pr.portaled {
                        out.write(MashEvent::new(Kind::PortalTravel, "", 0));
                    }
                }
            }
            continue;
        }
        tf.translation += step;
        if !pr.rocket {
            tf.rotation = Quat::from_rotation_arc(Vec3::Z, pr.vel.normalize_or_zero());
        }
    }
    Ok(())
}

fn spawn_bots(mut commands: Commands, assets: Res<BotAssets>, bots: Query<&Bot>, time: Res<Time>, mut timer: Local<f32>) {
    *timer -= time.delta_secs();
    if bots.iter().count() >= MAX_BOTS || *timer > 0.0 {
        return;
    }
    *timer = 2.5;
    let c = block_center(0, -1);
    let k = (time.elapsed_secs() * 7.31).sin() * 0.5 + 0.5;
    let k2 = (time.elapsed_secs() * 3.17).cos() * 0.5 + 0.5;
    let pos = c + Vec3::new((k - 0.5) * 2.0 * (LOT - 4.0), 0.0, (k2 - 0.5) * 2.0 * (LOT - 4.0));
    let rig = commands.spawn((Rig::person(crate::core::rig::SOLDIER, Some(crate::core::rig::HAIR_BUZZED)), Transform::from_xyz(0.0, -0.9, 0.0), Visibility::default())).id();
    commands
        .spawn((
            Bot { rig, hp: 100.0, goal: pos, fire_cd: 1.0, think: 0.0, vel: Vec3::ZERO, seen_player: false },
            Transform::from_translation(pos + Vec3::Y * 0.9),
            Visibility::default(),
            RigidBody::KinematicPositionBased,
            Collider::capsule_y(0.5, 0.35),
            CollisionGroups::new(G_NPC, Group::ALL),
        ))
        .add_child(rig);
}

/// Patrol, spot, take a firing position, shoot in bursts. Bots only see
/// what has line of sight, so cover (and voxel walls) matter.
#[allow(clippy::too_many_arguments)]
fn bots_think(
    time: Res<Time>,
    rapier: ReadRapierContext,
    p: Res<PlayerState>,
    fx: Res<FxAssets>,
    mut commands: Commands,
    mut bots: Query<(Entity, &mut Bot, &mut Transform)>,
    mut rigs: Query<&mut Rig>,
    mut sfx: MessageWriter<crate::core::audio::Sfx>,
) -> Result {
    let ctx = rapier.single()?;
    let dt = time.delta_secs();
    let player_head = p.pos + Vec3::Y * 1.5;
    for (e, mut b, mut tf) in bots.iter_mut() {
        let eye = tf.translation + Vec3::Y * 0.7;
        let to_p = player_head - eye;
        let dist = to_p.length();
        b.think -= dt;
        let filt = QueryFilter::new().exclude_sensors().exclude_rigid_body(e).groups(CollisionGroups::new(Group::ALL, G_WORLD | G_PLAYER));
        let sees = dist < 70.0 && ctx.cast_ray(eye, to_p / dist, dist, true, filt).is_some_and(|(hit, _)| hit == p.body);
        if sees {
            b.seen_player = true;
        }
        if b.think <= 0.0 {
            b.think = 1.2 + (tf.translation.x * 0.37).sin().abs();
            b.goal = if sees && dist > 18.0 {
                // Close the distance, offset to flank.
                p.pos + Vec3::new((tf.translation.z * 0.7).sin(), 0.0, (tf.translation.x * 0.7).cos()) * 8.0
            } else if sees {
                tf.translation + Vec3::new((time.elapsed_secs() + tf.translation.x).sin(), 0.0, (time.elapsed_secs() * 1.3).cos()) * 4.0
            } else if b.seen_player {
                p.pos
            } else {
                tf.translation + Vec3::new((tf.translation.z * 1.7 + time.elapsed_secs()).sin(), 0.0, (tf.translation.x * 1.3).cos()) * 10.0
            };
        }
        // Move toward goal; step around obstacles by probing.
        let mut to_goal = b.goal - tf.translation;
        to_goal.y = 0.0;
        let speed = if sees { 3.2 } else { 2.2 };
        let mut want = if to_goal.length() > 0.8 { to_goal.normalize() * speed } else { Vec3::ZERO };
        if want != Vec3::ZERO {
            let fwd = want.normalize();
            let probe = QueryFilter::new().exclude_sensors().exclude_rigid_body(e).groups(CollisionGroups::new(Group::ALL, G_WORLD));
            if ctx.cast_ray(tf.translation, fwd, 1.0, true, probe).is_some() {
                let side = Vec3::new(fwd.z, 0.0, -fwd.x);
                want = side * speed;
            }
        }
        b.vel = b.vel.lerp(want, (dt * 5.0).min(1.0));
        let next = tf.translation + b.vel * dt;
        let probe = QueryFilter::new().exclude_sensors().exclude_rigid_body(e).groups(CollisionGroups::new(Group::ALL, G_WORLD));
        let ground = ctx.cast_ray(next + Vec3::Y * 1.0, -Vec3::Y, 6.0, true, probe).map(|(_, t)| next.y + 1.0 - t);
        tf.translation = next;
        if let Some(gy) = ground {
            tf.translation.y = gy + 0.9;
        }
        let face = if sees { to_p } else { b.vel };
        if face.length() > 0.1 {
            tf.rotation = Quat::from_rotation_y(face.x.atan2(face.z));
        }
        if let Ok(mut r) = rigs.get_mut(b.rig) {
            let moving = b.vel.length() > 0.5;
            r.want = if sees && !moving { "Pistol_Aim_Neutral" } else if moving { if sees { "Jog_Fwd_Loop" } else { "Walk_Loop" } } else { "Pistol_Idle_Loop" };
            r.speed = if moving { (b.vel.length() / 3.0).clamp(0.7, 1.4) } else { 1.0 };
        }
        // Burst fire at the player.
        b.fire_cd -= dt;
        if sees && b.fire_cd <= 0.0 && dist < 60.0 {
            b.fire_cd = 0.35 + (dist / 60.0) * 0.6;
            let inacc = 0.035 + dist * 0.0012 + p.vel.length() * 0.004;
            let t = time.elapsed_secs() + tf.translation.x;
            let dir = (to_p / dist + Vec3::new((t * 17.0).sin(), (t * 23.0).sin(), (t * 29.0).cos()) * inacc).normalize();
            let muzzle = eye + dir * 0.6;
            sfx.write(crate::core::audio::Sfx::at("rifle", 0.5, dist));
            commands.spawn((
                Projectile { vel: dir * 220.0, owner: Owner::Bot, damage: 9.0, life: 1.5, rocket: false, portaled: false },
                Mesh3d(fx.cube.clone()),
                MeshMaterial3d(fx.tracer.clone()),
                Transform::from_translation(muzzle).with_scale(Vec3::new(0.03, 0.03, 0.7)),
            ));
        }
    }
    Ok(())
}

fn explosion_damage(mut ev: MessageReader<Explosion>, mut commands: Commands, bots: Query<(Entity, &Bot, &Transform)>, mut rigs: Query<&mut Rig>, p: Res<PlayerState>, mut out: MessageWriter<MashEvent>) {
    for e in ev.read() {
        for (b, bot, tf) in bots.iter() {
            if tf.translation.distance(e.pos) < e.radius * 1.3 {
                commands.entity(b).remove::<(Bot, Collider)>().insert(Dying(3.0));
                if let Ok(mut r) = rigs.get_mut(bot.rig) {
                    r.want = "Death01";
                }
                out.write(MashEvent::new(Kind::Kill { airborne: !p.on_ground, headshot: false }, "Blast Kill", 100));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn streaks(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut score: ResMut<Score>,
    mut ars: ResMut<Arsenal>,
    rig: Res<CamRig>,
    p: Res<PlayerState>,
    rapier: ReadRapierContext,
    mut hud: ResMut<Hud>,
    mut commands: Commands,
    assets: Res<BotAssets>,
    mut boom: MessageWriter<Explosion>,
    mut last: Local<u32>,
) -> Result {
    if p.health <= 0.0 {
        score.kill_streak = 0;
    }
    let s = score.kill_streak;
    if s != *last {
        for (k, need, name) in [(0, 3, "UAV"), (1, 5, "CARE PACKAGE"), (2, 7, "AIRSTRIKE")] {
            if s == need {
                ars.streak_ready[k] = true;
                hud.banner = Some((format!("{name} READY  [{}]", 5 + k), 2.5));
            }
        }
        *last = s;
    }
    ars.uav = (ars.uav - time.delta_secs()).max(0.0);
    if keys.just_pressed(KeyCode::Digit5) && ars.streak_ready[0] {
        ars.streak_ready[0] = false;
        ars.uav = 30.0;
        hud.banner = Some(("UAV ONLINE".into(), 2.0));
    }
    if keys.just_pressed(KeyCode::Digit6) && ars.streak_ready[1] {
        ars.streak_ready[1] = false;
        let at = p.pos + rig.forward.with_y(0.0).normalize_or_zero() * 6.0 + Vec3::Y * 40.0;
        commands.spawn((
            CarePackage,
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.head.clone()),
            Transform::from_translation(at).with_scale(Vec3::new(1.2, 0.8, 1.2)),
            RigidBody::Dynamic,
            Collider::cuboid(0.5, 0.5, 0.5),
            Velocity::default(),
            CollisionGroups::new(crate::core::phys::G_DYNAMIC, Group::ALL),
        ));
        hud.banner = Some(("CARE PACKAGE INBOUND".into(), 2.0));
    }
    if keys.just_pressed(KeyCode::Digit7) && ars.streak_ready[2] {
        let ctx = rapier.single()?;
        ars.streak_ready[2] = false;
        let target = aim_point(&rig, &ctx, p.body);
        ars.airstrike = Some((target, 2.0));
        hud.banner = Some(("AIRSTRIKE INBOUND".into(), 2.0));
    }
    if let Some((t, mut timer)) = ars.airstrike {
        timer -= time.delta_secs();
        if timer <= 0.0 {
            let dir = rig.forward.with_y(0.0).normalize_or_zero();
            for k in -2..=2 {
                boom.write(Explosion { pos: t + dir * (k as f32 * 6.0), radius: 5.0, power: 1.5 });
            }
            ars.airstrike = None;
        } else {
            ars.airstrike = Some((t, timer));
        }
    }
    Ok(())
}

fn care_pickup(mut commands: Commands, q: Query<(Entity, &Transform), With<CarePackage>>, mut player: ResMut<PlayerState>, mut ars: ResMut<Arsenal>, mut out: MessageWriter<MashEvent>) {
    for (e, tf) in q.iter() {
        if tf.translation.distance(player.pos + Vec3::Y) < 2.2 {
            commands.entity(e).despawn();
            ars.reserve = [300, 90, 10];
            ars.streak_ready = [true, false, true];
            player.health = 100.0;
            out.write(MashEvent::new(Kind::Streak, "Care Package", 250));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn hud(
    modes: Res<ActiveModes>,
    ars: Res<Arsenal>,
    bar: Res<Hotbar>,
    p: Res<PlayerState>,
    score: Res<Score>,
    bots: Query<&Transform, With<Bot>>,
    mut hud: ResMut<Hud>,
) {
    if !modes.on(Mode::Warfare) {
        return;
    }
    if let Some(s) = slot(bar.item) {
        hud.crosshair = true;
        let r = if ars.reloading > 0.0 { "  RELOADING".to_string() } else { String::new() };
        hud.top.push(format!("{} / {}{}", ars.ammo[s], ars.reserve[s], r));
    }
    hud.bars.push(("health".into(), p.health / 100.0, if p.health < 35.0 { [230, 60, 50] } else { [235, 235, 235] }));
    hud.top.push(format!("Kills {}  Streak {}", score.kills, score.kill_streak));
    let ready: Vec<&str> = ["5 UAV", "6 Care Package", "7 Airstrike"].iter().zip(ars.streak_ready).filter(|(_, r)| *r).map(|(n, _)| *n).collect();
    if !ready.is_empty() {
        hud.top.push(format!("Ready: {}", ready.join("  ")));
    }
    if ars.uav > 0.0 {
        // UAV: list enemy bearings relative to the player.
        let mut marks: Vec<String> = bots
            .iter()
            .map(|t| {
                let d = t.translation - p.pos;
                let ang = (d.x.atan2(d.z) - p.facing).to_degrees().rem_euclid(360.0);
                format!("{:>3.0}°/{:.0}m", ang, d.length())
            })
            .collect();
        marks.truncate(6);
        hud.top.push(format!("UAV: {}", marks.join("  ")));
    }
}

/// The gun files also contain loose bullets, boxes and spare magazines as
/// top-level nodes; keep only the weapon's own node tree visible.
fn hide_gun_extras(guns: Query<Entity, With<ViewGun>>, children: Query<&Children>, names: Query<&Name>, mut commands: Commands, mut done: Local<bool>) {
    if *done {
        return;
    }
    let mut found = false;
    for g in guns.iter() {
        // The scene's top-level nodes are the gun's grandchildren or children.
        let mut stack: Vec<Entity> = children.get(g).map(|c| c.iter().collect()).unwrap_or_default();
        while let Some(e) = stack.pop() {
            match names.get(e).map(|n| n.as_str()) {
                Ok("AK") | Ok("Pistol_Frame") => found = true,
                Ok("Magazine") | Ok("Bullet") | Ok("BulletBox") | Ok("BulletFired") | Ok("Pistol_Magazine") => {
                    commands.entity(e).insert(Visibility::Hidden);
                }
                _ => {
                    if let Ok(c) = children.get(e) {
                        stack.extend(c.iter());
                    }
                }
            }
        }
    }
    *done = found;
}

#[allow(clippy::type_complexity)]
fn view_model(
    rig: Res<CamRig>,
    bar: Res<Hotbar>,
    modes: Res<ActiveModes>,
    ars: Res<Arsenal>,
    time: Res<Time>,
    mut q: Query<(&mut Transform, &mut Visibility), (With<ViewModel>, Without<ViewGun>)>,
    mut guns: Query<(&ViewGun, &mut Visibility), Without<ViewModel>>,
) {
    let Ok((mut tf, mut vis)) = q.single_mut() else { return };
    for (g, mut v) in guns.iter_mut() {
        *v = if g.0 == bar.item { Visibility::Inherited } else { Visibility::Hidden };
    }
    let show = rig.first_person && modes.on(Mode::Warfare) && slot(bar.item).is_some();
    *vis = if show { Visibility::Visible } else { Visibility::Hidden };
    if !show {
        return;
    }
    let right = rig.forward.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(rig.forward);
    let bob = (time.elapsed_secs() * 7.0).sin() * 0.01;
    let hip = rig.eye + rig.forward * 0.35 + right * 0.2 - up * (0.18 + bob);
    // ADS: rear sight (top of receiver, ~0.13 m up) on the eye line, stock
    // tucked behind the camera.
    let sight_drop = if bar.item == Item::Pistol { 0.085 } else { 0.128 };
    let ads = rig.eye + rig.forward * 0.22 - up * sight_drop;
    let reload_dip = if ars.reloading > 0.0 { 0.2 } else { 0.0 };
    tf.translation = hip.lerp(ads, rig.ads) - up * reload_dip - rig.forward * ars.recoil * 1.5;
    tf.rotation = Transform::default().looking_to(-rig.forward, up).rotation;
    tf.scale = Vec3::ONE;
}
