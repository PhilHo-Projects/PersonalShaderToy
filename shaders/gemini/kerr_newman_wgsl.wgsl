// True Kerr-Newman Black Hole Raytracer in WGSL
// Translated from GLSL mathematical exact geodesics

fn rot2(a: f32) -> mat2x2<f32> {
    let s = sin(a); let c = cos(a);
    return mat2x2<f32>(c, -s, s, c);
}

fn hash3(p_in: vec3<f32>) -> f32 {
    var p = fract(p_in * vec3<f32>(12.9898, 78.233, 45.164));
    p += dot(p, p.yzx + 33.33);
    return fract((p.x + p.y) * p.z);
}

fn noise(x: vec3<f32>) -> f32 {
    let p = floor(x);
    let f = fract(x);
    let f2 = f * f * (3.0 - 2.0 * f);
    
    let n000 = hash3(p + vec3<f32>(0.0, 0.0, 0.0));
    let n100 = hash3(p + vec3<f32>(1.0, 0.0, 0.0));
    let n010 = hash3(p + vec3<f32>(0.0, 1.0, 0.0));
    let n110 = hash3(p + vec3<f32>(1.0, 1.0, 0.0));
    let n001 = hash3(p + vec3<f32>(0.0, 0.0, 1.0));
    let n101 = hash3(p + vec3<f32>(1.0, 0.0, 1.0));
    let n011 = hash3(p + vec3<f32>(0.0, 1.0, 1.0));
    let n111 = hash3(p + vec3<f32>(1.0, 1.0, 1.0));
    
    let nx00 = mix(n000, n100, f2.x);
    let nx10 = mix(n010, n110, f2.x);
    let nx01 = mix(n001, n101, f2.x);
    let nx11 = mix(n011, n111, f2.x);
    
    let nxy0 = mix(nx00, nx10, f2.y);
    let nxy1 = mix(nx01, nx11, f2.y);
    
    return mix(nxy0, nxy1, f2.z);
}

fn fbm(p_in: vec3<f32>) -> f32 {
    var p = p_in;
    var f = 0.0;
    var w = 0.5;
    for (var i = 0; i < 5; i++) {
        f += w * noise(p);
        p = p * 2.0;
        w *= 0.5;
    }
    return f;
}

const CONST_M: f32 = 0.5;
const EPSILON: f32 = 1e-6;

struct KerrGeometry {
    r: f32,
    r2: f32,
    a2: f32,
    f: f32,
    grad_r: vec3<f32>,
    grad_f: vec3<f32>,
    l_up: vec4<f32>,
    l_down: vec4<f32>,
    inv_r2_a2: f32,
    inv_den_f: f32,
    num_f: f32,
};

fn KerrSchildRadius(p: vec3<f32>, PhysicalSpinA: f32, r_sign: f32) -> f32 {
    let r_sign_len = r_sign * length(p);
    if (PhysicalSpinA == 0.0) { return r_sign_len; }

    let a2 = PhysicalSpinA * PhysicalSpinA;
    let rho2 = dot(p.xz, p.xz);
    let y2 = p.y * p.y;
    
    let b = rho2 + y2 - a2;
    let det = sqrt(b * b + 4.0 * a2 * y2);
    
    var r2 = 0.0;
    if (b >= 0.0) {
        r2 = 0.5 * (b + det);
    } else {
        r2 = (2.0 * a2 * y2) / max(1e-20, det - b);
    }
    return r_sign * sqrt(r2);
}

