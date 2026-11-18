use regex::Regex;
use std::collections::HashMap;
pub const VERTEX_WGSL: &str = r#"
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

pub(super) const MULTI_HEADER: &str = r#"
struct MultiPassUniforms {
  iTime: f32,
  iTimeDelta: f32,
  iFrame: i32,
  _pad0: f32,
  iResolution: vec4<f32>,
  iMouse: vec4<f32>,
  iDate: vec4<f32>,
  iChannelResolution: array<vec4<f32>, 4>,
  iViewportOrigin: vec2<f32>,
  iViewportSize: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: MultiPassUniforms;
@group(0) @binding(1) var iChannel0: texture_2d<f32>;
@group(0) @binding(2) var iChannel1: texture_2d<f32>;
@group(0) @binding(3) var iChannel2: texture_2d<f32>;
@group(0) @binding(4) var iChannel3: texture_2d<f32>;
@group(0) @binding(5) var iLinearSampler: sampler;

fn stSample(tex: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
  return textureSampleLevel(tex, iLinearSampler, vec2<f32>(uv.x, 1.0 - uv.y), 0.0);
}

fn stTexelFetch(tex: texture_2d<f32>, coord: vec2<i32>) -> vec4<f32> {
  let dims = vec2<i32>(textureDimensions(tex, 0));
  let y = clamp(dims.y - 1 - coord.y, 0, dims.y - 1);
  let x = clamp(coord.x, 0, dims.x - 1);
  return textureLoad(tex, vec2<i32>(x, y), 0);
}
"#;

pub(super) const MULTI_FOOTER: &str = r#"
@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
  let local_coord = ((frag_coord.xy - u.iViewportOrigin) / u.iViewportSize) * u.iResolution.xy;
  let st_frag_coord = vec2<f32>(local_coord.x, u.iResolution.y - local_coord.y);
  return mainImage(st_frag_coord);
}
"#;

pub(super) const GLSL_MULTI_HEADER: &str = r#"#version 450
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
  vec2 iViewportSize;
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

pub(super) const GLSL_MULTI_FOOTER: &str = r#"
layout(location = 0) out vec4 outColor;
void main() {
    vec4 color = vec4(0.0, 0.0, 0.0, 1.0);
    vec2 local_coord = ((gl_FragCoord.xy - iViewportOrigin) / iViewportSize) * iResolution.xy;
    vec2 adjusted_coord = vec2(local_coord.x, iResolution.y - local_coord.y);
    mainImage(color, adjusted_coord);
    outColor = color;
}
"#;

pub(super) const GLSL_COMPAT_HELPERS: &str = r#"


bool pst_isnan(float v) { return v != v; }
bvec2 pst_isnan(vec2 v) { return notEqual(v, v); }
bvec3 pst_isnan(vec3 v) { return notEqual(v, v); }
bvec4 pst_isnan(vec4 v) { return notEqual(v, v); }

bool pst_isinf(float v) { return floatBitsToUint(abs(v)) == 0x7f800000u; }
bvec2 pst_isinf(vec2 v) { return equal(floatBitsToUint(abs(v)), uvec2(0x7f800000u)); }
bvec3 pst_isinf(vec3 v) { return equal(floatBitsToUint(abs(v)), uvec3(0x7f800000u)); }
bvec4 pst_isinf(vec4 v) { return equal(floatBitsToUint(abs(v)), uvec4(0x7f800000u)); }
#define pst_texture(tex, coord) textureLod(tex, vec2((coord).x, 1.0 - (coord).y), 0.0)
#define pst_texelFetch(tex, coord, lod) texelFetch(tex, ivec2((coord).x, textureSize(tex, lod).y - 1 - (coord).y), lod)
"#;

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

