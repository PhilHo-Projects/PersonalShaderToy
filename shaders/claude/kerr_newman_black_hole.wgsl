// ============================================================================
// KERR-NEWMAN BLACK HOLE — WGSL Single-Pass Renderer
// Ported from the original GLSL by baopinshui (NPGS project)
//
// Full general-relativistic ray tracing in Kerr-Schild coordinates (+++-).
// Features: gravitational lensing, accretion disk with blackbody radiation,
// analytical Doppler + gravitational redshift, relativistic jets, photon ring,
// Hamiltonian correction for numerical stability.
// ============================================================================

// ---- Section 1: Constants ----

const PI: f32 = 3.14159265358979;
const TAU: f32 = 6.28318530717959;
const EPS: f32 = 1e-6;

const MASS: f32 = 0.5;
const SPIN: f32 = 0.99;
const CHARGE: f32 = 0.0;
const PHYS_A: f32 = 0.495;   // SPIN * MASS
const PHYS_Q: f32 = 0.0;     // CHARGE * MASS

const DISK_INNER: f32 = 1.5;
const DISK_OUTER: f32 = 25.0;
const DISK_THIN: f32 = 0.75;
const DISK_HOPPER: f32 = 0.24;

const FOV_RAD: f32 = 1.0472;
const MAX_STEPS: i32 = 120;
const ESCAPE_R: f32 = 500.0;

const TEMP_NORM: f32 = 100886.0;
const REDSHIFT_COLOR_EXP: f32 = 3.0;
const REDSHIFT_INTENSITY_EXP: f32 = 4.0;
const SHIFT_MAX: f32 = 1.0;
const BRIGHT_MUT: f32 = 1.0;
const DARK_MUT: f32 = 0.5;
const REDDENING: f32 = 0.3;
const SATURATION: f32 = 0.5;
const PHOTON_RING_BOOST: f32 = 7.0;
const PHOTON_RING_TEMP_BOOST: f32 = 2.0;
const PEAK_TEMP_NORM: f32 = 0.4879; // 0.05665278^0.25

// ---- Section 2: Structs ----

struct KerrGeo {
    r: f32, r2: f32, a2: f32, f: f32,
    grad_r: vec3<f32>, grad_f: vec3<f32>,
    l_up: vec4<f32>, l_down: vec4<f32>,
    inv_r2_a2: f32, inv_den_f: f32, num_f: f32,
};

struct GState {
    X: vec4<f32>,
    P: vec4<f32>,
};

// ---- Section 3: Utilities ----

fn saturateF(x: f32) -> f32 { return clamp(x, 0.0, 1.0); }
fn saturate3(v: vec3<f32>) -> vec3<f32> { return clamp(v, vec3<f32>(0.0), vec3<f32>(1.0)); }

fn safe_norm(v: vec3<f32>, fb: vec3<f32>) -> vec3<f32> {
    let l = length(v);
    if (l < 1e-8) { return fb; }
    return v / l;
}

fn soft_saturate(x: f32) -> f32 { return 1.0 - 1.0 / (max(x, 0.0) + 1.0); }

fn cubic_interp(x: f32) -> f32 { return 3.0 * x * x - 2.0 * x * x * x; }

