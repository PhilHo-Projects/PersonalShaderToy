import "./style.css";
import { MultiPassRenderer } from "./renderer/MultiPassRenderer";
import { WebGPURenderer } from "./renderer/WebGPURenderer";
import type {
  CatalogEntry,
  Language,
  Player,
  ShaderFile,
  Uniforms,
} from "./renderer/uniforms";

const el = <T extends HTMLElement = HTMLElement>(id: string) =>
  document.getElementById(id) as T;
const select = (id: string) => el<HTMLSelectElement>(id);
const stage = el("stage");
const library = select("library");
const status = (message: string, error = false) => {
  el("status").textContent = message;
  el("status").classList.toggle("error", error);
};
function option(value: string, text: string) {
  return new Option(text, value);
}
async function json<T>(url: string): Promise<T> {
  const response = await fetch(url);
  if (!response.ok) throw Error(response.status + " " + url);
  return response.json();
}

let compiler: Worker | undefined;
let job = 0;
const jobs = new Map<
  number,
  { resolve: (s: Partial<ShaderFile>) => void; reject: (e: Error) => void }
>();
function compile(
  doc: ShaderFile,
  operation: "parse" | "prepare" | "export",
): Promise<Partial<ShaderFile>> {
  if (!compiler) {
    compiler = new Worker(
      new URL("./worker/compiler.worker.ts", import.meta.url),
      { type: "module" },
    );
    compiler.onmessage = ({ data }) => {
      const cb = jobs.get(data.id);
      if (!cb) return;
      jobs.delete(data.id);
      if (data.error) cb.reject(Error(data.error));
      else cb.resolve(data);
    };
    compiler.onerror = (event) => {
      for (const cb of jobs.values()) cb.reject(Error(event.message));
      jobs.clear();
      compiler?.terminate();
      compiler = undefined;
    };
  }
  const id = ++job;
  return new Promise((resolve, reject) => {
    jobs.set(id, { resolve, reject });
    compiler!.postMessage({
      id,
      source: doc.source,
      language: doc.language,
      prepared: doc.prepared,
      operation,
    });
  });
}
function release(player: Player | undefined) {
  if (!player) return;
  player.dispose();
  if (player instanceof MultiPassRenderer)
    player.canvas
      .getContext("webgl2")
      ?.getExtension("WEBGL_lose_context")
      ?.loseContext();
}

let active: ShaderFile | undefined,
  player: Player | undefined,
  chosen: ShaderFile | undefined;
let ticket = 0,
  activeBackend = "",
  playing = true,
  frame = 0,
  time = 0,
  needsFrame = false;
let compiledMs = 0,
  firstFrameMs = 0,
  cpuMs = 0,
  opened = 0,
  lastDraw: number | undefined,
  lastReport = 0;
