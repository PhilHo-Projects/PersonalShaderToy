# Kerr-Newman Port Checklist

Status vocabulary:

- `exact`
- `renamed-equivalent`
- `simplified`
- `missing`
- `wrong-behavior`
- `partial`

## Buffer A Function Audit

| Original GLSL | Current WGSL | Status | Notes |
| --- | --- | --- | --- |
| `FragUvToDir` | none | `missing` | Current camera/ray init does not use the original helper stack yet. |
| `PosToNdc` | none | `missing` | Not ported. |
| `DirToNdc` | none | `missing` | Not ported. |
| `DirToFragUv` | none | `missing` | Not ported. |
| `PosToFragUv` | none | `missing` | Not ported. |
| `RandomStep` | `random_step` | `renamed-equivalent` | Used for pixel jitter and disk phase jitter. |
| `CubicInterpolate` | `cubic_interp` | `renamed-equivalent` | Present. |
| `PerlinNoise` | `noise3` / `perlin3` | `simplified` | Similar noise basis, not yet proven exact. |
| `SoftSaturate` | `soft_saturate` | `renamed-equivalent` | Present. |
| `PerlinNoise1D` | none | `missing` | Not ported. |
| `GenerateAccretionDiskNoise` | `gen_disk_noise` | `partial` | Full octave span is now restored; still not proven line-by-line exact. |
| `Vec2ToTheta` | `vec2_to_theta` | `renamed-equivalent` | Present. |
| `Shape` | `shape_fn` | `renamed-equivalent` | Present. |
| `KelvinToRgb` | `kelvin_to_rgb` | `renamed-equivalent` | Present. |
| `WavelengthToRgb` | `wavelength_to_rgb` | `partial` | Restored this round; needs direct visual validation with real shift values. |
| `hash43x` | `hash43x_bg` | `partial` | Ported for background stars. |
| `stars` | `stars_bg` | `partial` | Ported this round and visibly improved parity. |
| `hash(float)` | none | `missing` | Needed for original rain path. |
| `hash(vec2)` | `hash12` | `simplified` | Different implementation. |
| `hash2` | none | `missing` | Needed for original rain path. |
| `hash4(vec2)` | none | `missing` | Needed for original rain path. |
| `hash4(vec3)` | none | `missing` | Needed for original rain path. |
| `rune_line` | none | `missing` | Needed for original rain path. |
| `rune` | none | `missing` | Needed for original rain path. |
| `random_char` | none | `missing` | Needed for original rain path. |
| `rain` | none | `missing` | Antiverse background still unported. |
| `SampleBackground` | `sample_bg` | `partial` | Escaped-ray status logic, real shift input, and marched-ray direction handoff are restored; antiverse branch remains unported. |
| `ApplyToneMapping` | `apply_tone_mapping` | `partial` | Restored original-style pre-bloom mapping and real escaped-ray shift input. |
| `GetKeplerianAngularVelocity` | `kepler_omega` | `renamed-equivalent` | Present. |
| `KerrSchildRadius` | `kerr_schild_r` | `renamed-equivalent` | Present. |
| `GetZamoOmega` | none | `missing` | Not ported. |
| `IntersectKerrEllipsoid` | none | `missing` | Not ported. |
| `ComputeGeometryScalars` | `geo_scalars` | `partial` | Core path exists; tensor parity still unverified. |
| `ComputeGeometryGradients` | `geo_gradients` | `partial` | Present but not audited line-by-line. |
| `RaiseIndex` | `raise_idx` | `renamed-equivalent` | Present. |
| `LowerIndex` | `lower_idx` | `renamed-equivalent` | Present. |
| `GetInitialMomentum` | `init_momentum` | `partial` | Works, but camera/original helper parity still needs audit. |
| `ApplyHamiltonianCorrection` | `ham_correct` | `partial` | Present but not verified against full original algebra. |
| `GetDerivativesAnalytic` | `get_derivs` | `partial` | Present. |
| `GetIntermediateSign` | `intermediate_sign` | `renamed-equivalent` | Present. |
| `StepGeodesicRK4_Optimized` | `rk4_pre` | `partial` | Present but not yet exact-audited. |
| `HazeNoise01` | none | `missing` | Not ported. |
| `GetBaseNoise` | none | `missing` | Not ported. |
| `GetDiskHazeMask` | none | `missing` | Not ported. |
| `GetJetHazeMask` | none | `missing` | Not ported. |
| `IsInHazeBoundingVolume` | none | `missing` | Not ported. |
| `GetHazeForce` | none | `missing` | Not ported. |
| `DiskColor` | `disk_sample_at` | `partial` | Full sub-march, shell asymmetry, clamped disk-spin path, and inner-cloud `SamplePos.zx` frame are restored; remaining gap is mostly the last bright left patch and overall plume balance. |
| `JetColor` | `sample_jet` | `partial` | Original preset visibility gate is now restored, and the WGSL jet call now matches the GLSL's zero-spin input; the full sampled jet shape/compositing is still simplified. |
| `GridColorSimple` | none | `missing` | Not ported. |
| `GridColor` | none | `missing` | Not ported. |
| `IsAccretionDiskVisible` | none | `missing` | Not ported. |
| `IsJetVisible` | none | `missing` | Not ported. |
| `SolveCubicMaxReal` | none | `missing` | Not ported. |
| `SolveQuarticU` | none | `missing` | Not ported. |
| `GetDropFrameAngle` | none | `missing` | Not ported. |
| `GetShadowHalfAngleRN` | none | `missing` | Not ported. |
| `TraceRay` | inline `mainImage` path | `wrong-behavior` | Current WGSL does not yet expose the same `TraceResult` outputs or exact control flow. |
| `mainImage` | `mainImage` | `partial` | Camera-state wiring restored; still missing original escape/frequency/status plumbing. |

## Post Stack Audit

| Original GLSL | Current WGSL | Status | Notes |
| --- | --- | --- | --- |
| Buffer B camera/self-feedback | `ColorFetch`, `Grab1/4/8/16`, `CalcOffset`, `UpdateCameraState` | `partial` | Much closer to original than before; still needs behavioral check around keyboard-driven states. |
| Buffer C blur | `ColorFetchC`, `mainImage` | `renamed-equivalent` | Literal-style blur pass restored. |
| Buffer D blur | `ColorFetchD`, `mainImage` | `renamed-equivalent` | Literal-style blur pass restored. |
| Image composite | `ColorFetchImage`, `BloomFetch`, `Grab`, `CalcOffsetImage`, `GetBloom`, `mainImage` | `partial` | Close to original bloom/composite; depends on Buffer A parity for final look. |
