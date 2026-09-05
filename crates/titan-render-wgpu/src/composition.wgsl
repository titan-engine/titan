@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var overlay: texture_2d<f32>;
@group(0) @binding(2) var nearest: sampler;
struct Output { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) index: u32) -> Output {
    let positions = array<vec2<f32>,3>(vec2<f32>(-1,-1), vec2<f32>(3,-1), vec2<f32>(-1,3));
    let p = positions[index];
    return Output(vec4<f32>(p,0,1), vec2<f32>((p.x+1)*0.5,(1-p.y)*0.5));
}
fn decode(v: vec3<f32>) -> vec3<f32> {
    return select(pow((v+0.055)/1.055,vec3<f32>(2.4)),v/12.92,v<=vec3<f32>(0.04045));
}
fn composed(uv: vec2<f32>) -> vec4<f32> {
    // Both inputs store straight, encoded RGB. UI internal overlapping sprites
    // retain their established byte-space blending; cross-layer blending is linear.
    let background = textureSample(scene, nearest, uv);
    let ui = textureSample(overlay, nearest, uv);
    return vec4<f32>(mix(decode(background.rgb),decode(ui.rgb),ui.a),1);
}
@fragment fn fs_linear(input: Output) -> @location(0) vec4<f32> { return composed(input.uv); }
@fragment fn fs_encoded(input: Output) -> @location(0) vec4<f32> {
    let c = composed(input.uv);
    return vec4<f32>(select(1.055*pow(c.rgb,vec3<f32>(1.0/2.4))-0.055,c.rgb*12.92,c.rgb<=vec3<f32>(0.0031308)),1);
}
