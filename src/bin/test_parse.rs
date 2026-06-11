use std::{collections::HashMap, env, fs};

use naga::back::wgsl;
use naga::front::glsl;
use regex::Regex;

#[derive(Clone)]
struct ParsedPass {
    name: String,
    source: String,
    channels: [Option<String>; 4],
}

fn inline_const_array_sizes(source: &str) -> String {
    let const_re =
        Regex::new(r"\bconst\s+(?:int|uint)\s+([A-Za-z_]\w*)\s*=\s*(\d+)(?:u)?\s*;").unwrap();
    let array_re = Regex::new(r"\[\s*([A-Za-z_]\w*)\s*\]").unwrap();

    let mut scopes: Vec<HashMap<String, String>> = vec![HashMap::new()];
    let mut result = String::with_capacity(source.len());
    let mut segment = String::new();

    let apply_segment =
        |segment: &mut String, scopes: &mut Vec<HashMap<String, String>>, result: &mut String| {
            if segment.is_empty() {
                return;
            }

            for caps in const_re.captures_iter(segment.as_str()) {
                scopes
                    .last_mut()
                    .unwrap()
                    .insert(caps[1].to_string(), caps[2].to_string());
            }

            let rewritten = array_re.replace_all(segment.as_str(), |caps: &regex::Captures<'_>| {
                scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(&caps[1]))
                    .map(|value| format!("[{value}]"))
                    .unwrap_or_else(|| caps[0].to_string())
            });
            result.push_str(&rewritten);
            segment.clear();
        };

    for ch in source.chars() {
        match ch {
            '{' => {
                apply_segment(&mut segment, &mut scopes, &mut result);
                result.push(ch);
                scopes.push(HashMap::new());
            }
            '}' => {
                apply_segment(&mut segment, &mut scopes, &mut result);
                result.push(ch);
                if scopes.len() > 1 {
                    scopes.pop();
                }
            }
            _ => segment.push(ch),
        }
    }

    apply_segment(&mut segment, &mut scopes, &mut result);
    result
}

fn preprocess_glsl(source: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#version") {
            continue;
        }
        if trimmed.starts_with("precision ") {
            continue;
        }
        if trimmed.contains("uniform")
            && trimmed.contains("sampler2D")
            && !trimmed.contains("layout")
        {
            continue;
        }
        lines.push(line.to_string());
    }

    let mut result = inline_const_array_sizes(&lines.join("\n"));
    result = result.replace("isnan(", "pst_isnan(");
    result = result.replace("isinf(", "pst_isinf(");
    result = result.replace("texture(", "pst_texture(");
    result = result.replace("texelFetch(", "pst_texelFetch(");
    let re_func_sig = regex::Regex::new(r"(?m)^\s*\w[\w\s*]*?\s+(\w+)\s*\(([^)]*)\)").unwrap();
    let mut s2d_funcs: Vec<(String, String)> = Vec::new();
    for caps in re_func_sig.captures_iter(&result.clone()) {
        let func_name = caps[1].to_string();
        let params_str = &caps[2];
        for param_part in params_str.split(',') {
            let p = param_part.trim();
            if p.contains("sampler2D") {
                if let Some(name) = p.split_whitespace().last() {
                    s2d_funcs.push((func_name.clone(), name.to_string()));
                }
            }
        }
    }
    if s2d_funcs.is_empty() {
        return result;
    }

    let mut unique_params: Vec<String> = Vec::new();
    for (_, pname) in &s2d_funcs {
        if !unique_params.contains(pname) {
            unique_params.push(pname.clone());
        }
    }
    for pname in &unique_params {
        result = result.replace(
            &format!("in sampler2D {pname}"),
            &format!("in texture2D {pname}_t, sampler {pname}_s"),
        );
        result = result.replace(
            &format!("sampler2D {pname}"),
            &format!("texture2D {pname}_t, sampler {pname}_s"),
        );
        let constructor = format!("sampler2D({pname}_t, {pname}_s)");
        for func in &[
            "texture",
            "texelFetch",
            "textureSize",
            "textureLod",
            "textureLodOffset",
            "textureGrad",
        ] {
            result = result.replace(
                &format!("{func}({pname},"),
                &format!("{func}({constructor},"),
            );
        }
    }
    for (func_name, _) in &s2d_funcs {
        for ch in 0..4 {
            result = result.replace(
                &format!("{func_name}(iChannel{ch},"),
                &format!("{func_name}(t_iChannel{ch}, iLinearSampler,"),
            );
            result = result.replace(
                &format!("{func_name}( iChannel{ch},"),
                &format!("{func_name}( t_iChannel{ch}, iLinearSampler,"),
            );
        }
    }
    result
}

