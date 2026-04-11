// Mandelbrot Set - Claude
// A classic fractal with smooth coloring

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = (fragCoord - 0.5 * iResolution.xy) / iResolution.y;
    uv *= 2.5;
    uv.x -= 0.5;

    vec2 c = uv;
    vec2 z = vec2(0.0);
    float iter = 0.0;
    const float maxIter = 128.0;

    for (float i = 0.0; i < maxIter; i++) {
        z = vec2(z.x * z.x - z.y * z.y, 2.0 * z.x * z.y) + c;
        if (dot(z, z) > 4.0) break;
        iter++;
    }

    float t = iter / maxIter;
    // Smooth iteration count
    if (iter < maxIter) {
        float log_zn = log(dot(z, z)) / 2.0;
        float nu = log(log_zn / log(2.0)) / log(2.0);
        t = (iter + 1.0 - nu) / maxIter;
    }

    // Color palette
    vec3 col = 0.5 + 0.5 * cos(3.0 + t * 6.28318 * 2.0 + vec3(0.0, 0.6, 1.0));
    if (iter >= maxIter) col = vec3(0.0);

    fragColor = vec4(col, 1.0);
}
