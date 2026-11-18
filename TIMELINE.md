# Implementation tracker

## 2026-11-18 · Desktop and web viewers

| Check | Evidence on the Windows reference machine |
| --- | --- |
| Curated gallery | Both menus expose Cyberspace Data Warehouse and the five-pass Kerr–Newman Black Hole. Original source text and credits are retained. |
| Native and browser parity | At 1280 × 720, the original WebGL2 reference and new WebGL2 output matched in the image check. Kerr normalized mean RGB error was 0.0012 on WebGPU and OpenGL, 0.0013 on DX12, and 0.0100 on Vulkan. |
| HLSL from the inspector | Copied the full HLSL for both originals, reopened it in Chrome/WebGPU and native DX12, and compared rendered output. The four-pass centering fixture also round-tripped. |
| External iteration | A warm native save reached its new frame in 125 ms in the initial hardware run. An invalid edit retained the green test frame and reported line 2. An atomic replacement recovered and rendered the new file. |
| Render geometry | The three-buffer diagnostic stayed centered at 1280 × 720 and 640 × 360 and across native OpenGL → DX12 recreation. |
| Measurements | Pausing stopped live FPS. A native fixed-step sweep completed with equal measured frame counts on DX12, Vulkan and OpenGL and saved all three runs to JSON. |
| Web packaging | The browser runs as a static site. Both compiler modules load on demand. The old browser edit store can be exported through Advanced when present. |

Use [the project map](docs/PROJECT_MAP.md) to reproduce these checks. The numbers describe the reference machine and frame sequence. A full benchmark record includes the actual adapter, present mode and render dimensions.

## Earlier milestones

- April 2026 · Native feedback drift was traced to a shared pass uniform buffer. Individual pass buffers restored alignment. Historical shader-port comparisons remain in the Kerr–Newman reference folder.
- June 2026 · Native backend controls, GPU timestamps and automated benchmark sweeps were introduced. Leaving WGL required a fresh window for DXGI.
- August 2026 · The browser became a separately deployed static application.
