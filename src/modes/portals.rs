//! PORTALS mode: two linked portals on any wall, floor or voxel. Each portal
//! shows the other's view (render-to-texture with an oblique near plane so
//! the wall behind the exit is clipped), and anything that crosses one -
//! you, your board, your car, bullets - comes out the other with its
//! momentum rotated: speedy thing goes in, speedy thing comes out.

use crate::core::camera::{CamRig, MainCam};
use crate::core::hotbar::{Hotbar, Item};
use crate::core::input::{Latch, Look};
use crate::core::modes::{ActiveModes, Mode, ModeToggled, mode_on};
use crate::core::phys::{G_WORLD, Hole, Holes};
use crate::core::player::{JumpSim, Loco, MoveSet, PlayerState, SkateSim, WalkSim, enter_loco};
use crate::core::score::{Kind, MashEvent};
use crate::core::ui::Hud;
use crate::modes::streets::Car;
use crate::modes::swing::Web;
use crate::modes::warfare::Projectile;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraProjection, RenderTarget, SubCameraView};
use bevy::math::{Affine3A, Vec3A};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, TextureFormat};
use bevy::shader::ShaderRef;
use bevy_rapier3d::prelude::*;

pub const HALF: Vec2 = Vec2::new(0.75, 1.25);
const COLORS: [[f32; 3]; 2] = [[0.15, 0.55, 1.0], [1.0, 0.55, 0.1]];
const RT_W: u32 = 1280;
const RT_H: u32 = 720;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub center: Vec3,
    pub normal: Vec3,
    pub up: Vec3,
}

impl Place {
    pub fn right(&self) -> Vec3 {
        self.up.cross(self.normal)
    }
    pub fn rotation(&self) -> Quat {
        Quat::from_mat3(&Mat3::from_cols(self.right(), self.up, self.normal))
    }
    pub fn affine(&self) -> Affine3A {
        Affine3A::from_rotation_translation(self.rotation(), self.center)
    }
    /// Local (right, up, out) coordinates of a world point.
    pub fn local(&self, p: Vec3) -> Vec3 {
        let d = p - self.center;
        Vec3::new(d.dot(self.right()), d.dot(self.up), d.dot(self.normal))
    }
}

/// World transform that carries things entering `a` out of `b`.
pub fn through(a: &Place, b: &Place) -> Affine3A {
    b.affine() * Affine3A::from_rotation_y(std::f32::consts::PI) * a.affine().inverse()
}

#[derive(Resource, Default)]
pub struct Portals {
    pub p: [Option<Place>; 2],
    side: [f32; 2],
    cooldown: f32,
}

impl Portals {
    pub fn linked(&self) -> Option<(Place, Place)> {
        Some((self.p[0]?, self.p[1]?))
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct PortalMaterial {
    #[texture(0)]
    #[sampler(1)]
    view: Handle<Image>,
    #[uniform(2)]
    tint: LinearRgba,
}

impl Material for PortalMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://gamemash/modes/portal.wgsl".into()
    }
}

/// Perspective with an optional oblique near plane (Lengyel), adapted to
/// Bevy's reverse-Z infinite projection: z' = w - a * dot(C, p) puts the
/// clip plane at depth 1 and everything in front of it beyond the near plane.
#[derive(Clone, Debug)]
pub struct ObliqueProjection {
    pub fov: f32,
    pub aspect: f32,
    pub near: f32,
    pub plane: Option<Vec4>,
}

impl CameraProjection for ObliqueProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        let mut m = Mat4::perspective_infinite_reverse_rh(self.fov, self.aspect, self.near);
        if let Some(c) = self.plane {
            let w = m.row(3);
            let r = w - c * 0.5;
            m.x_axis.z = r.x;
            m.y_axis.z = r.y;
            m.z_axis.z = r.z;
            m.w_axis.z = r.w;
        }
        m
    }
    fn get_clip_from_view_for_sub(&self, _sub: &SubCameraView) -> Mat4 {
        self.get_clip_from_view()
    }
    fn update(&mut self, _w: f32, _h: f32) {}
    fn far(&self) -> f32 {
        1000.0
    }
    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        PerspectiveProjection { fov: self.fov, aspect_ratio: self.aspect, near: self.near, far: 1000.0, ..default() }.get_frustum_corners(z_near, z_far)
    }
}