fn glsl_to_wgsl(glsl_source: &str) -> Result<String, String> {
    let mut parser = glsl::Frontend::default();
    let options = glsl::Options::from(naga::ShaderStage::Fragment);
    let module = parser
        .parse(&options, glsl_source)
        .map_err(|errors| errors.emit_to_string(glsl_source))?;
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let info = validator.validate(&module).map_err(|e| format!("{}", e))?;
    wgsl::write_string(&module, &info, wgsl::WriterFlags::empty()).map_err(|e| format!("{}", e))
}

fn shader_requires_multi_pipeline(source: &str) -> bool {
    [
        "//! PASS:",
        "//!PASS:",
        "iChannelResolution",
        "iChannel0",
        "iChannel1",
        "iChannel2",
        "iChannel3",
        "iTimeDelta",
    ]
    .iter()
    .any(|needle| source.contains(needle))
}

fn pass_requires_multi_pipeline(pass: &ParsedPass) -> bool {
    pass.channels.iter().any(Option::is_some)
        || [
            "iChannelResolution",
            "iChannel0",
            "iChannel1",
            "iChannel2",
            "iChannel3",
            "iTimeDelta",
        ]
        .iter()
        .any(|needle| pass.source.contains(needle))
}

fn parse_passes(source: &str) -> Vec<ParsedPass> {
    let has_markers = source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("//! PASS:") || trimmed.starts_with("//!PASS:")
    });

    if !has_markers {
        return vec![ParsedPass {
            name: "Image".into(),
            source: source.to_string(),
            channels: [None, None, None, None],
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
                pass.source = src_lines.join("\n").trim().to_string();
                passes.push(pass);
                src_lines.clear();
            }

            current = Some(ParsedPass {
                name: name.trim().to_string(),
                source: String::new(),
                channels: [None, None, None, None],
            });
            continue;
        }

        if let Some(ref mut cur) = current {
            if let Some(rest) = trimmed.strip_prefix("//! iChannel") {
                if let Some(digit) = rest.chars().next().and_then(|c| c.to_digit(10)) {
                    if digit <= 3 {
                        if let Some(value) = rest.get(2..).map(|s| s.trim_start_matches(':').trim())
                        {
                            cur.channels[digit as usize] =
                                Some(if value.eq_ignore_ascii_case("self") {
                                    cur.name.clone()
                                } else {
                                    value.to_string()
                                });
                            continue;
                        }
                    }
                }
            }
        }

        src_lines.push(line);
    }

    if let Some(mut pass) = current {
        pass.source = src_lines.join("\n").trim().to_string();
        passes.push(pass);
    }

    if !passes
        .iter()
        .any(|pass| pass.name.eq_ignore_ascii_case("image"))
    {
        if let Some(last) = passes.last_mut() {
            last.name = "Image".into();
        }
    }

    passes
}

const VERTEX_WGSL: &str = r#"
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
  var pos = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
  );
  let xy = pos[vertex_index];
  return vec4<f32>(xy, 0.0, 1.0);
}
"#;

const HEADER: &str = r#"#version 450
layout(binding = 0, set = 0) uniform ShaderUniforms {
  float iTime; int iFrame; vec2 _pad0;
  vec4 iResolution; vec4 iMouse; vec4 iDate;
  vec2 iViewportOrigin; vec2 _pad1;
};
"#;

const FOOTER: &str = r#"
layout(location = 0) out vec4 outColor;
void main() {
    vec4 color = vec4(0.0, 0.0, 0.0, 1.0);
    vec2 local_coord = gl_FragCoord.xy - iViewportOrigin;
    vec2 adjusted_coord = vec2(local_coord.x, iResolution.y - local_coord.y);
    mainImage(color, adjusted_coord);
    outColor = color;
}
"#;

const MULTI_HEADER: &str = r#"#version 450
layout(binding = 0, set = 0) uniform MultiPassUniforms {
  float iTime;
  float iTimeDelta;
  int iFrame;
  float _pad0;
  vec4 iResolution;
  vec4 iMouse;
  vec4 iDate;
  vec4 iChannelResolution[4];
  vec2 iViewportOrigin;
  vec2 _pad1;
};

layout(binding = 1, set = 0) uniform texture2D t_iChannel0;
layout(binding = 2, set = 0) uniform texture2D t_iChannel1;
layout(binding = 3, set = 0) uniform texture2D t_iChannel2;
layout(binding = 4, set = 0) uniform texture2D t_iChannel3;
layout(binding = 5, set = 0) uniform sampler iLinearSampler;

