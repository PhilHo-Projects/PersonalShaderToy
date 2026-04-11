# PersonalShaderToy Desktop Roadmap

## Summary

- Build this as a `Tauri UI + Rust native renderer sidecar` desktop tool, not as a pure web renderer inside the webview.
- Phase 1 cleans and modularizes the current web app so the UI/domain/services layer can be shared by browser dev mode and the future Tauri desktop shell.
- The first product goal is a generic shader lab: import/edit shaders, switch preview backend, inspect diagnostics/FPS, and learn how the APIs differ.
- Browser-only mode remains supported during the migration.

## Delivery Phases

### Phase 0: Documentation and repo hygiene

- Create canonical root `CLAUDE.md` and mirrored root `AGENTS.md`.
- Create root `TIMELINE.md` as the current-phase tracker.
- Extend `.gitignore` for Tauri, Rust, and sidecar artifacts.
- Keep this document as the decision log and implementation guide.

### Phase 1: Stabilize and modularize the browser app

- Separate `ShaderLanguage`, `PreviewRuntime`, and `PreviewBackend` in the frontend state model.
- Introduce transport-neutral app concerns:
  - shader documents and session state
  - preview config
  - preview diagnostics/stats/capabilities
  - shader library state
- Move browser-specific file loading and watching behind services/interfaces.
- Move preview orchestration behind a `PreviewHost` abstraction while keeping browser preview fully functional.
- Preserve existing editing flow and most existing UI components.

### Phase 2: Bootstrap the Tauri shell

- Add a Tauri v2 shell around the existing frontend.
- Replace browser-only file-system assumptions with Tauri commands/events behind the `ShaderLibraryService`.
- Keep browser dev mode available for fast frontend iteration.
- Add local settings persistence and sidecar lifecycle scaffolding.

### Phase 3: Add the native preview sidecar

- Create a Rust sidecar using `wgpu` + `winit`.
- Scope the initial sidecar to:
  - native preview window
  - single-pass WGSL
  - backend capability detection
  - FPS and compile diagnostics telemetry
- Establish a minimal UI-to-sidecar protocol for:
  - startup handshake
  - backend/config updates
  - document load/update
  - telemetry and lifecycle events

### Phase 4: Tauri <-> sidecar integration

- Let Tauri own sidecar process startup/shutdown and crash recovery.
- Route preview controls through the desktop shell.
- Surface native telemetry, adapter details, and backend availability in the UI.
- Keep browser preview available as a fallback until native parity is acceptable.

### Phase 5: Shader-lab parity and education features

- Grow native preview support for:
  - shared uniforms and input
  - texture channels
  - multipass/ping-pong rendering
  - screenshots
  - file reloads
- Add normalized import/document metadata and translation diagnostics.
- Treat translation as a managed subsystem rather than ad hoc editing.
- Add educational surfaces for backend capabilities and runtime differences.

### Phase 6: Engine integration

- Defer Unity/Unreal export until the lab is stable.
- Introduce a constrained intermediate material/export model rather than exporting arbitrary shader text directly.
- Start with safe-subset exports and explicit validation failures when unsupported constructs are present.

## Core Interfaces

- `ShaderLanguage = 'glsl' | 'wgsl' | 'hlsl'`
- `PreviewRuntime = 'browser' | 'native'`
- `PreviewBackend = 'auto' | 'webgl2' | 'webgpu' | 'dx12' | 'vulkan' | 'metal' | 'opengl'`
- `PreviewConfig`
- `PreviewStats`
- `PreviewCapabilities`
- `PreviewDiagnostic`
- `ShaderLibraryService`
- `PreviewHost`
- `SettingsStore`

## Current Decisions

- The native preview should be a separate sidecar window/process.
- Monaco stays in place for now.
- The first native preview can be visually minimal.
- Engine export is not part of the first desktop milestone.
- Browser mode should remain a supported migration path throughout the early phases.

## Implemented Foundation

- Root project instructions and status docs now live at the repo root.
- Browser app state now distinguishes shader language from preview backend/runtime.
- Browser file access is routed through `ShaderLibraryService`.
- Browser preview orchestration is routed through `PreviewHost`.
- Tauri v2 shell scaffolding is present and validated with a debug build.
- Native preview sidecar scaffolding exists under `native/preview-sidecar` with a minimal JSON command/event protocol.