#[derive(Component)]
struct PortalView(usize);
#[derive(Component)]
struct PortalCam(usize);
#[derive(Component)]
struct PortalRoot(usize);

#[derive(Resource)]
pub struct PortalAssets {
    mats: [Handle<PortalMaterial>; 2],
    pub images: [Handle<Image>; 2],
}

pub struct PortalsPlugin;

impl Plugin for PortalsPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "portal.wgsl");
        app.add_plugins(MaterialPlugin::<PortalMaterial>::default())
            .init_resource::<Portals>()
            .add_systems(Startup, setup)
            .add_systems(FixedUpdate, (shoot, teleport).chain().in_set(MoveSet::Abilities).run_if(mode_on(Mode::Portals)))
            .add_systems(Update, (toggle, holes, hud.run_if(mode_on(Mode::Portals))))
            .add_systems(PostUpdate, (place_visuals, drive_cameras).after(crate::core::camera::follow).before(TransformSystems::Propagate));
    }
}

/// Portal surfaces live on render layer 1 (main camera only), so portal
/// cameras never sample their own target.
fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut pmats: ResMut<Assets<PortalMaterial>>, mut meshes: ResMut<Assets<Mesh>>, mut smats: ResMut<Assets<StandardMaterial>>) {
    let quad = meshes.add(Rectangle::new(HALF.x * 2.0, HALF.y * 2.0));
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let mut mats = vec![];
    let mut imgs = vec![];
    for i in 0..2 {
        let img = images.add(Image::new_target_texture(RT_W, RT_H, TextureFormat::Rgba8UnormSrgb, None));
        let c = COLORS[i];
        let m = pmats.add(PortalMaterial { view: img.clone(), tint: LinearRgba::new(c[0], c[1], c[2], 1.0) });
        mats.push(m.clone());
        imgs.push(img.clone());
        let ring = smats.add(StandardMaterial { base_color: Color::srgb(c[0], c[1], c[2]), emissive: LinearRgba::rgb(c[0] * 6.0, c[1] * 6.0, c[2] * 6.0), ..default() });
        commands
            .spawn((PortalRoot(i), Transform::default(), Visibility::Hidden))
            .with_children(|r| {
                r.spawn((PortalView(i), Mesh3d(quad.clone()), MeshMaterial3d(m), Transform::from_xyz(0.0, 0.0, 0.0), RenderLayers::layer(1)));
                for (pos, size) in [
                    (Vec3::new(0.0, HALF.y + 0.06, 0.0), Vec3::new(HALF.x * 2.0 + 0.24, 0.12, 0.08)),
                    (Vec3::new(0.0, -HALF.y - 0.06, 0.0), Vec3::new(HALF.x * 2.0 + 0.24, 0.12, 0.08)),
                    (Vec3::new(HALF.x + 0.06, 0.0, 0.0), Vec3::new(0.12, HALF.y * 2.0, 0.08)),
                    (Vec3::new(-HALF.x - 0.06, 0.0, 0.0), Vec3::new(0.12, HALF.y * 2.0, 0.08)),
                ] {
                    r.spawn((Mesh3d(cube.clone()), MeshMaterial3d(ring.clone()), Transform::from_translation(pos).with_scale(size)));
                }
            });
        let cam = commands.spawn((
            PortalCam(i),
            Camera3d::default(),
            Camera { order: -1 - i as isize, is_active: false, clear_color: if std::env::var("GAMEMASH_MAGENTA").is_ok() { Color::srgb(1.0, 0.0, 1.0).into() } else { Color::srgb(0.62, 0.76, 0.92).into() }, ..default() },
            RenderTarget::Image(img.into()),
            Projection::custom(ObliqueProjection { fov: 70f32.to_radians(), aspect: 16.0 / 9.0, near: 0.05, plane: None }),
            Transform::default(),
            RenderLayers::layer(0),
            DistanceFog { color: Color::srgb(0.66, 0.78, 0.92), falloff: FogFalloff::Linear { start: 140.0, end: 520.0 }, ..default() },
            crate::core::look::secondary_camera_look(),
        )).id();
        if crate::core::look::HIGH_QUALITY {
            // The physical sky has to render in portal views too.
            let _ = cam; // portal views: plain fogged sky for now
        }
    }
    commands.insert_resource(PortalAssets { mats: [mats[0].clone(), mats[1].clone()], images: [imgs[0].clone(), imgs[1].clone()] });
}