#define iChannel0 sampler2D(t_iChannel0, iLinearSampler)
#define iChannel1 sampler2D(t_iChannel1, iLinearSampler)
#define iChannel2 sampler2D(t_iChannel2, iLinearSampler)
#define iChannel3 sampler2D(t_iChannel3, iLinearSampler)
"#;

const MULTI_FOOTER: &str = r#"
layout(location = 0) out vec4 outColor;
void main() {
    vec4 color = vec4(0.0, 0.0, 0.0, 1.0);
    vec2 local_coord = gl_FragCoord.xy - iViewportOrigin;
    vec2 adjusted_coord = vec2(local_coord.x, iResolution.y - local_coord.y);
    mainImage(color, adjusted_coord);
    outColor = color;
}
"#;

const GLSL_COMPAT_HELPERS: &str = r#"
const float PST_FLT_MAX = 3.402823466e+38;

bool pst_isnan(float v) { return v != v; }
bvec2 pst_isnan(vec2 v) { return notEqual(v, v); }
bvec3 pst_isnan(vec3 v) { return notEqual(v, v); }
bvec4 pst_isnan(vec4 v) { return notEqual(v, v); }

bool pst_isinf(float v) { return abs(v) > PST_FLT_MAX; }
bvec2 pst_isinf(vec2 v) { return greaterThan(abs(v), vec2(PST_FLT_MAX)); }
bvec3 pst_isinf(vec3 v) { return greaterThan(abs(v), vec3(PST_FLT_MAX)); }
bvec4 pst_isinf(vec4 v) { return greaterThan(abs(v), vec4(PST_FLT_MAX)); }
#define pst_texture(tex, coord) texture(tex, vec2((coord).x, 1.0 - (coord).y))
#define pst_texelFetch(tex, coord, lod) texelFetch(tex, ivec2((coord).x, textureSize(tex, lod).y - 1 - (coord).y), lod)
"#;

fn load_shader() -> Result<(String, Option<String>), String> {
    if let Some(path) = env::args().nth(1) {
        let source =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read '{path}': {e}"))?;
        Ok((source, Some(path)))
    } else {
        Ok((
            r#"
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = (fragCoord - 0.5 * iResolution.xy) / iResolution.y;
    fragColor = vec4(uv, sin(iTime), 1.0);
}
"#
            .to_string(),
            None,
        ))
    }
}

fn main() {
    let (shader, source_path) = match load_shader() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("[FAIL] {error}");
            std::process::exit(1);
        }
    };

    let passes = parse_passes(&shader);
    let is_multi = passes.len() > 1 || passes.iter().any(pass_requires_multi_pipeline);

    for pass in passes {
        let cleaned = preprocess_glsl(&pass.source);
        let wrapped = if is_multi || shader_requires_multi_pipeline(&pass.source) {
            format!("{MULTI_HEADER}{GLSL_COMPAT_HELPERS}{cleaned}{MULTI_FOOTER}")
        } else {
            format!("{HEADER}{GLSL_COMPAT_HELPERS}{cleaned}{FOOTER}")
        };

        match glsl_to_wgsl(&wrapped) {
            Ok(wgsl) => {
                let wgsl = wgsl
                    .replace("@fragment \nfn main(", "@fragment \nfn fs_main(")
                    .replace("@fragment\nfn main(", "@fragment\nfn fs_main(");
                let full = format!("{VERTEX_WGSL}\n{wgsl}");

                assert!(full.contains("fn vs_main("), "Missing vs_main!");
                assert!(full.contains("fn fs_main("), "Missing fs_main!");
                assert!(
                    !full.contains("fn fs_main_1("),
                    "Accidentally renamed main_1!"
                );

                let mut parser = naga::front::wgsl::Frontend::new();
                if let Err(e) = parser.parse(&full) {
                    eprintln!("[FAIL] Combined WGSL parse error in {}: {:?}", pass.name, e);
                    std::process::exit(1);
                }

                println!("[PASS] {} -> {} bytes of WGSL", pass.name, full.len());
            }
            Err(e) => {
                eprintln!("[FAIL] GLSL→WGSL failed in {}:\n{}", pass.name, e);
                std::process::exit(1);
            }
        }
    }

    if let Some(path) = source_path {
        println!("[PASS] GLSL pipeline succeeded for {path}");
    } else {
        println!("[PASS] GLSL pipeline succeeded for inline sample");
    }
}
