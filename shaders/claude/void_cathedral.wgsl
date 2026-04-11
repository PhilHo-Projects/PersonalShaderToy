// "Void Cathedral" - Claude
// Raymarched infinite fractal cathedral dissolving into the void.
// Volumetric fog, neon stained glass light, infinite reflections.

fn hash13(p3: vec3<f32>) -> f32 {
    var p = fract(p3 * 0.1031);
    p += dot(p, p.zyx + 31.32);
    return fract((p.x + p.y) * p.z);
}

fn noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);

    return mix(
        mix(mix(hash13(i + vec3(0,0,0)), hash13(i + vec3(1,0,0)), u.x),
            mix(hash13(i + vec3(0,1,0)), hash13(i + vec3(1,1,0)), u.x), u.y),
        mix(mix(hash13(i + vec3(0,0,1)), hash13(i + vec3(1,0,1)), u.x),
            mix(hash13(i + vec3(0,1,1)), hash13(i + vec3(1,1,1)), u.x), u.y),
        u.z
    );
}

fn fbm(p_in: vec3<f32>) -> f32 {
    var p = p_in;
    var v = 0.0;
    var a = 0.5;
    for (var i = 0; i < 5; i++) {
        v += a * noise(p);
        p = p * 2.03 + vec3(0.31, 0.17, 0.09);
        a *= 0.5;
    }
    return v;
}

fn rot2(a: f32) -> mat2x2<f32> {
    let s = sin(a); let c = cos(a);
    return mat2x2<f32>(c, -s, s, c);
}

