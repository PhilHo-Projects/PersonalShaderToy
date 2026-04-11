use std::{
  collections::HashMap,
  fs,
  path::{Path, PathBuf},
  sync::Mutex,
  time::UNIX_EPOCH,
};

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

const DEFAULT_PROVIDERS: &[&str] = &["claude", "gemini", "openai"];
const SHADER_EXTENSIONS: &[&str] = &["glsl", "frag", "wgsl", "hlsl"];

#[derive(Clone, Serialize)]
pub struct ShaderFileResponse {
  pub name: String,
  pub provider: String,
  #[serde(rename = "type")]
  pub shader_type: String,
  pub modified: u64,
}

#[derive(Clone, Serialize)]
pub struct ShaderContentResponse {
  pub name: String,
  pub provider: String,
  #[serde(rename = "type")]
  pub shader_type: String,
  pub content: String,
  pub modified: u64,
}

#[derive(Clone, Serialize)]
pub struct ShaderLibraryEvent {
  #[serde(rename = "type")]
  pub event_type: String,
  pub provider: String,
  pub filename: String,
}

type LibraryResult<T> = Result<T, String>;

pub fn ensure_default_providers() -> LibraryResult<()> {
  let shaders_dir = shaders_dir();
  fs::create_dir_all(&shaders_dir).map_err(|error| error.to_string())?;

  for provider in DEFAULT_PROVIDERS {
    fs::create_dir_all(shaders_dir.join(provider)).map_err(|error| error.to_string())?;
  }

  Ok(())
}

pub fn start_watcher(app_handle: AppHandle, slot: &Mutex<Option<RecommendedWatcher>>) -> LibraryResult<()> {
  let root = shaders_dir();
  let watch_root = root.clone();

  let mut watcher = RecommendedWatcher::new(
    move |result: Result<Event, notify::Error>| match result {
      Ok(event) => {
        let event_type = match event.kind {
          EventKind::Create(_) => Some("file-added"),
          EventKind::Modify(_) => Some("file-changed"),
          EventKind::Remove(_) => Some("file-removed"),
          _ => None,
        };

        let Some(event_type) = event_type else {
          return;
        };

        for path in event.paths {
          if let Some((provider, filename)) = split_shader_path(&watch_root, &path) {
            let payload = ShaderLibraryEvent {
              event_type: event_type.to_string(),
              provider,
              filename,
            };
            let _ = app_handle.emit("shader-library-event", payload);
          }
        }
      }
      Err(error) => eprintln!("shader watcher error: {error}"),
    },
    Config::default(),
  )
  .map_err(|error| error.to_string())?;

  watcher
    .watch(&root, RecursiveMode::Recursive)
    .map_err(|error| error.to_string())?;

  *slot.lock().map_err(|_| "shader watcher mutex poisoned".to_string())? = Some(watcher);
  Ok(())
}

#[tauri::command]
pub fn list_shaders() -> LibraryResult<HashMap<String, Vec<ShaderFileResponse>>> {
  let root = shaders_dir();
  let mut result = HashMap::new();

  if !root.exists() {
    return Ok(result);
  }

  for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
    let entry = entry.map_err(|error| error.to_string())?;
    let file_type = entry.file_type().map_err(|error| error.to_string())?;
    if !file_type.is_dir() {
      continue;
    }

    let provider = entry.file_name().to_string_lossy().to_string();
    let mut files = Vec::new();

    for file in fs::read_dir(entry.path()).map_err(|error| error.to_string())? {
      let file = file.map_err(|error| error.to_string())?;
      let path = file.path();
      if !is_shader_file(&path) {
        continue;
      }

      let modified = modified_millis(&path)?;
      let Some(filename) = path.file_name().map(|name| name.to_string_lossy().to_string()) else {
        continue;
      };

      files.push(ShaderFileResponse {
        name: filename,
        provider: provider.clone(),
        shader_type: detect_shader_type(&path),
        modified,
      });
    }

    files.sort_by(|a, b| b.modified.cmp(&a.modified));
    result.insert(provider, files);
  }

  Ok(result)
}

#[tauri::command]
pub fn load_shader(provider: String, filename: String) -> LibraryResult<ShaderContentResponse> {
  let path = resolve_shader_path(&provider, &filename)?;
  let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;

  Ok(ShaderContentResponse {
    name: filename,
    provider,
    shader_type: detect_shader_type(&path),
    content,
    modified: modified_millis(&path)?,
  })
}

#[tauri::command]
pub fn save_shader(provider: String, filename: String, content: String) -> LibraryResult<()> {
  sanitize_segment(&provider)?;
  sanitize_segment(&filename)?;

  let dir = shaders_dir().join(&provider);
  fs::create_dir_all(&dir).map_err(|error| error.to_string())?;

  let path = dir.join(&filename);
  fs::write(path, content).map_err(|error| error.to_string())?;
  Ok(())
}

fn shaders_dir() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("shaders")
}

fn resolve_shader_path(provider: &str, filename: &str) -> LibraryResult<PathBuf> {
  sanitize_segment(provider)?;
  sanitize_segment(filename)?;

  let path = shaders_dir().join(provider).join(filename);
  if !path.exists() {
    return Err("File not found".to_string());
  }

  if !is_shader_file(&path) {
    return Err("Unsupported shader file type".to_string());
  }

  Ok(path)
}

fn sanitize_segment(value: &str) -> LibraryResult<()> {
  if value.is_empty() {
    return Err("Empty path segment".to_string());
  }

  let path = Path::new(value);
  if path.components().count() != 1 {
    return Err("Invalid path segment".to_string());
  }

  Ok(())
}

fn is_shader_file(path: &Path) -> bool {
  path
    .extension()
    .and_then(|value| value.to_str())
    .map(|value| SHADER_EXTENSIONS.contains(&value.to_ascii_lowercase().as_str()))
    .unwrap_or(false)
}

fn detect_shader_type(path: &Path) -> String {
  match path.extension().and_then(|value| value.to_str()).map(|value| value.to_ascii_lowercase()) {
    Some(extension) if extension == "wgsl" => "wgsl".to_string(),
    Some(extension) if extension == "hlsl" => "hlsl".to_string(),
    _ => "glsl".to_string(),
  }
}

fn modified_millis(path: &Path) -> LibraryResult<u64> {
  let modified = fs::metadata(path)
    .map_err(|error| error.to_string())?
    .modified()
    .map_err(|error| error.to_string())?
    .duration_since(UNIX_EPOCH)
    .map_err(|error| error.to_string())?;
  Ok(modified.as_millis() as u64)
}

fn split_shader_path(root: &Path, path: &Path) -> Option<(String, String)> {
  if !is_shader_file(path) {
    return None;
  }

  let relative = path.strip_prefix(root).ok()?;
  let mut components = relative.components();
  let provider = components.next()?.as_os_str().to_string_lossy().to_string();
  let filename = components.next()?.as_os_str().to_string_lossy().to_string();

  if components.next().is_some() {
    return None;
  }

  Some((provider, filename))
}
