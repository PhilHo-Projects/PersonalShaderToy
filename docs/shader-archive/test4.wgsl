// WGSL translation of shaders/Test/test3.glsl
// Original GLSL credit: Danilo Guanabara

fn mainImage(fragCoord: vec2<f32>) -> vec4<f32> {
    let r = u.iResolution.xy;
    let t = u.iTime;

    var c = vec3<f32>(0.0);
    var l = 0.0;
    var z = t;

    for (var i: i32 = 0; i < 3; i = i + 1) {
        var uv = vec2<f32>(0.0);
        var p = fragCoord / r;
        uv = p;
        p = p - vec2<f32>(0.5);
        p.x *= r.x / r.y;
        z += 0.07;
        l = length(p);
        uv += p / l * (sin(z) + 1.0) * abs(sin(l * 9.0 - z - z));

        let v = 0.01 / length((uv - floor(uv)) - vec2<f32>(0.5));
        if (i == 0) {
            c.x = v;
        } else if (i == 1) {
            c.y = v;
        } else {
            c.z = v;
        }
    }

    return vec4<f32>(c / l, t);
}