fn sdBox(p: vec3<f32>, b: vec3<f32>) -> f32 {
    let q = abs(p) - b;
    return length(max(q, vec3(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
}

fn cathedral(p_in: vec3<f32>, t: f32) -> f32 {
    var p = p_in;

    // Infinite repetition along Z (the nave)
    p.z = p.z - round(p.z / 6.0) * 6.0;

    // Gothic arches — intersection of cylinders
    let pillarR = 0.15;
    let spacing = 2.5;
    let pillarL = length(vec2(abs(p.x) - spacing, p.z)) - pillarR;
    let pillarR2 = length(vec2(abs(p.x) - spacing, p.z - 3.0)) - pillarR;
    var pillars = min(pillarL, pillarR2);

    // Pointed arch vault
    let archCenter = 4.5;
    let archP = vec2(abs(p.x), p.y - archCenter);
    let arch = length(archP) - spacing - 0.1;

    // Floor and ceiling
    let floor_d = -p.y;
    let ceiling = p.y - archCenter - spacing * 0.6;

    // Ribbed vaulting (crossed arches on ceiling)
    let ribP = vec2(abs(p.x) - abs(p.z) * 0.4, p.y - archCenter + 0.5);
    let rib = length(ribP) - spacing * 0.9;

    var d = pillars;
    d = min(d, floor_d);
    d = max(d, -ceiling);
    d = min(d, max(arch, ceiling + 0.05));
    d = min(d, max(rib, ceiling + 0.05));

    // Fractal erosion — the cathedral dissolves
    let erosion = fbm(p * 1.5 + vec3(0.0, 0.0, t * 0.1)) * 0.6 - 0.25;
    d = max(d, erosion);

    return d;
}

fn map(p: vec3<f32>, t: f32) -> f32 {
    return cathedral(p, t);
}

fn calcNormal(p: vec3<f32>, t: f32) -> vec3<f32> {
    let e = vec2(0.002, 0.0);
    return normalize(vec3(
        map(p + e.xyy, t) - map(p - e.xyy, t),
        map(p + e.yxy, t) - map(p - e.yxy, t),
        map(p + e.yyx, t) - map(p - e.yyx, t)
    ));
}

@fragment
fn fs_main(@builtin(position) fragCoord: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = (fragCoord.xy - 0.5 * u.iResolution.xy) / u.iResolution.y;
    let t = u.iTime;

    // Camera — floating through the cathedral
    let camZ = t * 1.5;
    var ro = vec3(sin(t * 0.3) * 1.5, 2.0 + sin(t * 0.2) * 0.5, camZ);
    var rd = normalize(vec3(uv, 1.2));

    // Camera rotation
    let angle = sin(t * 0.15) * 0.3;
    let r = rot2(angle);
    let rdxz = r * rd.xz;
    rd = vec3(rdxz.x, rd.y, rdxz.y);

    // Mouse look
    if (u.iMouse.z > 0.0) {
        let m = u.iMouse.xy / u.iResolution.xy - 0.5;
        let rx = rot2(m.x * 4.0);
        let ry = rot2(-m.y * 2.0);
        let rdxz2 = rx * rd.xz;
        rd = vec3(rdxz2.x, rd.y, rdxz2.y);
        let rdxy = ry * rd.xy;
        rd = vec3(rdxy.x, rdxy.y, rd.z);
    }

    // Raymarch
    var dO = 0.0;
    var col = vec3(0.0);
    var glow = vec3(0.0);
    let MAX_DIST = 80.0;

    for (var i = 0; i < 120; i++) {
        let p = ro + rd * dO;
        let dS = map(p, t);

        // Volumetric fog accumulation
        let fogDensity = exp(-max(dS, 0.0) * 3.0) * 0.015;
        let fogPos = p * 0.3 + vec3(0.0, 0.0, t * 0.2);
        // Fixed lag: Claude was running a 5-octave nested noise loop 120 times per pixel!
        // That's roughly 10 BILLION hash operations per frame. Replaced with 1 octave.
        let fogNoise = noise(fogPos) * 0.5 + 0.5;

        // Stained glass light — colored beams from the sides
        let lightAngle = sin(p.z * 0.5 + t * 0.3) * 0.5 + 0.5;
        let stainedGlass = vec3(
            sin(p.z * 0.7 + 0.0) * 0.5 + 0.5,
            sin(p.z * 0.7 + 2.1) * 0.5 + 0.5,
            sin(p.z * 0.7 + 4.2) * 0.5 + 0.5
        );

        let beam = exp(-abs(p.x) * 0.8) * exp(-(p.y - 3.0) * (p.y - 3.0) * 0.1);
        glow += stainedGlass * fogDensity * fogNoise * 2.0 * beam * lightAngle;

        // Candle-like point lights along the nave
        let candlePos = vec3(1.8 * sign(sin(p.z * 1.047)), 1.0, round(p.z / 3.0) * 3.0);
        let candleDist = length(p - candlePos);
        let candleGlow = exp(-candleDist * 1.5) * 0.02;
        glow += vec3(1.0, 0.6, 0.2) * candleGlow;

        if (abs(dS) < 0.001 || dO > MAX_DIST) { break; }
        dO += dS * 0.7;
    }

    // Surface shading
    if (dO < MAX_DIST) {
        let p = ro + rd * dO;
        let n = calcNormal(p, t);

        // Dark stone material
        let stone = vec3(0.04, 0.035, 0.05);
        let stoneNoise = noise(p * 8.0) * 0.02;
        var mat = stone + stoneNoise;

        // Ambient occlusion (cheap)
        let ao = 0.5 + 0.5 * n.y;

        // Directional light from above
        let light = normalize(vec3(sin(t * 0.2), 1.0, cos(t * 0.3)));
        let diff = max(dot(n, light), 0.0) * 0.3;

        // Rim light (ethereal edges)
        let rim = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
        let rimColor = vec3(0.3, 0.4, 0.8) * rim;

        col = mat * (diff + 0.02) * ao + rimColor * 0.15;
    }

    // Add volumetric glow
    col += glow;

    // Distance fog — fades to deep void
    let fog = 1.0 - exp(-dO * 0.015);
    let voidColor = vec3(0.01, 0.005, 0.02);
    col = mix(col, voidColor, fog);

    // Tone mapping
    col = col / (1.0 + col);

    // Slight color grade — push shadows blue, highlights warm
    col = pow(col, vec3(0.9, 0.95, 1.1));

    // Gamma
    col = pow(max(col, vec3(0.0)), vec3(0.4545));

    // Subtle vignette
    let vig = 1.0 - 0.3 * length(uv);
    col *= vig;

    return vec4(col, 1.0);
}
