//! PASS: Buffer A
//! iChannel0: self
// Gray-Scott Reaction-Diffusion Simulation
// Two chemicals (U and V) interact, diffuse, and create organic patterns.
// R channel = U concentration, G channel = V concentration

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec2 texel = 1.0 / iResolution.xy;

    // Simulation parameters — these create coral/mitosis patterns
    float feed = 0.037;
    float kill = 0.06;
    float dU = 0.21;  // diffusion rate of U
    float dV = 0.105; // diffusion rate of V
    float dt = 1.0;   // time step

    // Slowly vary parameters for evolving patterns
    feed += 0.008 * sin(iTime * 0.1);
    kill += 0.003 * cos(iTime * 0.07);

    // Read current state
    vec4 state = texture(iChannel0, uv);
    float U = state.r;
    float V = state.g;

    // Initialize on first frame
    if (iFrame < 2) {
        U = 1.0;
        V = 0.0;
        // Seed pattern — multiple spots
        for (int i = 0; i < 8; i++) {
            float fi = float(i);
            vec2 center = vec2(
                0.5 + 0.3 * cos(fi * 0.785 + 0.5),
                0.5 + 0.3 * sin(fi * 0.785 + 0.5)
            );
            float d = length(uv - center);
            if (d < 0.03) {
                V = 1.0;
                U = 0.5;
            }
        }
        // Center seed
        if (length(uv - 0.5) < 0.04) {
            V = 1.0; U = 0.5;
        }
    }

    // Laplacian (9-point stencil for smoother diffusion)
    float lapU = 0.0;
    float lapV = 0.0;

    // Cardinal directions (weight 0.2)
    for (int dx = -1; dx <= 1; dx++) {
        for (int dy = -1; dy <= 1; dy++) {
            if (dx == 0 && dy == 0) continue;
            vec2 offset = vec2(float(dx), float(dy)) * texel;
            vec4 neighbor = texture(iChannel0, uv + offset);
            float weight = (dx == 0 || dy == 0) ? 0.2 : 0.05;
            lapU += weight * (neighbor.r - U);
            lapV += weight * (neighbor.g - V);
        }
    }

    // Gray-Scott equations
    float reaction = U * V * V;
    float newU = U + dt * (dU * lapU - reaction + feed * (1.0 - U));
    float newV = V + dt * (dV * lapV + reaction - (kill + feed) * V);

    // Mouse interaction — drop chemical V
    if (iMouse.z > 0.0) {
        vec2 mouseUV = iMouse.xy / iResolution.xy;
        float d = length(uv - mouseUV);
        if (d < 0.02) {
            newV = 1.0;
            newU = 0.5;
        }
    }

    fragColor = vec4(clamp(newU, 0.0, 1.0), clamp(newV, 0.0, 1.0), 0.0, 1.0);
}

//! PASS: Buffer B
//! iChannel0: Buffer A
// Extract glow intensity from reaction-diffusion for bloom

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec2 texel = 1.0 / iResolution.xy;

    vec4 state = texture(iChannel0, uv);
    float V = state.g;

    // Edge detection via Sobel on V channel
    float tl = texture(iChannel0, uv + vec2(-texel.x, texel.y)).g;
    float t  = texture(iChannel0, uv + vec2(0, texel.y)).g;
    float tr = texture(iChannel0, uv + vec2(texel.x, texel.y)).g;
    float l  = texture(iChannel0, uv + vec2(-texel.x, 0)).g;
    float r  = texture(iChannel0, uv + vec2(texel.x, 0)).g;
    float bl = texture(iChannel0, uv + vec2(-texel.x, -texel.y)).g;
    float b  = texture(iChannel0, uv + vec2(0, -texel.y)).g;
    float br = texture(iChannel0, uv + vec2(texel.x, -texel.y)).g;

    float gx = -tl - 2.0*l - bl + tr + 2.0*r + br;
    float gy = -tl - 2.0*t - tr + bl + 2.0*b + br;
    float edge = sqrt(gx*gx + gy*gy);

    // Glow: edge intensity + V concentration hotspots
    float glow = edge * 3.0 + pow(V, 3.0) * 2.0;

    fragColor = vec4(glow, V, edge, 1.0);
}

//! PASS: Buffer C
//! iChannel0: Buffer B
//! iChannel1: self
// Dual-axis blur for bloom (ping-pong with self for iterative blur)

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec2 texel = 1.0 / iResolution.xy;

    // Alternate blur direction each frame
    vec2 dir = (iFrame % 2 == 0) ? vec2(1.0, 0.0) : vec2(0.0, 1.0);

    vec4 sum = vec4(0.0);
    float totalWeight = 0.0;

    // 13-tap Gaussian
    for (int i = -6; i <= 6; i++) {
        float fi = float(i);
        float weight = exp(-fi * fi / 18.0);
        vec2 offset = dir * fi * texel * 3.0;

        // Blend between fresh input (Buffer B) and previous blur (self)
        vec4 fresh = texture(iChannel0, uv + offset);
        vec4 blurred = texture(iChannel1, uv + offset);
        vec4 sample_val = mix(fresh, blurred, 0.6);

        sum += sample_val * weight;
        totalWeight += weight;
    }

    fragColor = sum / totalWeight;
}

//! PASS: Image
//! iChannel0: Buffer A
//! iChannel1: Buffer C
// Final compositing with artistic color mapping

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;

    // Read simulation state and bloom
    vec4 sim = texture(iChannel0, uv);
    vec4 bloom = texture(iChannel1, uv);

    float U = sim.r;
    float V = sim.g;

    // Artistic color mapping based on chemical concentrations
    // V creates the pattern structures, U is the background
    float pattern = V;
    float edge = bloom.b;
    float glow = bloom.r;

    // Deep ocean bioluminescence palette
    vec3 deep = vec3(0.005, 0.01, 0.03);          // void
    vec3 mid = vec3(0.0, 0.08, 0.15);             // deep water
    vec3 organism = vec3(0.0, 0.6, 0.8);          // living tissue
    vec3 hotspot = vec3(0.2, 0.9, 1.0);           // bioluminescent tips
    vec3 nerve = vec3(0.8, 0.3, 1.0);             // neural edges

    // Layer the colors
    vec3 col = deep;
    col = mix(col, mid, smoothstep(0.0, 0.3, U));
    col = mix(col, organism, smoothstep(0.05, 0.35, pattern));
    col = mix(col, hotspot, smoothstep(0.3, 0.6, pattern));

    // Edge glow (neon neural network look)
    col += nerve * edge * 1.5;

    // Bloom overlay
    col += vec3(0.1, 0.5, 0.7) * glow * 0.15;

    // Pulsing ambient light
    float pulse = sin(iTime * 0.5) * 0.5 + 0.5;
    col += deep * pulse * 0.5;

    // Tone mapping
    col = col / (0.8 + col);

    // Vignette
    float vig = 1.0 - 0.4 * pow(length(uv - 0.5) * 1.4, 2.0);
    col *= vig;

    // Gamma
    col = pow(max(col, vec3(0.0)), vec3(0.4545));

    fragColor = vec4(col, 1.0);
}
