# Working Rules

## Goal

Keep context cheap and intentional. Prefer navigational docs and manifests over ingesting giant source files.

## Default Read Order

1. `CLAUDE.md` or `AGENTS.md`
2. `docs/PROJECT_MAP.md`
3. `ROADMAP.md`
4. `TIMELINE.md`
5. `docs/SHADER_INDEX.md`
6. Shader-specific manifests/checklists before raw shader sources

## Token-Saving Rules

- Do not open `shaders/Shadertoy/kerr_newman_black_hole.glsl` unless the task is directly about Kerr-Newman behavior.
- Do not open multiple provider copies of the same shader unless the task is explicitly comparative.
- Prefer `rg` results, manifests, and chunk maps over full-file reads.
- When working on Kerr-Newman, use `docs/kerr-newman-port/manifest.md` as the routing document.
- When investigating native centering, resize artifacts, or multipass feedback drift, try `shaders/Test/test5.wgsl` before opening giant shader files.
- When a task only touches app wiring, stay in `src/`, `apps/web/src/`, `apps/web/server/`, and root docs.

## Repo Separation Rules

- Native app and browser app are separate products.
- Shared runtime surface is limited to `shaders/`.
- Do not assume browser preview logic can be reused in the Rust app, or vice versa, without explicit design work.

## Expensive Files

- `shaders/Shadertoy/kerr_newman_black_hole.glsl`
- `shaders/Test/UI Test 5.hlsl`
- `shaders/openai/kerr_newman_black_hole_multipass.wgsl`
- `shaders/claude/kerr_newman_multipass.wgsl`

If one of these must be opened, do it narrowly and only after checking the relevant docs.

## Good Practice

- Link back to exact files in notes and summaries.
- Keep `AGENTS.md` and `CLAUDE.md` short and repo-wide.
- Put shader-specific detail in `docs/` next to that shader's working notes.
