use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rerun-if-changed=shaders/catalog.json");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(root.join("shaders/catalog.json")).unwrap()).unwrap();
    let mut catalog = String::from("pub const CATALOG: &[Example] = &[\n");
    for entry in entries {
        let path = entry["path"].as_str().unwrap();
        println!("cargo:rerun-if-changed=shaders/{path}");
        let field = |name: &str| entry[name].as_str().unwrap();
        catalog += &format!(
            "Example {{title:{:?},language:{:?},credit:{:?},source:include_str!({:?})}},\n",
            field("title"),
            field("language"),
            field("credit"),
            root.join("shaders").join(path)
        );
    }
    fs::write(out.join("catalog.rs"), catalog + "];\n").unwrap();
    println!("cargo:rerun-if-env-changed=PST_SLANGC");
    println!("cargo:rerun-if-changed=tools/compilers.py");
    if env::var_os("PST_SLANGC").is_some() {
        return;
    }
    let tools = out.ancestors().nth(4).unwrap().join("compiler-tools/slang");
    let exe = if cfg!(windows) {
        "slangc.exe"
    } else {
        "slangc"
    };
    let compiler = tools.join("bin").join(exe);
    if !compiler.exists() {
        let python = if cfg!(windows) { "python" } else { "python3" };
        let result = Command::new(python)
            .arg(root.join("tools/compilers.py"))
            .arg("native")
            .arg(&tools)
            .status();
        if !result.is_ok_and(|s| s.success()) {
            println!("cargo:warning=Set PST_SLANGC to a Slang executable to enable HLSL input.");
            return;
        }
    }
    println!(
        "cargo:rustc-env=PST_BUILD_SLANGC={}",
        Path::new(&compiler).display()
    );
}