fn toggle(mut t: MessageReader<ModeToggled>, mut portals: ResMut<Portals>) {
    for ev in t.read() {
        if ev.mode == Mode::Portals && !ev.on {
            *portals = Portals::default();
        }
    }
}

fn shoot(latch: Res<Latch>, look: Res<Look>, bar: Res<Hotbar>, rig: Res<CamRig>, p: Res<PlayerState>, rapier: ReadRapierContext, mut portals: ResMut<Portals>) -> Result {
    if bar.item != Item::PortalGun || !look.captured {
        return Ok(());
    }
    let which = if latch.mouse(MouseButton::Left) {
        0
    } else if latch.mouse(MouseButton::Right) {
        1
    } else {
        return Ok(());
    };
    let ctx = rapier.single()?;
    let filt = QueryFilter::new().exclude_sensors().exclude_rigid_body(p.body).groups(CollisionGroups::new(Group::ALL, G_WORLD));
    let Some((_, hit)) = ctx.cast_ray_and_get_normal(rig.eye, rig.forward, 400.0, true, filt) else { return Ok(()) };
    let n = hit.normal.normalize_or_zero();
    // Walls stand portals upright; floors/ceilings orient them toward you.
    let up = if n.y.abs() < 0.7 {
        (Vec3::Y - n * n.y).normalize()
    } else {
        let f = rig.forward.with_y(0.0).normalize_or(Vec3::Z);
        (f - n * f.dot(n)).normalize() * if n.y > 0.0 { 1.0 } else { -1.0 }
    };
    let place = Place { center: hit.point + n * 0.02, normal: n, up };
    if let Some(other) = portals.p[1 - which]
        && other.center.distance(place.center) < 2.8
    {
        return Ok(());
    }
    info!("portal {which} placed at {:?} normal {:?}", place.center, place.normal);
    portals.p[which] = Some(place);
    portals.side = [1.0; 2];
    Ok(())
}

