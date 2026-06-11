# Kerr-Newman Visual Baseline

## Capture Conditions

- Workspace: `D:\PersonalShaderToy`
- Resolution: `1920x1080`
- Original renderer: `WebGL2 Multi-pass`
- Port renderer: `WebGPU Multi-pass`
- Browser automation: Playwright CLI
- Comparison date: `2026-04-09`

## Baseline Images

- Original GLSL target: `D:\PersonalShaderToy\docs\kerr-newman-port\baselines\original-kerr-baseline.png`
- WGSL after camera-state fix: `D:\PersonalShaderToy\docs\kerr-newman-port\baselines\openai-kerr-camera-state.png`
- WGSL after background + Buffer A tone-map chunk: `D:\PersonalShaderToy\docs\kerr-newman-port\baselines\openai-kerr-background-tonemap.png`
- WGSL after full disk-noise octaves: `D:\PersonalShaderToy\docs\kerr-newman-port\baselines\openai-kerr-disk-noise-fulloctaves.png`
- WGSL after full disk sub-march: `D:\PersonalShaderToy\docs\kerr-newman-port\baselines\openai-kerr-disk-fullmarch.png`
- WGSL after shell asymmetry + disk spin clamp: `D:\PersonalShaderToy\docs\kerr-newman-port\baselines\openai-kerr-shell-rotfact.png`
- WGSL after original jet-visibility gate: `D:\PersonalShaderToy\docs\kerr-newman-port\baselines\openai-kerr-no-jets.png`
- WGSL after background status/shift parity: `D:\PersonalShaderToy\docs\kerr-newman-port\baselines\openai-kerr-bg-status-shift.png`
- WGSL after background escape-direction parity: `D:\PersonalShaderToy\docs\kerr-newman-port\baselines\openai-kerr-bg-raydir-reset.png`
- WGSL after inner-cloud `SamplePos.zx` parity: `D:\PersonalShaderToy\docs\kerr-newman-port\baselines\openai-kerr-innercloud-zx.png`
- Native host centering diagnostic: `shaders/Test/test5.wgsl`

## Milestone Notes

### Original GLSL target

- Dense star field across most of the frame.
- Very dark space background.
- Hot bright accretion band with a strong warm upper arc.
- Clean black-hole silhouette without the extra inner dark-lobe artifact.
- Strong contrast between the bright disk and the dark lensing shell.

### WGSL after camera-state fix

- Major structural improvement over the older auto-orbit camera path.
- Still badly washed out.
- Star field almost absent.
- Blue fog dominated the image.
- Inner silhouette artifact remained obvious.

### WGSL after background + Buffer A tone-map chunk

- Dense star field restored.
- Background is darker and closer to the target.
- Lensing shell and silhouette read much more cleanly.
- Contrast improved substantially.
- Remaining visible gaps:
- The upper accretion band is still too cool/blue and not hot enough.
- The warm upper arc is still weaker than the original.
- The disk/body proportions are closer, but still not exact.
- Buffer A still uses `current_shift = 1.0`, so background spectral shift and pre-bloom mapping are not driven by real escape-frequency data yet.

### WGSL after full disk-noise octaves

- Restoring the full `GenerateAccretionDiskNoise` octave span immediately made the ring cloud less flat.
- The disk band gained better breakup and structure instead of reading like a single smeared plume.
- The image was still missing the original shell asymmetry and still under-sampling along the disk march.

### WGSL after full disk sub-march

- Removing the artificial disk sub-step cap produced a major parity jump.
- The ring cloud became denser and more continuous around the hole instead of dissolving into a short local streak.
- The upper band was still too symmetric and too cool compared with the original.

### WGSL after shell asymmetry + disk spin clamp

- The warm upper shell arc is visibly back and the ring cloud now bends with the same directional bias as the original.
- The accretion structure is no longer uniformly blue; orange/brown shell detail is showing up around the upper-right lensing band.
- Remaining gap: the disk plume is still somewhat brighter and broader than the original, and `FreqShift`-style escape data is still placeholder-driven in the final background path.

