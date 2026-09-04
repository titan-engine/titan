struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
}
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var nearest: sampler;

@vertex fn vs_sprite(@location(0) position: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) tint: vec4<f32>) -> VertexOutput {
    return VertexOutput(vec4<f32>(position, 0.0, 1.0), uv, tint);
}
@fragment fn fs_sprite(input: VertexOutput) -> @location(0) vec4<f32> {
    // Match the software renderer's integer tint rounding before blending.
    let straight = floor(textureSample(image, nearest, input.uv) * input.tint * 255.0 + 0.5) / 255.0;
    return vec4<f32>(straight.rgb * straight.a, straight.a);
}

@vertex fn vs_present(@builtin(vertex_index) index: u32) -> VertexOutput {
    let positions = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    let position = positions[index];
    return VertexOutput(vec4<f32>(position, 0.0, 1.0), vec2<f32>((position.x + 1.0) * 0.5, (1.0 - position.y) * 0.5), vec4<f32>(1.0));
}
fn straight_color(uv: vec2<f32>) -> vec4<f32> {
    let premultiplied = textureSample(image, nearest, uv);
    if premultiplied.a == 0.0 { return vec4<f32>(0.0); }
    return vec4<f32>(premultiplied.rgb / premultiplied.a, premultiplied.a);
}
@fragment fn fs_present(input: VertexOutput) -> @location(0) vec4<f32> {
    return straight_color(input.uv);
}
@fragment fn fs_present_srgb(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = straight_color(input.uv);
    // Sprites blend in byte color space to match the CPU reference. Decode
    // before writing an sRGB attachment, whose hardware encoder restores bytes.
    let linear = select(pow((color.rgb + 0.055) / 1.055, vec3<f32>(2.4)), color.rgb / 12.92, color.rgb <= vec3<f32>(0.04045));
    return vec4<f32>(linear, color.a);
}