fn ComputeGeometryScalars(X: vec3<f32>, PhysicalSpinA: f32, PhysicalQ: f32, r_sign: f32) -> KerrGeometry {
    var geo: KerrGeometry;
    geo.a2 = PhysicalSpinA * PhysicalSpinA;

    if (PhysicalSpinA == 0.0) {
        geo.r = r_sign * length(X);
        geo.r2 = geo.r * geo.r;
        let inv_r = 1.0 / geo.r;
        let inv_r2 = inv_r * inv_r;
        
        geo.l_up = vec4<f32>(X * inv_r, -1.0);
        geo.l_down = vec4<f32>(X * inv_r, 1.0);
        
        geo.num_f = (2.0 * CONST_M * geo.r - PhysicalQ * PhysicalQ);
        geo.f = (2.0 * CONST_M * inv_r - (PhysicalQ * PhysicalQ) * inv_r2);
        
        geo.inv_r2_a2 = inv_r2; 
        geo.inv_den_f = 0.0;
        return geo;
    }

    geo.r = KerrSchildRadius(X, PhysicalSpinA, r_sign);
    geo.r2 = geo.r * geo.r;
    let r3 = geo.r2 * geo.r;
    let z_coord = X.y;
    let z2 = z_coord * z_coord;
    
    geo.inv_r2_a2 = 1.0 / (geo.r2 + geo.a2);
    
    let lx = (geo.r * X.x - PhysicalSpinA * X.z) * geo.inv_r2_a2;
    let ly = X.y / geo.r;
    let lz = (geo.r * X.z + PhysicalSpinA * X.x) * geo.inv_r2_a2;
    
    geo.l_up = vec4<f32>(lx, ly, lz, -1.0);
    geo.l_down = vec4<f32>(lx, ly, lz, 1.0); 
    
    geo.num_f = 2.0 * CONST_M * r3 - PhysicalQ * PhysicalQ * geo.r2;
    let den_f = geo.r2 * geo.r2 + geo.a2 * z2;
    geo.inv_den_f = 1.0 / max(1e-20, den_f);
    geo.f = (geo.num_f * geo.inv_den_f);

    return geo;
}

fn ComputeGeometryGradients(X: vec3<f32>, PhysicalSpinA: f32, PhysicalQ: f32, geo_in: KerrGeometry) -> KerrGeometry {
    var geo = geo_in;
    let inv_r = 1.0 / geo.r;
    
    if (PhysicalSpinA == 0.0) {
        let inv_r2 = inv_r * inv_r;
        geo.grad_r = X * inv_r;
        let df_dr = (-2.0 * CONST_M + 2.0 * PhysicalQ * PhysicalQ * inv_r) * inv_r2;
        geo.grad_f = df_dr * geo.grad_r;
        return geo;
    }

    let R2 = dot(X, X);
    let D = 2.0 * geo.r2 - R2 + geo.a2;
    var denom_grad = geo.r * D;
    if (abs(denom_grad) < 1e-9) { denom_grad = sign(geo.r) * 1e-9; }
    let inv_denom_grad = 1.0 / denom_grad;
    
    geo.grad_r = vec3<f32>(
        X.x * geo.r2,
        X.y * (geo.r2 + geo.a2),
        X.z * geo.r2
    ) * inv_denom_grad;
    
    let z_coord = X.y;
    let z2 = z_coord * z_coord;
    
    let term_M  = -2.0 * CONST_M * geo.r2 * geo.r2 * geo.r;
    let term_Q  = 2.0 * PhysicalQ * PhysicalQ * geo.r2 * geo.r2;
    let term_Ma = 6.0 * CONST_M * geo.a2 * geo.r * z2;
    let term_Qa = -2.0 * PhysicalQ * PhysicalQ * geo.a2 * z2;
    
    let df_dr_num_reduced = term_M + term_Q + term_Ma + term_Qa;
    let df_dr = (geo.r * df_dr_num_reduced) * (geo.inv_den_f * geo.inv_den_f);
    
    let df_dy = -(geo.num_f * 2.0 * geo.a2 * z_coord) * (geo.inv_den_f * geo.inv_den_f);
    
    geo.grad_f = df_dr * geo.grad_r;
    geo.grad_f = vec3<f32>(geo.grad_f.x, geo.grad_f.y + df_dy, geo.grad_f.z);
    
    return geo;
}

