use shader_core::{Pass, Prepared};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

pub struct Example {
    pub title: &'static str,
    pub language: &'static str,
    pub credit: &'static str,
    pub source: &'static str,
}
include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

#[derive(Clone)]
pub struct Document {
    pub title: String,
    pub language: String,
    pub source: Arc<str>,
    pub path: Option<PathBuf>,
    pub credit: String,
}
impl Document {
    pub fn example(e: &Example) -> Self {
        Self {
            title: e.title.into(),
            language: e.language.into(),
            source: e.source.into(),
            path: None,
            credit: e.credit.into(),
        }
    }
    pub fn open(path: &Path) -> Result<Self, String> {
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let source = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let mut doc = Self::clipboard(&source);
        doc.title = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        match path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase()
            .as_str()
        {
            "glsl" | "frag" => doc.language = "glsl".into(),
            "hlsl" => doc.language = "hlsl".into(),
            "wgsl" => doc.language = "wgsl".into(),
            _ => {}
        }
        doc.path = Some(path);
        Ok(doc)
    }
    pub fn clipboard(source: &str) -> Self {
        let lower = source.to_ascii_lowercase();
        let lang = if lower.contains("fn ") || lower.contains("@fragment") {
            "wgsl"
        } else if lower.contains("sv_target") || lower.contains("sv_position") {
            "hlsl"
        } else {
            "glsl"
        };
        Self {
            title: format!("Clipboard.{lang}"),
            language: lang.into(),
            source: source.into(),
            path: None,
            credit: String::new(),
        }
    }
    pub fn compile(&self) -> Result<(Vec<Pass>, Vec<Prepared>), String> {
        let passes = shader_core::parse(&self.source)?;
        let mut prepared = Vec::new();
        for pass in &passes {
            if self.language == "hlsl" {
                let translated = Pass {
                    source: slang(pass)?,
                    ..pass.clone()
                };
                prepared.push(shader_core::prepare(&translated, "wgsl")?);
            } else {
                prepared.push(shader_core::prepare(pass, &self.language)?);
            }
        }
        Ok((passes, prepared))
    }
}

fn slang(pass: &Pass) -> Result<String, String> {
    let exe = if cfg!(windows) {
        "slangc.exe"
    } else {
        "slangc"
    };
    let adjacent = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("compilers/slang/bin").join(exe)))
        .filter(|p| p.exists());
    let compiler = std::env::var_os("PST_SLANGC")
        .map(PathBuf::from)
        .or(adjacent)
        .or_else(|| {
            option_env!("PST_BUILD_SLANGC")
                .map(PathBuf::from)
                .filter(|p| p.exists())
        })
        .unwrap_or_else(|| exe.into());
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let input = tmp.path().join("shader.hlsl");
    fs::write(
        &input,
        format!("#line {} \"shader.hlsl\"\n{}", pass.line, pass.source),
    )
    .map_err(|e| e.to_string())?;
    let mut cmd = Command::new(compiler);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let result = cmd
        .arg(&input)
        .args(["-entry", "main", "-stage", "fragment", "-target", "wgsl"])
        .output()
        .map_err(|e| {
            format!("Cannot start Slang ({e}). Set PST_SLANGC to the compiler executable.")
        })?;
    if !result.status.success() {
        return Err(format!(
            "{} · {}",
            pass.name,
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    String::from_utf8(result.stdout).map_err(|e| e.to_string())
}
