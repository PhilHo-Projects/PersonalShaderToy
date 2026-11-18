fn main() -> Result<(), String> {
    let source = match std::env::args_os().nth(1) {
        Some(path) => std::fs::read_to_string(path).map_err(|e| e.to_string())?,
        None => "fn mainImage(p:vec2f)->vec4f{return vec4f(p,0,1);}".into(),
    };
    for pass in shader_core::parse(&source)? {
        let shader = shader_core::prepare(&pass, "wgsl")?;
        println!("[PASS] {} · {} bytes WGSL", pass.name, shader.wgsl.len());
    }
    Ok(())
}
