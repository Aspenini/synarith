struct Settings {
    size: vec4<u32>, geom: vec4<f32>, viewbox: vec4<f32>, state: vec4<u32>, clock: vec4<f32>,
}
@group(0) @binding(0) var<uniform> cfg: Settings;
@group(0) @binding(1) var<storage, read> params: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> pixels: array<vec4<f32>>;
const TAU: f32 = 6.28318530718;
fn clean(v: vec4<f32>) -> vec4<f32> {
    return select(vec4<f32>(0.0), v, abs(v) <= vec4<f32>(3.402823e38));
}
fn sd(a: f32, b: f32) -> f32 {
    if abs(b) > 1e-20 {return a / b;} return 0.0;
}
fn vd(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(sd(a.x,b.x),sd(a.y,b.y),sd(a.z,b.z),sd(a.w,b.w));
}
fn sm(a: f32, b: f32, x: f32) -> f32 {
    var v = select(0.0,1.0,x >= b);
    if abs(b-a) >= 1e-10 {v = clamp((x-a)/(b-a),0.0,1.0);}
    return v*v*(3.0-2.0*v);
}
fn power(a: f32, b: f32) -> f32 {
    if a == 0.0 { if b == 0.0 {return 1.0;} return 0.0; }
    if a < 0.0 {if b != trunc(b) {return 0.0;} return pow(-a,b)*select(1.0,-1.0,abs(b-2.0*floor(b/2.0))>=1.0);}
    return pow(a,b);
}
fn hash(v: u32) -> u32 {
    var x = v; x = x ^ (x >> 16u); x = x * 0x7feb352du;
    x = x ^ (x >> 15u); x = x * 0x846ca68bu; return x ^ (x >> 16u);
}
fn hash2(p: vec2<i32>, seed: u32) -> f32 {
    return f32(hash(bitcast<u32>(p.x)*0x9e3779b9u ^ bitcast<u32>(p.y)*0x85ebca6bu ^ seed)>>8u)/16777216.0;
}
