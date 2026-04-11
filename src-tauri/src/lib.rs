mod preview_sidecar;
mod shader_library;

use std::sync::Mutex;

use notify::RecommendedWatcher;
use tauri::Manager;

#[derive(Default)]
struct AppState {
  shader_watcher: Mutex<Option<RecommendedWatcher>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .manage(AppState::default())
    .manage(preview_sidecar::SidecarState::default())
    .plugin(tauri_plugin_shell::init())
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }

      shader_library::ensure_default_providers()?;
      shader_library::start_watcher(app.handle().clone(), &app.state::<AppState>().shader_watcher)?;
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      shader_library::list_shaders,
      shader_library::load_shader,
      shader_library::save_shader,
      preview_sidecar::spawn_preview_sidecar,
      preview_sidecar::send_preview_command,
      preview_sidecar::stop_preview_sidecar,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
