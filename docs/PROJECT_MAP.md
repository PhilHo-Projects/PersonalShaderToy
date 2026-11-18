# Project map

## Run the viewers

From the repository root with Rust, Node 22 and Python 3.12 or later available:

```sh
cargo run --release --locked -- --example Kerr
cargo run --release --locked -- --open tests/fixtures/centering.wgsl
npm --prefix apps/web ci --include=dev
npm --prefix apps/web run dev
```

The hosted viewer runs at [shaderlab.philippeho.dev](https://shaderlab.philippeho.dev). Vite serves the local viewer on port 5199. Select an example, open or drop a file, or use **Paste and run**. A native file stays attached to its location and recompiles when saved. In a browser, reopen or drop the revised file. **View code** opens the original source or HLSL for the active pass. **Copy shader** includes the multipass wiring.

## Implementation

| Area | Entry point | Owns |
| --- | --- | --- |
| Gallery | `shaders/catalog.json` | Example order, identity, credit, source paths |
| Shader compiler | `shader-core/src/lib.rs`, `templates.rs` | Parsing, uniform/coordinate conventions, validation, WGSL preparation, HLSL generation |
| Native selection and reload | `src/main.rs`, `src/document.rs` | File watching, compiler worker, last successful document, Slang |
| Native display | `src/viewer.rs`, `src/renderer.rs` | egui viewer, physical render targets, passes, captures |
| Native measurements | `src/gpu_timer.rs`, `src/bench.rs`, `src/render_settings.rs` | Timestamp readback, reproducible sweeps, device/present settings |
| Browser | `apps/web/src/main.ts`, `renderer/`, `worker/compiler.worker.ts` | Playback, source inspector, renderers, local compilation |
| Static catalog build | `apps/web/scripts/build-shader-manifest.mjs` | Precompiled examples, compiler integrity and distribution |
| Regression checks | `tests/verify_viewers.py`, `tests/reference/` | Browser and native GPU captures, reloads, round trips and benchmark |

## Compiler setup

The native build provisions a checksum-pinned Slang distribution into the Cargo target directory on supported hosts. Set `PST_SLANGC` to use an installed `slangc`. To distribute a native executable, place the compiler distribution at `compilers/slang/` alongside the executable, preserving its `bin/` and library directories. The installer takes a destination, e.g. `python tools/compilers.py native target/release/compilers/slang`.

The browser ships a prebuilt Rust/WASM core. To regenerate it, install wasm-pack and the Rust `wasm32-unknown-unknown` target, then run `python tools/build-core.py`. The script provisions WASI SDK 34. An existing SDK can be selected with `WASI_SDK_PATH`. Node prebuild verifies the fingerprint, prepares both examples and installs the pinned browser Slang distribution. A fresh build needs network access to the compiler releases. Catalog playback uses the prepared artifacts directly.

## Verification

```sh
cargo test --workspace --locked
cargo build --release --locked
npm --prefix apps/web run build
python -m pip install Pillow playwright
python tests/verify_viewers.py
```

The hardware test uses installed Google Chrome and a Windows GPU with DX12, Vulkan and OpenGL. It writes evidence into ignored `target/viewer-checks/`. A native preview-only capture can also be run directly.

```sh
cargo run --release --locked -- --example Kerr --backend dx12 --size 1280x720 --capture kerr.png --frames 8
```

To benchmark from PowerShell, set `$env:PST_AUTO_BENCH='0.5,1'` and `$env:PST_AUTO_BENCH_SHADER='tests/fixtures/centering.wgsl'`, then run the executable with `--size 1280x720`. It writes a JSON result to `benchmarks/` and exits. Clear those variables before an interactive run.
