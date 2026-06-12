# Project Map

## Canonical Entry Points

- Native desktop app:
  - root `Cargo.toml`
  - root `src/main.rs`
- Standalone browser app:
  - `apps/web/package.json`
  - `apps/web/src/main.ts`

## Shared Surface

- The only intended shared runtime asset between native and web is `shaders/`.
- Do not assume renderer, transpiler, preview-host, or platform code is shared across the two apps.

## Working Areas

- Native app code: `src/`
  - `src/main.rs` — window shell, renderer, UI, shader pipeline
  - `src/render_settings.rs` — backend/adapter/present-mode/latency/DX12-compiler model
  - `src/gpu_timer.rs` — per-pass GPU timing via timestamp queries
  - `src/bench.rs` — frame stats, benchmark sweep state machine, JSON persistence
- Benchmark results land in `benchmarks/` (gitignored)
- Native helper binary: `src/bin/test_parse.rs`
- Browser app code: `apps/web/src/`
- Browser shader API/watch server: `apps/web/server/`
- Shader corpus: `shaders/`
- Kerr-Newman port docs and references: `docs/kerr-newman-port/`

## High-Value Docs

- Product direction and repo contract:
  - `CLAUDE.md`
  - `AGENTS.md`
- Current planning/tracking:
  - `ROADMAP.md`
  - `TIMELINE.md`
- Shader navigation and token-saving rules:
  - `docs/SHADER_INDEX.md`
  - `docs/WORKING_RULES.md`

## Common Run Commands

- Native app:
  - `cargo run --bin personal-shadertoy`
- Headless benchmark sweep (runs all backends, saves JSON, exits):
  - `$env:PST_AUTO_BENCH = '3,10'; $env:PST_AUTO_BENCH_SHADER = 'shaders\Test\test5.wgsl'; cargo run --bin personal-shadertoy`
- Native parser smoke test:
  - `cargo run --bin test_parse shaders/Shadertoy/kerr_newman_black_hole.glsl`
- Native WGSL multipass centering smoke test:
  - `cargo run --bin test_parse_wgsl shaders/Test/test5.wgsl`
- Browser app:
  - `cd apps/web && npm run dev`

## Read Order For New Tasks

1. Read `CLAUDE.md` or `AGENTS.md`.
2. Read this file.
3. Read `ROADMAP.md` and `TIMELINE.md`.
4. If the task is shader-specific, read `docs/SHADER_INDEX.md`.
5. Only then open raw shader files, and only the specific ones needed.
