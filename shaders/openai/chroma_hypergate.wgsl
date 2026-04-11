// "Chroma Hypergate" - OpenAI
// WebGPU/WGSL tunnel shader with interactive mouse look.

const TAU: f32 = 6.28318530718;
const MAX_STEPS: u32 = 112u;
const MAX_DIST: f32 = 72.0;
const SURF_DIST: f32 = 0.0012;
const CELL_LEN: f32 = 5.5;

fn rot(a: f32) -> mat2x2<f32> {
    let s = sin(a);
    let c = cos(a);
    return mat2x2<f32>(c, -s, s, c);
}

fn hash21(p: vec2<f32>) -> f32 {
    let n = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(n) * 43758.5453123);
}

fn palette(t: f32) -> vec3<f32> {
    return 0.45 + 0.45 * cos(TAU * vec3<f32>(t + 0.02, t + 0.21, t + 0.43));
}

fn lerp3(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> {
    return a + (b - a) * t;
}

fn sdBox(p: vec3<f32>, b: vec3<f32>) -> f32 {
    let q = abs(p) - b;
    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
}

fn sdOctahedron(p: vec3<f32>, s: f32) -> f32 {
    return (abs(p.x) + abs(p.y) + abs(p.z) - s) * 0.57735027;
}

fn hexDist2D(p: vec2<f32>, r: f32) -> f32 {
    let q = abs(p);
    return max(q.x * 0.8660254 + q.y * 0.5, q.y) - r;
}

fn minHit(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    if (b.x < a.x) {
        return b;
    }
    return a;
}

fn scene(p_in: vec3<f32>) -> vec4<f32> {
    var hit = vec4<f32>(1e6, 0.0, 0.0, 0.0);
    var p = p_in;

    let tunnelRadius = 3.15 + 0.18 * sin(p.z * 0.35 + u.iTime * 1.2);
    let tunnel = abs(hexDist2D(p.xy, tunnelRadius)) - 0.09;
    hit = minHit(hit, vec4<f32>(tunnel, 1.0, 0.08, floor(p.z / CELL_LEN)));

    let cell = floor(p.z / CELL_LEN);
    var q = p;
    q.z = (fract(p.z / CELL_LEN) - 0.5) * CELL_LEN;

    let gateRot = rot(cell * 0.7 + u.iTime * 0.85);
    let rotatedQ = gateRot * q.xy;
    q = vec3<f32>(rotatedQ.x, rotatedQ.y, q.z);

    let ring = max(abs(hexDist2D(q.xy, 2.35)) - 0.075, abs(q.z) - 0.18);
    hit = minHit(hit, vec4<f32>(ring, 3.0, 0.45, cell));

    var c = q;
    let cxy = rot(-u.iTime * 1.6 + cell * 1.1) * c.xy;
    c.x = cxy.x;
    c.y = cxy.y;
    let cyz = rot(u.iTime * 0.9 + cell * 0.6) * vec2<f32>(c.y, c.z);
    c.y = cyz.x;
    c.z = cyz.y;
    let crystal = abs(sdOctahedron(c, 0.92)) - 0.07;
    hit = minHit(hit, vec4<f32>(crystal, 2.0, 1.15, cell));

    var brace = q;
    let bx = abs(abs(brace.x) - 2.45) - 0.12;
    let strut = length(vec2<f32>(bx, brace.z)) - 0.16;
    let braces = max(strut, abs(brace.y) - 0.72);
    hit = minHit(hit, vec4<f32>(braces, 4.0, 0.32, cell));

    return hit;
}

fn sceneDist(p: vec3<f32>) -> f32 {
    return scene(p).x;
}

fn getNormal(p: vec3<f32>) -> vec3<f32> {
    let e = vec2<f32>(0.002, 0.0);
    return normalize(vec3<f32>(
        sceneDist(p + e.xyy) - sceneDist(p - e.xyy),
        sceneDist(p + e.yxy) - sceneDist(p - e.yxy),
        sceneDist(p + e.yyx) - sceneDist(p - e.yyx)
    ));
}

fn softShadow(ro: vec3<f32>, rd: vec3<f32>) -> f32 {
    var res = 1.0;
    var t = 0.05;
    for (var i = 0u; i < 12u; i = i + 1u) {
        let h = sceneDist(ro + rd * t);
        res = min(res, 10.0 * h / t);
        t += clamp(h, 0.05, 0.45);
        if (res < 0.001 || t > 8.0) {
            break;
        }
    }
    return clamp(res, 0.0, 1.0);
}

fn ambientOcclusion(p: vec3<f32>, n: vec3<f32>) -> f32 {
    var occ = 0.0;
    var scale = 1.0;
    for (var i = 1u; i < 6u; i = i + 1u) {
        let h = 0.08 * f32(i);
        let d = sceneDist(p + n * h);
        occ += (h - d) * scale;
        scale *= 0.68;
    }
    return clamp(1.0 - occ, 0.0, 1.0);
}

fn glowField(p: vec3<f32>) -> vec3<f32> {
    let r = length(p.xy);
    let ang = atan2(p.y, p.x);
    let t = u.iTime;

    let helixA = abs(r - (0.95 + 0.24 * sin(p.z * 0.9 - t * 3.4 + ang * 3.0)));
    let helixB = abs(r - (1.55 + 0.18 * sin(p.z * 1.25 + t * 2.5 - ang * 4.0)));
    let ribbonA = exp(-28.0 * helixA * helixA);
    let ribbonB = exp(-34.0 * helixB * helixB);

    let seam = exp(-42.0 * abs(hexDist2D(p.xy, 3.03)));
    let core = exp(-13.0 * r * r) * max(0.0, 0.5 + 0.5 * sin(p.z * 5.0 - t * 7.0));

    let warm = palette(0.11 * p.z - 0.05 * t + 0.08 * ang);
    let cool = palette(0.18 * p.z + 0.03 * t - 0.11 * ang + 0.31);

    return warm * ribbonA * 1.6
        + cool * ribbonB * 1.15
        + vec3<f32>(0.5, 0.9, 1.6) * seam * (0.6 + 0.4 * sin(p.z * 3.0 - t * 4.0))
        + vec3<f32>(1.5, 0.5, 2.4) * core * 0.55;
}

fn bgStars(rd: vec3<f32>) -> vec3<f32> {
    let uv = rd.xy / max(0.2, abs(rd.z));
    let cell = floor(uv * 52.0);
    let spark = pow(hash21(cell), 38.0);
    return vec3<f32>(spark) * palette(cell.x * 0.07 + cell.y * 0.13) * 1.8;
}

@fragment
fn fs_main(@builtin(position) fragCoord: vec4<f32>) -> @location(0) vec4<f32> {
    let fc = vec2<f32>(fragCoord.x, u.iResolution.y - fragCoord.y);
    let uv = (fc - 0.5 * u.iResolution.xy) / u.iResolution.y;
    let t = u.iTime;

    let travel = t * 4.8;
    var ro = vec3<f32>(
        0.48 * sin(t * 0.75) + 0.12 * sin(t * 1.8),
        0.32 * cos(t * 0.47),
        travel
    );
    let aimPoint = vec3<f32>(
        0.24 * sin(t * 0.9 + 0.8),
        0.16 * sin(t * 0.63),
        travel + 3.3
    );

    let forward = normalize(aimPoint - ro);
    let right = normalize(cross(forward, vec3<f32>(0.0, 1.0, 0.0)));
    let up = cross(right, forward);

    var rd = normalize(forward + uv.x * right + uv.y * up);

    if (u.iMouse.z > 0.0) {
        let m = (u.iMouse.xy - 0.5 * u.iResolution.xy) / u.iResolution.y;
        let yaw = rot(m.x * 2.6);
        let pitch = rot(-m.y * 2.2);
        let xz = yaw * vec2<f32>(rd.x, rd.z);
        rd.x = xz.x;
        rd.z = xz.y;
        let yz = pitch * vec2<f32>(rd.y, rd.z);
        rd.y = yz.x;
        rd.z = yz.y;
        rd = normalize(rd);
    }

    var travelDist = 0.0;
    var hit = vec4<f32>(1e6, 0.0, 0.0, 0.0);
    var hitPos = ro;
    var glow = vec3<f32>(0.0);
    var fog = vec3<f32>(0.0);
    var didHit = false;

    for (var i = 0u; i < MAX_STEPS; i = i + 1u) {
        let p = ro + rd * travelDist;
        let s = scene(p);
        let g = glowField(p);
        let safeD = max(abs(s.x), 0.03);
        let atten = exp(-0.018 * travelDist);

        glow += g * atten * (0.016 / (0.08 + safeD * safeD * 12.0));
        fog += g * atten * 0.0016;

        if (s.x < SURF_DIST) {
            hit = s;
            hitPos = p;
            didHit = true;
            break;
        }

        if (travelDist > MAX_DIST) {
            break;
        }

        travelDist += clamp(s.x * 0.92, 0.025, 0.7);
    }

    var col = bgStars(rd) * 0.1 + fog;

    if (didHit) {
        let n = getNormal(hitPos);
        let lightA = normalize(vec3<f32>(0.7, 0.6, -0.45));
        let lightB = normalize(vec3<f32>(-0.4, 0.2, 0.9));
        let shadow = softShadow(hitPos + n * 0.03, lightA);
        let ao = ambientOcclusion(hitPos, n);
        let diffA = max(dot(n, lightA), 0.0) * shadow;
        let diffB = max(dot(n, lightB), 0.0);
        let halfVec = normalize(lightA - rd);
        let spec = pow(max(dot(n, halfVec), 0.0), 64.0) * shadow;
        let fres = pow(1.0 - max(dot(n, -rd), 0.0), 5.0);

        let accent = palette(hit.w * 0.09 + hitPos.z * 0.03 + t * 0.05);
        var base = vec3<f32>(0.08, 0.09, 0.11);
        var emissive = vec3<f32>(0.0);
        var specScale = 1.0;

        if (hit.y < 1.5) {
            base = lerp3(vec3<f32>(0.04, 0.05, 0.08), vec3<f32>(0.17, 0.11, 0.07), 0.35 + 0.35 * sin(hitPos.z * 0.3));
            emissive = glowField(hitPos + n * 0.08) * 0.08;
            specScale = 0.8;
        } else if (hit.y < 2.5) {
            base = lerp3(vec3<f32>(0.14, 0.22, 0.45), accent, 0.72);
            emissive = accent * (0.6 + 0.4 * sin(t * 3.0 + hit.w));
            specScale = 2.4;
        } else if (hit.y < 3.5) {
            base = lerp3(vec3<f32>(0.13, 0.07, 0.04), accent, 0.42);
            emissive = accent * 0.26;
            specScale = 1.2;
        } else {
            base = lerp3(vec3<f32>(0.06, 0.11, 0.16), accent, 0.3);
            emissive = accent * 0.14;
            specScale = 1.0;
        }

        let internalGlow = glowField(hitPos - n * 0.06);
        col = base * (0.12 + 0.88 * ao) * (0.18 + 0.82 * diffA + 0.22 * diffB);
        col += spec * specScale * lerp3(vec3<f32>(1.0), accent, 0.55);
        col += internalGlow * (0.1 + 0.35 * fres);
        col += emissive * (1.0 + 1.8 * fres);
    } else {
        col += palette(0.06 * rd.z + t * 0.03) * 0.03;
    }

    col += glow * 1.1;

    let depthFog = 1.0 - exp(-0.015 * travelDist);
    col = lerp3(col, vec3<f32>(0.02, 0.015, 0.04), depthFog * 0.7);

    let centerBloom = 0.025 / (0.03 + dot(uv, uv));
    col += palette(t * 0.04 + uv.x * 0.2) * centerBloom * 0.12;

    col *= 1.18;
    col = (col * (2.51 * col + 0.03)) / (col * (2.43 * col + 0.59) + 0.14);
    col = pow(max(col, vec3<f32>(0.0)), vec3<f32>(0.4545));
    col *= 1.0 - 0.22 * dot(uv, uv);

    return vec4<f32>(col, 1.0);
}
