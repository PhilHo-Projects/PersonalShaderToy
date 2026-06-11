//! PASS: Buffer A
// Centering diagnostic: red aspect-correct disk at the exact canvas center.

fn mainImage(fragCoord: vec2<f32>) -> vec4<f32> {
    let res = u.iResolution.xy;
    let p = (fragCoord - 0.5 * res) / res.y;
    let d = length(p);
    let disk = smoothstep(0.22, 0.20, d);
    let ring = smoothstep(0.245, 0.235, abs(d - 0.34));
    return vec4<f32>(disk + ring * 0.8, ring * 0.08, ring * 0.04, 1.0);
}

//! PASS: Buffer B
//! iChannel0: Buffer A
// Green centered ring, sampling Buffer A to prove channel alignment.

fn mainImage(fragCoord: vec2<f32>) -> vec4<f32> {
    let res = u.iResolution.xy;
    let uv = fragCoord / res;
    let p = (fragCoord - 0.5 * res) / res.y;
    let a = stSample(iChannel0, uv).rgb;
    let d = length(p);
    let ring = smoothstep(0.018, 0.0, abs(d - 0.28));
    return vec4<f32>(a * 0.35 + vec3<f32>(0.0, ring, ring * 0.12), 1.0);
}

//! PASS: Buffer C
//! iChannel0: Buffer B
// Blue center cross, sampling Buffer B to prove pass-to-pass alignment.

fn mainImage(fragCoord: vec2<f32>) -> vec4<f32> {
    let res = u.iResolution.xy;
    let uv = fragCoord / res;
    let p = (fragCoord - 0.5 * res) / res.y;
    let b = stSample(iChannel0, uv).rgb;
    let cross = max(
        smoothstep(0.004, 0.0, abs(p.x)),
        smoothstep(0.004, 0.0, abs(p.y)),
    ) * smoothstep(0.32, 0.30, length(p));
    return vec4<f32>(b * 0.45 + vec3<f32>(cross * 0.18, cross * 0.35, cross), 1.0);
}

//! PASS: Image
//! iChannel0: Buffer A
//! iChannel1: Buffer B
//! iChannel2: Buffer C
// Final composite: centered disk, ring, cross, border, and quadrant tint.

fn mainImage(fragCoord: vec2<f32>) -> vec4<f32> {
    let res = u.iResolution.xy;
    let uv = fragCoord / res;
    let p = (fragCoord - 0.5 * res) / res.y;

    let a = stSample(iChannel0, uv).rgb;
    let b = stSample(iChannel1, uv).rgb;
    let c = stSample(iChannel2, uv).rgb;

    let center_cross = max(
        smoothstep(0.0025, 0.0, abs(p.x)),
        smoothstep(0.0025, 0.0, abs(p.y)),
    ) * smoothstep(0.19, 0.17, length(p));
    let center_dot = smoothstep(0.018, 0.0, length(p));
    let border = max(
        max(smoothstep(0.006, 0.0, uv.x), smoothstep(0.006, 0.0, 1.0 - uv.x)),
        max(smoothstep(0.006, 0.0, uv.y), smoothstep(0.006, 0.0, 1.0 - uv.y)),
    );
    let quadrant_tint = vec3<f32>(
        select(0.02, 0.08, uv.x > 0.5),
        select(0.02, 0.08, uv.y > 0.5),
        0.025,
    );

    var color = quadrant_tint + vec3<f32>(a.r, b.g, c.b);
    color += vec3<f32>(center_cross);
    color += center_dot * vec3<f32>(1.0, 1.0, 0.2);
    color += border * vec3<f32>(0.35);

    return vec4<f32>(color / (1.0 + color), 1.0);
}
