//! Block City: An open-world crime city with a block-sandbox layer: steal cars, dig, build and blow up the streets.

mod core;
mod modes;
mod sim;
mod world;

use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use bevy_rapier3d::prelude::*;

fn main() {
    let mut app = App::new();
    app
        .insert_resource(ClearColor(Color::srgb(0.62, 0.76, 0.92)))
        .insert_resource(Time::<Fixed>::from_hz(if cfg!(target_arch = "wasm32") { 60.0 } else { 120.0 }))
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            // No .meta sidecars ship with the assets; on the web each probe is a wasted 404.
            meta_check: bevy::asset::AssetMetaCheck::Never,
            ..default()
        }).set(WindowPlugin {
            primary_window: Some(Window {
                title: crate::core::game::TITLE.into(),
                canvas: Some("#bevy".into()),
                // On the web render at CSS pixels; Retina 2x quadruples fill cost.
                resolution: if cfg!(target_arch = "wasm32") {
                    bevy::window::WindowResolution::default().with_scale_factor_override(1.0)
                } else {
                    bevy::window::WindowResolution::default()
                },
                fit_canvas_to_parent: true,
                prevent_default_event_handling: true,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::default().in_fixed_schedule())
        .add_plugins(EguiPlugin::default())
        .insert_resource(bevy_egui::EguiGlobalSettings { auto_create_primary_context: false, ..default() })
        .add_plugins((
            core::scene::ScenePlugin,
            core::modes::ModesPlugin,
            core::input::InputPlugin,
            core::player::PlayerPlugin,
            core::camera::CameraPlugin,
            core::score::ScorePlugin,
            core::ui::UiPlugin,
            core::avatar::AvatarPlugin,
            core::hotbar::HotbarPlugin,
            core::fx::FxPlugin,
            core::look::LookPlugin,
            core::rig::RigPlugin,
            core::audio::AudioFxPlugin,
        ))
        .add_plugins((
            modes::skate::SkatePlugin,
            modes::jump::JumpPlugin,
            modes::warfare::WarfarePlugin,
            modes::streets::StreetsPlugin,
            modes::blocks::BlocksPlugin,
            modes::swing::SwingPlugin,
            modes::portals::PortalsPlugin,
            modes::bullettime::BulletTimePlugin,
            modes::raiders::RaidersPlugin,
            modes::boss::BossPlugin,
        ))
        .add_systems(Startup, world::city::build_city.run_if(core::scene::is(core::scene::World::City)))
        .add_systems(PostUpdate, world::materials::generate_mips);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(core::debug::DebugPlugin);
    app.run();
}
