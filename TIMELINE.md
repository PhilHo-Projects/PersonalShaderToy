# PersonalShaderToy Implementation Timeline

## Current Phase

- `Phase 1 / Phase 2 foundation`

## Phase Status

- Documentation/bootstrap: `complete`
- Browser modularization: `in progress`
- Tauri shell: `scaffolded and validated`
- Native sidecar: `scaffolded and compiling`
- Engine export: `deferred`

## Exit Criteria For The Current Stage

- Root `CLAUDE.md`, `AGENTS.md`, `ROADMAP.md`, and `TIMELINE.md` exist.
- `.gitignore` covers the upcoming desktop/native artifacts.
- Frontend state separates shader language from preview backend/runtime.
- Browser file access and preview logic are behind replaceable interfaces.
- Current browser workflow still builds and runs.
- Tauri shell can build against the current frontend without the Express shader server.

## Next Milestone

- Wire the native preview bridge into the Tauri shell and replace the browser preview in desktop mode.

## Current Blockers / Watch Items

- No git repo has been initialized yet, so local change tracking is file-based.
- The current app still has a central `src/main.ts`, but it now depends on service seams instead of raw fetch/websocket/renderer globals.
- Native preview spawning and lifecycle are scaffolded but not yet wired into the desktop UI.

## Verified Milestones

- `npm run build`
- `cargo check` in `src-tauri/`
- `cargo check` in `native/preview-sidecar/`
- `npm run tauri:build -- --debug`

## Notes

- Browser mode remains the baseline verification environment while desktop mode is scaffolded.
- Tauri and native preview work should not regress the existing shader editing workflow.