/// Carries the player, cars and bullets through.
#[allow(clippy::too_many_arguments)]
fn teleport(
    time: Res<Time>,
    modes: Res<ActiveModes>,
    mut portals: ResMut<Portals>,
    mut p: ResMut<PlayerState>,
    mut look: ResMut<Look>,
    mut skate: ResMut<SkateSim>,
    mut jump: ResMut<JumpSim>,
    mut walk: ResMut<WalkSim>,
    mut web: ResMut<Web>,
    mut cars: Query<&mut Car>,
    mut shots: Query<(&mut Projectile, &mut Transform)>,
    mut out: MessageWriter<MashEvent>,
) {
    let Some((a, b)) = portals.linked() else { return };
    let dt = time.delta_secs();
    portals.cooldown = (portals.cooldown - dt).max(0.0);
    let places = [a, b];

    // Player: centre of mass crossing the plane inside the frame.
    let body = p.pos + Vec3::Y * 0.9;
    for i in 0..2 {
        let (src, dst) = (places[i], places[1 - i]);
        let l = src.local(body);
        let inside = l.x.abs() < HALF.x + 0.2 && l.y.abs() < HALF.y + 0.4;
        let was = portals.side[i];
        portals.side[i] = l.z;
        if portals.cooldown <= 0.0 && inside && was > 0.0 && l.z <= 0.0 && l.z > -2.5 {
            let m = through(&src, &dst);
            let mut vel = m.transform_vector3(p.vel);
            // Always pop out with at least a little speed.
            let out_speed = vel.dot(dst.normal);
            if out_speed < 3.0 {
                vel += dst.normal * (3.0 - out_speed);
            }
            let new_body = m.transform_point3(body) + dst.normal * 0.35;
            p.pos = new_body - Vec3::Y * 0.9;
            p.vel = vel;
            // Camera keeps its relative orientation.
            let (sy, cy) = look.yaw.sin_cos();
            let (sp, cp) = look.pitch.sin_cos();
            let f = m.transform_vector3(Vec3::new(sy * cp, sp, cy * cp));
            look.yaw = f.x.atan2(f.z);
            look.pitch = f.y.clamp(-0.99, 0.99).asin();
            let fwd = m.transform_vector3(Vec3::new(p.facing.sin(), 0.0, p.facing.cos()));
            p.facing = fwd.x.atan2(fwd.z);
            match p.loco {
                Loco::Foot | Loco::Skate => {
                    let l = p.loco;
                    enter_loco(l, &mut p, &modes, &mut skate, &mut jump, &mut walk);
                }
                Loco::Swing => {
                    web.anchor = None;
                }
                Loco::Vehicle => {
                    for mut car in cars.iter_mut() {
                        if car.driver == crate::modes::streets::Driver::Player {
                            car.sim.pos = p.pos + Vec3::Y * 0.4;
                            car.sim.vel = vel;
                            car.sim.yaw = p.facing;
                            car.sim.on_ground = false;
                        }
                    }
                }
            }
            portals.side = [1.0; 2];
            portals.cooldown = 0.25;
            out.write(MashEvent::new(Kind::PortalTravel, if vel.length() > 15.0 { "Speedy Portal" } else { "" }, (vel.length() * 5.0) as u32));
            break;
        }
    }

    // NPC / traffic cars.
    for mut car in cars.iter_mut() {
        if car.driver == crate::modes::streets::Driver::Player {
            continue;
        }
        for i in 0..2 {
            let (src, dst) = (places[i], places[1 - i]);
            let next = car.sim.pos + car.sim.vel * dt;
            let (l0, l1) = (src.local(car.sim.pos), src.local(next));
            if l0.z > 0.0 && l1.z <= 0.0 && l1.x.abs() < HALF.x + 0.6 && l1.y.abs() < HALF.y + 0.6 {
                let m = through(&src, &dst);
                car.sim.pos = m.transform_point3(car.sim.pos) + dst.normal * 2.5;
                car.sim.vel = m.transform_vector3(car.sim.vel);
                let f = m.transform_vector3(Vec3::new(car.sim.yaw.sin(), 0.0, car.sim.yaw.cos()));
                car.sim.yaw = f.x.atan2(f.z);
                break;
            }
        }
    }

    // Projectiles: swept segment crossing.
    for (mut pr, mut tf) in shots.iter_mut() {
        let next = tf.translation + pr.vel * dt;
        for i in 0..2 {
            let (src, dst) = (places[i], places[1 - i]);
            let (l0, l1) = (src.local(tf.translation), src.local(next));
            if l0.z > 0.0 && l1.z <= 0.0 {
                let k = l0.z / (l0.z - l1.z);
                let hit = l0.lerp(l1, k);
                if hit.x.abs() < HALF.x && hit.y.abs() < HALF.y {
                    let m = through(&src, &dst);
                    let at = tf.translation.lerp(next, k);
                    tf.translation = m.transform_point3(at) + dst.normal * 0.05;
                    pr.vel = m.transform_vector3(pr.vel);
                    pr.portaled = true;
                    break;
                }
            }
        }
    }
}

fn holes(portals: Res<Portals>, mut holes: ResMut<Holes>, modes: Res<ActiveModes>) {
    holes.0.clear();
    if !modes.on(Mode::Portals) {
        return;
    }
    if let Some((a, b)) = portals.linked() {
        for pl in [a, b] {
            holes.0.push(Hole { center: pl.center, normal: pl.normal, up: pl.up, half: HALF });
        }
    }
}

