const MAX_STEPS: u32 = 100u;
const MAX_DIST: f32 = 100.0;
const SURF_DIST: f32 = 0.001;

fn rot(a: f32) -> mat2x2<f32> {
    let s = sin(a);
    let c = cos(a);
    return mat2x2<f32>(
      vec2<f32>(c, s),
      vec2<f32>(-s, c)
    );
}

fn sdBox(p: vec3<f32>, s: vec3<f32>) -> f32 {
    let d = abs(p) - s;
    return length(max(d, vec3<f32>(0.0))) + min(max(d.x, max(d.y, d.z)), 0.0);
}

fn getDist(p_in: vec3<f32>) -> f32 {
    var p = p_in;
    
    // infinite repetition
    let spacing = 4.0;
    let id = floor(p / spacing);
    p = (fract(p / spacing) - 0.5) * spacing;
    
    // Animate the boxes
    p.x += sin(u.iTime + id.z * 0.5) * 0.5;
    p.y += cos(u.iTime + id.x * 0.5) * 0.5;

    let rotMat = rot(u.iTime * 0.5 + id.y);
    let xz = rotMat * vec2<f32>(p.x, p.z);
    p.x = xz.x;
    p.z = xz.y;
    
    var d = sdBox(p, vec3<f32>(0.6));
    
    let sphere = length(p) - 0.8;
    d = max(d, -sphere);
    
    return d;
}

fn getNormal(p: vec3<f32>) -> vec3<f32> {
    let d = getDist(p);
    let e = vec2<f32>(0.001, 0.0);
    let n = d - vec3<f32>(
        getDist(p - e.xyy),
        getDist(p - e.yxy),
        getDist(p - e.yyx)
    );
    return normalize(n);
}

@fragment
fn fs_main(@builtin(position) fragCoord: vec4<f32>) -> @location(0) vec4<f32> {
    // Inverse Y for WebGPU coords
    let fc = vec2<f32>(fragCoord.x, u.iResolution.y - fragCoord.y);
    let uv = (fc - 0.5 * u.iResolution.xy) / u.iResolution.y;
    
    let ro = vec3<f32>(0.0, 0.0, u.iTime * 2.0); 
    
    // Mouse look
    var look = vec2<f32>(0.0);
    if (u.iMouse.z > 0.0) {
        look = (u.iMouse.xy - 0.5 * u.iResolution.xy) / u.iResolution.y;
    }
    
    // rotate ray direction based on look
    var rd = normalize(vec3<f32>(uv.x, uv.y, 1.0));
    
    let r1 = rot(-look.y * 2.0);
    let yz = r1 * vec2<f32>(rd.y, rd.z);
    rd.y = yz.x;
    rd.z = yz.y;
    
    let r2 = rot(look.x * 2.0);
    let xz = r2 * vec2<f32>(rd.x, rd.z);
    rd.x = xz.x;
    rd.z = xz.y;
    
    var dO: f32 = 0.0;
    var dS: f32 = 0.0;
    var p: vec3<f32> = vec3<f32>(0.0);
    var glow: f32 = 0.0;
    
    for(var i: u32 = 0u; i < MAX_STEPS; i = i + 1u) {
        p = ro + rd * dO;
        dS = getDist(p);
        dO = dO + dS;
        
        glow = glow + 0.005 / (dS * dS + 0.005);
        
        if (dO > MAX_DIST || dS < SURF_DIST) { break; }
    }
    
    var col = vec3<f32>(0.0);
    
    if (dO < MAX_DIST) {
        let n = getNormal(p);
        let lightDir = normalize(vec3<f32>(1.0, 2.0, -1.0));
        let diff = max(dot(n, lightDir), 0.0);
        
        let id = floor((p - rd * dS) / 4.0);
        let baseColor = 0.5 + 0.5 * cos(u.iTime + id.xxy + vec3<f32>(0.0, 2.0, 4.0));
        
        col = baseColor * diff;
    }
    
    // Glow
    let neonColor = 0.5 + 0.5 * cos(u.iTime * 0.5 + ro.z * 0.1 + vec3<f32>(0.5, 0.1, 0.9));
    col = col + neonColor * glow * 0.03;
    
    // distance fog
    col = mix(col, vec3<f32>(0.02, 0.01, 0.05), 1.0 - exp(-0.03 * dO));
    
    // ACES Tonemapping roughly
    col = (col * (2.51 * col + 0.03)) / (col * (2.43 * col + 0.59) + 0.14);
    
    // Gamma correction
    col = pow(abs(col), vec3<f32>(0.4545));
    
    return vec4<f32>(col, 1.0);
}