fn LowerIndex(P_contra: vec4<f32>, geo: KerrGeometry) -> vec4<f32> {
    let P_flat = vec4<f32>(P_contra.xyz, -P_contra.w);
    let L_dot_P = dot(geo.l_down, P_contra);
    return P_flat + geo.f * L_dot_P * geo.l_down;
}

fn RaiseIndex(P_cov: vec4<f32>, geo: KerrGeometry) -> vec4<f32> {
    let P_flat = vec4<f32>(P_cov.xyz, -P_cov.w);
    let L_dot_P = dot(geo.l_up, P_cov);
    return P_flat - geo.f * L_dot_P * geo.l_up;
}

struct State {
    X: vec4<f32>,
    P: vec4<f32>,
};

fn GetDerivativesAnalytic(S: State, PhysicalSpinA: f32, PhysicalQ: f32, geo_in: KerrGeometry) -> State {
    var deriv: State;
    var geo = ComputeGeometryGradients(S.X.xyz, PhysicalSpinA, PhysicalQ, geo_in);
    
    let l_dot_P = dot(geo.l_up.xyz, S.P.xyz) + geo.l_up.w * S.P.w;
    let P_flat = vec4<f32>(S.P.xyz, -S.P.w); 
    deriv.X = P_flat - geo.f * l_dot_P * geo.l_up;
    
    let grad_A = (-2.0 * geo.r * geo.inv_r2_a2) * geo.inv_r2_a2 * geo.grad_r;
    
    let rx_az = geo.r * S.X.x - PhysicalSpinA * S.X.z;
    let rz_ax = geo.r * S.X.z + PhysicalSpinA * S.X.x;
    
    var d_num_lx = S.X.x * geo.grad_r; 
    d_num_lx.x += geo.r; 
    d_num_lx.z -= PhysicalSpinA;
    let grad_lx = geo.inv_r2_a2 * d_num_lx + rx_az * grad_A;
    
    let grad_ly = (geo.r * vec3<f32>(0.0, 1.0, 0.0) - S.X.y * geo.grad_r) / geo.r2;
    
    var d_num_lz = S.X.z * geo.grad_r;
    d_num_lz.z += geo.r;
    d_num_lz.x += PhysicalSpinA;
    let grad_lz = geo.inv_r2_a2 * d_num_lz + rz_ax * grad_A;
    
    let P_dot_grad_l = S.P.x * grad_lx + S.P.y * grad_ly + S.P.z * grad_lz;
    
    let Force = 0.5 * ( (l_dot_P * l_dot_P) * geo.grad_f + (2.0 * geo.f * l_dot_P) * P_dot_grad_l );
    deriv.P = vec4<f32>(Force, 0.0); 
    
    return deriv;
}

fn GetInitialMomentum(RayDir: vec3<f32>, X: vec4<f32>, PhysicalSpinA: f32, PhysicalQ: f32) -> vec4<f32> {
    let geo = ComputeGeometryScalars(X.xyz, PhysicalSpinA, PhysicalQ, 1.0);
    let g_tt = -1.0 + geo.f;
    let time_comp = 1.0 / sqrt(max(1e-9, -g_tt));
    let U_up = vec4<f32>(0.0, 0.0, 0.0, time_comp);
    let U_down = LowerIndex(U_up, geo);

    let m_r = -normalize(X.xyz);
    var WorldUp = vec3<f32>(0.0, 1.0, 0.0);
    if (abs(dot(m_r, WorldUp)) > 0.999) {
        WorldUp = vec3<f32>(1.0, 0.0, 0.0);
    }
    let m_phi = normalize(cross(WorldUp, m_r)); 
    let m_theta = cross(m_phi, m_r); 

    let k_r     = dot(RayDir, m_r);
    let k_theta = dot(RayDir, m_theta);
    let k_phi   = dot(RayDir, m_phi);

    var e1 = vec4<f32>(m_r, 0.0);
    e1 += dot(e1, U_down) * U_up; 
    var e1_d = LowerIndex(e1, geo);
    let n1 = sqrt(max(1e-9, dot(e1, e1_d)));
    e1 /= n1; e1_d /= n1;

    var e2 = vec4<f32>(m_theta, 0.0);
    e2 += dot(e2, U_down) * U_up;
    e2 -= dot(e2, e1_d) * e1;
    var e2_d = LowerIndex(e2, geo);
    let n2 = sqrt(max(1e-9, dot(e2, e2_d)));
    e2 /= n2; e2_d /= n2;

    var e3 = vec4<f32>(m_phi, 0.0);
    e3 += dot(e3, U_down) * U_up;
    e3 -= dot(e3, e1_d) * e1;
    e3 -= dot(e3, e2_d) * e2;
    var e3_d = LowerIndex(e3, geo);
    let n3 = sqrt(max(1e-9, dot(e3, e3_d)));
    e3 /= n3;

    let P_up = U_up - (k_r * e1 + k_theta * e2 + k_phi * e3);
    return LowerIndex(P_up, geo);
}

