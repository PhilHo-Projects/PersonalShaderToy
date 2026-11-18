import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";

const web = fileURLToPath(new URL("../", import.meta.url));
const root = path.resolve(web, "../..");
const compiled = path.join(web, "compiler");
const out = path.join(web, "public/shaders");
const compilerAssets = path.join(web, "public/compilers");
const hash = createHash("sha256");
const coreFiles = (await fs.readdir(path.join(root, "shader-core/src")))
  .filter((f) => f.endsWith(".rs"))
  .sort();
for (const file of [
  "Cargo.lock",
  "shader-core/Cargo.toml",
  ...coreFiles.map((f) => "shader-core/src/" + f),
]) {
  hash.update(
    (await fs.readFile(path.join(root, file), "utf8")).replaceAll("\r\n", "\n"),
  );
}
if (
  (await fs.readFile(path.join(compiled, "source.sha256"), "utf8")).trim() !==
  hash.digest("hex")
) {
  throw Error(
    "The browser shader compiler is stale. Run python tools/build-core.py.",
  );
}
const install = spawnSync(
  process.platform === "win32" ? "python" : "python3",
  [
    path.join(root, "tools/compilers.py"),
    "web",
    path.join(compilerAssets, "slang"),
  ],
  { stdio: "inherit" },
);
if (install.status !== 0)
  throw Error("Installing the pinned Slang compiler failed.");
await fs.mkdir(path.join(compilerAssets, "core"), { recursive: true });
for (const name of ["shader_core.js", "shader_core_bg.wasm"])
  await fs.copyFile(
    path.join(compiled, name),
    path.join(compilerAssets, "core", name),
  );
const core = await import(
  pathToFileURL(path.join(compiled, "shader_core.js")).href
);
await core.default({
  module_or_path: await fs.readFile(path.join(compiled, "shader_core_bg.wasm")),
});
const catalog = JSON.parse(
  await fs.readFile(path.join(root, "shaders/catalog.json"), "utf8"),
);
const fresh = await fs.mkdtemp(path.join(web, "public/.catalog-"));
try {
  for (const entry of catalog) {
    const source = await fs.readFile(
      path.join(root, "shaders", entry.path),
      "utf8",
    );
    const passes = JSON.parse(core.graph_json(source));
    const prepared = passes.map((p) =>
      JSON.parse(core.prepare_json(JSON.stringify(p), entry.language)),
    );
    const hlsl = prepared.map((p) => core.export_hlsl(JSON.stringify(p)));
    await fs.writeFile(
      path.join(fresh, entry.id + ".json"),
      JSON.stringify({ ...entry, source, passes, prepared, hlsl }),
    );
  }
  await fs.writeFile(
    path.join(fresh, "manifest.json"),
    JSON.stringify(catalog),
  );
  await fs.rm(out, { recursive: true, force: true });
  await fs.rename(fresh, out);
  console.log("Prepared " + catalog.length + " shader examples.");
} finally {
  await fs.rm(fresh, { recursive: true, force: true });
}
