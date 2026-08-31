# ShaderLab Web Static Deploy — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serve `apps/web/` as a fully static, stateless site at `https://shaderlab.philippeho.dev` from a Coolify Docker application, with the shader corpus baked in at build time and visitor edits kept in their own browser.

**Architecture:** A build script copies the root `shaders/` corpus into `apps/web/public/shaders/` alongside a `manifest.json` matching the shape `ShaderLibraryService.list()` already returns. A second implementation of that existing interface, `StaticShaderLibraryService`, reads those files and stores visitor edits in localStorage. `main.ts` picks the implementation by `import.meta.env.PROD`, leaving the Express dev server and its file-watch loop completely untouched for local work.

**Tech Stack:** Vite 6, TypeScript 5.7, Monaco, Node 22 (build stage), nginx:alpine (serve stage), Coolify on Hetzner.

**Spec:** `docs/superpowers/specs/2026-08-31-shaderlab-web-deploy-design.md`

## Global Constraints

- Serve port is **8000** (matches the house pattern set by `fontmaker-specimen`).
- Docker build context is the **repository root**, never `apps/web/` — the manifest script reads `../../../shaders`, which sits above the web app. Coolify config is therefore `base_directory: /` and `dockerfile_location: /apps/web/Dockerfile`.
- Domain is exactly `https://shaderlab.philippeho.dev`.
- Coolify resource uses **manual releases, no webhook** (standing project rule).
- The Express server (`apps/web/server/`), `HttpShaderLibraryService`, and the `dev` npm scripts are **not modified**. Production must never call `/api` or port 4781.
- localStorage override keys are namespaced `pst:shader:<provider>/<filename>`.
- Shader filenames contain spaces (e.g. `Metatron Cube 2.glsl`). Every URL built from a provider or filename **must** be `encodeURIComponent`-escaped.
- `AGENTS.md` must stay a mirror of `CLAUDE.md` except for line 3, which intentionally differs.

## Testing Note

Neither half of this repository has a test framework, and the approved spec decided against introducing one as part of a deployment task. Tasks therefore substitute **exact runnable verification commands with expected output** for the usual red/green test cycle. Every task still ends in an independently verifiable deliverable — do not mark a step done without running its command and seeing the stated result.

---

### Task 1: Scrub infrastructure notes from tracked instruction files

Removes the server IP, SSH user, nginx paths, unrelated project listing, and org secret names from the two tracked instruction files, so the repository is safe to publish. All of this content is duplicated in the user's global `~/.claude/CLAUDE.md` and is lost from nowhere.

**Files:**
- Modify: `CLAUDE.md` (delete lines 58-103)
- Modify: `AGENTS.md` (delete lines 58-103)

**Interfaces:**
- Consumes: nothing
- Produces: a repository safe to make public — required by Task 6

- [ ] **Step 1: Confirm the section boundary before cutting**

```bash
sed -n '56,60p' CLAUDE.md
```

Expected: line 57 is the "Third host invariant" bullet, line 58 is blank, line 59 is `## SSH & Server Access`. If the line numbers differ, find the `## SSH & Server Access` header and cut from the blank line immediately above it instead of trusting the numbers.

- [ ] **Step 2: Truncate both files**

```bash
sed -i '58,$d' CLAUDE.md && sed -i '58,$d' AGENTS.md
```

- [ ] **Step 3: Verify the sensitive content is gone and the useful content survives**

```bash
grep -nE "<infra-markers>" CLAUDE.md AGENTS.md; echo "exit=$?"
```

Expected: no matches, `exit=1`.

```bash
grep -c "^## " CLAUDE.md AGENTS.md
```

Expected: `5` for both — Product Direction, Implementation Priorities, Repo Conventions, Recent Native Rendering Notes, Render Lab Notes.

- [ ] **Step 4: Verify the two files still mirror each other**

```bash
diff CLAUDE.md AGENTS.md
```

Expected: exactly one difference, at line 3 (the canonical-source vs mirror sentence). Any other difference is a mistake.

- [ ] **Step 5: Commit**

```bash
git add CLAUDE.md AGENTS.md
git commit -m "docs: drop infrastructure notes from tracked instruction files

The repository is going public. Server address, SSH user, nginx paths,
the unrelated project listing, and the org secret names all live in the
global instruction file already."
```

---

### Task 2: Shader manifest build script

