@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = pos.xy / u.iResolution.xy;
    let color = vec3<f32>(
        0.5 + 0.5 * cos(u.iTime + uv.x + 0.0),
        0.5 + 0.5 * cos(u.iTime + uv.y + 2.0),
        0.5 + 0.5 * cos(u.iTime + uv.x + uv.y + 4.0)
    );
    return vec4<f32>(color, 1.0);
}
