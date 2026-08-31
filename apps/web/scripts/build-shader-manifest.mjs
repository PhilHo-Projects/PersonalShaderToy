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

    // Deliberate divergence from the dev API, which lists every provider
    // directory even when empty: shaders/User/ exists for the native app to
    // write into and is always empty here, and the hosted site has no way to
    // create shaders, so an empty section would just be noise.
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
