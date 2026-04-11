// "Quantum Hyperdrive" - Gemini
// Insane ultra-speed warp tunnel with neon glowing tech structures.

#define MAX_STEPS 80
#define SURF_DIST 0.01
#define MAX_DIST 60.0

mat2 rot(float a) {
    float s = sin(a), c = cos(a);
    return mat2(c, -s, s, c);
}

// Kaleidoscopic folding array
vec3 fold(vec3 p) {
    for(int i = 0; i < 4; i++) {
        p.xy = abs(p.xy);
        p.xy *= rot(0.25 * 3.14159);
    }
    return p;
}

float map(vec3 p) {
    float t = iTime * 3.0; // Very fast
    
    // Twist the tunnel based on depth and time
    p.xy *= rot(p.z * 0.05 + sin(t*0.2)*0.5);
    
    // Apply kaleidoscopic folding
    vec3 fp = fold(p);
    
    // Hollow inner core
    float tunnel = length(p.xy) - 3.5;
    
    // Add complex geometric extrusions (tech greebles) that fly towards you
    vec3 q = p;
    q.z = mod(q.z + t * 5.0, 4.0) - 2.0; 
    
    // Angular repetition for columns
    float angle = atan(q.y, q.x);
    float radius = length(q.xy);
    angle = mod(angle, 3.14159 / 4.0) - 3.14159 / 8.0;
    q.x = radius * cos(angle);
    q.y = radius * sin(angle);
    
    float blocks = length(max(abs(q) - vec3(3.0, 0.5, 0.5), 0.0)) - 0.2;
    float columns = length(max(abs(fp.xy - vec2(5.0, 0.0)) - vec2(0.5, 0.5), 0.0)) - 0.2;
    
    // Combine geometry
    float d = min(tunnel, blocks);
    d = min(d, columns);
    
    // Add some noise to the walls to make it less perfectly smooth
    float bump = sin(p.x * 10.0) * sin(p.y * 10.0) * sin(p.z * 2.0) * 0.05;
    d -= bump;

    return d * 0.7; // Scale factor to prevent ray step overshooting
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = (fragCoord - 0.5 * iResolution.xy) / iResolution.y;
    
    // Super fast camera motion down the Z tube
    vec3 ro = vec3(0.0, 0.0, iTime * 15.0); 
    vec3 rd = normalize(vec3(uv * 1.5, 1.0)); // Wide FOV
    
    // Dramatic camera roll and sway
    ro.x += sin(iTime * 2.0) * 0.5;
    ro.y += cos(iTime * 1.5) * 0.5;
    rd.xy *= rot(sin(iTime * 0.8) * 1.2); 
    
    // Mouse interact
    if(iMouse.z > 0.0) {
        vec2 m = (iMouse.xy / iResolution.xy - 0.5) * 2.0;
        rd.xy *= rot(-m.x * 2.5);
        rd.yz *= rot(m.y * 2.5);
    }
    
    float dO = 0.0;
    float glow = 0.0;
    
    // Raymarching
    for(int i = 0; i < MAX_STEPS; i++) {
        vec3 p = ro + rd * dO;
        float dS = map(p);
        
        // Accumulate energy glow (volumetric pseudo-lighting)
        glow += 0.02 / (0.01 + abs(dS)) * (1.0 - float(i)/float(MAX_STEPS));
        
        if(abs(dS) < SURF_DIST || dO > MAX_DIST) break;
        dO += dS;
    }
    
    vec3 col = vec3(0.0);
    
    // Shading
    if(dO < MAX_DIST) {
        vec3 p = ro + rd * dO;
        vec2 e = vec2(0.01, 0.0);
        vec3 n = normalize(vec3(
            map(p + e.xyy) - map(p - e.xyy),
            map(p + e.yxy) - map(p - e.yxy),
            map(p + e.yyx) - map(p - e.yyx)
        ));
        
        // Fast flying lights inside the tunnel
        vec3 lightPos1 = ro + vec3(sin(iTime * 5.0) * 2.0, cos(iTime * 3.0) * 2.0, 10.0);
        vec3 lightPos2 = ro + vec3(0.0, 0.0, 20.0);
        
        vec3 l1 = normalize(lightPos1 - p);
        vec3 l2 = normalize(lightPos2 - p);
        
        float dif1 = max(dot(n, l1), 0.0);
        float dif2 = max(dot(n, l2), 0.0);
        float fresnel = pow(1.0 - max(dot(n, -rd), 0.0), 4.0);
        
        // Base dark metal
        vec3 albedo = vec3(0.1);
        col = albedo * (dif1 + dif2 * 0.5);
        
        // Pulsing neon edge highlights blending colors from cyan to magenta
        vec3 neonColor = mix(vec3(0.1, 0.9, 1.0), vec3(1.0, 0.1, 0.9), sin(p.z * 0.1 - iTime)*0.5+0.5);
        col += neonColor * fresnel * 2.5; 
        
        // Laser scanning grids racing by
        float grid = step(0.95, sin(p.z * 5.0 + iTime * 20.0)) * step(0.9, sin(atan(p.y, p.x) * 20.0));
        col += neonColor * grid * 5.0;
        
        // Distance fog to hide clipping
        col = mix(col, vec3(0.0), smoothstep(10.0, MAX_DIST, dO));
    }
    
    // Overlay Volumetric Plasma Core
    vec3 coreColor = mix(vec3(0.9, 0.0, 0.5), vec3(0.0, 0.8, 1.0), sin(iTime * 3.0)*0.5+0.5);
    col += glow * coreColor * 0.15;
    
    // Stargate speed-lines radially
    float speedLines = fract(atan(uv.y, uv.x) * 10.0 + sin(length(uv) * 20.0 - iTime * 50.0));
    col += speedLines * 0.05 * vec3(0.5, 0.8, 1.0) * length(uv);
    
    // Vignette / tunnel end fade
    col *= 1.0 - length(uv) * 0.5;
    
    // High contrast tonemapping + gamma
    col = smoothstep(0.0, 1.0, col);
    col = pow(col, vec3(0.6)); 
    
    fragColor = vec4(col, 1.0);
}
