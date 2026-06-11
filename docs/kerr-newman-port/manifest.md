# Kerr-Newman Port Manifest

## Canonical Sources

- Original multipass shader: `shaders/Shadertoy/kerr_newman_black_hole.glsl`
- Split Buffer A reference: `docs/kerr-newman-port/reference/FinalBoss/BufferA.txt`
- Active WGSL target: `shaders/openai/kerr_newman_black_hole_multipass.wgsl`

## Pass Graph Lock

- `Buffer A`
- `Buffer B` with `iChannel0: Buffer A`, `iChannel1: self`, `iChannel3: keyboard`
- `Buffer C` with `iChannel0: Buffer B`
- `Buffer D` with `iChannel0: Buffer C`
- `Image` with `iChannel0: Buffer A`, `iChannel3: Buffer D`

## Pass Ranges

| Pass | Original GLSL | Current WGSL |
| --- | --- | --- |
| Buffer A | `1-3377` | `1-1048` |
| Buffer B | `3378-3639` | `1049-1297` |
| Buffer C | `3640-3690` | `1298-1329` |
| Buffer D | `3691-3741` | `1330-1361` |
| Image | `3742-3876` | `1362-1461` |

## Buffer A Chunk Map

| Chunk | Original GLSL | Split BufferA.txt | Current WGSL | Notes |
| --- | --- | --- | --- | --- |
| Random/jitter setup | `195-240` | `192-237` | `62-159` | Core noise helpers exist; exactness still mixed. |
| Color science | `287-325` | `284-322` | `160-205` | `KelvinToRgb` present; `WavelengthToRgb` restored in current WGSL milestone. |
| Geometry + tensors | `633-1152` | `630-1149` | `206-426` | Main math path exists, but needs one-by-one equivalence audit. |
| Background/stars | `329-599` | `326-596` | `452-528` | Current milestone now uses original-style dense star field and spectral remap. |
| Disk sampling | `1452-1913` | `1449-1910` | `529-706` | Implemented but still visually divergent from original hot band. |
| Jets | `1762-1913` | `1759-1910` | `707-750` | Present but not yet audited line-by-line. |
| Tone mapping | `602-615` | `599-612` | `751-772` | Current milestone restored original-style Buffer A pre-bloom tone mapping. |
| Trace + integration | `2425-3377` | `2422-3377` | `773-1048` | Camera-state read from Buffer B restored; still missing original `TraceResult` richness. |

## Milestones

| Milestone | Status | Notes |
| --- | --- | --- |
| CRLF-safe multipass parsing | Done | `src/renderer/shaderParser.ts` normalizes line endings before pass parsing. |
| Post stack parity (`Buffer B/C/D/Image`) | Done | Replaced with a more literal translation of the original post/bloom stack. |
| Native multipass host centering | Done | `2026-04-24`: fixed Rust host pass-uniform lifetime bug. Each compiled pass now owns its own uniform buffer, resolving sampled-buffer drift exposed by `shaders/Test/test5.wgsl`. |
| Buffer A camera/state from Buffer B | Done | `Buffer A` now reads camera basis and universe sign from `iChannel2: Buffer B`. |
| Background + Buffer A tone-map chunk | In progress | Dense stars, spectral background shift, and pre-history tone map are now in; antiverse rain and frequency-shift plumbing still remain. |
| Physics/core audit | Pending | Needs chunk-by-chunk mapping against original helpers. |

## Immediate Next Chunks

1. Replace the placeholder `current_shift = 1.0` path with real escape-frequency tracking from the ray trace.
2. Audit `DiskColor` constants and control flow against the original hot upper arc behavior.
3. Restore original `TraceResult`-style output semantics so background, tone mapping, and history blend use the same data as GLSL.

## Fresh-Chat Handoff

- Treat `D:\PersonalShaderToy\shaders\Shadertoy\kerr_newman_black_hole.glsl` and `D:\PersonalShaderToy\docs\kerr-newman-port\reference\FinalBoss\BufferA.txt` as the code oracle.
- Treat `D:\PersonalShaderToy\docs\kerr-newman-port\baselines\original-kerr-baseline.png` as the visual oracle.
- Do not rewrite the whole shader.
- Do not chase native centering/framing by opening the giant shader first. Re-check `shaders/Test/test5.wgsl` if host drift is suspected.
- Do not re-open already-fixed areas unless a direct dependency forces it:
- stars/background density
- event horizon / shell asymmetry
- jet visibility gate
- full disk-noise octave span
- full disk sub-march
- Focus only on the final mismatch:
- the bright left cloud patch is still too visible / slightly misplaced relative to the original
- it may be a layering/refraction problem rather than a raw brightness constant problem
- Work in narrow slices only:
- load one original GLSL chunk
- load the matching WGSL chunk
- compare formulas and coordinate conventions literally
- patch only that chunk
- verify with a reset screenshot before moving on
- Most likely remaining source:
- the still-unported heat-haze / refraction subsystem in `BufferA.txt` around `1154-1443` and `2545-2697`
- This is a better suspect than stars, jets, or shell boost now, because those areas already received direct parity fixes.
- Secondary suspects if heat haze is not the cause:
- any remaining `DiskColor` plume-balance differences around `1449-1758`
- any subtle drift in `stars()` / `hash43x()` feeding the left-side background cloud around `329-377`
- Acceptance target for the next patch:
- the left bright patch should tuck behind or blend into the cloud/ring more like the original instead of reading as a separate over-bright blob
