// Portal surface: shows the linked portal's camera image, sampled in screen
// space so the view lines up with the player's perspective.
#import bevy_pbr::{mesh_view_bindings::view, forward_io::VertexOutput, utils::coords_to_viewport_uv}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var view_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var view_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> tint: vec4<f32>;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let uv = coords_to_viewport_uv(mesh.position.xy, view.viewport);
    let c = textureSample(view_texture, view_sampler, uv);
    // tint.a > 0.5 means the other portal isn't placed yet: show a swirl.
    if (tint.a > 0.5) {
        let p = mesh.uv - vec2<f32>(0.5);
        let r = length(p);
        let swirl = 0.5 + 0.5 * sin(r * 30.0 - atan2(p.y, p.x) * 3.0);
        return vec4<f32>(tint.rgb * (0.35 + 0.4 * swirl), 1.0);
    }
    return vec4<f32>(c.rgb, 1.0);
}
