use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::Mutex,
    thread,
};

use tauri::{AppHandle, Emitter, State};

pub struct SidecarState {
    process: Mutex<Option<SidecarProcess>>,
}

struct SidecarProcess {
    child: Child,
    stdin: ChildStdin,
}

impl Default for SidecarState {
    fn default() -> Self {
        Self {
            process: Mutex::new(None),
        }
    }
}

impl Drop for SidecarState {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.process.lock() {
            if let Some(mut proc) = slot.take() {
                let _ = writeln!(proc.stdin, r#"{{"type":"shutdown"}}"#);
                let _ = proc.stdin.flush();
                let _ = proc.child.kill();
                let _ = proc.child.wait();
            }
        }
    }
}

#[tauri::command]
pub fn spawn_preview_sidecar(
    app: AppHandle,
    state: State<'_, SidecarState>,
    backend: Option<String>,
) -> Result<(), String> {
    let mut slot = state
        .process
        .lock()
        .map_err(|_| "sidecar mutex poisoned".to_string())?;

    // Kill existing sidecar if one is running
    if let Some(mut proc) = slot.take() {
        let _ = writeln!(proc.stdin, r#"{{"type":"shutdown"}}"#);
        let _ = proc.stdin.flush();
        let _ = proc.child.kill();
        let _ = proc.child.wait();
    }

    let sidecar_path = resolve_sidecar_path()?;

    let mut cmd = Command::new(&sidecar_path);
    if let Some(ref backend) = backend {
        cmd.arg(backend);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn sidecar at '{}': {e}", sidecar_path))?;

    let stdin = child.stdin.take().ok_or("Failed to get sidecar stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to get sidecar stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to get sidecar stderr")?;

    // Read sidecar stdout (JSON events) in a background thread
    let app_stdout = app.clone();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let _ = app_stdout.emit("preview-sidecar-event", trimmed.to_string());
        }
        // Sidecar process exited — notify frontend
        let _ = app_stdout.emit(
            "preview-sidecar-event",
            r#"{"type":"diagnostic","level":"warning","message":"Native preview sidecar exited."}"#
                .to_string(),
        );
    });

    // Read sidecar stderr in a background thread
    let app_stderr = app.clone();
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            let text = line.trim();
            if text.is_empty() {
                continue;
            }
            let payload = serde_json::json!({
                "type": "diagnostic",
                "level": "warning",
                "message": format!("[sidecar stderr] {text}")
            })
            .to_string();
            let _ = app_stderr.emit("preview-sidecar-event", payload);
        }
    });

    *slot = Some(SidecarProcess { child, stdin });
    Ok(())
}

#[tauri::command]
pub fn send_preview_command(
    state: State<'_, SidecarState>,
    command: String,
) -> Result<(), String> {
    let mut slot = state
        .process
        .lock()
        .map_err(|_| "sidecar mutex poisoned".to_string())?;

    let proc = slot.as_mut().ok_or("Sidecar is not running")?;

    writeln!(proc.stdin, "{command}")
        .map_err(|e| format!("Failed to write to sidecar stdin: {e}"))?;
    proc.stdin
        .flush()
        .map_err(|e| format!("Failed to flush sidecar stdin: {e}"))?;

    Ok(())
}

#[tauri::command]
pub fn stop_preview_sidecar(state: State<'_, SidecarState>) -> Result<(), String> {
    let mut slot = state
        .process
        .lock()
        .map_err(|_| "sidecar mutex poisoned".to_string())?;

    if let Some(mut proc) = slot.take() {
        // Try graceful shutdown, then force kill
        let _ = writeln!(proc.stdin, r#"{{"type":"shutdown"}}"#);
        let _ = proc.stdin.flush();
        let _ = proc.child.kill();
        let _ = proc.child.wait();
    }

    Ok(())
}

/// Resolve the sidecar binary path.
///
/// During development this finds the binary in the preview-sidecar cargo
/// target directory.  A future milestone will bundle it alongside the Tauri
/// app for production builds.
fn resolve_sidecar_path() -> Result<String, String> {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let base = manifest_dir
        .join("..")
        .join("native")
        .join("preview-sidecar")
        .join("target");

    let bin_name = if cfg!(target_os = "windows") {
        "preview-sidecar.exe"
    } else {
        "preview-sidecar"
    };

    // Prefer release build, fall back to debug
    for profile in ["release", "debug"] {
        let path = base.join(profile).join(bin_name);
        if path.exists() {
            return path
                .canonicalize()
                .map_err(|e| e.to_string())?
                .to_str()
                .map(String::from)
                .ok_or_else(|| "Sidecar path is not valid UTF-8".to_string());
        }
    }

    Err(format!(
        "Preview sidecar binary not found. Run 'cargo build' in native/preview-sidecar/ first. Searched: {}",
        base.join("debug").join(bin_name).display()
    ))
}