Generates the static corpus the production build serves. Deliberately mirrors the extension allowlist, provider discovery, and type mapping in `apps/web/server/routes/shaders.ts` so the static listing and the dev API cannot drift.

**Files:**
- Create: `apps/web/scripts/build-shader-manifest.mjs`
- Modify: `apps/web/package.json` (add `prebuild`, remove `naga-wasm`)
- Modify: `.gitignore` (ignore generated output)

**Interfaces:**
- Consumes: the root `shaders/` corpus
- Produces: `apps/web/public/shaders/manifest.json` shaped `{ [provider]: { name, provider, type, modified }[] }`, plus `apps/web/public/shaders/<provider>/<filename>` for every shader. Task 3 fetches both.

- [ ] **Step 1: Write the script**

Create `apps/web/scripts/build-shader-manifest.mjs`:

```js
// Generates the static shader corpus that the production build serves.
// Mirrors the extension allowlist, provider discovery, and type mapping in
// apps/web/server/routes/shaders.ts so the static listing and the dev API agree.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const SHADERS_DIR = path.resolve(SCRIPT_DIR, '../../../shaders');
const OUT_DIR = path.resolve(SCRIPT_DIR, '../public/shaders');

const EXTENSIONS = ['.glsl', '.frag', '.wgsl', '.hlsl'];

function getShaderType(filename) {
  const ext = path.extname(filename).toLowerCase();
  if (ext === '.wgsl') return 'wgsl';
  if (ext === '.hlsl') return 'hlsl';
  return 'glsl';
}

function getProviders() {
  return fs.readdirSync(SHADERS_DIR, { withFileTypes: true })
    .filter((d) => d.isDirectory() && !d.name.startsWith('.'))
    .map((d) => d.name);
}

function main() {
  if (!fs.existsSync(SHADERS_DIR)) {
    console.error(`Shader corpus not found at ${SHADERS_DIR}`);
    process.exit(1);
  }

  fs.rmSync(OUT_DIR, { recursive: true, force: true });
  fs.mkdirSync(OUT_DIR, { recursive: true });

  const manifest = {};
  let total = 0;

  for (const provider of getProviders()) {
    const srcDir = path.join(SHADERS_DIR, provider);
    const files = fs.readdirSync(srcDir)
      .filter((f) => EXTENSIONS.includes(path.extname(f).toLowerCase()));

    if (files.length === 0) continue;

    fs.mkdirSync(path.join(OUT_DIR, provider), { recursive: true });

    manifest[provider] = files
      .map((name) => {
        fs.copyFileSync(path.join(srcDir, name), path.join(OUT_DIR, provider, name));
        total += 1;
        return {
          name,
          provider,
          type: getShaderType(name),
          modified: fs.statSync(path.join(srcDir, name)).mtimeMs,
        };
      })
      .sort((a, b) => b.modified - a.modified);
  }

  if (total === 0) {
    console.error('Shader corpus is empty — refusing to emit an empty manifest.');
    process.exit(1);
  }

  fs.writeFileSync(path.join(OUT_DIR, 'manifest.json'), JSON.stringify(manifest, null, 2));
  console.log(`Shader manifest: ${total} files across ${Object.keys(manifest).length} providers -> ${OUT_DIR}`);
}

main();
```

- [ ] **Step 2: Run it and verify the output**

```bash
cd apps/web && node scripts/build-shader-manifest.mjs
```

Expected: `Shader manifest: 19 files across 6 providers -> ...`

```bash
node -e "const m=require('./public/shaders/manifest.json');console.log(Object.keys(m).sort().join(','));console.log(Object.values(m).flat().length)"
```

Expected: `Shadertoy,Test,WebGPU,claude,gemini,openai` then `19`.

Confirm a filename containing spaces was copied verbatim:

```bash
ls "public/shaders/Test/"
```

Expected: includes `Metatron Cube 2.glsl`.

- [ ] **Step 3: Wire it into the build and drop the dead dependency**

In `apps/web/package.json`, add to `scripts` (npm runs `prebuild` automatically before `build`):

```json
"prebuild": "node scripts/build-shader-manifest.mjs",
```

Remove `"naga-wasm": "^1.0.0",` from `dependencies` — nothing imports it. Confirm:

```bash
grep -rn "naga-wasm\|naga_wasm" apps/web/src apps/web/server; echo "exit=$?"
```

Expected: no matches, `exit=1`.

- [ ] **Step 4: Ignore the generated output**

Append to the root `.gitignore`:

