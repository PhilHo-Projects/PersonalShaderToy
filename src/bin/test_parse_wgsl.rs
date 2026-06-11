use std::{env, fs};

#[derive(Clone)]
struct ParsedPass {
    name: String,
    source: String,
}

fn parse_passes(source: &str) -> Vec<ParsedPass> {
    let has_markers = source.lines().any(|line| {
        let t = line.trim();
        t.starts_with("//! PASS:") || t.starts_with("//!PASS:")
    });
    if !has_markers {
        return vec![ParsedPass {
            name: "Image".into(),
            source: source.to_string(),
        }];
    }
    let mut passes = Vec::new();
    let mut current: Option<ParsedPass> = None;
    let mut src_lines: Vec<&str> = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed
            .strip_prefix("//! PASS:")
            .or_else(|| trimmed.strip_prefix("//!PASS:"))
        {
            if let Some(mut pass) = current.take() {
                pass.source = src_lines.join("\n");
                passes.push(pass);
                src_lines.clear();
            }
            current = Some(ParsedPass {
                name: name.trim().to_string(),
                source: String::new(),
            });
            continue;
        }
        if let Some(ref _cur) = current {
            if trimmed.starts_with("//! iChannel") {
                continue;
            }
        }
        src_lines.push(line);
    }
    if let Some(mut pass) = current {
        pass.source = src_lines.join("\n");
        passes.push(pass);
    }
    passes
}

const WGSL_HEADER: &str = r#"
struct Uniforms {
  iResolution: vec4<f32>,
  iMouse: vec4<f32>,
  iDate: vec4<f32>,
  iChannelResolution: array<vec4<f32>, 4>,
  iTime: f32,
  iTimeDelta: f32,
  iFrame: i32,
  _pad0: f32,
  iViewportOrigin: vec2<f32>,
  _pad1: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var iChannel0: texture_2d<f32>;
@group(0) @binding(2) var iChannel1: texture_2d<f32>;
@group(0) @binding(3) var iChannel2: texture_2d<f32>;
@group(0) @binding(4) var iChannel3: texture_2d<f32>;
@group(0) @binding(5) var iLinearSampler: sampler;

fn stSample(tex: texture_2d<f32>, coord: vec2<f32>) -> vec4<f32> {
  return textureSampleLevel(tex, iLinearSampler, vec2<f32>(coord.x, 1.0 - coord.y), 0.0);
}
fn stTexelFetch(tex: texture_2d<f32>, coord: vec2<i32>) -> vec4<f32> {
  let dim = vec2<i32>(textureDimensions(tex, 0));
  return textureLoad(tex, vec2<i32>(coord.x, dim.y - 1 - coord.y), 0);
}


"#;

const WGSL_FOOTER: &str = r#"
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
  var p = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 3.0, -1.0),
    vec2<f32>(-1.0,  3.0),
  );
  return vec4<f32>(p[vi], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
  return mainImage(pos.xy);
}
"#;

fn main() {
    let path = env::args()
        .nth(1)
        .expect("Usage: test_parse_wgsl <path.wgsl>");
    let source = fs::read_to_string(&path).expect("read shader");
    let passes = parse_passes(&source);
    let mut all_ok = true;
    for pass in passes {
        let wrapped = format!("{WGSL_HEADER}\n{}\n{WGSL_FOOTER}", pass.source);
        let mut parser = naga::front::wgsl::Frontend::new();
        match parser.parse(&wrapped) {
            Ok(module) => {
                let mut validator = naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::all(),
                );
                match validator.validate(&module) {
                    Ok(_) => println!("[PASS] {}", pass.name),
                    Err(e) => {
                        eprintln!("[FAIL VALIDATE] {}: {:?}", pass.name, e);
                        all_ok = false;
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "[FAIL PARSE] {}:\n{}",
                    pass.name,
                    e.emit_to_string(&wrapped)
                );
                all_ok = false;
            }
        }
    }
    if !all_ok {
        std::process::exit(1);
    }
}