const samples: number[] = [];
const keys = new Set<number>();
let mouse: Uniforms["iMouse"] = [0, 0, 0, 0];
let failure = false;
function clearSamples() {
  samples.length = 0;
  lastDraw = undefined;
  cpuMs = 0;
}
function rewind() {
  frame = 0;
  time = 0;
  needsFrame = true;
  clearSamples();
  player?.reset();
}
function configureCanvas() {
  if (!player) return;
  const box = stage.getBoundingClientRect(),
    scale = Number(select("scale").value);
  const fixed = Number(select("size").value);
  const height = Number.isFinite(fixed)
    ? fixed
    : Math.min(box.height, (box.width * 9) / 16) * devicePixelRatio;
  const h = Math.max(1, Math.round(height * scale)),
    w = Math.max(1, Math.round(((height * 16) / 9) * scale));
  if (player.canvas.width !== w || player.canvas.height !== h) {
    player.canvas.width = w;
    player.canvas.height = h;
    rewind();
  }
}
function showMetrics() {
  const sorted = [...samples].sort((a, b) => a - b);
  const avg = sorted.length
    ? sorted.reduce((a, b) => a + b) / sorted.length
    : 0;
  const ms = (v: number) => v.toFixed(2) + " ms";
  el("fps").textContent = playing && avg ? (1000 / avg).toFixed(0) : "—";
  el("avg").textContent = playing && avg ? ms(avg) : "—";
  el("p95").textContent =
    playing && avg ? ms(sorted[Math.ceil(sorted.length * 0.95) - 1]) : "—";
  el("pixels").textContent = player
    ? player.canvas.width + " × " + player.canvas.height
    : "—";
  el("backend-name").textContent = activeBackend || "—";
  const gpu = playing ? player?.timings : null;
  el("gpu").textContent = gpu ? ms(gpu.reduce((a, b) => a + b, 0)) : "—";
  const passTimes = gpu
    ?.map((t, i) => (active?.passes?.[i].name ?? "Image") + " " + ms(t))
    .join("   ");
  el("details").textContent = player
    ? [
        "Frame " + frame,
        "CPU submission " + ms(cpuMs),
        "Compile " + ms(compiledMs),
        "First frame " + (firstFrameMs ? ms(firstFrameMs) : "—"),
        passTimes || "GPU timestamps unavailable or pending",
      ].join("   ·   ")
    : "";
}
function tick(now: number) {
  requestAnimationFrame(tick);
  if (!player || document.hidden || (!playing && !needsFrame)) return;
  const delta = lastDraw === undefined ? 0 : Math.max(0, now - lastDraw);
  const nextTime = time + (playing ? delta / 1000 : 0);
  const d = new Date(),
    canvas = player.canvas;
  const u: Uniforms = {
    iTime: nextTime,
    iTimeDelta: delta / 1000,
    iFrame: frame,
    iResolution: [canvas.width, canvas.height, 1],
    iMouse: mouse,
    iDate: [
      d.getFullYear(),
      d.getMonth(),
      d.getDate(),
      d.getHours() * 3600 +
        d.getMinutes() * 60 +
        d.getSeconds() +
        d.getMilliseconds() / 1000,
    ],
  };
  const start = performance.now();
  try {
    player.updateKeyboard(keys);
    player.render(u);
  } catch (e) {
    status(String(e), true);
    failure = true;
    playing = false;
    needsFrame = false;
    el("play").textContent = "Play";
    clearSamples();
    showMetrics();
    return;
  }
  cpuMs = performance.now() - start;
  if (delta > 0) {
    samples.push(delta);
    if (samples.length > 240) samples.shift();
  }
  if (!firstFrameMs) firstFrameMs = performance.now() - opened;
  lastDraw = now;
  frame++;
  time = nextTime;
  needsFrame = false;
  if (now - lastReport >= 250 || frame === 1) {
    showMetrics();
    lastReport = now;
  }
}
async function load(getDoc: () => Promise<ShaderFile>, label: string) {
  const id = ++ticket,
    start = performance.now();
  status("Loading " + label + "…");
  failure = false;
  let next: Player | undefined;
  try {
    const doc = await getDoc();
    if (id !== ticket) return;
    chosen = doc;
    const request = select("backend").value;
    let backend =
      request === "auto"
        ? doc.language === "glsl"
          ? "webgl2"
          : "webgpu"
        : request;
    if (backend === "webgl2") {
      if (doc.language !== "glsl")
        throw Error(doc.language.toUpperCase() + " playback requires WebGPU.");
      if (!doc.passes) Object.assign(doc, await compile(doc, "parse"));
      if (id !== ticket) return;
      try {
        const gl = new MultiPassRenderer(document.createElement("canvas"));
        next = gl;
        const result = gl.compile(doc.passes!);
        if (!result.success) throw Error(result.errors);
      } catch (e) {
        release(next);
        next = undefined;
        if (
          request === "auto" &&
          /WebGL2 not supported|Floating-point buffer rendering/.test(String(e))
        )
          backend = "webgpu";
        else throw e;
      }
    }
    if (backend === "webgpu") {
      if (!doc.prepared) Object.assign(doc, await compile(doc, "prepare"));
      if (id !== ticket) return;
      next = await WebGPURenderer.create(doc.prepared!);
    }
    if (id !== ticket) {
      release(next);
      return;
    }
    release(player);
    player = next;
    active = doc;
    activeBackend = backend === "webgpu" ? "WebGPU" : "WebGL2";
    stage.replaceChildren(player!.canvas);
    configureCanvas();
    rewind();
    compiledMs = performance.now() - start;
    firstFrameMs = 0;
    opened = start;
    el("title").textContent = doc.title;
    el("credit").textContent =
      doc.language.toUpperCase() +
      " · " +
      doc.passes!.length +
      (doc.passes!.length === 1 ? " pass" : " passes");
    if (doc.id) {
      const entry = catalog.find((e) => e.id === doc.id);
      if (entry) el("credit").textContent += " · " + entry.credit;
    }
    for (const key of ["play", "reset", "code"])
      el<HTMLButtonElement>(key).disabled = false;
    status(playing ? "Ready" : "Paused");
    showMetrics();
    if (el<HTMLDialogElement>("inspector").open) void refreshInspector();
  } catch (e) {
    release(next);
    if (id === ticket) {
      failure = true;
      status(
        label +
          " · " +
          String(e instanceof Error ? e.message : e) +
          (active ? "\nShowing last successful preview: " + active.title : ""),
        true,
      );
    }
  }
}
let catalog: CatalogEntry[] = [];
const cached = new Map<string, ShaderFile>();
function loadCatalog(id: string) {
  const entry = catalog.find((c) => c.id === id);
  if (!entry) return;
  return load(async () => {
    const doc =
      cached.get(id) ??
      (await json<ShaderFile>("/shaders/" + encodeURIComponent(id) + ".json"));
    cached.set(id, doc);
    return doc;
  }, entry.title);
}
let localDocument: Promise<ShaderFile> | undefined;
library.onchange = () => {
  if (library.value === "local" && localDocument) {
    const doc = localDocument;
    void load(() => doc, library.selectedOptions[0].text);
  } else void loadCatalog(library.value);
};
select("backend").onchange = () => {
  const doc = chosen ?? active;
  if (doc) void load(async () => doc, doc.title);
};
select("size").onchange = select("scale").onchange = () => {
  configureCanvas();
  showMetrics();
};
new ResizeObserver(configureCanvas).observe(stage);
function detect(source: string): Language {
  if (/\bfn\s+\w+\s*\(|@fragment/.test(source)) return "wgsl";
  return /\bSV_(Target|Position)|\bfloat[234]\s+main\s*\(/i.test(source)
    ? "hlsl"
    : "glsl";
}
function localOption(name: string) {
  library.querySelector('[value="local"]')?.remove();
  library.add(option("local", name));
  library.value = "local";
}
function openFile(file: File) {
  localOption(file.name);
  localDocument = (async (): Promise<ShaderFile> => {
    const source = await file.text(),
      ext = file.name.split(".").pop()?.toLowerCase();
    const language =
      ext === "wgsl" || ext === "hlsl"
        ? ext
        : ext === "glsl" || ext === "frag"
          ? "glsl"
          : detect(source);
    return { title: file.name, source, language };
  })();
  const doc = localDocument;
  void load(() => doc, file.name);
}
el("open").onclick = () => el<HTMLInputElement>("file").click();
el<HTMLInputElement>("file").onchange = (e) => {
  const input = e.currentTarget as HTMLInputElement;
  if (input.files?.[0]) openFile(input.files[0]);
  input.value = "";
};
document.addEventListener("dragover", (e) => e.preventDefault());
document.addEventListener("drop", (e) => {
  e.preventDefault();
  if (e.dataTransfer?.files[0]) openFile(e.dataTransfer.files[0]);
});
function pasteAndRun(source: string, language = detect(source)) {
  if (!source.trim()) return;
  const title = "Clipboard." + language;
  localOption(title);
  const doc = Promise.resolve({ source, language, title });
  localDocument = doc;
  void load(() => doc, title);
}
el("paste").onclick = async () => {
  try {
    const text = await navigator.clipboard.readText();
    if (text.trim()) {
      pasteAndRun(text);
      return;
    }
  } catch {}
  el<HTMLDialogElement>("paste-dialog").showModal();
  el("input").focus();
};
el<HTMLTextAreaElement>("input").oninput = (e) => {
  select("paste-type").value = detect((e.target as HTMLTextAreaElement).value);
};
el("run").onclick = () => {
  pasteAndRun(
    el<HTMLTextAreaElement>("input").value,
    select("paste-type").value as Language,
  );
  el<HTMLDialogElement>("paste-dialog").close();
};
window.addEventListener("paste", (e) => {
  if (document.querySelector("dialog[open]")) return;
  const text = e.clipboardData?.getData("text");
  if (text) {
    e.preventDefault();
    pasteAndRun(text);
  }
});
el("play").onclick = () => {
  playing = !playing;
  clearSamples();
  el("play").textContent = playing ? "Pause" : "Play";
  if (!failure) status(playing ? "Ready" : "Paused");
  showMetrics();
};
el("reset").onclick = () => {
  rewind();
  showMetrics();
};
el("full").onclick = () => {
  void (
    document.fullscreenElement
      ? document.exitFullscreen()
      : document.querySelector(".viewer")!.requestFullscreen()
  ).catch((e) => status(e.message, true));
};
el("advanced").onclick = () => {
  const s = el("settings");
  s.hidden = !s.hidden;
  el("advanced").setAttribute("aria-expanded", String(!s.hidden));
};
document.addEventListener("visibilitychange", () => {
  clearSamples();
  keys.clear();
  showMetrics();
});
window.addEventListener("blur", () => keys.clear());
stage.addEventListener("keydown", (e) => {
  keys.add(e.keyCode);
  if (e.key === " ") e.preventDefault();
});
window.addEventListener("keyup", (e) => keys.delete(e.keyCode));
function pointer(e: PointerEvent) {
  if (!player) return;
  const c = player.canvas,
    b = c.getBoundingClientRect(),
    scale = Math.min(b.width / c.width, b.height / c.height);
  mouse[0] = (e.clientX - b.left - (b.width - c.width * scale) / 2) / scale;
  mouse[1] =
    c.height - (e.clientY - b.top - (b.height - c.height * scale) / 2) / scale;
}
stage.onpointerdown = (e) => {
  stage.focus();
  stage.setPointerCapture(e.pointerId);
  pointer(e);
  mouse[2] = mouse[0];
  mouse[3] = mouse[1];
};
stage.onpointermove = pointer;
stage.onpointerup = stage.onpointercancel = () => {
  mouse[2] = -Math.abs(mouse[2]);
  mouse[3] = -Math.abs(mouse[3]);
};
let inspectSerial = 0;
async function refreshInspector() {
  const doc = active,
    id = ++inspectSerial;
  if (!doc) return;
  const hlsl =
    select("source-type").value === "hlsl" && doc.language !== "hlsl";
  el("copy-status").textContent = "";
  if (hlsl && !doc.hlsl) {
    el("source").textContent = "Generating HLSL…";
    try {
      Object.assign(doc, await compile(doc, "export"));
    } catch (e) {
      if (id === inspectSerial) el("source").textContent = String(e);
      return;
    }
  }
  if (id !== inspectSerial) return;
  const p = select("pass"),
    index = Math.min(Number(p.value) || 0, doc.passes!.length - 1);
  p.replaceChildren(
    ...doc.passes!.map((pass, i) => option(String(i), pass.name)),
  );
  p.value = String(index);
  el("source").textContent = hlsl
    ? doc.hlsl![index]
    : doc.passes![index].source;
}
el("code").onclick = () => {
  el<HTMLDialogElement>("inspector").showModal();
  void refreshInspector();
};
select("pass").onchange = select("source-type").onchange = () => {
  void refreshInspector();
};
function wholeSource(doc: ShaderFile) {
  if (select("source-type").value === "original" || doc.language === "hlsl")
    return doc.source;
  if (!doc.hlsl) throw Error("HLSL is still compiling.");
  if (doc.passes!.length === 1 && !doc.passes![0].channels.some(Boolean))
    return doc.hlsl[0];
  return doc
    .passes!.map((p, i) =>
      [
        "//! PASS: " + p.name,
        ...p.channels.flatMap((ch, n) =>
          ch ? ["//! iChannel" + n + ": " + (ch === p.name ? "self" : ch)] : [],
        ),
        doc.hlsl![i],
      ].join("\n"),
    )
    .join("\n\n");
}
async function copy(all: boolean) {
  try {
    const doc = active!;
    const code = all
      ? wholeSource(doc)
      : select("source-type").value === "hlsl" && doc.language !== "hlsl"
        ? doc.hlsl?.[Number(select("pass").value)]
        : doc.passes![Number(select("pass").value)].source;
    if (!code) throw Error("Source unavailable.");
    await navigator.clipboard.writeText(code);
    el("copy-status").textContent = all ? "Shader copied" : "Pass copied";
  } catch (e) {
    el("copy-status").textContent = String(e);
  }
}
el("copy-pass").onclick = () => void copy(false);
el("copy-all").onclick = () => void copy(true);
document
  .querySelectorAll<HTMLElement>("[data-close]")
  .forEach((b) => (b.onclick = () => b.closest("dialog")!.close()));
function savedEdits() {
  const edits: Record<string, string> = {};
  try {
    for (const key of Object.keys(localStorage))
      if (key.startsWith("pst:shader:"))
        edits[key.slice(11)] = localStorage.getItem(key)!;
  } catch {}
  return edits;
}
el("recover").hidden = !Object.keys(savedEdits()).length;
el("recover").onclick = () => {
  const a = document.createElement("a");
  a.href = URL.createObjectURL(
    new Blob([JSON.stringify(savedEdits(), null, 2)], {
      type: "application/json",
    }),
  );
  a.download = "shader-edits.json";
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
};
requestAnimationFrame(tick);
void (async () => {
  catalog = await json<CatalogEntry[]>("/shaders/manifest.json");
  if (!catalog.length) throw Error("Shader library is empty.");
  library.replaceChildren(...catalog.map((c) => option(c.id, c.title)));
  await loadCatalog(catalog[0].id);
})().catch((e) => status(String(e), true));