```
apps/web/public/shaders/
```

Verify git does not see the generated corpus:

```bash
git status --porcelain apps/web/public; echo "exit=$?"
```

Expected: no output.

- [ ] **Step 5: Verify a full build still succeeds and ships the corpus**

```bash
cd apps/web && rm -rf node_modules/.vite && npm install && npm run build
```

Expected: the manifest line prints first, then `✓ built in …`.

```bash
ls apps/web/dist/shaders/manifest.json && ls apps/web/dist/shaders/openai/
```

Expected: manifest present, `openai` shaders present in `dist/`.

- [ ] **Step 6: Commit**

```bash
git add apps/web/scripts/build-shader-manifest.mjs apps/web/package.json apps/web/package-lock.json .gitignore
git commit -m "feat(web): bake the shader corpus into the build as static assets

Adds a prebuild step emitting public/shaders/manifest.json plus a copy of
every shader, so the production bundle needs no API. Drops naga-wasm,
which was declared but never imported."
```

---

### Task 3: StaticShaderLibraryService

The production implementation of the existing `ShaderLibraryService` interface. Reads the generated corpus, keeps visitor edits in localStorage, and does no watching.

**Files:**
- Create: `apps/web/src/services/StaticShaderLibraryService.ts`

**Interfaces:**
- Consumes: `apps/web/public/shaders/manifest.json` and the copied shader files from Task 2. Implements `ShaderLibraryService` from `apps/web/src/services/ShaderLibraryService.ts` (`list`, `load`, `save`, `watch`).
- Produces: class `StaticShaderLibraryService`, constructed with no arguments. Beyond the interface it exposes exactly two extra methods that Task 4 calls:
  - `hasOverride(provider: string, filename: string): boolean`
  - `clearOverride(provider: string, filename: string): void`

- [ ] **Step 1: Write the service**

Create `apps/web/src/services/StaticShaderLibraryService.ts`:

```ts
import type { ShaderContent, ShaderFile, ShaderListing } from '../types/shader.js';
import type { ShaderLibraryChangeEvent, ShaderLibraryService } from './ShaderLibraryService.js';

const OVERRIDE_PREFIX = 'pst:shader:';

function overrideKey(provider: string, filename: string): string {
  return `${OVERRIDE_PREFIX}${provider}/${filename}`;
}

/**
 * Production shader library. The corpus is baked into the build, so there is no
 * server: listing and loading are plain fetches, and a visitor's edits live only
 * in their own browser.
 */
export class StaticShaderLibraryService implements ShaderLibraryService {
  private manifest: ShaderListing | null = null;

  constructor(private base = '/shaders') {}

  async list(): Promise<ShaderListing> {
    const res = await fetch(`${this.base}/manifest.json`);
    if (!res.ok) {
      throw new Error(`Shader manifest unavailable (${res.status})`);
    }
    this.manifest = await res.json() as ShaderListing;
    return this.manifest;
  }

  async load(provider: string, filename: string): Promise<ShaderContent> {
    const meta = await this.findMeta(provider, filename);

    const override = this.readOverride(provider, filename);
    if (override !== null) {
      return { ...meta, content: override };
    }

    // Shader filenames contain spaces; both segments must be escaped.
    const url = `${this.base}/${encodeURIComponent(provider)}/${encodeURIComponent(filename)}`;
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error(`Shader unavailable (${res.status})`);
    }
    return { ...meta, content: await res.text() };
  }

  /** Persists to the visitor's browser only. Throws on quota so the caller can report it. */
  async save(provider: string, filename: string, content: string): Promise<void> {
    localStorage.setItem(overrideKey(provider, filename), content);
  }

  /** No file watching in production; the corpus is immutable for the life of the deploy. */
  watch(_onEvent: (event: ShaderLibraryChangeEvent) => void): () => void {
    return () => {};
  }

  hasOverride(provider: string, filename: string): boolean {
    return this.readOverride(provider, filename) !== null;
  }

  clearOverride(provider: string, filename: string): void {
    try {
      localStorage.removeItem(overrideKey(provider, filename));
    } catch {
      // A browser refusing storage has no override to clear.
    }
  }

  private readOverride(provider: string, filename: string): string | null {
    try {
      return localStorage.getItem(overrideKey(provider, filename));
    } catch {
      // Private browsing can throw on access rather than returning null.
      return null;
    }
  }

  private async findMeta(provider: string, filename: string): Promise<ShaderFile> {
    if (!this.manifest) {
      await this.list();
    }

    const entry = this.manifest?.[provider]?.find((f) => f.name === filename);
    if (entry) {
      return entry;
    }

    const ext = filename.split('.').pop()?.toLowerCase();
    return {
      name: filename,
      provider,
      type: ext === 'wgsl' ? 'wgsl' : ext === 'hlsl' ? 'hlsl' : 'glsl',
      modified: Date.now(),
    };
  }
}
```

