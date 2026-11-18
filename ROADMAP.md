# Shader viewer roadmap

The product is a fast way to test a fragment shader without launching an engine. The first release gate is the original five-pass Kerr–Newman running on the desktop and in a browser. Authoring takes place in an external editor or model of the user's choice.

| Milestone | Implementation | Acceptance |
| --- | --- | --- |
| 1. Curate a reference gallery | Keep the original Cyberspace and Kerr–Newman. Remove ten provider experiments, archive scratch sources and preserve the isolated Gargantua composite as a reference. | Both menus list the same two sources. Reference captures predate host changes. |
| 2. Unify shader preparation | One Rust core parses passes, validates GLSL/WGSL, prepares WGSL for GPUs, and exports HLSL via SPIR-V and SPIRV-Cross. Slang imports HLSL into the same pipeline. | A complete HLSL shader copied from the inspector reopens with its channels and produces the reference image. |
| 3. Restore fast iteration | Choose, drop, open or paste. Show original source and HLSL on demand. Watch local desktop files, debounce editor writes and keep the last good preview on failure. | A warm save of a small shader reaches its first new frame within one second on the reference machine. Invalid edits report a source line and keep the good image. |
| 4. Make render output comparable | Use individual pass uniforms, feedback textures and a dedicated final image. Expose physical resolution and render scale. | Both originals pass fixed-image comparison across DX12, Vulkan, OpenGL, WebGL2 and WebGPU. Multipass centering stays within one pixel after resizing. |
| 5. Measure actual work | Show rendered FPS, mean interval, p95, pixels, active backend and measured shader GPU time below the image. Put controls and per-pass figures in Advanced. | Paused playback clears live FPS. Missing GPU timestamps display as unavailable. Native sweeps save exact sample counts, shader step, adapter, resolution and applied present mode. |
| 6. Publish the standalone web viewer | Ship a static build with baked examples and lazy compiler assets. Remove the development write service and editor. | The production origin serves both examples and loads local HLSL without uploading user source. |

## Decisions

- A backend choice changes the graphics API. Identical output for the same shader is expected. Use **Advanced** to see the resolved backend, change native adapter/presentation options, or compare measured frame costs.
- Native and browser hosts have separate render loops and presentation constraints. The shared compiler, manifest, shader inputs and fixed captures define parity.
- Benchmarks use a 1/60 s shader step, neutral input and a locked pixel size. The first successful backend establishes warmup and measurement frame counts for the remaining runs. Compare timings only across matching hardware, dimensions, frame counts and presentation modes.
- The curated example source is the original GLSL. HLSL is generated from that exact shader. A user file can be tried immediately without committing it or modifying a gallery entry.
- Keep Unity/Unreal project export, engine-specific includes, compute kernels and external asset graphs deferred until there is a concrete shader that needs them.

See [the project map](docs/PROJECT_MAP.md) for commands and [the tracker](TIMELINE.md) for verification.
