# ShaderLab Web: Public Static Deploy — Design

Date: 2026-08-31
Status: Approved by user

## Goal

Host the browser app (`apps/web/`) as its own Coolify Docker application at
`https://shaderlab.philippeho.dev`, served as a fully static bundle with no server-side
state. The portfolio links to it as an external showcase alongside a public GitHub link
that covers both halves of the project.

The native Rust shader lab is **not** deployed and is not part of this work. It remains a
desktop application, showcased via screenshots and the repository link.

## Background

- `apps/web/` is a Vite + TypeScript app. It renders entirely on the visitor's GPU through
  `WebGL2Renderer` / `WebGPURenderer`; the server never touches shader compilation.
- The split between the native and browser apps is clean. Tauri was removed in commit
  `fece7bd`; `PreviewRuntime` is a one-member union (`'browser'`), and neither app
  references the other. The only shared asset is the root `shaders/` directory, reached by
  the relative path in `apps/web/server/shaderRoot.ts`.
- `apps/web/server/` is an Express + chokidar + ws server whose only production-relevant
  job is listing and reading shader files. Its file-watch broadcast exists for local
  iteration.
- `POST /api/shaders/:provider/:filename` writes to disk with no authentication, no
  provider validation, and no path sanitisation on `filename`. It must not be exposed
  publicly.
- `npm run build` already succeeds: ~8.7s, `dist/` is 4.5MB (1.2MB gzipped).
- The shader corpus is 20 files totalling 350KB.

## Approach (chosen: static bundle, dev server retained)

Ship the shader corpus as build-time static assets and keep all editing client-side. The
Express server is retained untouched for local development, where watching the shared
`shaders/` directory alongside the Rust app is genuinely useful.

`ShaderLibraryService` is already an interface with a single implementation. Adding a
second implementation and selecting it by build mode is the whole architectural change.

Rejected alternatives:

- **Deploy Express with the write route hardened.** Requires path sanitisation, an auth
  story, a persistent volume, and ~100MB idle RAM, to deliver cross-browser shader saves
  that a showcase does not need.
- **Read-only static gallery.** Simplest to host, but removes the editing loop, which is
  the interesting half of a shader site.

## Components

### 1. Repository visibility (`CLAUDE.md`, `AGENTS.md`)

`PhilHo-Projects/PersonalShaderToy` becomes public so the portfolio's GitHub link resolves
for visitors.

Before flipping visibility, remove the infrastructure sections from the two tracked
instruction files. Both currently carry an SSH & Server Access block (server IP, default
user), nginx config paths, a listing of unrelated projects on that server, and the names of
the GitHub org secrets. This content is duplicated in the user's global
`~/.claude/CLAUDE.md`, so deleting it from the repository copies loses nothing.

Sections to delete from both files: `SSH & Server Access`, `Nginx`, `Project Structure`,
`n8n`, `Github`. Sections to keep: `Product Direction`, `Implementation Priorities`,
`Repo Conventions`, `Recent Native Rendering Notes`, `Render Lab Notes`.

`AGENTS.md` must continue to mirror `CLAUDE.md` per the repo contract.

### 2. Shader manifest build step (`apps/web/scripts/build-shader-manifest.mjs`)

A Node script that walks `../../shaders` and emits into `apps/web/public/shaders/`:

- `manifest.json` — the same shape `ShaderLibraryService.list()` already returns:
  `{ [provider]: ShaderFile[] }`, where `ShaderFile` is `{ name, provider, type, modified }`.
- A copy of each shader file at `<provider>/<filename>`.

It reuses the extension allowlist and `getShaderType` mapping that
`apps/web/server/routes/shaders.ts` uses, so the static listing and the dev API agree.
Provider directories are discovered from disk, matching `getProviders()`.

Wired into `package.json` as a `prebuild` script so `npm run build` always regenerates it.
`apps/web/public/shaders/` is gitignored — it is generated output.

### 3. `StaticShaderLibraryService` (`apps/web/src/services/`)

Implements the existing `ShaderLibraryService` interface:

- `list()` — fetches `/shaders/manifest.json`.
- `load(provider, filename)` — returns the localStorage override if one exists, otherwise
  fetches `/shaders/<provider>/<filename>` and returns it as `ShaderContent`.
- `save(provider, filename, content)` — writes to localStorage under
  `pst:shader:<provider>/<filename>`.
- `watch(onEvent)` — registers nothing and returns a no-op unsubscribe. There is no file
  watching in production.

Selection happens at the single existing wiring point in `apps/web/src/main.ts`:

```ts
const shaderLibrary: ShaderLibraryService = import.meta.env.PROD
  ? new StaticShaderLibraryService()
  : new HttpShaderLibraryService();
```

`HttpShaderLibraryService`, `apps/web/server/`, and the `dev` npm scripts are unchanged.

### 4. Local edit model and revert affordance

A visitor's edits persist in their own browser and never reach the server. Because `load()`
prefers the override, a visitor who saves a broken shader would otherwise be permanently
stuck with it.