@fragment
fn fs_main(@builtin(position) fragCoord: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = (fragCoord.xy - 0.5 * u.iResolution.xy) / u.iResolution.y;
    
    var mx = u.iMouse.x / u.iResolution.x;
    var my = u.iMouse.y / u.iResolution.y;
    if (u.iMouse.z <= 0.0) { mx = 0.5; my = 0.5; }
    mx = -(mx - 0.5) * 6.28 + u.iTime * 0.1;
    my = -(my - 0.5) * 3.14 + 0.3;
    
    let camRad = 15.0;
    var ro = vec3<f32>(0.0, 0.0, -camRad);
    
    let rx = rot2(my);
    ro = vec3<f32>(ro.x, ro.y * rx[0][0] + ro.z * rx[1][0], ro.y * rx[0][1] + ro.z * rx[1][1]);
    let ry = rot2(mx);
    ro = vec3<f32>(ro.x * ry[0][0] + ro.z * ry[1][0], ro.y, ro.x * ry[0][1] + ro.z * ry[1][1]);
    
    let fwd = normalize(-ro);
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), fwd));
    let up = cross(fwd, right);
    let rd = normalize(fwd + right * uv.x + up * uv.y);
    
    let PhysicalSpinA = 0.99;
    let PhysicalQ = 0.0;
    
    var s0: State;
    s0.X = vec4<f32>(ro, 0.0);
    s0.P = GetInitialMomentum(rd, s0.X, PhysicalSpinA, PhysicalQ);
    
    var transmittance = 1.0;
    var diskGlow = vec3<f32>(0.0);
    var hitHorizon = false;
    
    let rs = 1.0;
    let inner_r = 1.5;
    let outer_r = 12.0;

    for (var i = 0; i < 400; i++) {
        let geo0 = ComputeGeometryScalars(s0.X.xyz, PhysicalSpinA, PhysicalQ, 1.0);
        let r = geo0.r;
        
        if (r < rs) {
            hitHorizon = true;
            break;
        }
        if (r > 30.0) {
            break;
        }
        
        let dt = max(0.01, min(0.3, r * 0.05));
        
        // Exact geodesic RK4 solving step!
        let k1 = GetDerivativesAnalytic(s0, PhysicalSpinA, PhysicalQ, geo0);
        
        var s1: State;
        s1.X = s0.X + 0.5 * dt * k1.X;
        s1.P = s0.P + 0.5 * dt * k1.P;
        let geo1 = ComputeGeometryScalars(s1.X.xyz, PhysicalSpinA, PhysicalQ, 1.0);
        let k2 = GetDerivativesAnalytic(s1, PhysicalSpinA, PhysicalQ, geo1);
        
        var s2: State;
        s2.X = s0.X + 0.5 * dt * k2.X;
        s2.P = s0.P + 0.5 * dt * k2.P;
        let geo2 = ComputeGeometryScalars(s2.X.xyz, PhysicalSpinA, PhysicalQ, 1.0);
        let k3 = GetDerivativesAnalytic(s2, PhysicalSpinA, PhysicalQ, geo2);
        
        var s3: State;
        s3.X = s0.X + dt * k3.X;
        s3.P = s0.P + dt * k3.P;
        let geo3 = ComputeGeometryScalars(s3.X.xyz, PhysicalSpinA, PhysicalQ, 1.0);
        let k4 = GetDerivativesAnalytic(s3, PhysicalSpinA, PhysicalQ, geo3);
        
        // Apply stepping
        let prevX = s0.X;
        s0.X = s0.X + (dt / 6.0) * (k1.X + 2.0 * k2.X + 2.0 * k3.X + k4.X);
        s0.P = s0.P + (dt / 6.0) * (k1.P + 2.0 * k2.P + 2.0 * k3.P + k4.P);
        
        // --- Render Volumetric Disk ---
        let diskHeight = 0.1 * r;
        if (abs(s0.X.y) < diskHeight && r > inner_r && r < outer_r) {
            let diskNorm = (r - inner_r) / (outer_r - inner_r);
            let angle = atan2(s0.X.z, s0.X.x);
            let vel = 1.0 / sqrt(r); 
            let rotAngle = angle - u.iTime * vel * 2.0;

            let curPos = vec3<f32>(cos(rotAngle)*r, s0.X.y * 5.0, sin(rotAngle)*r) * 3.0;
            let nval = fbm(curPos + vec3<f32>(0.0, u.iTime * 1.5, 0.0));
            
            let yFalloff = 1.0 - abs(s0.X.y)/diskHeight;
            let density = smoothstep(0.4, 0.7, nval) * yFalloff 
                        * smoothstep(0.0, 0.1, diskNorm) * smoothstep(1.0, 0.5, diskNorm);

            if (density > 0.01) {
                // Relativistic doppler shift calculation using momentum
                let dx = s0.X.xyz - prevX.xyz;
                let dir = normalize(dx);
                let flowDir = vec3<f32>(-sin(angle), 0.0, cos(angle));
                let doppler = dot(dir, flowDir) * vel * 3.0; 
                
                let hotCol = vec3<f32>(1.0, 0.45, 0.1); 
                var shiftCol = hotCol;
                if (doppler > 0.0) {
                    shiftCol = mix(hotCol, vec3<f32>(0.2, 0.8, 1.0), min(1.0, doppler)); 
                } else {
                    shiftCol = mix(hotCol, vec3<f32>(0.8, 0.1, 0.0), min(1.0, -doppler * 1.5)); 
                }
                
                let emission = shiftCol * density * 8.0 * (inner_r/r);
                let absorption = density * 5.0;
                
                diskGlow += emission * transmittance * dt;
                transmittance *= exp(-absorption * dt);
            }
        }
        
        if (transmittance < 0.01) { break; }
    }
    
    var finalCol = diskGlow;
    if (!hitHorizon && transmittance > 0.01) {
        // Cosmos Background
        let bgDir = normalize(s0.X.xyz);
        let band = exp(-abs(bgDir.y)*8.0) * fbm(bgDir * 10.0);
        var bgCol = vec3<f32>(0.06, 0.12, 0.20) * band;
        
        let sHash = hash3(bgDir * 300.0);
        if (sHash > 0.995) {
            bgCol += vec3<f32>(pow((sHash - 0.995)*200.0, 2.0));
        }
        finalCol += transmittance * bgCol;
    }
    
    // Tonemapping
    finalCol = finalCol * 1.4;
    finalCol = (finalCol*(2.51*finalCol+0.03))/(finalCol*(2.43*finalCol+0.59)+0.14);
    
    let vig = 1.0 - 0.5 * dot(uv, uv);
    finalCol *= vig;
    
    return vec4<f32>(pow(max(vec3<f32>(0.0), finalCol), vec3<f32>(1.0/2.2)), 1.0);
}