- [ ] **Step 2: Verify it typechecks against the existing interface**

```bash
cd apps/web && npx tsc --noEmit
```

Expected: no output, exit 0. A structural mismatch against `ShaderLibraryService` fails here.

- [ ] **Step 3: Commit**

```bash
git add apps/web/src/services/StaticShaderLibraryService.ts
git commit -m "feat(web): add StaticShaderLibraryService for the hosted build

Reads the baked-in corpus over fetch and keeps visitor edits in
localStorage. No server, no write endpoint, no file watching."
```

---

### Task 4: Wire service selection and the revert affordance

Selects the implementation by build mode and gives visitors a way out of a broken edit. Without the revert control a saved-broken shader is permanently stuck, because `load()` prefers the override.

**Files:**
- Modify: `apps/web/src/main.ts` (line 55 wiring; `FileBrowser` callbacks around line 102; `saveCurrentShader` around line 369)
- Modify: `apps/web/src/ui/FileBrowser.ts`
- Modify: `apps/web/src/style.css`

**Interfaces:**
- Consumes: `StaticShaderLibraryService` from Task 3, including `hasOverride` and `clearOverride`.
- Produces: `FileBrowserCallbacks` gains two optional members — `hasOverride?: (provider: string, filename: string) => boolean` and `onRevert?: (provider: string, filename: string) => void`. `FileBrowser` gains two public methods — `markOverride(provider: string, filename: string): void` and `clearOverrideMark(provider: string, filename: string): void`.

- [ ] **Step 1: Extend FileBrowser with override marking**

In `apps/web/src/ui/FileBrowser.ts`, extend the callbacks interface:

```ts
export interface FileBrowserCallbacks {
  onFileSelect: (provider: string, filename: string) => void;
  onRefresh: () => void;
  hasOverride?: (provider: string, filename: string) => boolean;
  onRevert?: (provider: string, filename: string) => void;
}
```

Replace `createFileItem` with a version that tags the element and attaches the control:

```ts
  private createFileItem(file: ShaderFile): HTMLElement {
    const item = document.createElement('div');
    item.className = 'fb-file';
    item.dataset.provider = file.provider;
    item.dataset.filename = file.name;
    const typeColor = file.type === 'glsl' ? '#a6e3a1' : file.type === 'wgsl' ? '#89b4fa' : '#fab387';
    item.innerHTML = `<span class="fb-file-type" style="color:${typeColor}">${file.type}</span>
      <span class="fb-file-name">${file.name}</span>`;
    item.addEventListener('click', () => {
      if (this.activeItem) this.activeItem.classList.remove('active');
      item.classList.add('active');
      this.activeItem = item;
      this.cb.onFileSelect(file.provider, file.name);
    });

    if (this.cb.hasOverride?.(file.provider, file.name)) {
      this.attachRevert(item, file.provider, file.name);
    }

    return item;
  }

  markOverride(provider: string, filename: string) {
    const item = this.findItem(provider, filename);
    if (item) this.attachRevert(item, provider, filename);
  }

  clearOverrideMark(provider: string, filename: string) {
    const item = this.findItem(provider, filename);
    if (!item) return;
    item.classList.remove('edited');
    item.querySelector('.fb-revert')?.remove();
  }

  private attachRevert(item: HTMLElement, provider: string, filename: string) {
    if (item.querySelector('.fb-revert')) return;
    item.classList.add('edited');

    const btn = document.createElement('button');
    btn.className = 'fb-revert';
    btn.title = 'Discard your edits and restore the original shader';
    btn.textContent = '\u21ba';
    btn.addEventListener('click', (e) => {
      // Without this the click also selects the file and reloads the override.
      e.stopPropagation();
      this.cb.onRevert?.(provider, filename);
    });
    item.appendChild(btn);
  }

  private findItem(provider: string, filename: string): HTMLElement | null {
    const list = this.sections.get(provider);
    if (!list) return null;
    for (const child of Array.from(list.children)) {
      if ((child as HTMLElement).dataset.filename === filename) {
        return child as HTMLElement;
      }
    }
    return null;
  }

  /**
   * A failed manifest fetch must not look like an empty library — a broken
   * deploy has to be visibly broken.
   */
  renderError(message: string) {
    const tree = this.el.querySelector('.fb-tree')!;
    tree.innerHTML = '';
    this.sections.clear();
    this.activeItem = null;

    const error = document.createElement('div');
    error.className = 'fb-error';
    error.textContent = message;
    tree.appendChild(error);
  }
```

