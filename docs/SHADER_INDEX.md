# Shader Index

## Usage Rule

- Do not open the largest shader files by default.
- Start from this index and any per-shader manifest/checklist first.

## High-Cost Files

These are the files most likely to waste tokens if opened casually.

| File | Approx Size | Role | Read First |
| --- | ---: | --- | --- |
| `shaders/Shadertoy/kerr_newman_black_hole.glsl` | 168 KB | Canonical giant Shadertoy source | `docs/kerr-newman-port/manifest.md` |
| `shaders/Test/UI Test 5.hlsl` | 91 KB | Large HLSL test dump | Only if task is explicitly about HLSL test coverage |
| `shaders/openai/kerr_newman_black_hole_multipass.wgsl` | 56 KB | Main WGSL Kerr-Newman port target | `docs/kerr-newman-port/checklist.md` |
| `shaders/claude/kerr_newman_multipass.wgsl` | 45 KB | Alternate large WGSL port | Read only for cross-port comparison |

## Shader Families

### Kerr-Newman Family

- Canonical GLSL source:
  - `shaders/Shadertoy/kerr_newman_black_hole.glsl`
- Main active WGSL target:
  - `shaders/openai/kerr_newman_black_hole_multipass.wgsl`
- Supporting reference copies:
  - `shaders/openai/kerr_newman_black_hole.wgsl`
  - `shaders/claude/kerr_newman_black_hole.wgsl`
  - `shaders/claude/kerr_newman_multipass.wgsl`
  - `shaders/gemini/kerr_newman_wgsl.wgsl`
- Read these docs first:
  - `docs/kerr-newman-port/manifest.md`
  - `docs/kerr-newman-port/checklist.md`
  - `docs/kerr-newman-port/visual-baseline.md`
- Reference assets:
  - `docs/kerr-newman-port/reference/FinalBoss/`
  - `docs/kerr-newman-port/baselines/`

### Shadertoy Imports

- `shaders/Shadertoy/gargantua_bloom.glsl`
- `shaders/Shadertoy/cyberspace_data_warehouse.glsl`
- Usually read these only when working on import/parsing/compatibility behavior.

### Browser/Renderer Samples

- `shaders/WebGPU/`
- Small WGSL samples for renderer smoke checks and quick repros.

### Provider Buckets

- `shaders/openai/`
- `shaders/claude/`
- `shaders/gemini/`
- These are useful for comparing generated ports, but do not assume they are all equally current.

### Test Bucket

- `shaders/Test/`
- Scratch/test assets, including large HLSL cases.
- Treat these as task-specific fixtures, not primary architecture docs.
- `shaders/Test/test5.wgsl` is the preferred cheap native multipass centering diagnostic. It exercises Buffer A/B/C/Image sampling alignment without opening Kerr-Newman-sized sources.

## Task Routing

- If the task is about repo architecture or app wiring:
  - Do not open giant shaders first.
- If the task is about Kerr-Newman visual parity:
  - Start from the three `docs/kerr-newman-port/*.md` files.
- If the task is about native centering, resize artifacts, or sampled-buffer drift:
  - Reproduce first with `shaders/Test/test5.wgsl`.
  - Only open Kerr-Newman after the host path passes that small fixture.
- If the task is about GLSL/WGSL transpilation:
  - Open the specific failing shader plus `src/main.rs` or the browser renderer/transpiler code as needed.
