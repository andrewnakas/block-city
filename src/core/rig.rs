//! Rigged human characters (CC0 Quaternius UAL mannequin + its 43 clips).
//! Any entity with a `Rig` gets the model as a child; gameplay just sets
//! `rig.want` to a clip name and the rig cross-fades to it.

use bevy::animation::RepeatAnimation;
use bevy::prelude::*;
use std::collections::HashMap;
use std::time::Duration;

/// Animation library (clips) — all bodies share its skeleton.
pub const ANIMS: &str = "models/UAL1_Standard.glb";
pub const SKATER: &str = "models/people/skater.gltf";
pub const SOLDIER: &str = "models/people/soldier.gltf";
pub const HAIR_PARTED: &str = "models/people/Hair_SimpleParted.gltf";
pub const HAIR_BUZZED: &str = "models/people/Hair_Buzzed.gltf";

/// Head bone inverse bind matrix of the base body (column major): seats the
/// "origin at 0" hair meshes on the head bone.
const HEAD_IBM: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 0.99958, -0.02897, 0.0, 0.0, 0.02897, 0.99958, 0.0, 0.0, -1.59862, 0.06373, 1.0];

/// Clips the game uses (names as authored in the asset).
pub const CLIPS: &[&str] = &[
    "Idle_Loop",
    "Walk_Loop",
    "Jog_Fwd_Loop",
    "Sprint_Loop",
    "Jump_Start",
    "Jump_Loop",
    "Jump_Land",
    "Crouch_Idle_Loop",
    "Crouch_Fwd_Loop",
    "Push_Loop",
    "Roll",
    "Death01",
    "Hit_Chest",
    "Pistol_Idle_Loop",
    "Pistol_Aim_Neutral",
    "Pistol_Shoot",
    "Pistol_Reload",
    "Punch_Cross",
    "Driving_Loop",
    "Swim_Idle_Loop",
    "Punch_Jab",
    "Sword_Attack",
    "Sword_Idle",
    "Hit_Head",
    "Fixing_Kneeling",
];

#[derive(Component)]
pub struct Rig {
    pub want: &'static str,
    pub speed: f32,
    pub model: &'static str,
    pub hair: Option<&'static str>,
    /// Replaces the mannequin's test material (clothing / uniform look).
    pub tint: Option<Handle<StandardMaterial>>,
    current: &'static str,
    player: Option<Entity>,
    spawned: bool,
    hair_done: bool,
}

impl Rig {
    pub fn tinted(m: Handle<StandardMaterial>) -> Self {
        Self { tint: Some(m), ..default() }
    }
    pub fn person(model: &'static str, hair: Option<&'static str>) -> Self {
        Self { model, hair, ..default() }
    }
    pub fn with_tint(mut self, m: Handle<StandardMaterial>) -> Self {
        self.tint = Some(m);
        self
    }
}

impl Default for Rig {
    fn default() -> Self {
        Self { want: "Idle_Loop", speed: 1.0, model: SKATER, hair: None, tint: None, current: "", player: None, spawned: false, hair_done: false }
    }
}

#[derive(Resource)]
struct RigAssets {
    gltf: Handle<Gltf>,
    models: HashMap<&'static str, Handle<Gltf>>,
    graph: Option<Handle<AnimationGraph>>,
    nodes: HashMap<&'static str, AnimationNodeIndex>,
}

pub struct RigPlugin;

impl Plugin for RigPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, |mut commands: Commands, assets: Res<AssetServer>| {
            let models = [SKATER, SOLDIER].into_iter().map(|m| (m, assets.load(m))).collect();
            commands.insert_resource(RigAssets { gltf: assets.load(ANIMS), models, graph: None, nodes: HashMap::new() });
        })
        .add_systems(Update, (build_graph, spawn_models, attach_players, attach_hair, retint, drive).chain());
    }
}

fn build_graph(mut ra: ResMut<RigAssets>, gltfs: Res<Assets<Gltf>>, mut graphs: ResMut<Assets<AnimationGraph>>) {
    if ra.graph.is_some() {
        return;
    }
    let Some(g) = gltfs.get(&ra.gltf) else { return };
    let mut graph = AnimationGraph::new();
    let mut nodes = HashMap::new();
    for name in CLIPS {
        if let Some(clip) = g.named_animations.get(*name) {
            nodes.insert(*name, graph.add_clip(clip.clone(), 1.0, graph.root));
        } else {
            warn!("rig clip {name} missing");
        }
    }
    ra.nodes = nodes;
    ra.graph = Some(graphs.add(graph));
}