- [ ] **Step 2: Style the control**

Append to `apps/web/src/style.css`:

```css
.fb-file.edited .fb-file-name::after {
  content: "\2022";
  color: var(--warning);
  margin-left: 6px;
}

.fb-revert {
  margin-left: auto;
  background: none;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 13px;
  line-height: 1;
  padding: 0 2px;
}

.fb-revert:hover { color: var(--warning); }
.fb-file.active .fb-revert { color: var(--bg-primary); }

.fb-error {
  padding: 10px 14px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--error);
}
```

- [ ] **Step 3: Surface a failed library load in the file browser**

`refreshShaderLibrary` currently logs to the output panel and leaves the tree
silently empty, which makes a broken deploy look like an empty corpus. Replace it
in `apps/web/src/main.ts` (around line 259):

```ts
async function refreshShaderLibrary() {
  try {
    const data = await shaderLibrary.list();
    fileBrowser.render(data);
  } catch {
    fileBrowser.renderError('Could not load the shader library.');
    outputPanel.log('Failed to refresh shader library.', 'error');
  }
}
```

- [ ] **Step 4: Wire the service selection in main.ts**

Add the import beside the existing `HttpShaderLibraryService` import:

```ts
import { StaticShaderLibraryService } from './services/StaticShaderLibraryService.js';
```

Replace line 55:

```ts
const shaderLibrary: ShaderLibraryService = import.meta.env.PROD
  ? new StaticShaderLibraryService()
  : new HttpShaderLibraryService();

// Only the static library carries per-visitor overrides; in dev, saves go to disk.
const overrideStore = shaderLibrary instanceof StaticShaderLibraryService ? shaderLibrary : null;
```

- [ ] **Step 5: Wire the FileBrowser callbacks**

Extend the `new FileBrowser({ ... })` call around line 102 with the two new callbacks, leaving `onRefresh` and `onFileSelect` as they are:

```ts
  hasOverride: (provider, filename) => overrideStore?.hasOverride(provider, filename) ?? false,
  onRevert: async (provider, filename) => {
    if (!overrideStore) return;
    overrideStore.clearOverride(provider, filename);
    fileBrowser.clearOverrideMark(provider, filename);
    try {
      const data = await shaderLibrary.load(provider, filename);
      session.setCurrentFile({ provider, filename });
      state.currentFile = { provider, filename };
      toolbar.setActiveType(data.type);
      await applyDocument(data.content, data.type);
      outputPanel.log(`Reverted ${provider}/${filename} to the original`, 'info');
    } catch {
      outputPanel.log(`Failed to reload ${provider}/${filename}`, 'error');
    }
  },
```

- [ ] **Step 6: Mark the file as edited after a successful save**

Replace the body of `saveCurrentShader` (around line 369):

```ts
async function saveCurrentShader() {
  if (!state.currentFile) {
    return;
  }

  const { provider, filename } = state.currentFile;

  try {
    await shaderLibrary.save(provider, filename, session.getSerializedSource());
    if (overrideStore) {
      fileBrowser.markOverride(provider, filename);
      outputPanel.log(`Saved ${provider}/${filename} to this browser`, 'info');
    }
  } catch {
    // Quota exceeded or storage refused; the edit is still live in the editor.
    outputPanel.log(`Failed to save ${provider}/${filename}`, 'error');
  }
}
```

- [ ] **Step 7: Typecheck and build**

```bash
cd apps/web && npx tsc --noEmit && npm run build
```

Expected: no type errors, build succeeds.

- [ ] **Step 8: Verify the production bundle never calls the API**

```bash
grep -rn "4781\|/api/shaders" apps/web/dist/assets/*.js; echo "exit=$?"
```

Expected: no matches, `exit=1`. A match means `HttpShaderLibraryService` was not tree-shaken out and the `import.meta.env.PROD` branch is wrong.

- [ ] **Step 9: Verify behaviour in a real browser**

