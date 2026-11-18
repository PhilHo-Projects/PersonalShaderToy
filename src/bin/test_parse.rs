fn main() -> Result<(), String> {
    let source = match std::env::args_os().nth(1) {
        Some(path) => std::fs::read_to_string(path).map_err(|e| e.to_string())?,
        None => "void mainImage(out vec4 c, in vec2 p){c=vec4(p,0,1);}".into(),
    };
    for pass in shader_core::parse(&source)? {
        let shader = shader_core::prepare(&pass, "glsl")?;
        println!("[PASS] {} · {} bytes WGSL", pass.name, shader.wgsl.len());
    }
    Ok(())
}
