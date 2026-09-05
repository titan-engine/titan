struct DrawUniforms {
    mvp: mat4x4<f32>,
    color_diffuse: vec4<f32>,
    light_ambient: vec4<f32>,
}
@group(0) @binding(0) var<uniform> draw: DrawUniforms;
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
}
@vertex fn vs_mesh(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = draw.mvp * vec4<f32>(position, 1.0);
    out.normal = normal;
    return out;
}
fn shade(normal: vec3<f32>) -> vec3<f32> {
    // Opposing authored normals may interpolate to zero. Such fragments receive
    // ambient only, rather than an undefined normalize(0) result.
    let scale = max(max(abs(normal.x), abs(normal.y)), abs(normal.z));
    var n = vec3<f32>(0.0);
    if scale > 0.0 {
        n = normalize(normal / scale);
    }
    let intensity = clamp(draw.light_ambient.w + draw.color_diffuse.w * max(dot(n, draw.light_ambient.xyz), 0.0), 0.0, 1.0);
    return draw.color_diffuse.rgb * intensity;
}
@fragment fn fs_linear(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(shade(in.normal), 1.0);
}
@fragment fn fs_encoded(in: VertexOutput) -> @location(0) vec4<f32> {
    let linear = shade(in.normal);
    let encoded = select(1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055, 12.92 * linear, linear <= vec3<f32>(0.0031308));
    return vec4<f32>(encoded, 1.0);
}