fn spawn_models(mut commands: Commands, ra: Res<RigAssets>, gltfs: Res<Assets<Gltf>>, mut rigs: Query<(Entity, &mut Rig)>) {
    if ra.graph.is_none() {
        return;
    }
    for (e, mut rig) in rigs.iter_mut() {
        if rig.spawned {
            continue;
        }
        let Some(g) = ra.models.get(rig.model).and_then(|h| gltfs.get(h)) else { continue };
        let Some(scene) = g.default_scene.clone() else { continue };
        rig.spawned = true;
        commands.entity(e).with_children(|c| {
            c.spawn((WorldAssetRoot(scene.clone()), Transform::default(), Visibility::Inherited));
        });
    }
}

/// Animation players appear inside the spawned scene; hook each to its rig.
fn attach_players(
    mut commands: Commands,
    ra: Res<RigAssets>,
    players: Query<Entity, (With<AnimationPlayer>, Without<AnimationGraphHandle>)>,
    parents: Query<&ChildOf>,
    mut rigs: Query<&mut Rig>,
) {
    let Some(graph) = ra.graph.clone() else { return };
    for p in players.iter() {
        let mut cur = p;
        while let Ok(parent) = parents.get(cur) {
            cur = parent.parent();
            if let Ok(mut rig) = rigs.get_mut(cur) {
                rig.player = Some(p);
                rig.current = "";
                commands.entity(p).insert((AnimationGraphHandle(graph.clone()), AnimationTransitions::new()));
                break;
            }
        }
    }
}

/// Hair is a separate unskinned mesh: parent it to the body's head bone.
fn attach_hair(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut rigs: Query<(Entity, &mut Rig)>,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    for (e, mut rig) in rigs.iter_mut() {
        if rig.hair_done || !rig.spawned {
            continue;
        }
        let Some(hair) = rig.hair else {
            rig.hair_done = true;
            continue;
        };
        let head = children.iter_descendants(e).find(|d| names.get(*d).is_ok_and(|n| n.as_str() == "Head"));
        if let Some(head) = head {
            let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(hair));
            let tf = Transform::from_matrix(Mat4::from_cols_array(&HEAD_IBM));
            commands.entity(head).with_children(|c| {
                c.spawn((WorldAssetRoot(scene), tf, Visibility::Inherited));
            });
            rig.hair_done = true;
        }
    }
}

#[derive(Component)]
struct Tinted;

/// Swap the mannequin's material on every mesh under a tinted rig.
fn retint(
    mut commands: Commands,
    meshes: Query<(Entity, &MeshMaterial3d<StandardMaterial>), Without<Tinted>>,
    parents: Query<&ChildOf>,
    rigs: Query<&Rig>,
) {
    for (e, _) in meshes.iter() {
        let mut cur = e;
        while let Ok(p) = parents.get(cur) {
            cur = p.parent();
            if let Ok(rig) = rigs.get(cur) {
                if let Some(t) = &rig.tint {
                    commands.entity(e).insert((MeshMaterial3d(t.clone()), Tinted));
                } else {
                    commands.entity(e).insert(Tinted);
                }
                break;
            }
        }
    }
}

fn drive(ra: Res<RigAssets>, mut rigs: Query<&mut Rig>, mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>) {
    for mut rig in rigs.iter_mut() {
        let Some(pe) = rig.player else { continue };
        let Ok((mut player, mut tr)) = players.get_mut(pe) else { continue };
        if rig.want != rig.current {
            let Some(&node) = ra.nodes.get(rig.want) else { continue };
            let looping = rig.want.ends_with("_Loop");
            let fade = if rig.current.is_empty() { 0 } else { 180 };
            let anim = tr.play(&mut player, node, Duration::from_millis(fade));
            if looping {
                anim.repeat();
            } else {
                anim.set_repeat(RepeatAnimation::Never);
            }
            rig.current = rig.want;
        }
        if let Some(&node) = ra.nodes.get(rig.current)
            && let Some(a) = player.animation_mut(node)
        {
            a.set_speed(rig.speed);
        }
    }
}
