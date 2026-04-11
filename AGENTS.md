# PersonalShaderToy Instructions

This file mirrors `CLAUDE.md`. Keep both files synchronized.

## Product Direction

- The long-term architecture is `Tauri UI + Rust native renderer sidecar`.
- The browser app remains a supported development mode during migration.
- Early milestones prioritize:
  - modular frontend architecture
  - backend/runtime switching
  - preview diagnostics and FPS realism
  - safe shader-lab workflows
- Engine export is intentionally deferred until the generic shader lab is stable.

## Implementation Priorities

1. Keep the current browser app functional while extracting reusable services and state.
2. Separate shader language from preview backend/runtime in the frontend model.
3. Move file access and preview orchestration behind replaceable adapters.
4. Use Tauri for desktop shell concerns, not as the primary graphics backend.
5. Keep the native preview intentionally minimal until backend switching and telemetry are solid.

## Repo Conventions

- Root planning/status docs:
  - `ROADMAP.md` is the implementation plan and decision log.
  - `TIMELINE.md` is the current implementation tracker.
- `AGENTS.md` must stay in sync with this file.
- Browser-only code should remain runnable throughout the migration.
- Avoid tying new app logic directly to DOM, fetch, or WebSocket globals when an interface can isolate them.