Serve the production build and drive it with the preview tools:

```bash
cd apps/web && npx vite preview --port 4173 --strictPort
```

Confirm each of these, in order:
1. The file browser lists all 6 providers and 19 shaders.
2. A GLSL shader (e.g. `claude/reaction_diffusion.glsl`) renders on WebGL2.
3. A WGSL shader (e.g. `claude/void_cathedral.wgsl`) renders on WebGPU.
4. Editing a shader and compiling updates the preview.
5. Saving marks the file with a dot and a revert control appears.
6. Reloading the page restores the edited version.
7. Clicking revert restores the original and the dot disappears.
8. The console has no errors and the network panel shows no request to `/api` or port 4781.

- [ ] **Step 10: Commit**

```bash
git add apps/web/src/main.ts apps/web/src/ui/FileBrowser.ts apps/web/src/style.css
git commit -m "feat(web): select the shader library by build mode, add revert

Production uses the static library with localStorage edits; dev keeps the
Express-backed one. Because load() prefers a local override, a saved-broken
shader needed an escape hatch, so edited files now show a revert control."
```

---

### Task 5: Container

Multi-stage image built from the repository root, because the shader corpus lives above `apps/web/`.

**Files:**
- Create: `apps/web/Dockerfile`
- Create: `apps/web/nginx.conf`
- Create: `.dockerignore` (repository root)

**Interfaces:**
- Consumes: the build produced by Tasks 2-4.
- Produces: an image serving `dist/` on port 8000, with `/shaders/manifest.json` reachable. Task 6 deploys it.

- [ ] **Step 1: Write the root .dockerignore**

The context is the whole repository, which holds `target/` and a 207MB `node_modules/`. Without this the context transfer dominates build time. Create `.dockerignore` at the repository root:

```
.git
.gitignore
.claude
.playwright-mcp
**/node_modules
**/dist
apps/web/public/shaders
benchmarks
target
src
examples
docs
Cargo.toml
Cargo.lock
*.png
*.md
```

Note `shaders/` is deliberately **not** excluded — the build needs it.

- [ ] **Step 2: Write the nginx config**

Create `apps/web/nginx.conf`:

```nginx
server {
  listen 8000;
  server_name _;
  root /usr/share/nginx/html;
  index index.html;

  # Vite content-hashes asset filenames, so they can be cached indefinitely.
  location /assets/ {
    expires 1y;
    add_header Cache-Control "public, immutable";
  }

  # The corpus and entry point are not hashed; caching them serves stale
  # shaders after a redeploy.
  location /shaders/ {
    add_header Cache-Control "no-cache";
  }

  location = /index.html {
    add_header Cache-Control "no-cache";
  }

  location / {
    try_files $uri $uri/ /index.html;
  }
}
```

- [ ] **Step 3: Write the Dockerfile**

Create `apps/web/Dockerfile`:

```dockerfile
# Build context is the REPOSITORY ROOT, not apps/web. The shader corpus at
# shaders/ sits above the web app and must be copied into the build.
FROM node:22-alpine AS build

WORKDIR /build

COPY apps/web/package.json apps/web/package-lock.json ./apps/web/
RUN cd apps/web && npm ci

COPY apps/web ./apps/web
COPY shaders ./shaders

# Triggers the prebuild step, which emits public/shaders/ from ../../../shaders.
RUN cd apps/web && npm run build

FROM nginx:alpine

COPY apps/web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /build/apps/web/dist /usr/share/nginx/html

EXPOSE 8000
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s \
  CMD wget -q --spider http://127.0.0.1:8000/shaders/manifest.json || exit 1

CMD ["nginx", "-g", "daemon off;"]
```

- [ ] **Step 4: Build the image locally**

```bash
docker build -f apps/web/Dockerfile -t shaderlab:test .
```

Expected: the manifest line (`Shader manifest: 19 files across 6 providers`) appears in the build output, then the image builds.

If Docker is unavailable on this machine, skip to Task 6 and let Coolify perform the first build — but treat that build's log as this step's verification and do not mark Task 5 complete until it succeeds.

- [ ] **Step 5: Run the container and verify it serves correctly**

```bash
docker run -d --rm --name shaderlab-test -p 8123:8000 shaderlab:test
```

```bash
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8123/
```

Expected: `200`.

```bash
curl -s http://localhost:8123/shaders/manifest.json | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>console.log(Object.values(JSON.parse(s)).flat().length))"
```

