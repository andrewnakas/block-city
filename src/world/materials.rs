//! Realistic surfaces: photo-scanned CC0 PBR sets (Poly Haven) mapped onto
//! the procedural city by surface type. Textures tile in world space.

use bevy::image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Surf {
    Asphalt,
    Pavement,
    Concrete,
    Wood,
    Metal,
    Dirt,
    Brick,
    Glass,
    Paint,
    Glow,
}

impl Surf {
    fn dir(self) -> Option<&'static str> {
        match self {
            Surf::Asphalt => Some("asphalt_02"),
            Surf::Pavement => Some("concrete_pavement"),
            Surf::Concrete | Surf::Paint => Some("concrete_floor_02"),
            Surf::Wood => Some("plywood"),
            Surf::Metal => Some("blue_metal_plate"),
            Surf::Dirt => Some("dirt"),
            Surf::Brick => Some("red_brick_03"),
            Surf::Glass | Surf::Glow => None,
        }
    }
    /// Metres covered by one texture repeat.
    fn tile(self) -> f32 {
        match self {
            Surf::Asphalt => 4.0,
            Surf::Pavement => 3.0,
            Surf::Concrete | Surf::Paint => 2.5,
            Surf::Wood => 2.4,
            Surf::Metal => 1.0,
            Surf::Dirt => 3.0,
            Surf::Brick => 2.0,
            Surf::Glass | Surf::Glow => 1.0,
        }
    }
}

/// Which surface a builder colour stands for (the city is authored in colours).
pub fn surf_for(c: [u8; 3]) -> Surf {
    match c {
        [52, 54, 60] => Surf::Asphalt,
        [150, 150, 146] => Surf::Pavement,
        [178, 176, 170] | [196, 192, 184] | [160, 160, 168] | [140, 140, 150] | [120, 120, 128] | [150, 150, 150] | [180, 180, 190] | [200, 200, 205] => Surf::Concrete,
        [168, 120, 72] | [126, 108, 70] | [110, 96, 70] | [90, 80, 60] => Surf::Wood,
        [200, 200, 210] | [70, 70, 76] | [60, 64, 70] | [90, 96, 110] => Surf::Metal,
        [150, 140, 110] | [120, 96, 70] | [110, 168, 96] => Surf::Dirt,
        [70, 90, 120] => Surf::Glass,
        [255, 240, 200] => Surf::Glow,
        [160, 140, 120] | [200, 120, 80] => Surf::Brick,
        [r, g, b] if r.abs_diff(g) < 25 && g.abs_diff(b) < 25 && r > 100 => Surf::Brick,
        _ => Surf::Paint,
    }
}

fn load(assets: &AssetServer, path: String, srgb: bool) -> Handle<Image> {
    assets.load_with_settings(path, move |s: &mut ImageLoaderSettings| {
        s.is_srgb = srgb;
        s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            anisotropy_clamp: 8,
            ..default()
        });
    })
}

/// Builds a material for a surface, tinted by the authored colour.
pub fn make(assets: &AssetServer, mats: &mut Assets<StandardMaterial>, c: [u8; 3]) -> Handle<StandardMaterial> {
    let surf = surf_for(c);
    let tint = |k: f32| Color::srgb(c[0] as f32 / 255.0 * k, c[1] as f32 / 255.0 * k, c[2] as f32 / 255.0 * k);
    let mut m = StandardMaterial { perceptual_roughness: 1.0, metallic: 0.0, ..default() };
    if let Some(dir) = surf.dir() {
        m.base_color_texture = Some(load(assets, format!("textures/{dir}/diff.jpg"), true));
        m.normal_map_texture = Some(load(assets, format!("textures/{dir}/nor_gl.jpg"), false));
        let arm = load(assets, format!("textures/{dir}/arm.jpg"), false);
        m.metallic_roughness_texture = Some(arm.clone());
        m.occlusion_texture = Some(arm);
        m.uv_transform = Affine2::from_scale(Vec2::splat(1.0 / surf.tile()));
    }
    match surf {
        // Photo textures carry their own colour; keep the tint subtle.
        Surf::Asphalt | Surf::Pavement | Surf::Concrete | Surf::Wood | Surf::Dirt | Surf::Brick => {
            m.base_color = Color::WHITE.mix(&tint(1.6), 0.25);
        }
        Surf::Metal => {
            m.base_color = tint(1.5);
            m.metallic = 1.0;
        }
        Surf::Paint => {
            m.base_color = tint(1.0);
        }
        Surf::Glass => {
            m.base_color = Color::srgb(0.08, 0.1, 0.13);
            m.perceptual_roughness = 0.06;
            m.reflectance = 0.9;
            m.metallic = 0.2;
        }
        Surf::Glow => {
            m.base_color = tint(1.0);
            m.emissive = LinearRgba::from(tint(1.0)) * 4.0;
            m.perceptual_roughness = 0.5;
        }
    }
    mats.add(m)
}

/// JPG/PNG textures load with a single mip level, which shimmers badly at a
/// distance. Build the mip chain on load (box filter, RGBA8).
pub fn generate_mips(mut ev: MessageReader<AssetEvent<Image>>, mut images: ResMut<Assets<Image>>) {
    use bevy::render::render_resource::{TextureFormat, TextureUsages};
    for e in ev.read() {
        let AssetEvent::LoadedWithDependencies { id } = e else { continue };
        let Some(mut img) = images.get_mut(*id) else { continue };
        let d = &img.texture_descriptor;
        if d.mip_level_count > 1
            || d.usage.contains(TextureUsages::RENDER_ATTACHMENT)
            || !matches!(d.format, TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm)
            || d.size.depth_or_array_layers != 1
        {
            continue;
        }
        let (mut w, mut h) = (d.size.width as usize, d.size.height as usize);
        let Some(base) = img.data.take() else { continue };
        let mut out = base.clone();
        let mut prev = base;
        let mut levels = 1;
        while w > 1 || h > 1 {
            let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
            let mut next = vec![0u8; nw * nh * 4];
            for y in 0..nh {
                for x in 0..nw {
                    for c in 0..4 {
                        let mut sum = 0u32;
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let sx = (x * 2 + dx).min(w - 1);
                            let sy = (y * 2 + dy).min(h - 1);
                            sum += prev[(sy * w + sx) * 4 + c] as u32;
                        }
                        next[(y * nw + x) * 4 + c] = (sum / 4) as u8;
                    }
                }
            }
            out.extend_from_slice(&next);
            prev = next;
            w = nw;
            h = nh;
            levels += 1;
        }
        img.data = Some(out);
        img.texture_descriptor.mip_level_count = levels;
    }
}
