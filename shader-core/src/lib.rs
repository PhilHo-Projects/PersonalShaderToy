//! Shader preparation shared by the desktop renderer, web worker and catalog builder.
mod templates;
use naga::{
    ShaderStage,
    valid::{Capabilities, ValidationFlags, Validator},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
pub use templates::VERTEX_WGSL;
use templates::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pass {
    pub name: String,
    pub channels: [Option<String>; 4],
    pub source: String,
    pub line: usize,
}

pub fn parse(source: &str) -> Result<Vec<Pass>, String> {
    let mut passes = Vec::new();
    let mut current = Pass {
        name: "Image".into(),
        channels: Default::default(),
        source: String::new(),
        line: 1,
    };
    let mut marked = false;
    for (i, text) in source.lines().enumerate() {
        let directive = text
            .trim()
            .strip_prefix("//!")
            .and_then(|v| v.trim().split_once(':'));
        match directive {
            Some((key, value)) if key.trim().eq_ignore_ascii_case("pass") => {
                if marked {
                    passes.push(current);
                } else if !current.source.trim().is_empty()
                    && current
                        .source
                        .lines()
                        .any(|s| !s.trim().is_empty() && !s.trim().starts_with("//"))
                {
                    return Err(format!(
                        "Line {}: code before the first PASS directive",
                        i + 1
                    ));
                }
                marked = true;
                current = Pass {
                    name: value.trim().to_owned(),
                    channels: Default::default(),
                    source: String::new(),
                    line: i + 2,
                };
            }
            Some((key, value)) if key.trim().starts_with("iChannel") => {
                let index = key.trim()[8..]
                    .parse::<usize>()
                    .ok()
                    .filter(|v| *v < 4)
                    .ok_or_else(|| {
                        format!("Line {}: expected iChannel0 through iChannel3", i + 1)
                    })?;
                let value = value.trim();
                current.channels[index] = Some(if value.eq_ignore_ascii_case("self") {
                    current.name.clone()
                } else {
                    value.to_owned()
                });
                current.source.push('\n');
            }
            _ => {
                current.source.push_str(text);
                current.source.push('\n');
            }
        }
    }
    passes.push(current);
    let mut names = HashSet::new();
    for p in &passes {
        if p.name.is_empty() || !names.insert(p.name.clone()) {
            return Err(format!(
                "Line {}: empty or duplicate pass '{}'",
                p.line, p.name
            ));
        }
        if p.source.trim().is_empty() {
            return Err(format!("{}: empty shader", p.name));
        }
    }
    if passes.last().map(|p| p.name.as_str()) != Some("Image") {
        return Err("The last pass must be named Image".into());
    }
    for p in &passes {
        for ch in p.channels.iter().flatten() {
            if ch != "keyboard" && (ch == "Image" || !names.contains(ch)) {
                return Err(format!("{}: unavailable channel '{ch}'", p.name));
            }
        }
    }
    Ok(passes)
}

pub fn serialize_passes(passes: &[Pass]) -> String {
    if passes.len() == 1 && passes[0].channels.iter().all(Option::is_none) {
        return passes[0].source.clone();
    }
    let mut output = String::new();
    for p in passes {
        output.push_str(&format!("//! PASS: {}\n", p.name));
        for (i, ch) in p.channels.iter().enumerate() {
            if let Some(ch) = ch {
                output.push_str(&format!(
                    "//! iChannel{i}: {}\n",
                    if ch == &p.name { "self" } else { ch }
                ));
            }
        }
        output.push_str(&p.source);
        if !output.ends_with('\n') {
            output.push('\n');
        }
    }
    output
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prepared {
    pub name: String,
    pub channels: [Option<String>; 4],
    pub wgsl: String,
    pub entry: String,
    pub legacy: bool,
}

fn location(pass: &Pass, generated: u32, prefix: usize) -> usize {
    pass.line + (generated as usize).saturating_sub(prefix + 1)
}

pub fn prepare(pass: &Pass, language: &str) -> Result<Prepared, String> {
    let mut validator = Validator::new(ValidationFlags::all(), Capabilities::all());
    let mut prefix_lines = 0;
    let wgsl = match language {
        "glsl" => {
            let body = preprocess_glsl(&pass.source);
            let prefix = format!("{GLSL_MULTI_HEADER}{GLSL_COMPAT_HELPERS}");
            let footer = if body.contains("void mainImage") && !body.contains("void main()") {
                GLSL_MULTI_FOOTER
            } else {
                ""
            };
            let full = format!("{prefix}{body}\n{footer}");
            let module = naga::front::glsl::Frontend::default()
                .parse(&ShaderStage::Fragment.into(), &full)
                .map_err(|e| {
                    e.errors
                        .iter()
                        .map(|e| {
                            format!(
                                "{} line {}: {}",
                                pass.name,
                                location(
                                    pass,
                                    e.meta.location(&full).line_number,
                                    prefix.lines().count()
                                ),
                                e.kind
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })?;
            let info = validator
                .validate(&module)
                .map_err(|e| format!("{}: {}", pass.name, e.emit_to_string(&full)))?;
            naga::back::wgsl::write_string(&module, &info, naga::back::wgsl::WriterFlags::empty())
                .map_err(|e| e.to_string())?
        }
        "wgsl" => {
            let complete = pass.source.contains("@fragment");
            let own_bindings =
                pass.source.contains("var<uniform>") || pass.source.contains("@group(");
            let prefix = if own_bindings || (complete && !pass.source.contains("u.")) {
                ""
            } else {
                MULTI_HEADER
            };
            prefix_lines = prefix.lines().count();
            let footer = if complete { "" } else { MULTI_FOOTER };
            format!("{prefix}{}\n{footer}", pass.source)
        }
        _ => {
            return Err(format!(
                "{}: expected GLSL or WGSL after HLSL compilation",
                pass.name
            ));
        }
    };
    let module = naga::front::wgsl::parse_str(&wgsl).map_err(|e| {
        format!(
            "{} line {}: {}",
            pass.name,
            location(
                pass,
                e.location(&wgsl).map_or(1, |l| l.line_number),
                prefix_lines
            ),
            e.message()
        )
    })?;
    validator.validate(&module).map_err(|e| {
        format!(
            "{} line {}: {}",
            pass.name,
            location(
                pass,
                e.location(&wgsl).map_or(1, |l| l.line_number),
                prefix_lines
            ),
            e.emit_to_string(&wgsl)
        )
    })?;
    let entry = module
        .entry_points
        .iter()
        .find(|e| e.stage == ShaderStage::Fragment)
        .ok_or_else(|| format!("{}: missing fragment entry point", pass.name))?
        .name
        .clone();
    let legacy = module.global_variables.iter().any(|(_, global)| {
        let Some(binding) = &global.binding else {
            return false;
        };
        let naga::TypeInner::Struct { members, .. } = &module.types[global.ty].inner else {
            return false;
        };
        binding.group == 0
            && binding.binding == 0
            && members
                .iter()
                .any(|field| field.offset == 4 && field.name.as_deref() == Some("iFrame"))
    });
    Ok(Prepared {
        name: pass.name.clone(),
        channels: pass.channels.clone(),
        wgsl,
        entry,
        legacy,
    })
}

pub fn hlsl(pass: &Prepared) -> Result<String, String> {
    use naga::back::spv;
    use spirv_cross2::{
        Compiler, Module,
        compile::hlsl::{CompilerOptions, HlslShaderModel},
        targets::Hlsl,
    };
    let module = naga::front::wgsl::parse_str(&pass.wgsl).map_err(|e| e.to_string())?;
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|e| e.to_string())?;
    let mut options = spv::Options::default();
    for (_, v) in module.global_variables.iter() {
        if let Some(b) = v.binding.as_ref() {
            options.binding_map.insert(
                *b,
                spv::BindingInfo {
                    descriptor_set: b.group,
                    binding: b.binding,
                    binding_array_size: None,
                },
            );
        }
    }
    options.fake_missing_bindings = false;
    options.flags.insert(spv::WriterFlags::DEBUG);
    let words = spv::write_vec(
        &module,
        &info,
        &options,
        Some(&spv::PipelineOptions {
            shader_stage: ShaderStage::Fragment,
            entry_point: pass.entry.clone(),
        }),
    )
    .map_err(|e| e.to_string())?;
    let compiler = Compiler::<Hlsl>::new(Module::from_words(&words)).map_err(|e| e.to_string())?;
    let mut options = CompilerOptions::default();
    options.shader_model = HlslShaderModel::ShaderModel6_0;
    compiler
        .compile(&options)
        .map(|v| v.to_string())
        .map_err(|e| format!("{}: {e}", pass.name))
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn graph_json(source: &str) -> Result<String, String> {
    serde_json::to_string(&parse(source)?).map_err(|e| e.to_string())
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn prepare_json(pass: &str, language: &str) -> Result<String, String> {
    serde_json::to_string(&prepare(
        &serde_json::from_str(pass).map_err(|e| e.to_string())?,
        language,
    )?)
    .map_err(|e| e.to_string())
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn export_hlsl(pass: &str) -> Result<String, String> {
    hlsl(&serde_json::from_str(pass).map_err(|e| e.to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feedback_round_trips_and_dangling_input_fails() {
        let p = parse("//! PASS: A\n//! iChannel0: self\nx\n//! PASS: Image\n//! iChannel1: A\ny")
            .unwrap();
        assert_eq!(p[0].channels[0].as_deref(), Some("A"));
        assert_eq!(
            parse(&serialize_passes(&p)).unwrap()[1].channels,
            p[1].channels
        );
        assert!(parse("//! PASS: Image\n//! iChannel2: Missing\nx").is_err());
    }
    #[test]
    fn source_line_survives_wrapper() {
        let p = parse("// a\nfn mainImage(p: vec2f) -> vec4f {\nreturn missing;\n}").unwrap();
        assert!(prepare(&p[0], "wgsl").unwrap_err().contains("Image line 3"));
    }
}
