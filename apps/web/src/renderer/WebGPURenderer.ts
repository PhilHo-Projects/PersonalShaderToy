import type { Player, PreparedPass, Uniforms } from "./uniforms";

const vertexSource =
  "@vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4f { var p=array(vec2f(-1,-1),vec2f(3,-1),vec2f(-1,3)); return vec4f(p[i],0,1); }";
type Target = { textures: GPUTexture[]; read: number };
type Pass = {
  source: PreparedPass;
  pipeline: GPURenderPipeline;
  uniform: GPUBuffer;
  target?: Target;
};

const lostDevices = new WeakSet<GPUDevice>();
let devicePromise: Promise<GPUDevice> | undefined;
async function deviceForPreview() {
  if (!devicePromise)
    devicePromise = (async () => {
      const adapter = await navigator.gpu?.requestAdapter();
      if (!adapter) throw Error("WebGPU adapter unavailable");
      const device = await adapter.requestDevice({
        requiredFeatures: adapter.features.has("timestamp-query")
          ? ["timestamp-query"]
          : [],
      });
      void device.lost.then(() => {
        lostDevices.add(device);
        devicePromise = undefined;
      });
      return device;
    })().catch((error) => {
      devicePromise = undefined;
      throw error;
    });
  return devicePromise;
}

export class WebGPURenderer implements Player {
  readonly canvas = document.createElement("canvas");
  timings: number[] | null = null;
  private readonly ctx: GPUCanvasContext;
  private readonly bgl: GPUBindGroupLayout;
  private readonly sampler: GPUSampler;
  private readonly blank: GPUTexture;
  private readonly keyboard: GPUTexture;
  private readonly keys = new Uint8Array(256 * 3 * 4);
  private readonly passes: Pass[] = [];
  private readonly byName = new Map<string, Target>();
  private readonly format = navigator.gpu.getPreferredCanvasFormat();
  private size = "";
  private output?: GPUTexture;
  private query?: GPUQuerySet;
  private queryResolve?: GPUBuffer;
  private readback?: GPUBuffer;
  private reading = false;
  private disposed = false;
  private generation = 0;

  private constructor(private device: GPUDevice) {
    const ctx = this.canvas.getContext("webgpu");
    if (!ctx) throw Error("WebGPU canvas unavailable");
    this.ctx = ctx;
    ctx.configure({
      device,
      format: this.format,
      usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_DST,
    });
    const f = GPUShaderStage.FRAGMENT;
    this.bgl = device.createBindGroupLayout({
      entries: [
        { binding: 0, visibility: f, buffer: { type: "uniform" } },
        ...[1, 2, 3, 4].map((binding) => ({
          binding,
          visibility: f,
          texture: {},
        })),
        { binding: 5, visibility: f, sampler: {} },
      ],
    });
    this.sampler = device.createSampler({
      magFilter: "linear",
      minFilter: "linear",
    });
    this.blank = this.makeTexture(
      1,
      1,
      "rgba8unorm",
      GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
    );
    this.device.queue.writeTexture(
      { texture: this.blank },
      new Uint8Array([0, 0, 0, 255]),
      {},
      [1, 1],
    );
    this.keyboard = this.makeTexture(
      256,
      3,
      "rgba8unorm",
      GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
    );
  }

  static async create(sources: PreparedPass[]) {
    const device = await deviceForPreview();
    const player = new WebGPURenderer(device);
    try {
      const layout = device.createPipelineLayout({
        bindGroupLayouts: [player.bgl],
      });
      const vertex = device.createShaderModule({ code: vertexSource });
      for (const source of sources) {
        const module = device.createShaderModule({ code: source.wgsl });
        const errors = (await module.getCompilationInfo()).messages.filter(
          (m) => m.type === "error",
        );
        if (errors.length)
          throw Error(
            source.name +
              " · " +
              errors.map((m) => m.lineNum + ": " + m.message).join("\n"),
          );
        const pipeline = await device.createRenderPipelineAsync({
          layout,
          vertex: { module: vertex, entryPoint: "vs" },
          fragment: {
            module,
            entryPoint: source.entry,
            targets: [
              {
                format: source.name === "Image" ? player.format : "rgba16float",
              },
            ],
          },
        });
        const uniform = device.createBuffer({
          size: 144,
          usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
        });
        player.passes.push({ source, pipeline, uniform });
      }
      if (device.features.has("timestamp-query")) {
        const size = sources.length * 16;
        player.query = device.createQuerySet({
          type: "timestamp",
          count: sources.length * 2,
        });
        player.queryResolve = device.createBuffer({
          size,
          usage: GPUBufferUsage.QUERY_RESOLVE | GPUBufferUsage.COPY_SRC,
        });
        player.readback = device.createBuffer({
          size,
          usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
        });
      }
      return player;
    } catch (error) {
      player.dispose();
      throw error;
    }
  }

