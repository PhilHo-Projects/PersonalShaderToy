//! PASS: Buffer A
//! iChannel0: self
// Physics simulation — orbiting particles with trail feedback

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;

    // Feedback from previous frame (trail effect)
    vec4 prev = texture(iChannel0, uv) * 0.95;

    // Generate moving light sources
    vec3 col = vec3(0.0);
    for (int i = 0; i < 5; i++) {
        float fi = float(i);
        float t = iTime * (0.5 + fi * 0.15) + fi * 1.2566;
        vec2 center = vec2(
            0.5 + 0.3 * cos(t) * sin(t * 0.3 + fi),
            0.5 + 0.3 * sin(t) * cos(t * 0.7 + fi)
        );
        float d = length(uv - center);
        float brightness = 0.005 / (d * d + 0.001);

        vec3 particleCol = 0.5 + 0.5 * cos(6.28 * (fi / 5.0) + vec3(0.0, 2.1, 4.2));
        col += particleCol * brightness;
    }

    fragColor = max(prev, vec4(col, 1.0));
}

//! PASS: Buffer B
//! iChannel0: Buffer A
// Horizontal blur pass for bloom

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec2 texel = 1.0 / iResolution.xy;

    vec4 sum = vec4(0.0);
    float weights[5] = float[](0.227027, 0.194596, 0.121622, 0.054054, 0.016216);

    sum += texture(iChannel0, uv) * weights[0];
    for (int i = 1; i < 5; i++) {
        float offset = float(i) * 2.0;
        sum += texture(iChannel0, uv + vec2(texel.x * offset, 0.0)) * weights[i];
        sum += texture(iChannel0, uv - vec2(texel.x * offset, 0.0)) * weights[i];
    }

    fragColor = sum;
}

//! PASS: Buffer C
//! iChannel0: Buffer B
// Vertical blur pass for bloom

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec2 texel = 1.0 / iResolution.xy;

    vec4 sum = vec4(0.0);
    float weights[5] = float[](0.227027, 0.194596, 0.121622, 0.054054, 0.016216);

    sum += texture(iChannel0, uv) * weights[0];
    for (int i = 1; i < 5; i++) {
        float offset = float(i) * 2.0;
        sum += texture(iChannel0, uv + vec2(0.0, texel.y * offset)) * weights[i];
        sum += texture(iChannel0, uv - vec2(0.0, texel.y * offset)) * weights[i];
    }

    fragColor = sum;
}

//! PASS: Image
//! iChannel0: Buffer A
//! iChannel1: Buffer C
// Final compositing — original + bloom

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;

    vec4 scene = texture(iChannel0, uv);
    vec4 bloom = texture(iChannel1, uv);

    // Combine: scene + additive bloom
    vec3 col = scene.rgb + bloom.rgb * 0.6;

    // Tone mapping (Reinhard)
    col = col / (1.0 + col);

    // Vignette
    float vig = 1.0 - 0.4 * length(uv - 0.5);
    col *= vig;

    // Gamma
    col = pow(col, vec3(0.4545));

    fragColor = vec4(col, 1.0);
}