The file browser therefore needs a way to discard a local override and return to the
shipped original. Scope: a per-file control visible only when an override exists for that
file, plus the underlying `clearOverride(provider, filename)` on the static service.
`LocalStorageSettingsStore` establishes the storage-access pattern to follow.

Quota handling: a `save()` that throws (private browsing, quota exceeded) must surface a
diagnostic through the existing `DiagnosticsSink` rather than failing silently or throwing
into the UI.

### 5. Container (`apps/web/Dockerfile`, `apps/web/nginx.conf`, root `.dockerignore`)

**Build context is the repository root, not `apps/web/`.** The manifest script reads
`../../shaders`, which sits above `apps/web/`, so a context rooted at `apps/web/` cannot see
the shader corpus. Coolify's `base_directory` sets the build context, therefore:

- `base_directory: /`
- `dockerfile_location: /apps/web/Dockerfile`

This differs from `fontmaker-specimen`, whose web directory is self-contained. The Dockerfile
copies `apps/web/` and `shaders/` explicitly from the root context.

Multi-stage build:

1. `node:22-alpine` — copy `apps/web/package*.json`, install, copy `apps/web/` and
   `shaders/`, run `npm run build` (which triggers the manifest prebuild).
2. `nginx:alpine` — serve `dist/` on port 8000.

The nginx config provides SPA fallback to `index.html`, long-lived immutable caching for
Vite's content-hashed assets, and no caching for `index.html` or `manifest.json`.

`EXPOSE 8000`, a `HEALTHCHECK`, and an explicit `CMD` match the house pattern established
by `fontmaker-specimen`.

Because the context is the repository root, a root `.dockerignore` is mandatory — it must
exclude `target/`, every `node_modules/`, `dist/`, `benchmarks/`, and the `.git` directory.
`target/` alone is large enough to dominate build time otherwise.

### 6. Coolify resource and DNS

- Application following `fontmaker-specimen`: `dockerfile` build pack, Public GitHub
  source, `git_branch: master`, exposes 8000.
- `base_directory: /` and `dockerfile_location: /apps/web/Dockerfile`, per the build-context
  constraint in section 5.
- Manual releases, no webhook, per the standing deployment rule.
- Domain `https://shaderlab.philippeho.dev`.
- Cloudflare A record `shaderlab` → `<hetzner-ip>`, DNS-only.

### 7. Dependency cleanup

Remove `naga-wasm` from `apps/web/package.json`. It is declared but imported nowhere.

`apps/web/src/worker/compiler.worker.ts` is a mock — an artificial 800ms delay and a
hardcoded rejection message — but it is reachable only through `compileHlsl()`, and the
corpus contains no `.hlsl` files. It is left in place. If HLSL ever becomes reachable from
the UI, this must be revisited before it can surface on a public site.

## Error handling

- Manifest fetch failure renders an error state in the file browser rather than an empty
  library, so a broken deploy is visibly broken.
- Shader fetch failure surfaces through the existing `ErrorOverlay` / `DiagnosticsSink`
  path, identical to how a compile failure is reported today.
- localStorage reads and writes are wrapped; a failure degrades to "edits will not persist"
  rather than breaking the editor.
- WebGPU absence is already handled — `BrowserPreviewHost.initialize()` falls back to
  WebGL2 and marks the backend unavailable with a reason.

## Testing

Neither half of the repository has a test framework. Rather than introduce one as part of a
deployment task, verification is done against the real production build through the browser
preview tools:

1. `npm run build`, serve `dist/`, confirm the manifest lists all 20 shaders across all
   providers.
2. A GLSL shader renders on WebGL2.
3. A WGSL shader renders on WebGPU.
4. Edit → compile → run succeeds in the browser.
5. An edit survives a page reload; revert restores the shipped original.
6. No console errors, no network requests to `/api` or port 4781 in the production build.
7. After deploy, the same checks against `https://shaderlab.philippeho.dev`.

The manifest script and `StaticShaderLibraryService` are both pure enough to unit test, and
adding vitest for them is a reasonable follow-up if the app grows.

## Out of scope

- **Monaco bundle size.** It accounts for 3.4MB of the 4.5MB bundle because the barrel
  import pulls every language mode. Trimming to editor core plus the two existing Monarch
  grammars would approach 1MB. Deferred to its own pass.
- **Portfolio integration.** The showcase links out; no shared code or embedding.
- **Rust WASM port.** The native lab's value is adapter enumeration, DX12 compiler
  selection, GPU timestamp queries, and the benchmark sweep — none of which survive a
  browser port.

## Known divergence from the native app

The hosted web app cannot reproduce the render lab. It exposes WebGL2 and WebGPU only: no
adapter picker, no present-mode or frame-latency control, no DX12 compiler choice, no
per-pass GPU timing, no benchmark sweep. The site's description should state this directly
so the web app reads as a different surface rather than a degraded copy.
