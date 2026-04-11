// Raymarched Sphere - OpenAI
// A simple raymarched scene with lighting

float sdSphere(vec3 p, float r) {
    return length(p) - r;
}

float sdPlane(vec3 p) {
    return p.y + 1.0;
}

float map(vec3 p) {
    float d = sdSphere(p - vec3(0, 0, 0), 1.0);
    d = min(d, sdPlane(p));
    return d;
}

vec3 calcNormal(vec3 p) {
    vec2 e = vec2(0.001, 0.0);
    return normalize(vec3(
        map(p + e.xyy) - map(p - e.xyy),
        map(p + e.yxy) - map(p - e.yxy),
        map(p + e.yyx) - map(p - e.yyx)
    ));
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = (fragCoord - 0.5 * iResolution.xy) / iResolution.y;

    // Camera
    vec3 ro = vec3(3.0 * sin(iTime * 0.5), 2.0, 3.0 * cos(iTime * 0.5));
    vec3 ta = vec3(0, 0, 0);
    vec3 ww = normalize(ta - ro);
    vec3 uu = normalize(cross(ww, vec3(0, 1, 0)));
    vec3 vv = cross(uu, ww);
    vec3 rd = normalize(uv.x * uu + uv.y * vv + 1.5 * ww);

    // Raymarch
    float t = 0.0;
    for (int i = 0; i < 80; i++) {
        vec3 p = ro + t * rd;
        float d = map(p);
        if (d < 0.001 || t > 20.0) break;
        t += d;
    }

    vec3 col = vec3(0.05, 0.05, 0.1); // sky

    if (t < 20.0) {
        vec3 p = ro + t * rd;
        vec3 n = calcNormal(p);
        vec3 light = normalize(vec3(1, 2, 3));

        float diff = max(dot(n, light), 0.0);
        float spec = pow(max(dot(reflect(-light, n), -rd), 0.0), 32.0);
        float ao = 0.5 + 0.5 * n.y;

        col = vec3(0.2, 0.5, 0.8) * diff * ao + vec3(1.0) * spec * 0.5;
        col += vec3(0.02);

        // Checkerboard for ground
        if (p.y < -0.99) {
            float checker = mod(floor(p.x) + floor(p.z), 2.0);
            col = mix(vec3(0.15), vec3(0.4), checker) * (diff * 0.5 + 0.5);
        }
    }

    col = pow(col, vec3(0.4545)); // gamma
    fragColor = vec4(col, 1.0);
}