pub(super) fn preprocess_glsl(source: &str) -> String {
    let mut lines: Vec<String> = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();

        // Strip #version directives (our header provides it)
        if trimmed.starts_with("#version") {
            lines.push(String::new());
            continue;
        }
        // Strip precision qualifiers (not valid in GLSL 450)
        if trimmed.starts_with("precision ") {
            lines.push(String::new());
            continue;
        }
        // Strip uniform sampler2D declarations (our header provides split texture/sampler)
        if trimmed.contains("uniform")
            && trimmed.contains("sampler2D")
            && !trimmed.contains("layout")
        {
            lines.push(String::new());
            continue;
        }

        lines.push(line.to_string());
    }

    let mut result = inline_const_array_sizes(&lines.join("\n"));
    result = result.replace("isnan(", "pst_isnan(");
    result = result.replace("isinf(", "pst_isinf(");
    result = result.replace("texture(", "pst_texture(");
    result = result.replace("texelFetch(", "pst_texelFetch(");

    // --- Phase 1: Collect functions that have sampler2D params ---
    // We need (func_name, param_name, param_position_in_arglist) for each
    let re_func_sig = regex::Regex::new(r"(?m)^\s*\w[\w\s*]*?\s+(\w+)\s*\(([^)]*)\)").unwrap();

    // Collect: (func_name, param_name)
    let mut s2d_funcs: Vec<(String, String)> = Vec::new();
    for caps in re_func_sig.captures_iter(&result.clone()) {
        let func_name = caps[1].to_string();
        let params_str = &caps[2];
        // Check if any param has sampler2D type
        for param_part in params_str.split(',') {
            let p = param_part.trim();
            if p.contains("sampler2D") {
                // Extract param name (last word after sampler2D)
                if let Some(name) = p.split_whitespace().last() {
                    s2d_funcs.push((func_name.clone(), name.to_string()));
                }
            }
        }
    }

    if s2d_funcs.is_empty() {
        return result;
    }

    // --- Phase 2: Rewrite function signatures ---
    // sampler2D pname -> texture2D pname_t, sampler pname_s
    let mut unique_params: Vec<String> = Vec::new();
    for (_, pname) in &s2d_funcs {
        if !unique_params.contains(pname) {
            unique_params.push(pname.clone());
        }
    }

    for pname in &unique_params {
        // Replace `in sampler2D pname` first (more specific)
        let sig_from2 = format!("in sampler2D {pname}");
        let sig_to2 = format!("in texture2D {pname}_t, sampler {pname}_s");
        result = result.replace(&sig_from2, &sig_to2);

        // Then bare `sampler2D pname`
        let sig_from = format!("sampler2D {pname}");
        let sig_to = format!("texture2D {pname}_t, sampler {pname}_s");
        result = result.replace(&sig_from, &sig_to);

        // --- Phase 3: Rewrite texture/texelFetch calls inside the function body ---
        let constructor = format!("sampler2D({pname}_t, {pname}_s)");

        for func in &[
            "texture",
            "texelFetch",
            "textureSize",
            "textureLod",
            "textureLodOffset",
            "textureGrad",
        ] {
            let usage = format!("{func}({pname},");
            let usage_to = format!("{func}({constructor},");
            result = result.replace(&usage, &usage_to);
        }
    }

    // --- Phase 4: Rewrite call sites ---
    // For each function that now takes (texture2D t, sampler s, ...) instead of (sampler2D, ...),
    // find calls like `FuncName(iChannel0, ...)` and expand to `FuncName(t_iChannel0, iLinearSampler, ...)`
    for (func_name, _pname) in &s2d_funcs {
        for ch in 0..4 {
            let call_from = format!("{func_name}(iChannel{ch},");
            let call_to = format!("{func_name}(t_iChannel{ch}, iLinearSampler,");
            result = result.replace(&call_from, &call_to);

            // Also handle with space: `FuncName( iChannel0,`
            let call_from2 = format!("{func_name}( iChannel{ch},");
            let call_to2 = format!("{func_name}( t_iChannel{ch}, iLinearSampler,");
            result = result.replace(&call_from2, &call_to2);
        }
    }

    result
}