  private makeTexture(
    width: number,
    height: number,
    format: GPUTextureFormat,
    usage: GPUTextureUsageFlags,
  ) {
    return this.device.createTexture({ size: [width, height], format, usage });
  }
  reset() {
    this.size = "";
    this.timings = null;
    this.generation++;
  }
  updateKeyboard(keys: Set<number>) {
    for (let i = 0; i < 256; i++) {
      const offset = i * 4,
        held = keys.has(i),
        down = held && !this.keys[offset];
      this.keys[1024 + offset] = down ? 255 : 0;
      if (down) this.keys[2048 + offset] ^= 255;
      this.keys[offset] = held ? 255 : 0;
    }
  }
  private resize(w: number, h: number) {
    if (this.size === w + "x" + h) return;
    this.size = w + "x" + h;
    this.output?.destroy();
    this.output = this.makeTexture(
      w,
      h,
      this.format,
      GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
    );
    for (const p of this.passes) {
      p.target?.textures.forEach((t) => t.destroy());
      if (p.source.name === "Image") continue;
      p.target = {
        read: 0,
        textures: [0, 1].map(() =>
          this.makeTexture(
            w,
            h,
            "rgba16float",
            GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING,
          ),
        ),
      };
      this.byName.set(p.source.name, p.target);
    }
  }

  render(u: Uniforms) {
    if (lostDevices.has(this.device))
      throw Error("WebGPU device lost. Select a shader to restart playback.");
    const [w, h] = u.iResolution;
    this.resize(w, h);
    const queue = this.device.queue,
      enc = this.device.createCommandEncoder();
    queue.writeTexture(
      { texture: this.keyboard },
      this.keys,
      { bytesPerRow: 1024 },
      [256, 3],
    );
    const timed = !!this.query && !this.reading;
    for (const [i, p] of this.passes.entries()) {
      const bytes = new ArrayBuffer(144),
        v = new Float32Array(bytes);
      v[0] = u.iTime;
      v[1] = u.iTimeDelta;
      new Int32Array(bytes)[p.source.legacy ? 1 : 2] = u.iFrame;
      v.set(u.iResolution, 4);
      v.set(u.iMouse, 8);
      v.set(u.iDate, 12);
      const entries: GPUBindGroupEntry[] = [
        { binding: 0, resource: { buffer: p.uniform } },
      ];
      for (const [n, ch] of p.source.channels.entries()) {
        const buf = this.byName.get(ch ?? "");
        const tex =
          ch === "keyboard"
            ? this.keyboard
            : buf
              ? buf.textures[buf.read]
              : this.blank;
        v.set([tex.width, tex.height, 1, 0], 16 + n * 4);
        entries.push({ binding: n + 1, resource: tex.createView() });
      }
      v.set([0, 0, w, h], p.source.legacy ? 16 : 32);
      queue.writeBuffer(p.uniform, 0, bytes);
      entries.push({ binding: 5, resource: this.sampler });
      const out = p.target
        ? p.target.textures[1 - p.target.read]
        : this.output!;
      const pass = enc.beginRenderPass({
        colorAttachments: [
          {
            view: out.createView(),
            loadOp: "clear",
            clearValue: [0, 0, 0, 1],
            storeOp: "store",
          },
        ],
        timestampWrites: timed
          ? {
              querySet: this.query!,
              beginningOfPassWriteIndex: 2 * i,
              endOfPassWriteIndex: 2 * i + 1,
            }
          : undefined,
      });
      pass.setPipeline(p.pipeline);
      pass.setBindGroup(
        0,
        this.device.createBindGroup({ layout: this.bgl, entries }),
      );
      pass.draw(3);
      pass.end();
      if (p.target) p.target.read = 1 - p.target.read;
    }
    enc.copyTextureToTexture(
      { texture: this.output! },
      { texture: this.ctx.getCurrentTexture() },
      [w, h],
    );
    if (timed) {
      const bytes = this.passes.length * 16;
      enc.resolveQuerySet(
        this.query!,
        0,
        this.passes.length * 2,
        this.queryResolve!,
        0,
      );
      enc.copyBufferToBuffer(this.queryResolve!, 0, this.readback!, 0, bytes);
    }
    queue.submit([enc.finish()]);
    if (timed) {
      this.reading = true;
      const generation = this.generation,
        buffer = this.readback!;
      void buffer
        .mapAsync(GPUMapMode.READ)
        .then(() => {
          const times = new BigUint64Array(buffer.getMappedRange());
          if (!this.disposed && generation === this.generation)
            this.timings = this.passes.map(
              (_, i) => Number(times[2 * i + 1] - times[2 * i]) / 1e6,
            );
          buffer.unmap();
        })
        .catch(() => {})
        .finally(() => {
          this.reading = false;
        });
    }
  }
  dispose() {
    this.disposed = true;
    for (const p of this.passes) {
      p.uniform.destroy();
      p.target?.textures.forEach((t) => t.destroy());
    }
    this.output?.destroy();
    this.blank.destroy();
    this.keyboard.destroy();
    this.query?.destroy();
    this.queryResolve?.destroy();
    this.readback?.destroy();
    this.ctx.unconfigure();
  }
}
