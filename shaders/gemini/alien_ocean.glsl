// "Alien Ocean" - Gemini
// Physical water rendering with raymarched procedural waves, reflections, and an alien sky setting.

#define MAX_STEPS 100
#define SURF_DIST 0.01
#define MAX_DIST 120.0

mat2 rot(float a) {
    float s = sin(a), c = cos(a);
    return mat2(c, -s, s, c);
}

// Hash function for noise
float hash(vec2 p) {
    p  = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}

// 2D Value Noise
float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f*f*(3.0-2.0*f);
    return mix(
        mix(hash(i + vec2(0.0,0.0)), hash(i + vec2(1.0,0.0)), f.x),
        mix(hash(i + vec2(0.0,1.0)), hash(i + vec2(1.0,1.0)), f.x),
        f.y
    );
}

// Fractal Brownian Motion for waves
float fbm(vec2 p) {
    float h = 0.0;
    float a = 0.5;
    mat2 m = mat2(0.8, -0.6, 0.6, 0.8);
    for(int i = 0; i < 6; i++) {
        h += a * noise(p);
        p = m * p * 2.0;
        a *= 0.5;
    }
    return h;
}

// Geometry for the shifting ocean
float mapOcean(vec3 p) {
    vec2 uv1 = p.xz * 0.2 + iTime * 0.5;
    vec2 uv2 = p.xz * 0.4 - iTime * 0.3;
    float h = fbm(uv1) * 0.8 + fbm(uv2 * rot(0.5)) * 0.5;
    return p.y - h + 1.5; 
}

// Procedural Alien Skybox
vec3 getSky(vec3 rd) {
    float t = max(rd.y, 0.0);
    // Sunset-style gradient
    vec3 skyBase = mix(vec3(0.9, 0.2, 0.4), vec3(0.1, 0.2, 0.6), t + 0.1); 
    
    // Twin suns
    vec3 sunDir1 = normalize(vec3(0.8, 0.2, 1.0));
    vec3 sunDir2 = normalize(vec3(-0.4, 0.4, 0.8));
    
    float sun1 = pow(max(dot(rd, sunDir1), 0.0), 250.0);
    float sun1Glow = pow(max(dot(rd, sunDir1), 0.0), 10.0);
    float sun2 = pow(max(dot(rd, sunDir2), 0.0), 80.0);
    
    skyBase += vec3(1.0, 0.9, 0.5) * sun1 * 2.5;         // Primary sun
    skyBase += vec3(1.0, 0.5, 0.1) * sun1Glow * 0.5;     // Primary sun corona
    skyBase += vec3(0.4, 0.8, 1.0) * sun2 * 1.5;         // Secondary blue dwarf
    
    // Atmospheric sweeping clouds
    if (rd.y > 0.0) {
        vec2 clUv = rd.xz / (rd.y + 0.1);
        float cl = fbm(clUv * 2.0 - iTime * 0.1);
        skyBase = mix(skyBase, vec3(1.0, 0.8, 0.9), cl * 0.6 * smoothstep(0.0, 0.3, rd.y));
    }
    
    return skyBase;
}

vec3 getNormal(vec3 p) {
    vec2 e = vec2(0.01, 0.0);
    vec3 n;
    n.x = mapOcean(p + e.xyy) - mapOcean(p - e.xyy);
    n.y = 2.0 * e.x;
    n.z = mapOcean(p + e.yyx) - mapOcean(p - e.yyx);
    return normalize(n);
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = (fragCoord - 0.5 * iResolution.xy) / iResolution.y;
    
    // Boat/Camera cruising the ocean
    vec3 ro = vec3(0.0, 1.5, iTime * 3.0); 
    
    // Head-bobbing over waves
    ro.y += fbm(ro.xz * 0.2 + iTime * 0.5) * 0.8;
    
    vec3 rd = normalize(vec3(uv, 1.0));
    
    // Mouse Interaction
    if(iMouse.z > 0.0) {
        vec2 m = (iMouse.xy / iResolution.xy - 0.5) * 2.0;
        rd.yz *= rot(-m.y * 1.5);
        rd.xz *= rot(-m.x * 2.5);
    } else {
        // Auto look slightly up at the sunset
        rd.yz *= rot(0.15 + sin(iTime*0.4)*0.05); 
    }
    
    // Raymarch Water
    float dO = 0.0;
    bool hit = false;
    for(int i = 0; i < MAX_STEPS; i++) {
        vec3 p = ro + rd * dO;
        float dS = mapOcean(p);
        if(dS < SURF_DIST) {
            hit = true;
            break;
        }
        if(dO > MAX_DIST) break;
        // Conservative stepping for height-fields to prevent overshoots
        dO += dS * 0.6; 
    }
    
    vec3 col = getSky(rd);
    
    if(hit) {
        vec3 p = ro + rd * dO;
        vec3 n = getNormal(p);
        
        // Microfacet / specular reflections
        vec3 ref = reflect(rd, n);
        vec3 skyRef = getSky(ref);
        
        // Fresnel reflection magnitude
        float fresnel = pow(1.0 - max(dot(n, -rd), 0.0), 5.0);
        
        // Deep alien water color
        vec3 waterBase = vec3(0.01, 0.05, 0.2);
        
        // Blend water absorption with pure sky reflections
        col = mix(waterBase, skyRef, fresnel * 0.9 + 0.1);
        
        // Soft horizon / distance fog blending
        col = mix(col, getSky(rd), smoothstep(40.0, MAX_DIST, dO));
    }
    
    // Contrast boost & Tonemapping
    col = smoothstep(0.0, 1.1, col);
    col = pow(col, vec3(0.4545)); // Gamma correction
    
    fragColor = vec4(col, 1.0);
}
