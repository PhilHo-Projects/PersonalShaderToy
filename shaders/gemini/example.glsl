// "Neon Singularity" - Gemini
// Way beyond a simple Mandelbrot or Plasma. 
// Fully 3D, neon glowing, raymarched recursive structure to 1-up Claude.

#define MAX_STEPS 100
#define SURF_DIST 0.001
#define MAX_DIST 50.0

mat2 rot(float a) {
    float s = sin(a), c = cos(a);
    return mat2(c, -s, s, c);
}

float sdBox(vec3 p, vec3 b) {
    vec3 q = abs(p) - b;
    return length(max(q,0.0)) + min(max(q.x,max(q.y,q.z)),0.0);
}

float map(vec3 p) {
    float t = iTime * 0.5;
    
    // Main rotating container
    vec3 bp = p;
    bp.xz *= rot(t);
    bp.yz *= rot(t * 0.5);
    
    float box = sdBox(bp, vec3(1.5)) - 0.2;
    float s = length(bp) - 1.8;
    
    // A hollowed out sphere/box hybrid
    float d = max(box, -s); 
    
    // Add recursive/fractal floating structures
    vec3 fp = p;
    for(int i = 0; i < 4; i++) {
        fp = abs(fp) - vec3(1.0, 0.5, 1.0);
        fp.xy *= rot(t * 0.2 + float(i));
        fp.yz *= rot(t * 0.3 - float(i));
        float b2 = sdBox(fp, vec3(0.1, 2.0, 0.1));
        d = min(d, b2);
    }

    return d;
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = (fragCoord - 0.5 * iResolution.xy) / iResolution.y;
    
    vec3 ro = vec3(0.0, 0.0, -8.0);
    vec3 rd = normalize(vec3(uv, 1.0));
    
    // Mouse Interaction
    if(iMouse.z > 0.0) {
        vec2 m = iMouse.xy / iResolution.xy;
        ro.yz *= rot(-m.y * 3.14 + 1.0);
        ro.xz *= rot(-m.x * 6.28);
        rd.yz *= rot(-m.y * 3.14 + 1.0);
        rd.xz *= rot(-m.x * 6.28);
    } else {
        // Auto pan if mouse isn't interacting
        ro.xz *= rot(iTime * 0.2);
        rd.xz *= rot(iTime * 0.2);
    }
    
    float dO = 0.0;
    vec3 col = vec3(0.0);
    float glow = 0.0;
    
    // Raymarching Loop
    for(int i = 0; i < MAX_STEPS; i++) {
        vec3 p = ro + rd * dO;
        float dS = map(p);
        
        // Accumulate glow based on proximity to surfaces
        glow += 0.01 / (0.01 + abs(dS)) * (1.0 - float(i) / float(MAX_STEPS));
        
        if(abs(dS) < SURF_DIST || dO > MAX_DIST) break;
        dO += dS;
    }
    
    // Surface Shading
    if(dO < MAX_DIST) {
        vec3 p = ro + rd * dO;
        vec2 e = vec2(0.001, 0.0);
        vec3 n = normalize(vec3(
            map(p + e.xyy) - map(p - e.xyy),
            map(p + e.yxy) - map(p - e.yxy),
            map(p + e.yyx) - map(p - e.yyx)
        ));
        
        vec3 l = normalize(vec3(1.0, 2.0, -3.0));
        float dif = max(dot(n, l), 0.0);
        float fresnel = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
        
        col = vec3(0.05, 0.05, 0.1) * dif;
        col += vec3(0.1, 0.8, 1.0) * fresnel; // Neon rim lighting
    }
    
    // Add volumetric glow with color shift
    vec3 glowCol = mix(vec3(0.9, 0.1, 0.8), vec3(0.1, 0.9, 1.0), sin(iTime * 0.5 + uv.x * 2.0) * 0.5 + 0.5);
    col += glow * glowCol * 0.15;
    
    // Subtle animated background gradient
    col += vec3(0.01, 0.01, 0.03) * (1.0 - length(uv));
    
    // Tonemapping / Gamma correction
    col = col / (1.0 + col);
    col = pow(col, vec3(0.4545));
    
    fragColor = vec4(col, 1.0);
}
