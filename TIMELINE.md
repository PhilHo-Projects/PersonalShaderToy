# PersonalShaderToy Implementation Timeline

## Current Phase

- `Phase 3 / Phase 4 — sidecar integration`

## Phase Status

- Documentation/bootstrap: `complete`
- Browser modularization: `complete`
- Tauri shell: `complete`
- Native sidecar: `wired into Tauri shell`
- Tauri <-> sidecar integration: `initial wiring complete`
- Engine export: `deferred`

## Exit Criteria For The Current Stage

- Root `CLAUDE.md`, `AGENTS.md`, `ROADMAP.md`, and `TIMELINE.md` exist.
- `.gitignore` covers the upcoming desktop/native artifacts.
- Frontend state separates shader language from preview backend/runtime.
- Browser file access and preview logic are behind replaceable interfaces.
- Current browser workflow still builds and runs.
- Tauri shell can build against the current frontend without the Express shader server.
- Native preview sidecar is spawned from Tauri commands and communicates via stdin/stdout JSON.
- Backend selector in desktop mode enables native backends (DX12, Vulkan, Metal, OpenGL).
- Selecting a native backend spawns the sidecar, swaps the preview panel, and streams stats/diagnostics.
- Switching back to a browser backend (WebGL2/WebGPU) stops the sidecar and restores browser preview.

## Next Milestone

- End-to-end testing of the native preview path in a Tauri debug build.
- Add crash recovery and auto-restart for the sidecar process.
- Input forwarding (mouse/keyboard) from Tauri UI to sidecar window.

## Current Blockers / Watch Items

- Sidecar binary path is resolved from the cargo build directory; production bundling is not yet configured.
- Backend switching requires restarting the sidecar process (the current sidecar does not support runtime backend changes).
- Native preview currently only accepts WGSL shaders; GLSL and HLSL require translation before sending to sidecar.

## Verified Milestones

- `npm run build`
- `cargo check` in `src-tauri/`
- `cargo check` in `native/preview-sidecar/`
- `npm run tauri:build -- --debug`
- Git repo initialized with all Codex work committed.

## Notes

- Browser mode remains the baseline verification environment while desktop mode is scaffolded.
- Tauri and native preview work should not regress the existing shader editing workflow.
- The sidecar is spawned via `std::process::Command` in Tauri Rust commands, not via the Tauri shell plugin JS API. This gives Tauri ownership of the process lifecycle.