Expected: `19`.

Verify a spaced filename is served (this is what the `encodeURIComponent` handling in Task 3 depends on):

```bash
curl -s -o /dev/null -w "%{http_code}\n" "http://localhost:8123/shaders/Test/Metatron%20Cube%202.glsl"
```

Expected: `200`.

Verify SPA fallback:

```bash
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8123/does-not-exist
```

Expected: `200` (serves `index.html`).

```bash
docker stop shaderlab-test
```

- [ ] **Step 6: Commit**

```bash
git add apps/web/Dockerfile apps/web/nginx.conf .dockerignore
git commit -m "feat(web): containerize the static build for Coolify

Multi-stage node -> nginx on 8000. Build context is the repository root
because the shader corpus lives above apps/web, so a root .dockerignore
is required to keep target/ and node_modules out of the context."
```

---

### Task 6: Push, publish the repository, and deploy

The only task with outward-facing effects. Every step here is irreversible or public.

**Files:** none — infrastructure only.

**Interfaces:**
- Consumes: everything from Tasks 1-5, committed.
- Produces: a live site at `https://shaderlab.philippeho.dev`.

- [ ] **Step 1: Push the branch**

```bash
git push origin master
```

- [ ] **STOP — Step 2: Get explicit confirmation before making the repository public**

Making `PhilHo-Projects/PersonalShaderToy` public is outward-facing and effectively irreversible — it can be indexed and cloned within minutes. Confirm with the user before running anything in this step, even though the plan is approved.

First re-verify the scrub actually landed on the pushed commit:

```bash
git show HEAD:CLAUDE.md | grep -cE "<infra-markers>"; echo "exit=$?"
```

Expected: `0` and `exit=1`.

Then, only after the user confirms:

```bash
gh repo edit PhilHo-Projects/PersonalShaderToy --visibility public --accept-visibility-change-consequences
```

Verify:

```bash
gh repo view PhilHo-Projects/PersonalShaderToy --json visibility
```

Expected: `{"visibility":"PUBLIC"}`.

- [ ] **Step 3: Add the DNS record**

Cloudflare A record: name `shaderlab`, content `<hetzner-ip>`, **DNS-only** (grey cloud, matching the other app subdomains).

The Cloudflare MCP servers in this session are unauthenticated, so unless a Cloudflare API token is available locally, ask the user to add this record in the dashboard. Confirm propagation before deploying:

```bash
nslookup shaderlab.philippeho.dev 1.1.1.1
```

Expected: resolves to `<hetzner-ip>`.

- [ ] **Step 4: Discover the Coolify project and server UUIDs**

```powershell
. <coolify-env-file>
$h = @{ Authorization = "Bearer $env:COOLIFY_TOKEN" }
Invoke-RestMethod -Uri "$env:COOLIFY_URL/api/v1/projects" -Headers $h | Select-Object name, uuid
Invoke-RestMethod -Uri "$env:COOLIFY_URL/api/v1/servers" -Headers $h | Select-Object name, uuid
```

Record the target project UUID and the `localhost` server UUID. Never print `COOLIFY_TOKEN`.

- [ ] **Step 5: Create the application**

Create a `dockerfile` build-pack application against the now-public repository. Fill
`$projectUuid` and `$serverUuid` from Step 4.

The three settings that differ from the `fontmaker-specimen` template must be exact —
`base_directory` of `/apps/web` would build without the shader corpus:

```powershell
. <coolify-env-file>
$h = @{ Authorization = "Bearer $env:COOLIFY_TOKEN"; "Content-Type" = "application/json" }

$projectUuid = "<from Step 4>"
$serverUuid  = "<from Step 4>"

$body = @{
  project_uuid        = $projectUuid
  server_uuid         = $serverUuid
  environment_name    = "production"
  name                = "shaderlab"
  git_repository      = "https://github.com/PhilHo-Projects/PersonalShaderToy"
  git_branch          = "master"
  build_pack          = "dockerfile"
  base_directory      = "/"
  dockerfile_location = "/apps/web/Dockerfile"
  ports_exposes       = "8000"
  domains             = "https://shaderlab.philippeho.dev"
  instant_deploy      = $false
} | ConvertTo-Json

$app = Invoke-RestMethod -Method Post -Uri "$env:COOLIFY_URL/api/v1/applications/public" -Headers $h -Body $body
$app.uuid
```

Record the returned UUID. Then confirm the settings landed as sent — Coolify silently
normalises some fields:

