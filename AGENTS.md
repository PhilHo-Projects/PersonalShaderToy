# PersonalShaderToy instructions

Keep `AGENTS.md` and `CLAUDE.md` identical.

## Product

The desktop viewer uses winit, egui and wgpu in one native window. The browser viewer is a separate static application. Both run gallery shaders and local GLSL, WGSL and HLSL fragment programs. Source inspection and copy are read-only. Desktop users can work in their own editor and save to reload. Engine-specific material and project export are deferred.

## Where to work

- Start with `docs/PROJECT_MAP.md` for entry points and commands, `docs/SHADER_INDEX.md` for examples and fixtures, and `ROADMAP.md` for compatibility decisions. `TIMELINE.md` tracks milestones.
- `shaders/catalog.json` is the entire bundled gallery. Keep original author credits and source intact. Put diagnostics under `tests/fixtures/` and historical experiments under `docs/`.
- Share source preparation, pass graph semantics and compiler versions through `shader-core/`. Keep the native and browser UI, device selection and presentation code independent.
- Build the browser copy of the compiler using `python tools/build-core.py` after changing the core or its lockfile. The site build verifies the fingerprint.
- Run `cargo test --workspace --locked`, build both release surfaces, then run `python tests/verify_viewers.py` on a GPU for changes to bindings, host geometry, compiler output or pass sequencing.

## Rendering invariants

- Every pass owns its uniform buffer. Buffers use ping-pong feedback. Inputs from an earlier pass read this frame, inputs from a later pass or the same pass read the previous frame. The final pass is Image.
- Preview dimensions are physical render-target pixels. Draw Image into a dedicated texture and show that texture in the UI. Use `tests/fixtures/centering.wgsl` to diagnose geometry and sampling before inspecting Kerr–Newman.
- Preserve the last valid pipeline on failed input. Report the failed pass and source location when the compiler supplies one. Record shader-frame intervals and actual submitted GPU work separately.
- Surface capabilities constrain presentation and usage flags. Leaving WGL for a DXGI backend requires a fresh native window. Wake the winit event loop for pending compiles and redraws.

## Web release

The public site is a manually released static Dockerfile app. Use the repository root as build context and `apps/web/Dockerfile` as the Dockerfile. The site serves baked catalog files and loads the compiler on demand in a browser worker. Local shader contents stay in the browser. Keep the site free of server-side shader write endpoints and keep deployment credentials out of this public repository.
