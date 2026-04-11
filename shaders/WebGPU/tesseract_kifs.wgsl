const MAX_STEPS: u32 = 250u;
const MAX_DIST: f32 = 10.0;
const SURF_DIST: f32 = 0.001;

// Standard 2D rotation
fn rot(a: f32) -> mat2x2<f32> {
    let s = sin(a); let c = cos(a);
    return mat2x2<f32>(vec2<f32>(c, s), vec2<f32>(-s, c));
}

fn mandelbulb(p: vec3<f32>, trap: ptr<function, vec4<f32>>) -> f32 {
    var z = p;
    var dr = 1.0;
    var r = 0.0;
    
    // Power of the mandelbulb oscillates over time
    let power = 8.0 + 2.0 * sin(u.iTime * 0.2);
    
    var localTrap = vec4<f32>(100.0, 100.0, 100.0, 100.0);

    for (var i = 0u; i < 11u; i = i + 1u) {
        r = length(z);
        if (r > 2.0) { break; }
        
        // Extract polar coordinates
        var theta = acos(z.y / r);
        var phi = atan2(z.z, z.x);
        
        dr = pow(r, power - 1.0) * power * dr + 1.0;
        
        let zr = pow(r, power);
        theta = theta * power;
        phi = phi * power;
        
        // Convert back to cartesian coordinates
        z = zr * vec3<f32>(sin(theta)*cos(phi), cos(theta), sin(theta)*sin(phi));
        z += p;
        
        localTrap = min(localTrap, vec4<f32>(abs(z.x), abs(z.y), abs(z.z), r));
    }
    
    *trap = localTrap;
    return 0.5 * log(r) * r / dr;
}

fn getDist(p: vec3<f32>, trap: ptr<function, vec4<f32>>) -> f32 {
    return mandelbulb(p, trap);
}

fn getNormal(p: vec3<f32>) -> vec3<f32> {
    var dummyTrap: vec4<f32>;
    let e = vec2<f32>(0.001, 0.0);
    let d = getDist(p, &dummyTrap);
    let n = vec3<f32>(
        getDist(p + vec3<f32>(e.x, e.y, e.y), &dummyTrap) - d,
        getDist(p + vec3<f32>(e.y, e.x, e.y), &dummyTrap) - d,
        getDist(p + vec3<f32>(e.y, e.y, e.x), &dummyTrap) - d
    );
    return normalize(n);
}

@fragment
fn fs_main(@builtin(position) fragCoord: vec4<f32>) -> @location(0) vec4<f32> {
    let fc = vec2<f32>(fragCoord.x, u.iResolution.y - fragCoord.y);
    let uv = (fc - 0.5 * u.iResolution.xy) / u.iResolution.y;

    // Orbiting Camera
    let camRad = 2.5;
    let ro = vec3<f32>(sin(u.iTime * 0.2) * camRad, 0.5 + sin(u.iTime * 0.1), cos(u.iTime * 0.2) * camRad);
    let lookAt = vec3<f32>(0.0, 0.0, 0.0);
    
    let fwd = normalize(lookAt - ro);
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), fwd));
    let up = cross(fwd, right);
    let rd = normalize(fwd + right * uv.x + up * uv.y);
    
    var t = 0.0;
    var trap = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    var d = 0.0;
    var steps: f32 = 0.0;
    
    for (var i = 0u; i < MAX_STEPS; i = i + 1u) {
        let p = ro + rd * t;
        d = getDist(p, &trap);
        if (d < SURF_DIST || t > MAX_DIST) { break; }
        t += d;
        steps += 1.0;
    }
    
    var col = vec3<f32>(0.0, 0.0, 0.0);
    
    if (t < MAX_DIST) {
        let p = ro + rd * t;
        let n = getNormal(p);
        
        let lightPos = vec3<f32>(2.0, 5.0, 3.0);
        let l = normalize(lightPos - p);
        let diff = max(dot(n, l), 0.0);
        
        let refl = reflect(rd, n);
        let spec = pow(max(dot(refl, l), 0.0), 32.0);
        
        let ao = clamp(1.0 - steps / f32(MAX_STEPS), 0.0, 1.0);
        
        let trapColor = vec3<f32>(
            sin(trap.x * 20.0 + u.iTime) * 0.5 + 0.5,
            cos(trap.y * 15.0 - u.iTime) * 0.5 + 0.5,
            sin(trap.w * 10.0) * 0.5 + 0.5
        );
        
        col = mix(vec3<f32>(0.1, 0.3, 0.8), vec3<f32>(1.0, 0.4, 0.1), trapColor) * diff * ao + spec * 0.5;
    } else {
        col = vec3<f32>(0.02, 0.02, 0.03); 
    }
    
    // Core glow based on ray steps
    col += vec3<f32>(0.1, 0.5, 1.0) * (steps / f32(MAX_STEPS)) * 2.0;
    
    // Tone mapping
    col = col / (1.0 + col);
    col = pow(abs(col), vec3<f32>(1.0 / 2.2, 1.0 / 2.2, 1.0 / 2.2));
    
    // Vignette
    col *= 1.0 - 0.4 * dot(uv, uv);
    
    return vec4<f32>(col, 1.0);
}
