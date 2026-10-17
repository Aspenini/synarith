struct Settings {
    size: vec4<u32>, geom: vec4<f32>, viewbox: vec4<f32>, state: vec4<u32>, clock: vec4<f32>,
}
@group(0) @binding(0) var<uniform> cfg: Settings;
@group(0) @binding(1) var<storage, read> params: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> pixels: array<vec4<f32>>;
const TAU: f32 = 6.28318530718;
fn mandelbrot(c: vec2<f32>, limit: i32) -> f32 {
    var z = vec2<f32>(0.0);
    for(var iteration=0; iteration<limit; iteration++) {
        z = vec2<f32>(z.x*z.x-z.y*z.y+c.x,2.0*z.x*z.y+c.y);
        let squared_radius = z.x*z.x+z.y*z.y;
        if squared_radius > 256.0 {
            let escape_count = f32(iteration)+2.0-log2(0.5*log(squared_radius));
            return clamp(escape_count/f32(limit),0.0,0.999999);
        }
    }
    return 1.0;
}
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
fn positive_mod(v: i32, period: i32) -> i32 {
    if v >= 0 {return i32(u32(v) % u32(period));}
    let remainder = (0u - bitcast<u32>(v)) % u32(period);
    return select(period - i32(remainder), 0, remainder == 0u);
}
fn noise_corner(cell: vec2<i32>, offset: vec2<i32>, f: vec2<f32>, seed: u32, period: vec2<i32>, gradient: bool) -> f32 {
    var at = cell+offset;
    if all(period > vec2<i32>(0)) {at = vec2<i32>(positive_mod(at.x,period.x),positive_mod(at.y,period.y));}
    let h = hash2(at,seed);
    if gradient {let a = h*TAU; return dot(vec2<f32>(cos(a),sin(a)),f-vec2<f32>(offset));}
    return h;
}
fn noise(p: vec2<f32>, seed: u32, period: vec2<i32>, gradient: bool) -> f32 {
    let cell = vec2<i32>(floor(p)); let f = p-floor(p); let u = f*f*f*(f*(f*6.0-15.0)+10.0);
    let a = noise_corner(cell,vec2<i32>(0,0),f,seed,period,gradient)*(1.0-u.x)+noise_corner(cell,vec2<i32>(1,0),f,seed,period,gradient)*u.x;
    let b = noise_corner(cell,vec2<i32>(0,1),f,seed,period,gradient)*(1.0-u.x)+noise_corner(cell,vec2<i32>(1,1),f,seed,period,gradient)*u.x;
    let v = a*(1.0-u.y)+b*u.y; return select(v,v*0.7+0.5,gradient);
}
fn cellular(p: vec2<f32>, seed: u32) -> f32 {
    let cell = vec2<i32>(floor(p)); let f = p-floor(p); var distance = 4.0;
    for(var y = -1; y<=1; y++) {for(var x = -1; x<=1; x++) {
        let offset = vec2<i32>(x,y); let delta = vec2<f32>(offset)+vec2<f32>(hash2(cell+offset,seed),hash2(cell+offset,seed ^ 0x68bc21ebu))-f;
        distance = min(distance,dot(delta,delta));
    }}
    return min(sqrt(distance),1.0);
}
fn segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let v = b-a; let w = p-a; return length(w-v*clamp(sd(dot(w,v),dot(v,v)),0.0,1.0));
}
fn crossing(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> bool {
    return ((a.y>p.y)!=(b.y>p.y)) && p.x < (b.x-a.x)*sd(p.y-a.y,b.y-a.y)+a.x;
}
fn vertex(i: i32, count: i32, outer: f32, inner: f32) -> vec2<f32> {
    let angle = f32(i)/f32(count)*TAU-TAU*0.25;
    let r = select(outer,inner,i%2==1); return r*vec2<f32>(cos(angle),sin(angle));
}
fn polygon(p: vec2<f32>, count: i32, outer: f32, inner: f32) -> f32 {
    var distance = 3.402823e38; var inside = false; var a = vertex(count-1,count,outer,inner);
    for(var i=0;i<count;i++) {let b = vertex(i,count,outer,inner);distance=min(distance,segment(p,a,b));if crossing(p,a,b) {inside = !inside;} a=b;}
    return select(distance,-distance,inside);
}
fn cubic(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, d: vec2<f32>, u: f32) -> vec2<f32> {
    let v = 1.0-u; return v*v*v*a+3.0*v*v*u*b+3.0*v*u*u*c+u*u*u*d;
}
fn path_distance(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, d: vec2<f32>, count: i32, closed: bool) -> f32 {
    var previous = a;var distance = 3.402823e38;var inside = false;
    for(var j=1;j<=count;j++) {let next = cubic(a,b,c,d,f32(j)/f32(count));distance=min(distance,segment(p,previous,next));if crossing(p,previous,next) {inside = !inside;} previous=next;}
    if closed {distance=min(distance,segment(p,previous,a));if crossing(p,previous,a) {inside = !inside;}}
    return select(distance,-distance,closed && inside);
}
fn blend(a: vec4<f32>, b: vec4<f32>, mode: i32) -> vec4<f32> {
    let aa=clamp(a.w,0.0,1.0);let ba=clamp(b.w,0.0,1.0);
    let x=vec3<f32>(sd(a.x,aa),sd(a.y,aa),sd(a.z,aa));
    let y=vec3<f32>(sd(b.x,ba),sd(b.y,ba),sd(b.z,ba)); var v=x;
    if mode==1 {v=x*y;} else if mode==2 {v=x+y-x*y;} else if mode==3 {v=select(vec3<f32>(1.0)-2.0*(1.0-x)*(1.0-y),2.0*x*y,y<=vec3<f32>(0.5));}
    return vec4<f32>((1.0-ba)*a.xyz+(1.0-aa)*b.xyz+aa*ba*v,aa+ba*(1.0-aa));
}