```powershell
Invoke-RestMethod -Uri "$env:COOLIFY_URL/api/v1/applications/$($app.uuid)" -Headers $h |
  Select-Object name, base_directory, dockerfile_location, ports_exposes, build_pack, git_branch
```

Expected: `base_directory` is `/`, `dockerfile_location` is `/apps/web/Dockerfile`,
`ports_exposes` is `8000`. If the API rejects a field, correct it with a PATCH to the same
endpoint rather than recreating the application.

- [ ] **Step 6: Deploy and watch the build**

```powershell
Invoke-RestMethod -Method Post -Uri "$env:COOLIFY_URL/api/v1/deploy?uuid=$($app.uuid)" -Headers $h
```

Read the build log. Confirm `Shader manifest: 19 files across 6 providers` appears — its
absence means the build context is wrong and `shaders/` was not copied.

Traefik returns 503 for roughly 30-60s after a redeploy while it picks up the recreated container. Poll rather than concluding the deploy failed.

- [ ] **Step 7: Verify the live site**

```bash
curl -s -o /dev/null -w "%{http_code}\n" https://shaderlab.philippeho.dev/
```

Expected: `200`.

```bash
curl -s https://shaderlab.philippeho.dev/shaders/manifest.json | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>console.log(Object.values(JSON.parse(s)).flat().length))"
```

Expected: `19`.

Then repeat the eight browser checks from Task 4 Step 9 against the live URL. The site is not done until a WGSL shader renders on WebGPU and an edit survives a reload there.

- [ ] **Step 8: Set manual releases**

Confirm the application is set to manual releases with **no** webhook configured, per the standing project rule.

---

### Task 7: Record the deployment in the project docs

Keeps the planning docs accurate, matching the existing convention where `TIMELINE.md` tracks shipped work.

**Files:**
- Modify: `TIMELINE.md`
- Modify: `CLAUDE.md` and `AGENTS.md` (append a short "Web Deployment" section)

**Interfaces:**
- Consumes: the live deployment from Task 6.
- Produces: nothing downstream.

- [ ] **Step 1: Read the existing convention**

```bash
tail -30 TIMELINE.md
```

Match the surrounding entry format rather than inventing one.

- [ ] **Step 2: Add the timeline entry**

Record, dated `2026-08-31`: the browser app is deployed at `https://shaderlab.philippeho.dev` as a static Coolify Docker app; the corpus is baked in at build time; visitor edits are localStorage-only; the Express server remains dev-only.

- [ ] **Step 3: Add a Web Deployment section to both instruction files**

Append the same section to `CLAUDE.md` and `AGENTS.md`, capturing the facts a future session cannot derive from the code:

- `apps/web/` deploys to `https://shaderlab.philippeho.dev` as a Coolify `dockerfile` app, manual releases, no webhook.
- Build context is the repository root; `base_directory: /`, `dockerfile_location: /apps/web/Dockerfile`. It cannot be `/apps/web` because the manifest script reads `../../../shaders`.
- Production uses `StaticShaderLibraryService`; `HttpShaderLibraryService` and `apps/web/server/` are dev-only and must never be exposed, as the POST route is an unauthenticated unsanitised file write.
- The hosted app intentionally diverges from the native lab: WebGL2/WebGPU only, no adapter picker, present-mode control, DX12 compiler choice, GPU timing, or benchmark sweep.

- [ ] **Step 4: Verify the mirror still holds**

```bash
diff CLAUDE.md AGENTS.md
```

Expected: exactly one difference, at line 3.

- [ ] **Step 5: Commit and push**

```bash
git add TIMELINE.md CLAUDE.md AGENTS.md
git commit -m "docs: record the shaderlab.philippeho.dev web deployment"
git push origin master
```

---

## Deferred

Tracked here so they are not silently lost:

- **Monaco bundle trim.** Monaco is 3.4MB of the 4.5MB bundle because the barrel import pulls every language mode. Importing editor core plus the two existing Monarch grammars would approach 1MB.
- **Unit tests.** `build-shader-manifest.mjs` and `StaticShaderLibraryService` are both pure and worth covering if vitest is ever added.
- **The mock HLSL compiler worker.** `apps/web/src/worker/compiler.worker.ts` is a stub with a fabricated error message. It is unreachable today because the corpus has no `.hlsl` files, but it must be removed or made honest before any HLSL path becomes reachable from the UI on a public site.
