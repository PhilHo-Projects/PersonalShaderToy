import type {
  MainModule,
  ClassHandle,
} from "../../public/compilers/slang/interface";
import type { Language, ParsedPass, PreparedPass } from "../renderer/uniforms";

interface Core {
  graph_json(source: string): string;
  prepare_json(pass: string, language: string): string;
  export_hlsl(pass: string): string;
}
async function loadCore(): Promise<Core> {
  const url = new URL("/compilers/core/shader_core.js", self.location.origin)
    .href;
  const core = await import(/* @vite-ignore */ url);
  await core.default();
  return core;
}
let core: Promise<Core> | undefined;
let slang: Promise<MainModule> | undefined;
async function compileHlsl(pass: ParsedPass): Promise<string> {
  const url = new URL("/compilers/slang/slang-wasm.js", self.location.origin)
    .href;
  const module = await (slang ??= import(/* @vite-ignore */ url).then(
    (m) => m.default() as Promise<MainModule>,
  ));
  const global = module.createGlobalSession();
  const target = module
    .getCompileTargets()
    .find((t: { name: string }) => t.name === "WGSL");
  if (!global || !target) throw Error("Slang WGSL target unavailable");
  const session = global.createSession(target.value);
  if (!session) {
    global.delete();
    throw Error("Cannot create a Slang compiler session");
  }
  const handles: ClassHandle[] = [];
  try {
    const shader = session.loadModuleFromSource(
      "\n".repeat(pass.line - 1) + pass.source,
      "shader",
      "/input.hlsl",
    );
    if (shader) handles.push(shader);
    const entry = shader?.findAndCheckEntryPoint("main", 5); // Slang fragment stage
    if (entry) handles.push(entry);
    const composite =
      entry && session.createCompositeComponentType([shader, entry]);
    if (composite) handles.push(composite);
    const linked = composite?.link();
    if (linked) handles.push(linked);
    if (!linked) throw Error(pass.name + " · " + module.getLastError().message);
    return linked.getEntryPointCode(0, 0);
  } finally {
    for (const handle of handles.reverse()) handle.delete();
    session.delete();
    global.delete();
  }
}
self.onmessage = async ({
  data,
}: {
  data: {
    id: number;
    source: string;
    language: Language;
    operation: "parse" | "prepare" | "export";
    prepared?: PreparedPass[];
  };
}) => {
  try {
    const api = await (core ??= loadCore());
    const passes = JSON.parse(api.graph_json(data.source)) as ParsedPass[];
    let prepared = data.prepared;
    if (data.operation !== "parse" && !prepared) {
      prepared = [];
      for (const p of passes) {
        const input =
          data.language === "hlsl" ? { ...p, source: await compileHlsl(p) } : p;
        prepared.push(
          JSON.parse(
            api.prepare_json(
              JSON.stringify(input),
              data.language === "hlsl" ? "wgsl" : data.language,
            ),
          ),
        );
      }
    }
    const hlsl =
      data.operation === "export"
        ? prepared!.map((p) => api.export_hlsl(JSON.stringify(p)))
        : undefined;
    self.postMessage({ id: data.id, passes, prepared, hlsl });
  } catch (e) {
    self.postMessage({
      id: data.id,
      error: String(e instanceof Error ? e.message : e),
    });
  }
};