fn random_step(input: vec2<f32>, seed: f32) -> f32 {
    return fract(sin(dot(input + fract(11.4514 * sin(seed)), vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// ---- Section 4: Hash & Noise ----

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn hash13(p: vec3<f32>) -> f32 {
    var q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn noise3(x: vec3<f32>) -> f32 {
    let p = floor(x);
    let f = fract(x);
    let u = f * f * (3.0 - 2.0 * f);
    let n000 = hash13(p);
    let n100 = hash13(p + vec3<f32>(1.0, 0.0, 0.0));
    let n010 = hash13(p + vec3<f32>(0.0, 1.0, 0.0));
    let n110 = hash13(p + vec3<f32>(1.0, 1.0, 0.0));
    let n001 = hash13(p + vec3<f32>(0.0, 0.0, 1.0));
    let n101 = hash13(p + vec3<f32>(1.0, 0.0, 1.0));
    let n011 = hash13(p + vec3<f32>(0.0, 1.0, 1.0));
    let n111 = hash13(p + vec3<f32>(1.0, 1.0, 1.0));
    return mix(mix(mix(n000, n100, u.x), mix(n010, n110, u.x), u.y),
               mix(mix(n001, n101, u.x), mix(n011, n111, u.x), u.y), u.z);
}

fn perlin3(pos: vec3<f32>) -> f32 {
    let pi = floor(pos);
    let pf = fract(pos);
    let sx = pf.x * pf.x * (3.0 - 2.0 * pf.x);
    let sy = pf.y * pf.y * (3.0 - 2.0 * pf.y);
    let sz = pf.z * pf.z * (3.0 - 2.0 * pf.z);
    let h = vec3<f32>(12.9898, 78.233, 213.765);
    let v000 = 2.0 * fract(sin(dot(pi, h)) * 43758.5453) - 1.0;
    let v100 = 2.0 * fract(sin(dot(pi + vec3<f32>(1.0, 0.0, 0.0), h)) * 43758.5453) - 1.0;
    let v010 = 2.0 * fract(sin(dot(pi + vec3<f32>(0.0, 1.0, 0.0), h)) * 43758.5453) - 1.0;
    let v110 = 2.0 * fract(sin(dot(pi + vec3<f32>(1.0, 1.0, 0.0), h)) * 43758.5453) - 1.0;
    let v001 = 2.0 * fract(sin(dot(pi + vec3<f32>(0.0, 0.0, 1.0), h)) * 43758.5453) - 1.0;
    let v101 = 2.0 * fract(sin(dot(pi + vec3<f32>(1.0, 0.0, 1.0), h)) * 43758.5453) - 1.0;
    let v011 = 2.0 * fract(sin(dot(pi + vec3<f32>(0.0, 1.0, 1.0), h)) * 43758.5453) - 1.0;
    let v111 = 2.0 * fract(sin(dot(pi + vec3<f32>(1.0, 1.0, 1.0), h)) * 43758.5453) - 1.0;
    return mix(mix(mix(v000, v100, sx), mix(v010, v110, sx), sy),
               mix(mix(v001, v101, sx), mix(v011, v111, sx), sy), sz);
}

fn gen_disk_noise(pos: vec3<f32>, start_lvl: f32, end_lvl: f32, contrast: f32) -> f32 {
    var acc = 10.0;
    let i_start = i32(floor(start_lvl));
    let i_end = i32(ceil(end_lvl));
    let max_iter = min(i_end - i_start, 5);
    for (var delta = 0; delta < max_iter; delta += 1) {
        let i = i_start + delta;
        let fi = f32(i);
        let w = max(0.0, min(end_lvl, fi + 1.0) - max(start_lvl, fi));
        if (w > 0.0) {
            let freq = pow(3.0, fi);
            let n = perlin3(pos * freq);
            acc *= (1.0 + 0.1 * n * w);
        }
    }
    return log(1.0 + pow(max(0.1 * acc, 0.0), contrast));
}

fn fbm3(p_in: vec3<f32>) -> f32 {
    var p = p_in;
    var val = 0.0;
    var amp = 0.5;
    for (var i = 0; i < 4; i += 1) {
        val += amp * noise3(p);
        p = p * 2.03 + vec3<f32>(1.7, -0.8, 0.5);
        amp *= 0.5;
    }
    return val;
}

// ---- Section 5: Color Science ----

fn kelvin_to_rgb(kelvin: f32) -> vec3<f32> {
    if (kelvin < 400.0) { return vec3<f32>(0.0); }
    let teff = (kelvin - 6500.0) / (6500.0 * kelvin * 2.2);
    var rgb = vec3<f32>(
        exp(2.05539304e4 * teff),
        exp(2.63463675e4 * teff),
        exp(3.30145739e4 * teff)
    );
    var scale = 1.0 / max(max(1.5 * rgb.r, rgb.g), rgb.b);
    if (kelvin < 1000.0) { scale *= (kelvin - 400.0) / 600.0; }
    return rgb * scale;
}

// ---- Section 6: Kerr-Newman Geometry ----

fn kerr_schild_r(p: vec3<f32>, a: f32, r_sign: f32) -> f32 {
    if (a == 0.0) { return r_sign * length(p); }
    let a2 = a * a;
    let rho2 = p.x * p.x + p.z * p.z;
    let y2 = p.y * p.y;
    let b = rho2 + y2 - a2;
    let det = sqrt(b * b + 4.0 * a2 * y2);
    var r2: f32;
    if (b >= 0.0) { r2 = 0.5 * (b + det); }
    else { r2 = (2.0 * a2 * y2) / max(1e-20, det - b); }
    return r_sign * sqrt(max(r2, 0.0));
}

fn geo_scalars(X: vec3<f32>, a: f32, Q: f32, r_sign: f32) -> KerrGeo {
    var g: KerrGeo;
    g.a2 = a * a;
    g.grad_r = vec3<f32>(0.0);
    g.grad_f = vec3<f32>(0.0);

    if (a == 0.0) {
        g.r = r_sign * length(X);
        g.r2 = g.r * g.r;
        let inv_r = 1.0 / max(abs(g.r), 1e-20);
        let inv_r2 = inv_r * inv_r;
        g.l_up = vec4<f32>(X * inv_r, -1.0);
        g.l_down = vec4<f32>(X * inv_r, 1.0);
        g.num_f = 2.0 * MASS * g.r - Q * Q;
        g.f = (2.0 * MASS * inv_r - Q * Q * inv_r2);
        g.inv_r2_a2 = inv_r2;
        g.inv_den_f = 0.0;
        return g;
    }

    g.r = kerr_schild_r(X, a, r_sign);
    g.r2 = g.r * g.r;
    let r3 = g.r2 * g.r;
    let z2 = X.y * X.y;
    g.inv_r2_a2 = 1.0 / max(g.r2 + g.a2, 1e-20);
    let lx = (g.r * X.x - a * X.z) * g.inv_r2_a2;
    let ly = X.y / max(abs(g.r), 1e-20);
    let lz = (g.r * X.z + a * X.x) * g.inv_r2_a2;
    g.l_up = vec4<f32>(lx, ly, lz, -1.0);
    g.l_down = vec4<f32>(lx, ly, lz, 1.0);
    g.num_f = 2.0 * MASS * r3 - Q * Q * g.r2;
    let den_f = g.r2 * g.r2 + g.a2 * z2;
    g.inv_den_f = 1.0 / max(den_f, 1e-20);
    g.f = g.num_f * g.inv_den_f;
    return g;
}

fn geo_gradients(X: vec3<f32>, a: f32, Q: f32, gin: KerrGeo) -> KerrGeo {
    var g = gin;
    let inv_r = 1.0 / max(abs(g.r), 1e-20);

    if (a == 0.0) {
        let inv_r2 = inv_r * inv_r;
        g.grad_r = X * inv_r;
        let df_dr = (-2.0 * MASS + 2.0 * Q * Q * inv_r) * inv_r2;
        g.grad_f = df_dr * g.grad_r;
        return g;
    }

    let R2 = dot(X, X);
    let D = 2.0 * g.r2 - R2 + g.a2;
    var dg = g.r * D;
    if (abs(dg) < 1e-9) { dg = max(abs(g.r), 1.0) * 1e-9 * sign(g.r); }
    let id = 1.0 / dg;
    g.grad_r = vec3<f32>(X.x * g.r2, X.y * (g.r2 + g.a2), X.z * g.r2) * id;

    let z2 = X.y * X.y;
    let tm = -2.0 * MASS * g.r2 * g.r2 * g.r;
    let tq = 2.0 * Q * Q * g.r2 * g.r2;
    let tma = 6.0 * MASS * g.a2 * g.r * z2;
    let tqa = -2.0 * Q * Q * g.a2 * z2;
    let df_dr = (g.r * (tm + tq + tma + tqa)) * (g.inv_den_f * g.inv_den_f);
    let df_dy = -(g.num_f * 2.0 * g.a2 * X.y) * (g.inv_den_f * g.inv_den_f);
    g.grad_f = df_dr * g.grad_r + vec3<f32>(0.0, df_dy, 0.0);
    return g;
}

// ---- Section 7: Index Operations ----

fn lower_idx(P: vec4<f32>, g: KerrGeo) -> vec4<f32> {
    let pf = vec4<f32>(P.xyz, -P.w);
    return pf + g.f * dot(g.l_down, P) * g.l_down;
}

fn raise_idx(P: vec4<f32>, g: KerrGeo) -> vec4<f32> {
    let pf = vec4<f32>(P.xyz, -P.w);
    return pf - g.f * dot(g.l_up, P) * g.l_up;
}

// ---- Section 8: Geodesic Equations ----

fn get_derivs(S: GState, a: f32, Q: f32, gin: KerrGeo) -> GState {
    var d: GState;
    let g = geo_gradients(S.X.xyz, a, Q, gin);
    let ldp = dot(g.l_up.xyz, S.P.xyz) + g.l_up.w * S.P.w;
    d.X = vec4<f32>(S.P.xyz, -S.P.w) - g.f * ldp * g.l_up;

    let grad_A = (-2.0 * g.r * g.inv_r2_a2) * g.inv_r2_a2 * g.grad_r;
    let rx_az = g.r * S.X.x - a * S.X.z;
    let rz_ax = g.r * S.X.z + a * S.X.x;
    let d_lx = S.X.x * g.grad_r + vec3<f32>(g.r, 0.0, -a);
    let grad_lx = g.inv_r2_a2 * d_lx + rx_az * grad_A;
    let grad_ly = (g.r * vec3<f32>(0.0, 1.0, 0.0) - S.X.y * g.grad_r) / max(g.r2, 1e-20);
    let d_lz = S.X.z * g.grad_r + vec3<f32>(a, 0.0, g.r);
    let grad_lz = g.inv_r2_a2 * d_lz + rz_ax * grad_A;
    let pdgl = S.P.x * grad_lx + S.P.y * grad_ly + S.P.z * grad_lz;
    d.P = vec4<f32>(0.5 * ((ldp * ldp) * g.grad_f + (2.0 * g.f * ldp) * pdgl), 0.0);
    return d;
}

fn intermediate_sign(s: vec4<f32>, c: vec4<f32>, cs: f32, a: f32) -> f32 {
    if (s.y * c.y < 0.0) {
        let t = s.y / (s.y - c.y);
        if (length(mix(s.xz, c.xz, t)) < abs(a)) { return -cs; }
    }
    return cs;
}

// ---- Section 9: Hamiltonian Correction ----

fn ham_correct(Pin: vec4<f32>, X: vec4<f32>, E: f32, a: f32, Q: f32, rs: f32) -> vec4<f32> {
    var P = Pin;
    P.w = -E;
    let g = geo_scalars(X.xyz, a, Q, rs);
    let lds = dot(g.l_up.xyz, P.xyz);
    let p2 = dot(P.xyz, P.xyz);
    let cA = p2 - g.f * lds * lds;
    let cB = 2.0 * g.f * lds * P.w;
    let cC = -P.w * P.w * (1.0 + g.f);
    let disc = cB * cB - 4.0 * cA * cC;
    if (disc >= 0.0 && abs(cA) > 1e-9) {
        let sd = sqrt(disc);
        let dn = 2.0 * cA;
        let k1 = (-cB + sd) / dn;
        let k2 = (-cB - sd) / dn;
        var k: f32;
        if (abs(k1 - 1.0) < abs(k2 - 1.0)) { k = k1; } else { k = k2; }
        k = mix(k, 1.0, saturateF(abs(k - 1.0) / 0.1 - 1.0));
        P = vec4<f32>(P.xyz * k, P.w);
    }
    return P;
}

// ---- Section 10: Initial Momentum ----

fn init_momentum(rd: vec3<f32>, X: vec4<f32>, a: f32, Q: f32, us: f32) -> vec4<f32> {
    let g = geo_scalars(X.xyz, a, Q, us);
    let g_tt = -1.0 + g.f;
    let tc = 1.0 / sqrt(max(1e-9, -g_tt));
    let U_up = vec4<f32>(0.0, 0.0, 0.0, tc);
    let U_dn = lower_idx(U_up, g);

    let m_r = safe_norm(-X.xyz, vec3<f32>(0.0, 0.0, 1.0));
    var wu = vec3<f32>(0.0, 1.0, 0.0);
    if (abs(dot(m_r, wu)) > 0.999) { wu = vec3<f32>(1.0, 0.0, 0.0); }
    let m_p = safe_norm(cross(wu, m_r), vec3<f32>(1.0, 0.0, 0.0));
    let m_t = safe_norm(cross(m_p, m_r), vec3<f32>(0.0, 1.0, 0.0));

    let kr = dot(rd, m_r);
    let kt = dot(rd, m_t);
    let kp = dot(rd, m_p);

    var e1 = vec4<f32>(m_r, 0.0);
    e1 += dot(e1, U_dn) * U_up;
    var e1d = lower_idx(e1, g);
    let n1 = sqrt(max(1e-9, dot(e1, e1d)));
    e1 /= n1; e1d /= n1;

    var e2 = vec4<f32>(m_t, 0.0);
    e2 += dot(e2, U_dn) * U_up;
    e2 -= dot(e2, e1d) * e1;
    var e2d = lower_idx(e2, g);
    let n2 = sqrt(max(1e-9, dot(e2, e2d)));
    e2 /= n2; e2d /= n2;

    var e3 = vec4<f32>(m_p, 0.0);
    e3 += dot(e3, U_dn) * U_up;
    e3 -= dot(e3, e1d) * e1;
    e3 -= dot(e3, e2d) * e2;
    let e3d = lower_idx(e3, g);
    let n3 = sqrt(max(1e-9, dot(e3, e3d)));
    e3 /= n3;

    return lower_idx(U_up - (kr * e1 + kt * e2 + kp * e3), g);
}

// ---- Section 11: RK4 Step (with precomputed k1) ----

fn rk4_pre(s0: GState, k1: GState, a: f32, Q: f32, dt: f32, rs: f32) -> GState {
    var s1: GState;
    s1.X = s0.X + 0.5 * dt * k1.X;
    s1.P = s0.P + 0.5 * dt * k1.P;
    let si1 = intermediate_sign(s0.X, s1.X, rs, a);
    let g1 = geo_scalars(s1.X.xyz, a, Q, si1);
    let k2 = get_derivs(s1, a, Q, g1);

    var s2: GState;
    s2.X = s0.X + 0.5 * dt * k2.X;
    s2.P = s0.P + 0.5 * dt * k2.P;
    let si2 = intermediate_sign(s0.X, s2.X, rs, a);
    let g2 = geo_scalars(s2.X.xyz, a, Q, si2);
    let k3 = get_derivs(s2, a, Q, g2);

    var s3: GState;
    s3.X = s0.X + dt * k3.X;
    s3.P = s0.P + dt * k3.P;
    let si3 = intermediate_sign(s0.X, s3.X, rs, a);
    let g3 = geo_scalars(s3.X.xyz, a, Q, si3);
    let k4 = get_derivs(s3, a, Q, g3);

    var nxt: GState;
    nxt.X = s0.X + (dt / 6.0) * (k1.X + 2.0 * k2.X + 2.0 * k3.X + k4.X);
    nxt.P = s0.P + (dt / 6.0) * (k1.P + 2.0 * k2.P + 2.0 * k3.P + k4.P);
    return nxt;
}

// ---- Section 12: Keplerian Angular Velocity ----

fn kepler_omega(r: f32, a: f32, Q: f32) -> f32 {
    let mq = MASS * r - Q * Q;
    if (mq < 0.0) { return 0.0; }
    let sq = sqrt(mq);
    return sq / max(EPS, r * r + 0.5 * a * sq);
}

// ---- Section 13: Shape Function ----

fn shape_fn(x: f32, alpha: f32, beta: f32) -> f32 {
    let k = pow(alpha + beta, alpha + beta) / (pow(alpha, alpha) * pow(beta, beta));
    return k * pow(max(x, 0.0), alpha) * pow(max(1.0 - x, 0.0), beta);
}

fn vec2_to_theta(v1: vec2<f32>, v2: vec2<f32>) -> f32 {
    let d = dot(v1, v2);
    let c = v1.x * v2.y - v1.y * v2.x;
    let ang = asin(clamp(0.999999 * c / max(length(v1) * length(v2), 1e-9), -1.0, 1.0));
    let dx = step(0.0, d);
    let cx = step(0.0, c);
    return mix(mix(-PI - ang, PI - ang, cx), ang, dx);
}

// ---- Section 14: Background ----

fn star_layer(uv: vec2<f32>, thresh: f32) -> vec3<f32> {
    let cell = floor(uv);
    let loc = fract(uv) - 0.5;
    let h = hash12(cell);
    let sz = mix(18.0, 80.0, pow(h, 6.0));
    let spark = exp(-sz * dot(loc, loc)) * smoothstep(thresh, 1.0, h);
    return mix(vec3<f32>(0.70, 0.82, 1.10), vec3<f32>(1.25, 0.92, 0.70), hash12(cell + 17.0)) * spark;
}

fn sample_bg(dir_in: vec3<f32>) -> vec3<f32> {
    let dir = safe_norm(dir_in, vec3<f32>(0.0, 0.0, 1.0));
    let sph = vec2<f32>(atan2(dir.z, dir.x) / TAU + 0.5, acos(clamp(dir.y, -1.0, 1.0)) / PI);
    let band = fbm3(dir * 7.0);
    let dust = fbm3(dir * 12.0 + vec3<f32>(4.0, -2.0, 1.0));
    let gal = exp(-8.0 * abs(dir.y + 0.05 * sin(dir.x * 6.0))) * (0.35 + 0.65 * band);
    let warm = exp(-16.0 * abs(dir.y - 0.03)) * dust * dust;
    var col = vec3<f32>(0.015, 0.022, 0.038);
    col += vec3<f32>(0.10, 0.15, 0.26) * gal;
    col += vec3<f32>(0.20, 0.10, 0.06) * warm * 0.5;
    col += star_layer(sph * vec2<f32>(420.0, 220.0), 0.988) * 1.5;
    col += star_layer(sph * vec2<f32>(960.0, 520.0) + vec2<f32>(21.3, 11.8), 0.9978) * 2.0;
    return col;
}

// ---- Section 15: Accretion Disk (per-point sampler, matching GLSL DiskColor) ----

fn disk_sample_at(
    sample_pos: vec3<f32>, dir_y: f32, P_cov: vec4<f32>, E_obs: f32,
    emission_time: f32, shell_depth: f32
) -> vec4<f32> {
    let pos_r = kerr_schild_r(sample_pos, PHYS_A, 1.0);
    let pos_y = sample_pos.y;
    if (pos_r < DISK_INNER || pos_r > DISK_OUTER) { return vec4<f32>(0.0); }

    let geo_h = DISK_THIN + max(0.0, (length(sample_pos.xz) - 3.0) * DISK_HOPPER);

    // Non-linear effective radius (from GLSL)
    let x = (pos_r - DISK_INNER) / max(1e-6, DISK_OUTER - DISK_INNER);
    let a_p = max(1.0, (DISK_OUTER - DISK_INNER) / 10.0);
    var eff_r: f32;
    if (abs(a_p - 1.0) < 0.01) { eff_r = x; }
    else { eff_r = (-1.0 + sqrt(max(0.0, 1.0 + 4.0 * a_p * a_p * x - 4.0 * x * a_p))) / (2.0 * a_p - 2.0); }

    let density = shape_fn(eff_r, 0.9, 1.5);
    let nl = max(0.0, 2.0 - 0.6 * geo_h);

    // Inner cloud bounds
    let inner_eff_r = (pos_r - DISK_INNER) / min(DISK_OUTER - DISK_INNER, 12.0);
    let inner_cloud_bound = max(geo_h, DISK_THIN) * max(0.0, 1.0 - 5.0 * pow(max(inner_eff_r, 0.0), 2.0));
    let union_bound = max(geo_h * 1.5, max(0.0, inner_cloud_bound));
    if (abs(pos_y) > union_bound) { return vec4<f32>(0.0); }

    // Thickness noise using LogTheta
    let rot_r_thick = pos_r + 0.25 / 3.0 * emission_time;
    let log_theta_thick = vec2_to_theta(sample_pos.zx, vec2<f32>(cos(-2.0 * log(max(1e-6, pos_r))), sin(-2.0 * log(max(1e-6, pos_r)))));
    let thick_noise = gen_disk_noise(vec3<f32>(1.5 * log_theta_thick, rot_r_thick, 0.0), -0.7 + nl, 1.3 + nl, 80.0);

    let base_frac = 0.4 + 0.6 * clamp(geo_h - 0.5, 0.0, 2.5) / 2.5;
    let perturbed_h = max(1e-6, geo_h * density * (base_frac + (1.0 - base_frac) * soft_saturate(thick_noise)));

    let in_main = abs(pos_y) < perturbed_h;
    let in_cloud = abs(pos_y) < max(0.0, inner_cloud_bound);
    if (!in_main && !in_cloud) { return vec4<f32>(0.0); }

    // Spiral coordinates
    let u_val = sqrt(max(1e-6, pos_r));
    let kc = PHYS_A * 0.70710678;
    var spiral_theta: f32;
    if (abs(kc) < 0.001 * u_val * u_val * u_val) {
        spiral_theta = -16.9705627 / u_val;
    } else {
        let k = sign(kc) * pow(abs(kc), 0.33333333);
        let lt = (pos_r - k * u_val + k * k) / max(1e-9, pow(u_val + k, 2.0));
        spiral_theta = (5.6568542 / k) * (0.5 * log(max(1e-9, abs(lt))) + 1.7320508 * (atan2(2.0 * u_val - k, 1.7320508 * k) - 1.5707963));
    }
    let pos_theta = vec2_to_theta(sample_pos.xz, vec2<f32>(cos(-spiral_theta), sin(-spiral_theta)));
    let pos_log_theta = vec2_to_theta(sample_pos.xz, vec2<f32>(cos(-2.0 * log(max(1e-6, pos_r))), sin(-2.0 * log(max(1e-6, pos_r)))));

    // Analytical redshift
    let omega = kepler_omega(max(DISK_INNER, pos_r), PHYS_A, PHYS_Q);
    let inv_r = 1.0 / max(1e-6, pos_r);
    let inv_r2 = inv_r * inv_r;
    let V_pot = inv_r - PHYS_Q * PHYS_Q * inv_r2;
    let g_tt_l = -(1.0 - V_pot);
    let g_tp = -PHYS_A * V_pot;
    let g_pp = pos_r * pos_r + PHYS_A * PHYS_A + PHYS_A * PHYS_A * V_pot;
    let norm_m = g_tt_l + 2.0 * omega * g_tp + omega * omega * g_pp;
    let u_t = 1.0 / sqrt(max(0.01, -norm_m));
    let P_phi = -sample_pos.x * P_cov.z + sample_pos.z * P_cov.x;
    let E_emit = u_t * (E_obs - omega * P_phi);
    let freq = 1.0 / max(1e-6, E_emit);

    // Temperature
    let temp_factor = pow(max(pow(inv_r, 3.0) * max(1.0 - sqrt(DISK_INNER * inv_r), 1e-6), 1e-20), 0.25);
    let disk_temp = TEMP_NORM * temp_factor;
    var vision_temp = disk_temp * pow(max(freq, 0.01), REDSHIFT_COLOR_EXP);

    // Brightness with radius scaling (from GLSL)
    let bwr_base = 0.05 * min(DISK_OUTER / 1000.0, 1000.0 / DISK_OUTER);
    let bwr = bwr_base + 0.55 / exp(5.0 * eff_r) * mix(0.2 + 0.8 * abs(dir_y), 1.0, clamp(geo_h - 0.8, 0.2, 1.0));
    let bwr2 = bwr * pow(max(temp_factor / PEAK_TEMP_NORM, 0.001), 0.5);

    let rot_pos_r = pos_r + 0.25 / 3.0 * emission_time;
    var sc = vec4<f32>(0.0);
    // GLSL Density = 0.7 * VertMix * DenAndThiFactor^2 (squared!)
    var eff_d = 0.0;

    // 1. Main disk noise with level-dependent parameters
    if (in_main) {
        let levelmut = 0.91 * log(1.0 + 0.06 / 0.91 * max(0.0, min(1000.0, pos_r) - 10.0));
        let conmut = 80.0 * log(1.0 + 0.006 * max(0.0, min(1000000.0, pos_r) - 10.0));

        var main_n = gen_disk_noise(
            vec3<f32>(0.1 * rot_pos_r, 0.1 * pos_y, 0.02 * pow(DISK_OUTER, 0.7) * pos_theta),
            nl + 2.0 - levelmut, nl + 4.0 - levelmut, 80.0 - conmut
        );
        // Angular wrap at theta ~ -PI
        if (pos_theta + PI < 0.1 * PI) {
            let bl = (pos_theta + PI) / (0.1 * PI);
            let wn = gen_disk_noise(
                vec3<f32>(0.1 * rot_pos_r, 0.1 * pos_y, 0.02 * pow(DISK_OUTER, 0.7) * (pos_theta + TAU)),
                nl + 2.0 - levelmut, nl + 4.0 - levelmut, 80.0 - conmut
            );
            main_n = main_n * bl + wn * (1.0 - bl);
        }

        // 2. Spiral arm modulation (outer disk)
        if (pos_r > max(0.15379 * DISK_OUTER, 0.15379 * 64.0)) {
            let ts = pos_r * 4.65114e-6 - 0.1 / 3.0 * emission_time;
            var spir = gen_disk_noise(
                vec3<f32>(0.1 * (ts - 0.08 * DISK_OUTER * pos_log_theta), 0.1 * pos_y, 0.02 * pow(DISK_OUTER, 0.7) * pos_log_theta),
                nl + 2.0 - levelmut, nl + 3.0 - levelmut, 80.0 - conmut
            );
            if (pos_log_theta + PI < 0.1 * PI) {
                let bl = (pos_log_theta + PI) / (0.1 * PI);
                let wn = gen_disk_noise(
                    vec3<f32>(0.1 * (ts - 0.08 * DISK_OUTER * (pos_log_theta + TAU)), 0.1 * pos_y, 0.02 * pow(DISK_OUTER, 0.7) * (pos_log_theta + TAU)),
                    nl + 2.0 - levelmut, nl + 3.0 - levelmut, 80.0 - conmut
                );
                spir = spir * bl + wn * (1.0 - bl);
            }
            let mf = 0.5 + 0.5 * max(-1.0, 1.0 - exp(-0.15 * (100.0 * pos_r / max(DISK_OUTER, 64.0) - 20.0)));
            main_n *= mix(1.0, clamp(0.7 * spir * 1.5 - 0.5, 0.0, 3.0), mf);
        }

        let vert_mix = max(0.0, 1.0 - abs(pos_y) / perturbed_h);
        // GLSL: Density *= 0.7 * VerticalMixFactor * Density  (squares the density)
        eff_d = 0.7 * vert_mix * density * density;
        sc = vec4<f32>(vec3<f32>(main_n * eff_d * 1.4), eff_d * eff_d / 0.3);

        // Vertical emission profile
        let rel_h = clamp(abs(pos_y) / perturbed_h, 0.0, 1.0);
        sc = vec4<f32>(sc.xyz * max(0.0, 0.2 + 2.0 * sqrt(max(0.0, rel_h * rel_h + 0.001))), sc.w);
    }

    // Photon ring boost
    sc = vec4<f32>(sc.xyz * (1.0 + PHOTON_RING_BOOST * saturateF(0.3 * shell_depth - 0.1)), sc.w);
    vision_temp *= 1.0 + PHOTON_RING_TEMP_BOOST * saturateF(0.3 * shell_depth - 0.1);

    // 3. Inner cloud dust
    if (in_cloud) {
        let inner_omega = kepler_omega(3.0, PHYS_A, PHYS_Q);
        let inner_phase = PI / (PI / max(1e-6, inner_omega)) * emission_time;
        let inner_rot = 0.666666 * inner_phase;
        let inner_theta = vec2_to_theta(sample_pos.xz, vec2<f32>(cos(inner_rot), sin(inner_rot)));
        let dust_bound = max(geo_h, DISK_THIN) * max(0.0, 1.0 - 5.0 * pow(max(inner_eff_r, 0.0), 2.0));
        let dust_int = max(0.0, 1.0 - pow(pos_y / max(1e-6, dust_bound), 2.0));
        if (dust_int > 0.0) {
            let dust_n = gen_disk_noise(
                vec3<f32>(1.5 * fract((1.5 * inner_theta + inner_phase) / TAU) * TAU, pos_r, pos_y),
                0.0, 6.0, 80.0
            );
            let dv = dust_int * dust_n;
            let sf = sqrt(max(0.0, 1.0001 - dir_y * dir_y));
            sc += 0.02 * vec4<f32>(vec3<f32>(dv), 0.2 * dv) * sf;
        }
    }

    // Apply color, redshift, edge fade
    sc = vec4<f32>(sc.xyz * bwr2 * kelvin_to_rgb(vision_temp) * min(pow(max(freq, 0.01), REDSHIFT_INTENSITY_EXP), SHIFT_MAX), sc.w);
    sc *= min(1.0, 1.3 * (DISK_OUTER - pos_r) / (DISK_OUTER - DISK_INNER));
    sc = vec4<f32>(sc.xyz, sc.w * 0.125);

    // Boost factors (from GLSL)
    let boost1 = 5.0 / (max(DISK_THIN, 0.2) + DISK_HOPPER * 0.5 * DISK_OUTER);
    let boost2_base = 100.0 / DISK_OUTER;
    let boost2_mix = exp(-pow(20.0 * pos_r / DISK_OUTER, 2.0));
    let boost_rgb = max(boost1, mix(boost2_base, 0.3 + 0.7 * boost2_base, boost2_mix));
    let boost_a = max(boost1, mix(boost2_base, 1.0, boost2_mix));
    sc = vec4<f32>(sc.xyz * boost_rgb, sc.w * boost_a);

    // Viewing angle & thin disk boosts (GLSL uses squared density)
    sc = vec4<f32>(sc.xyz * mix(1.0, max(1.0, abs(dir_y) / 0.2), clamp(0.3 - 0.6 * (perturbed_h / max(1e-6, eff_d) - 1.0), 0.0, 0.3)), sc.w);
    sc = vec4<f32>(sc.xyz * (1.0 + 1.2 * max(0.0, max(0.0, min(1.0, 3.0 - 2.0 * DISK_THIN)) * min(0.5, 1.0 - 5.0 * DISK_HOPPER))), sc.w);

    // Brightness/dark mutators
    let xf = (pos_r - DISK_INNER) / (DISK_OUTER - DISK_INNER);
    sc = vec4<f32>(sc.xyz * BRIGHT_MUT * clamp(4.0 - 18.0 * xf, 1.0, 4.0), sc.w * DARK_MUT * clamp(5.0 - 24.0 * xf, 1.0, 5.0));

    return sc;
}

// ---- Section 16: Jets ----

fn sample_jet(pos: vec3<f32>, P_cov: vec4<f32>, a: f32, Q: f32, time: f32) -> vec3<f32> {
    let rho = length(pos.xz);
    let rho2 = rho * rho;
    let pr = kerr_schild_r(pos, a, 1.0);
    let py = pos.y;
    let wid = abs(py);
    var accum = vec3<f32>(0.0);
    var in_jet = false;

    if (rho2 < 2.0 * DISK_INNER * DISK_INNER + 0.0009 * py * py && pr < 1.414 * DISK_OUTER) {
        in_jet = true;
        let shp = 1.0 / sqrt(max(1e-9, DISK_INNER * DISK_INNER + 0.0004 * py * py));
        let core = max(0.0, 1.0 - 5.0 * shp * abs(1.0 - pow(rho * shp, 2.0))) * shp;
        let hm = max(0.0, 1.0 - exp(-0.0001 * py * py / max(1e-6, DISK_INNER * DISK_INNER)));
        let df = exp(-2.0 * pr * pr / max(1e-6, DISK_OUTER * DISK_OUTER));
        accum += vec3<f32>(0.5) * core * hm * df;
    }

    if (rho < 1.3 * DISK_INNER + 0.25 * wid && rho > 0.7 * DISK_INNER + 0.15 * wid && pr < 30.0 * DISK_INNER) {
        in_jet = true;
        let shp = 1.0 / max(1e-9, DISK_INNER + 0.2 * wid);
        let shell = max(0.0, 1.0 - 2.0 * abs(1.0 - pow(rho * shp, 2.0))) * shp;
        let h1 = 1.0 - exp(-py * py / max(1e-6, DISK_INNER * DISK_INNER));
        let h2 = exp(-0.005 * py * py / max(1e-6, DISK_INNER * DISK_INNER));
        accum += vec3<f32>(0.5) * shell * h1 * h2;
    }

    if (!in_jet) { return vec3<f32>(0.0); }

    let jv = safe_norm(vec3<f32>(pos.z, 0.0, -pos.x), vec3<f32>(1.0, 0.0, 0.0)) * 0.05 + vec3<f32>(0.0, sign(py), 0.0) * 0.9;
    let uj = vec4<f32>(jv, 1.0);
    let g = geo_scalars(pos, a, Q, 1.0);
    let ul = lower_idx(uj, g);
    let nsq = dot(uj, ul);
    let uj_n = uj * (1.0 / sqrt(max(1e-6, abs(nsq))));
    let E_emit = -(P_cov.x * uj_n.x + P_cov.y * uj_n.y + P_cov.z * uj_n.z + P_cov.w * uj_n.w);
    let freq = 1.0 / max(1e-6, E_emit);

    let jt = 100000.0 * freq;
    return accum * kelvin_to_rgb(jt) * min(pow(max(freq, 0.01), 2.0), 3.0);
}

// ---- Section 17: Tone Mapping ----

fn tone_map(c_in: vec3<f32>) -> vec3<f32> {
    // Filmic tone mapping suitable for single-pass (no bloom)
    // Gentle Reinhard with color preservation + warm color grading
    var c = c_in;

    // Exposure boost to compensate for missing bloom
    c *= 1.8;

    // Luminance-preserving Reinhard
    let lum = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    if (lum < 1e-6) { return vec3<f32>(0.0); }
    let mapped_lum = lum / (1.0 + lum);
    c *= mapped_lum / lum;

    // S-curve for contrast
    c = c * c * (vec3<f32>(3.0) - 2.0 * c);

    // Warm color grade (slightly boost reds, reduce blues)
    c = pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.15, 1.10, 1.0));
    c = saturate3(c * 1.01);

    // Gamma
    c = pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    return c;
}

// ---- Section 18: Main Shader ----

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let res = u.iResolution.xy;
    let uv = frag_coord.xy / res;
    let time = u.iTime * 2.0;

    // Camera
    let fov = tan(FOV_RAD * 0.5);
    var cam_pos = vec3<f32>(-2.0, -3.6, 22.0);
    var cam_fwd = normalize(vec3<f32>(0.0, 0.15, -1.0));
    var cam_right = normalize(cross(cam_fwd, vec3<f32>(-0.5, 1.0, 0.0)));
    var cam_up = normalize(cross(cam_right, cam_fwd));

    // Mouse or auto-orbit
    if (u.iMouse.z > 0.0) {
        let mouse = (u.iMouse.xy - 0.5 * res) / res.y;
        let yaw = -mouse.x * 3.0;
        let pitch = mouse.y * 2.0;
        // Yaw
        let cy = cos(yaw); let sy = sin(yaw);
        cam_fwd = vec3<f32>(cy * cam_fwd.x + sy * cam_fwd.z, cam_fwd.y, -sy * cam_fwd.x + cy * cam_fwd.z);
        cam_right = vec3<f32>(cy * cam_right.x + sy * cam_right.z, cam_right.y, -sy * cam_right.x + cy * cam_right.z);
        // Pitch
        let cp = cos(pitch); let sp = sin(pitch);
        cam_fwd = vec3<f32>(cam_fwd.x, cp * cam_fwd.y - sp * cam_fwd.z, sp * cam_fwd.y + cp * cam_fwd.z);
        cam_up = normalize(cross(cam_right, cam_fwd));
        cam_right = normalize(cross(cam_fwd, cam_up));
    } else {
        // Gentle auto-orbit
        let t = u.iTime * 0.04;
        let yaw = 0.15 * sin(t);
        let pitch = 0.05 * sin(t * 0.7);
        let cy = cos(yaw); let sy = sin(yaw);
        cam_fwd = vec3<f32>(cy * cam_fwd.x + sy * cam_fwd.z, cam_fwd.y, -sy * cam_fwd.x + cy * cam_fwd.z);
        cam_right = vec3<f32>(cy * cam_right.x + sy * cam_right.z, cam_right.y, -sy * cam_right.x + cy * cam_right.z);
        let cp = cos(pitch); let sp = sin(pitch);
        cam_fwd = vec3<f32>(cam_fwd.x, cp * cam_fwd.y - sp * cam_fwd.z, sp * cam_fwd.y + cp * cam_fwd.z);
        cam_up = normalize(cross(cam_right, cam_fwd));
        cam_right = normalize(cross(cam_fwd, cam_up));
    }

    // Flip Y to match Shadertoy convention (y=0 at bottom)
    let pixel_uv = vec2<f32>(frag_coord.x - 0.5 * res.x, 0.5 * res.y - frag_coord.y) / res.y;
    let ray_dir = normalize(cam_fwd + fov * (pixel_uv.x * cam_right + pixel_uv.y * cam_up));

    // Event horizon
    let h_disc = MASS * MASS - PHYS_A * PHYS_A - PHYS_Q * PHYS_Q;
    var evt_r = 0.0;
    var naked = false;
    if (h_disc >= 0.0) { evt_r = MASS + sqrt(h_disc); }
    else { naked = true; }

    // Initialize ray
    var st: GState;
    st.X = vec4<f32>(cam_pos, 0.0);
    var uni_s = 1.0;
    st.P = init_momentum(ray_dir, st.X, PHYS_A, PHYS_Q, uni_s);
    let E_cons = -st.P.w;

    var result = vec4<f32>(0.0);
    var hit_h = false;
    var last_r = kerr_schild_r(cam_pos, PHYS_A, 1.0);
    var shell_depth = 0.0;
    var ray_dir_cur = ray_dir;
    var last_dr = 0.0;
    var turn_count = 0;

    // Bounding sphere skip
    let cam_dist = length(cam_pos);
    let boundary = max(DISK_OUTER + 1.0, 501.0);
    if (cam_dist > boundary) {
        let b = dot(cam_pos, ray_dir);
        let c_val = cam_dist * cam_dist - (boundary - 1.0) * (boundary - 1.0);
        let delta = b * b - c_val;
        if (delta < 0.0) {
            return vec4<f32>(tone_map(sample_bg(ray_dir)), 1.0);
        }
        let t_enter = -b - sqrt(max(delta, 0.0));
        if (t_enter > 0.0) {
            // Advance ray to bounding sphere
            // We can't modify X directly in init, so we re-init after moving
            st.X = vec4<f32>(cam_pos + ray_dir * t_enter * 0.99, 0.0);
            st.P = init_momentum(ray_dir, st.X, PHYS_A, PHYS_Q, uni_s);
        }
    }

    // Jitter for anti-aliasing and disk sub-stepping phase
    let jitter = random_step(uv, fract(u.iTime));
    var ray_march_phase = jitter;

    // Main ray marching loop
    for (var step_i = 0; step_i < MAX_STEPS; step_i += 1) {
        let g = geo_scalars(st.X.xyz, PHYS_A, PHYS_Q, uni_s);
        let r = g.r;

        // Termination
        if (!naked && uni_s > 0.0 && r < evt_r) { hit_h = true; break; }
        if (length(st.X.xyz) > boundary) { break; }
        if (result.a > 0.99) { break; }

        // Compute derivatives for adaptive step + precomputed k1
        let s0 = st;
        let k1 = get_derivs(s0, PHYS_A, PHYS_Q, g);

        // Adaptive step size (physics-based)
        let vel_mag = length(k1.X.xyz);
        let force_mag = length(k1.P.xyz);
        let mom_mag = length(st.P.xyz);
        let rho = length(st.X.xz);
        let dist_ring = sqrt(st.X.y * st.X.y + pow(max(rho - abs(PHYS_A), 0.0), 2.0));

        let step_geo = dist_ring / max(vel_mag, 1e-9);
        let step_force = mom_mag / max(force_mag, 1e-15);
        var dL = 0.5 * min(step_geo, step_force);
        dL = max(dL, 1e-7);

        // Radial turning detection
        let cur_dr = dot(g.grad_r, k1.X.xyz);
        if (step_i > 0 && cur_dr * last_dr < 0.0) { turn_count += 1; }
        last_dr = cur_dr;
        if (turn_count > 2) { break; }

        // Check for condensed photons near horizon
        let P_up = raise_idx(st.P, g);
        if (P_up.w > 10000.0 && !naked && uni_s > 0.0) { hit_h = true; break; }

        // Gravity fade at boundary
        // Gravity fade at boundary (reserved for multi-pass extension)
        // let gfade = cubic_interp(saturateF(1.0 - (length(st.X.xyz) - 100.0) / (boundary - 100.0)));

        // RK4 step with NEGATIVE affine parameter (critical for backward ray tracing!)
        st = rk4_pre(s0, k1, PHYS_A, PHYS_Q, -dL, uni_s);

        // Update universe sign
        if (s0.X.y * st.X.y < 0.0) {
            let t_c = s0.X.y / (s0.X.y - st.X.y);
            if (length(mix(s0.X.xz, st.X.xz, t_c)) < abs(PHYS_A)) {
                uni_s *= -1.0;
            }
        }

        // Hamiltonian correction (positive universe only)
        if (uni_s > 0.0) {
            st.P = ham_correct(st.P, st.X, E_cons, PHYS_A, PHYS_Q, uni_s);
        }

        let step_vec = st.X.xyz - s0.X.xyz;
        let step_len = max(length(step_vec), EPS);
        ray_dir_cur = step_vec / step_len;

        // Photon shell tracking
        let new_r = kerr_schild_r(st.X.xyz, PHYS_A, uni_s);
        let dr_dl = (new_r - last_r) / max(step_len, 1e-9);
        if (new_r < 1.6 + pow(abs(SPIN), 0.666666)) {
            let avg_r = 0.5 * last_r + 0.5 * new_r;
            shell_depth += step_len / max(avg_r, 0.1) / (1.0 + 1000.0 * dr_dl * dr_dl);
        }
        last_r = new_r;

        // Volumetric disk sub-stepping (matching GLSL DiskColor)
        if (uni_s > 0.0) {
            let max_h = DISK_THIN + max(0.0, DISK_HOPPER * DISK_OUTER) + 2.0;
            // Quick bounds check (both endpoints)
            let skip_y = (s0.X.y > max_h && st.X.y > max_h) || (s0.X.y < -max_h && st.X.y < -max_h);
            if (!skip_y) {
                // Closest approach check for radial bounds
                let p0_xz = s0.X.xz;
                let p1_xz = st.X.xz;
                let seg = p1_xz - p0_xz;
                let seg_sq = dot(seg, seg);
                var t_cl = 0.0;
                if (seg_sq > 1e-8) { t_cl = clamp(-dot(p0_xz, seg) / seg_sq, 0.0, 1.0); }
                let cp = p0_xz + seg * t_cl;
                let r_start = kerr_schild_r(s0.X.xyz, PHYS_A, 1.0);
                let r_end = kerr_schild_r(st.X.xyz, PHYS_A, 1.0);

                if (dot(cp, cp) < (DISK_OUTER * 1.1) * (DISK_OUTER * 1.1) && max(r_start, r_end) > DISK_INNER * 0.9) {
                    // Phase-based sub-stepping through the disk
                    let start_pos = s0.X.xyz;
                    let total_dist = step_len;
                    var traveled = 0.0;

                    for (var sub_i = 0; sub_i < 12; sub_i += 1) {
                        if (traveled >= total_dist || result.a > 0.99) { break; }

                        let cur_pos = start_pos + ray_dir_cur * traveled;
                        let dist_bh = length(cur_pos);

                        // Adaptive sub-step size (from GLSL)
                        let ssb = max(DISK_OUTER, 12.0);
                        var ss = 0.15 + 0.25 * min(max(0.0, 0.5 * (0.5 * dist_bh / max(10.0, ssb) - 1.0)), 1.0);
                        if (dist_bh >= 2.0 * ssb) { ss *= dist_bh; }
                        else if (dist_bh >= ssb) { ss *= ((1.0 + 0.25 * max(dist_bh - 12.0, 0.0)) * (2.0 * ssb - dist_bh) + dist_bh * (dist_bh - ssb)) / ssb; }
                        else { ss *= min(1.0 + 0.25 * max(dist_bh - 12.0, 0.0), dist_bh); }
                        ss = max(0.01, ss);

                        let dist_next = ray_march_phase * ss;
                        let remaining = total_dist - traveled;
                        if (dist_next > remaining) {
                            ray_march_phase -= remaining / ss;
                            ray_march_phase = max(ray_march_phase, 0.0);
                            break;
                        }

                        traveled += dist_next;
                        let sp = start_pos + ray_dir_cur * traveled;
                        let t_interp = min(1.0, traveled / max(1e-9, total_dist));
                        let e_time = time + mix(s0.X.w, st.X.w, t_interp);

                        let ds = disk_sample_at(sp, ray_dir_cur.y, st.P, E_cons, e_time, shell_depth);

                        if (ds.x + ds.y + ds.z > 1e-7 || ds.w > 1e-7) {
                            let step_col = ds * dist_next;
                            // Reddening accumulation (exact GLSL formula)
                            let aG = 1.0 + REDDENING * 2.0;
                            let aB = 1.0 + REDDENING * 5.0;
                            let rem_a = max(1.0 - result.a, 0.0);
                            let s_rgb = (step_col.r + step_col.g + step_col.b) * pow(rem_a, aG);
                            let denom = step_col.r * pow(rem_a, 1.0) + step_col.g * pow(rem_a, aG) + step_col.b * pow(rem_a, aB);
                            if (denom > 1e-6) {
                                var r_c = s_rgb * step_col.r * pow(rem_a, 1.0) / denom;
                                var g_c = s_rgb * step_col.g * pow(rem_a, aG) / denom;
                                var b_c = s_rgb * step_col.b * pow(rem_a, aB) / denom;
                                let cs = r_c + g_c + b_c;
                                if (cs > 1e-9) {
                                    r_c *= pow(max(3.0 * r_c / cs, 0.0), SATURATION);
                                    g_c *= pow(max(3.0 * g_c / cs, 0.0), SATURATION);
                                    b_c *= pow(max(3.0 * b_c / cs, 0.0), SATURATION);
                                }
                                result = vec4<f32>(result.xyz + vec3<f32>(r_c, g_c, b_c), result.w + step_col.w * rem_a);
                            }
                        }
                        ray_march_phase = 1.0;
                    }
                }
            }

            // Sample jets
            if (abs(st.X.y) > DISK_INNER * 0.3 && rho < DISK_OUTER * 0.5) {
                let jc = sample_jet(st.X.xyz, st.P, PHYS_A, PHYS_Q, time);
                let jl = (jc.r + jc.g + jc.b);
                if (jl > 1e-7) {
                    let rem = max(1.0 - result.a, 0.0);
                    result = vec4<f32>(result.xyz + jc * rem * step_len * 0.3, result.w);
                }
            }
        }
    }

    // Background for escaped rays
    var final_col = result.xyz;
    if (!hit_h && result.a < 0.99) {
        let rem = 1.0 - result.a;
        let g_esc = geo_scalars(st.X.xyz, PHYS_A, PHYS_Q, uni_s);
        let raised = raise_idx(st.P, g_esc);
        var esc_dir = safe_norm(raised.xyz, ray_dir);
        if (!(esc_dir.x == esc_dir.x)) { esc_dir = safe_norm(st.X.xyz, ray_dir); }
        final_col += sample_bg(esc_dir) * rem;
    }

    // NaN protection
    if (!(final_col.x == final_col.x) || !(final_col.y == final_col.y) || !(final_col.z == final_col.z)) {
        final_col = sample_bg(ray_dir) * 0.5;
    }

    final_col = tone_map(final_col);
    return vec4<f32>(final_col, 1.0);
}