fn place_visuals(portals: Res<Portals>, modes: Res<ActiveModes>, mut roots: Query<(&PortalRoot, &mut Transform, &mut Visibility)>, assets: Res<PortalAssets>, mut pmats: ResMut<Assets<PortalMaterial>>) {
    let linked = portals.linked().is_some();
    for (r, mut tf, mut vis) in roots.iter_mut() {
        match (modes.on(Mode::Portals), portals.p[r.0]) {
            (true, Some(pl)) => {
                *vis = Visibility::Visible;
                *tf = Transform { translation: pl.center, rotation: pl.rotation(), ..default() };
            }
            _ => *vis = Visibility::Hidden,
        }
        if let Some(mut m) = pmats.get_mut(&assets.mats[r.0]) {
            let want = if linked { 0.0 } else { 1.0 };
            if m.tint.alpha != want {
                m.tint.alpha = want;
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn drive_cameras(
    portals: Res<Portals>,
    modes: Res<ActiveModes>,
    rig: Res<CamRig>,
    windows: Query<&Window>,
    main: Query<&Transform, (With<MainCam>, Without<PortalCam>)>,
    mut cams: Query<(&PortalCam, &mut Camera, &mut Transform, &mut Projection), Without<MainCam>>,
) {
    let Ok(main_tf) = main.single() else { return };
    let aspect = windows.iter().next().map(|w| w.width() / w.height().max(1.0)).unwrap_or(16.0 / 9.0);
    for (pc, mut cam, mut tf, mut proj) in cams.iter_mut() {
        let Some((a, b)) = portals.linked().filter(|_| modes.on(Mode::Portals)) else {
            cam.is_active = false;
            continue;
        };
        let places = [a, b];
        // Camera i renders what you see *through* portal i: the view out of portal 1-i.
        let (src, dst) = (places[pc.0], places[1 - pc.0]);
        let visible = src.center.distance(rig.eye) < 160.0 && (src.center - rig.eye).dot(src.normal) < 0.0;
        cam.is_active = visible;
        if !visible {
            continue;
        }
        let m = through(&src, &dst);
        let world = Mat4::from(m) * main_tf.to_matrix();
        *tf = Transform::from_matrix(world);
        // Clip plane: the exit portal's surface, in this camera's view space.
        let view = world.inverse();
        let n = view.transform_vector3(dst.normal).normalize();
        let c = view.transform_point3(dst.center - dst.normal * 0.02);
        let plane = n.extend(-n.dot(c));
        if let Projection::Custom(custom) = proj.as_mut()
            && let Some(op) = custom.get_mut::<ObliqueProjection>()
        {
            op.fov = rig.fov.to_radians();
            op.aspect = aspect;
            op.plane = Some(plane);
        }
    }
}

fn hud(bar: Res<Hotbar>, portals: Res<Portals>, mut hud: ResMut<Hud>) {
    if bar.item == Item::PortalGun {
        hud.crosshair = true;
        let s = |o: &Option<Place>| if o.is_some() { "placed" } else { "-" };
        hud.top.push(format!("Portal gun: LMB blue ({})  RMB orange ({})", s(&portals.p[0]), s(&portals.p[1])));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(center: Vec3, normal: Vec3) -> Place {
        Place { center, normal, up: Vec3::Y }
    }

    #[test]
    fn walking_into_a_wall_portal_exits_the_other_facing_out() {
        let a = wall(Vec3::new(0.0, 1.0, 10.0), Vec3::NEG_Z);
        let b = wall(Vec3::new(50.0, 1.0, 0.0), Vec3::X);
        let m = through(&a, &b);
        // Just past a's surface (entered) maps to just in front of b.
        let p = m.transform_point3(Vec3::new(0.0, 1.0, 11.0));
        assert!((p - Vec3::new(51.0, 1.0, 0.0)).length() < 1e-4, "{p:?}");
        // Moving into a (+Z) exits b moving out (+X).
        let v = m.transform_vector3(Vec3::new(0.0, 0.0, 5.0));
        assert!((v - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-4, "{v:?}");
    }

    #[test]
    fn floor_to_wall_converts_fall_into_launch() {
        let floor = Place { center: Vec3::ZERO, normal: Vec3::Y, up: Vec3::Z };
        let w = wall(Vec3::new(0.0, 5.0, 30.0), Vec3::NEG_Z);
        let v = through(&floor, &w).transform_vector3(Vec3::new(0.0, -20.0, 0.0));
        assert!((v - Vec3::new(0.0, 0.0, -20.0)).length() < 1e-3, "{v:?}");
    }

    #[test]
    fn oblique_plane_sits_at_depth_one() {
        let op = ObliqueProjection { fov: 1.2, aspect: 1.5, near: 0.05, plane: Some(Vec4::new(0.0, 0.0, -1.0, -5.0)) };
        let m = op.get_clip_from_view();
        // Plane z = -5 in view space (5 m ahead): depth should be exactly 1.
        let on = m * Vec4::new(0.3, 0.2, -5.0, 1.0);
        assert!((on.z / on.w - 1.0).abs() < 1e-4);
        // Farther points are in front of the clip plane: 0 < depth < 1.
        let far = m * Vec4::new(0.0, 0.0, -40.0, 1.0);
        let d = far.z / far.w;
        assert!(d > 0.0 && d < 1.0, "{d}");
        // Nearer points are clipped (depth > 1).
        let near = m * Vec4::new(0.0, 0.0, -2.0, 1.0);
        assert!(near.z / near.w > 1.0);
    }
}