### WGSL after original jet-visibility gate

- The bright axial beam / helix through the hole is gone.
- This matched the original preset more closely because the GLSL configuration uses `iAccretionRate = 5e-4`, which keeps `IsJetVisible(...)` false.
- The remaining visible work is now back in the disk plume width/brightness and final shift plumbing, not in the jet path.

### WGSL after background status/shift parity

- Background is now only blended for escaped rays, matching the original `Status` logic instead of leaking sky through every non-horizon ray.
- The placeholder `current_shift = 1.0` path is gone; escaped rays now use the original-style `FreqShift` clamp.
- This reduced a real source of false background brightness around the disk/shell boundary.

### WGSL after background escape-direction parity

- The background handoff now uses the marched ray direction, matching the GLSL `normalize(RayDir)` path instead of a metric-raised momentum vector.
- This is the correct parity fix for background patch placement around the lensing shell.

### WGSL after inner-cloud `SamplePos.zx` parity

- The inner-cloud angular frame now matches the original (`SamplePos.zx` rather than `SamplePos.xz`), and the dust-thickness denominator now follows the GLSL more literally.
- After these fixes, the remaining bright left patch appears to be the last meaningful visual mismatch and is no longer explained by an obvious port typo in the final background/dust handoff.

### Native host centering fix

- Date: `2026-04-24`
- Diagnostic shader: `shaders/Test/test5.wgsl`
- Symptom: the final Image-pass center marker was correct, but sampled Buffer A/B/C content drifted right/down.
- Cause: the Rust native multipass renderer reused one shared uniform buffer for all passes; Image-pass viewport uniforms could overwrite earlier offscreen pass uniforms before submitted GPU work executed.
- Fix: each compiled native multipass pass now owns and binds its own uniform buffer.
- Effect on Kerr-Newman work: remaining differences should be investigated as shader parity or visual-baseline mismatches unless `test5.wgsl` shows host drift again.

## Chunk Verdicts

| Chunk | Result | Notes |
| --- | --- | --- |
| Post stack parity (`Buffer B/C/D/Image`) | Neutral-to-positive | Needed to stabilize the pipeline, but did not solve the main visual mismatch on its own. |
| Buffer A camera/state from `Buffer B` | Positive | Fixed a structural mismatch in scene framing and movement behavior. |
| Background/stars + Buffer A tone-map | Positive | First major visual parity jump: stars, darkness, and contrast are now in the right direction. |
| Disk-noise octave restoration | Positive | Recovered missing cloud structure that had been flattened by an artificial 4-octave cap. |
| Full disk sub-march | Positive | Restored much more of the original volumetric ring-cloud continuity. |
| Shell asymmetry + disk spin clamp | Positive | Brought back the warm upper arc and improved the directional bias of the ring cloud. |
| Original jet-visibility gate | Positive | Removed the spurious bright axial helix by matching the original preset's `IsJetVisible` behavior. |
| Background status + shift parity | Positive | Removed false sky contribution from non-escaped rays and restored original-style `FreqShift` use. |
| Background escape-direction parity | Neutral-to-positive | Corrected the code path to match the GLSL's `RayDir` handoff for background lookup. |
| Inner-cloud `SamplePos.zx` parity | Neutral-to-positive | Fixed an exact angular-frame mismatch in the inner-cloud dust path. |
| Native multipass host centering | Positive | Fixed sampled-buffer drift by replacing shared multipass uniforms with per-pass uniform buffers in the Rust host. |

## Next Visual Sign-Off Targets

1. Investigate the last bright left patch as a likely missing heat-haze/refraction effect rather than a remaining obvious `DiskColor` typo.
2. Narrow the remaining disk plume width and brightness so it stops reading broader than the original.
3. Audit any remaining `DiskColor` color-temperature differences around the brightest white band.
4. Re-check the silhouette once the left-patch mismatch is resolved, because the inner shell highlight is still a little hotter than the original.
