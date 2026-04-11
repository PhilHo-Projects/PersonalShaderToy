import type { Uniforms } from './uniforms.js';
import type { ParsedPass } from './shaderParser.js';

const VERTEX_WGSL = `
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
  var pos = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
  );
  let xy = pos[vertex_index];
  return vec4<f32>(xy, 0.0, 1.0);
}
`;

const SINGLE_HEADER = `
struct ShaderUniforms {
  iTime: f32,
  iFrame: i32,
  _pad0: vec2<f32>,
  iResolution: vec4<f32>,
  iMouse: vec4<f32>,
  iDate: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: ShaderUniforms;
`;

const SINGLE_FOOTER = `
@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
  return mainImage(frag_coord.xy);
}
`;

const MULTI_HEADER = `
struct MultiPassUniforms {
  iTime: f32,
  iTimeDelta: f32,
  iFrame: i32,
  _pad0: f32,
  iResolution: vec4<f32>,
  iMouse: vec4<f32>,
  iDate: vec4<f32>,
  iChannelResolution: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> u: MultiPassUniforms;
@group(0) @binding(1) var iChannel0: texture_2d<f32>;
@group(0) @binding(2) var iChannel1: texture_2d<f32>;
@group(0) @binding(3) var iChannel2: texture_2d<f32>;
@group(0) @binding(4) var iChannel3: texture_2d<f32>;
@group(0) @binding(5) var iLinearSampler: sampler;

fn stSample(tex: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
  return textureSampleLevel(tex, iLinearSampler, vec2<f32>(uv.x, 1.0 - uv.y), 0.0);
}

fn stTexelFetch(tex: texture_2d<f32>, coord: vec2<i32>) -> vec4<f32> {
  let dims = vec2<i32>(textureDimensions(tex, 0));
  let y = clamp(dims.y - 1 - coord.y, 0, dims.y - 1);
  let x = clamp(coord.x, 0, dims.x - 1);
  return textureLoad(tex, vec2<i32>(x, y), 0);
}
`;

const MULTI_FOOTER = `
@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
  let st_frag_coord = vec2<f32>(frag_coord.x, u.iResolution.y - frag_coord.y);
  return mainImage(st_frag_coord);
}
`;

export interface CompileResult {
  success: boolean;
  errors?: string;
  passName?: string;
}

type RendererMode = 'single' | 'multi' | 'none';

interface SinglePassState {
  pipeline: GPURenderPipeline;
  bindGroup: GPUBindGroup;
}

interface MultiPassPipelineState {
  name: string;
  pipeline: GPURenderPipeline;
  channels: (string | null)[];
  renderTarget: GpuPingPongTarget | null;
}

class GpuPingPongTarget {
  private textures: [GPUTexture, GPUTexture];
  private views: [GPUTextureView, GPUTextureView];
  private readIndex = 0;

  constructor(
    private device: GPUDevice,
    public width: number,
    public height: number,
    private format: GPUTextureFormat,
  ) {
    this.textures = [this.createTexture(), this.createTexture()];
    this.views = [this.textures[0].createView(), this.textures[1].createView()];
  }

  get readView(): GPUTextureView {
    return this.views[this.readIndex];
  }

  get writeView(): GPUTextureView {
    return this.views[1 - this.readIndex];
  }

  swap() {
    this.readIndex = 1 - this.readIndex;
  }

  resize(width: number, height: number) {
    const nextWidth = Math.max(1, width);
    const nextHeight = Math.max(1, height);
    if (nextWidth === this.width && nextHeight === this.height) {
      return;
    }
    this.width = nextWidth;
    this.height = nextHeight;
    this.destroy();
    this.textures = [this.createTexture(), this.createTexture()];
    this.views = [this.textures[0].createView(), this.textures[1].createView()];
    this.readIndex = 0;
  }

  destroy() {
    for (const texture of this.textures) {
      texture.destroy();
    }
  }

  private createTexture(): GPUTexture {
    return this.device.createTexture({
      size: { width: this.width, height: this.height, depthOrArrayLayers: 1 },
      format: this.format,
      usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING,
    });
  }
}

export class WebGPURenderer {
  private context: GPUCanvasContext | null = null;
  private device: GPUDevice | null = null;
  private presentationFormat: GPUTextureFormat = 'bgra8unorm';
  private vertexModule: GPUShaderModule | null = null;

  private singleBindGroupLayout: GPUBindGroupLayout | null = null;
  private singlePipelineLayout: GPUPipelineLayout | null = null;
  private singleUniformBuffer: GPUBuffer | null = null;
  private singleUniformFloats = new Float32Array(16);
  private singleUniformInts = new Int32Array(this.singleUniformFloats.buffer);
  private single: SinglePassState | null = null;

  private multiBindGroupLayout: GPUBindGroupLayout | null = null;
  private multiPipelineLayout: GPUPipelineLayout | null = null;
  private multiUniformBuffer: GPUBuffer | null = null;
  private multiUniformFloats = new Float32Array(32);
  private multiUniformInts = new Int32Array(this.multiUniformFloats.buffer);
  private multiPasses: MultiPassPipelineState[] = [];
  private multiTargets = new Map<string, GpuPingPongTarget>();
  private offscreenFormat: GPUTextureFormat = 'rgba16float';
  private linearSampler: GPUSampler | null = null;
  private dummyTexture: GPUTexture | null = null;
  private dummyTextureView: GPUTextureView | null = null;
  private keyboardTexture: GPUTexture | null = null;
  private keyboardTextureView: GPUTextureView | null = null;
  private keyboardData = new Uint8Array(256 * 3 * 4);
  private hasKeyboardChannel = false;

  private mode: RendererMode = 'none';

  constructor(public canvas: HTMLCanvasElement) {}

  async init(): Promise<boolean> {
    if (!navigator.gpu) {
      return false;
    }

    const adapter = await navigator.gpu.requestAdapter();
    if (!adapter) {
      return false;
    }

    this.device = await adapter.requestDevice();
    this.context = this.canvas.getContext('webgpu');
    if (!this.context) {
      return false;
    }

    this.presentationFormat = navigator.gpu.getPreferredCanvasFormat();
    this.context.configure({
      device: this.device,
      format: this.presentationFormat,
      alphaMode: 'premultiplied',
    });

    this.vertexModule = this.device.createShaderModule({ code: VERTEX_WGSL, label: 'fullscreen-vertex' });

    this.singleBindGroupLayout = this.device.createBindGroupLayout({
      entries: [{
        binding: 0,
        visibility: GPUShaderStage.FRAGMENT,
        buffer: { type: 'uniform' },
      }],
    });
    this.singlePipelineLayout = this.device.createPipelineLayout({
      bindGroupLayouts: [this.singleBindGroupLayout],
    });
    this.singleUniformBuffer = this.device.createBuffer({
      size: this.singleUniformFloats.byteLength,
      usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
    });

    this.multiBindGroupLayout = this.device.createBindGroupLayout({
      entries: [
        {
          binding: 0,
          visibility: GPUShaderStage.FRAGMENT,
          buffer: { type: 'uniform' },
        },
        {
          binding: 1,
          visibility: GPUShaderStage.FRAGMENT,
          texture: { sampleType: 'float', viewDimension: '2d', multisampled: false },
        },
        {
          binding: 2,
          visibility: GPUShaderStage.FRAGMENT,
          texture: { sampleType: 'float', viewDimension: '2d', multisampled: false },
        },
        {
          binding: 3,
          visibility: GPUShaderStage.FRAGMENT,
          texture: { sampleType: 'float', viewDimension: '2d', multisampled: false },
        },
        {
          binding: 4,
          visibility: GPUShaderStage.FRAGMENT,
          texture: { sampleType: 'float', viewDimension: '2d', multisampled: false },
        },
        {
          binding: 5,
          visibility: GPUShaderStage.FRAGMENT,
          sampler: { type: 'filtering' },
        },
      ],
    });
    this.multiPipelineLayout = this.device.createPipelineLayout({
      bindGroupLayouts: [this.multiBindGroupLayout],
    });
    this.multiUniformBuffer = this.device.createBuffer({
      size: this.multiUniformFloats.byteLength,
      usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
    });

    this.linearSampler = this.device.createSampler({
      magFilter: 'linear',
      minFilter: 'linear',
      mipmapFilter: 'linear',
      addressModeU: 'clamp-to-edge',
      addressModeV: 'clamp-to-edge',
    });

    this.dummyTexture = this.device.createTexture({
      size: { width: 1, height: 1, depthOrArrayLayers: 1 },
      format: 'rgba8unorm',
      usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
    });
    this.dummyTextureView = this.dummyTexture.createView();
    this.device.queue.writeTexture(
      { texture: this.dummyTexture },
      new Uint8Array([0, 0, 0, 255]),
      { bytesPerRow: 4 },
      { width: 1, height: 1, depthOrArrayLayers: 1 },
    );

    this.keyboardTexture = this.device.createTexture({
      size: { width: 256, height: 3, depthOrArrayLayers: 1 },
      format: 'rgba8unorm',
      usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
    });
    this.keyboardTextureView = this.keyboardTexture.createView();
    this.device.queue.writeTexture(
      { texture: this.keyboardTexture },
      this.keyboardData,
      { bytesPerRow: 256 * 4, rowsPerImage: 3 },
      { width: 256, height: 3, depthOrArrayLayers: 1 },
    );

    this.device.lost.then((info) => {
      console.error(`WebGPU device lost: ${info.message}`);
    });

    return true;
  }

  get usesKeyboard(): boolean {
    return this.mode === 'multi' && this.hasKeyboardChannel;
  }

  updateKeyboard(keys: Set<number>) {
    for (let i = 0; i < 256; i++) {
      this.keyboardData[i * 4] = keys.has(i) ? 255 : 0;
    }
  }

  compile(source: string): CompileResult {
    if (!this.device || !this.vertexModule || !this.singleBindGroupLayout || !this.singlePipelineLayout || !this.singleUniformBuffer) {
      return { success: false, errors: 'WebGPU is not initialized.' };
    }

    try {
      const wrapped = wrapSinglePassSource(source);
      const module = this.device.createShaderModule({
        code: wrapped.code,
        label: 'single-pass-fragment',
      });

      this.captureValidation('single-pass', module);

      const pipeline = this.device.createRenderPipeline({
        layout: this.singlePipelineLayout,
        vertex: {
          module: this.vertexModule,
          entryPoint: 'vs_main',
        },
        fragment: {
          module,
          entryPoint: 'fs_main',
          targets: [{ format: this.presentationFormat }],
        },
        primitive: { topology: 'triangle-list' },
      });

      const bindGroup = this.device.createBindGroup({
        layout: this.singleBindGroupLayout,
        entries: [{
          binding: 0,
          resource: { buffer: this.singleUniformBuffer },
        }],
      });

      this.single = { pipeline, bindGroup };
      this.mode = 'single';
      return { success: true };
    } catch (error) {
      return { success: false, errors: formatGpuError(error) };
    }
  }

  compileMultiPass(parsedPasses: ParsedPass[]): CompileResult {
    if (!this.device || !this.vertexModule || !this.multiBindGroupLayout || !this.multiPipelineLayout || !this.multiUniformBuffer) {
      return { success: false, errors: 'WebGPU is not initialized.' };
    }

    const nextPipelines: MultiPassPipelineState[] = [];
    const nextTargets = new Map<string, GpuPingPongTarget>();
    const width = Math.max(1, this.canvas.width);
    const height = Math.max(1, this.canvas.height);

    try {
      for (const pass of parsedPasses) {
        const isImage = pass.name.toLowerCase() === 'image';
        const wrapped = wrapMultiPassSource(pass.source);
        const module = this.device.createShaderModule({
          code: wrapped.code,
          label: `multipass-${pass.name}`,
        });

        this.captureValidation(pass.name, module);

        const targetFormat = isImage ? this.presentationFormat : this.offscreenFormat;
        const pipeline = this.device.createRenderPipeline({
          layout: this.multiPipelineLayout,
          vertex: {
            module: this.vertexModule,
            entryPoint: 'vs_main',
          },
          fragment: {
            module,
            entryPoint: 'fs_main',
            targets: [{ format: targetFormat }],
          },
          primitive: { topology: 'triangle-list' },
        });

        let renderTarget: GpuPingPongTarget | null = null;
        if (!isImage) {
          renderTarget = new GpuPingPongTarget(this.device, width, height, this.offscreenFormat);
          nextTargets.set(pass.name, renderTarget);
        }

        nextPipelines.push({
          name: pass.name,
          pipeline,
          channels: pass.channels.slice(),
          renderTarget,
        });
      }
    } catch (error) {
      for (const target of nextTargets.values()) {
        target.destroy();
      }
      return { success: false, errors: formatGpuError(error) };
    }

    this.disposeMultiPass();
    this.multiPasses = nextPipelines;
    this.multiTargets = nextTargets;
    this.hasKeyboardChannel = parsedPasses.some((pass) =>
      pass.channels.some((channel) => channel?.toLowerCase() === 'keyboard'),
    );
    this.mode = 'multi';
    return { success: true };
  }

  render(u: Uniforms) {
    if (!this.device || !this.context) {
      return;
    }

    if (this.mode === 'single') {
      this.renderSingle(u);
      return;
    }

    if (this.mode === 'multi') {
      this.renderMulti(u);
    }
  }

  dispose() {
    this.single = null;
    this.disposeMultiPass();

    this.singleUniformBuffer?.destroy();
    this.singleUniformBuffer = null;

    this.multiUniformBuffer?.destroy();
    this.multiUniformBuffer = null;

    this.dummyTexture?.destroy();
    this.dummyTexture = null;
    this.dummyTextureView = null;

    this.keyboardTexture?.destroy();
    this.keyboardTexture = null;
    this.keyboardTextureView = null;

    this.mode = 'none';
  }

  private renderSingle(u: Uniforms) {
    if (!this.device || !this.context || !this.single || !this.singleUniformBuffer) {
      return;
    }

    this.singleUniformFloats[0] = u.iTime;
    this.singleUniformInts[1] = u.iFrame | 0;
    this.singleUniformFloats[4] = u.iResolution[0];
    this.singleUniformFloats[5] = u.iResolution[1];
    this.singleUniformFloats[6] = u.iResolution[2];
    this.singleUniformFloats[7] = 0.0;
    this.singleUniformFloats[8] = u.iMouse[0];
    this.singleUniformFloats[9] = u.iMouse[1];
    this.singleUniformFloats[10] = u.iMouse[2];
    this.singleUniformFloats[11] = u.iMouse[3];
    this.singleUniformFloats[12] = u.iDate[0];
    this.singleUniformFloats[13] = u.iDate[1];
    this.singleUniformFloats[14] = u.iDate[2];
    this.singleUniformFloats[15] = u.iDate[3];

    this.device.queue.writeBuffer(this.singleUniformBuffer, 0, this.singleUniformFloats);

    const encoder = this.device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [{
        view: this.context.getCurrentTexture().createView(),
        clearValue: { r: 0, g: 0, b: 0, a: 1 },
        loadOp: 'clear',
        storeOp: 'store',
      }],
    });
    pass.setPipeline(this.single.pipeline);
    pass.setBindGroup(0, this.single.bindGroup);
    pass.draw(3);
    pass.end();
    this.device.queue.submit([encoder.finish()]);
  }

  private renderMulti(u: Uniforms) {
    if (!this.device || !this.context || !this.multiUniformBuffer || !this.multiBindGroupLayout || !this.linearSampler || !this.dummyTextureView) {
      return;
    }

    this.resizeTargetsIfNeeded();

    if (this.hasKeyboardChannel && this.keyboardTexture) {
      this.device.queue.writeTexture(
        { texture: this.keyboardTexture },
        this.keyboardData,
        { bytesPerRow: 256 * 4, rowsPerImage: 3 },
        { width: 256, height: 3, depthOrArrayLayers: 1 },
      );
    }

    const encoder = this.device.createCommandEncoder();

    for (const passState of this.multiPasses) {
      this.writeMultiUniforms(u, passState.channels);

      const bindGroup = this.device.createBindGroup({
        layout: this.multiBindGroupLayout,
        entries: [
          {
            binding: 0,
            resource: { buffer: this.multiUniformBuffer },
          },
          {
            binding: 1,
            resource: this.resolveChannelView(passState.channels[0]),
          },
          {
            binding: 2,
            resource: this.resolveChannelView(passState.channels[1]),
          },
          {
            binding: 3,
            resource: this.resolveChannelView(passState.channels[2]),
          },
          {
            binding: 4,
            resource: this.resolveChannelView(passState.channels[3]),
          },
          {
            binding: 5,
            resource: this.linearSampler,
          },
        ],
      });

      const colorView = passState.renderTarget
        ? passState.renderTarget.writeView
        : this.context.getCurrentTexture().createView();

      const passEncoder = encoder.beginRenderPass({
        colorAttachments: [{
          view: colorView,
          clearValue: { r: 0, g: 0, b: 0, a: 1 },
          loadOp: 'clear',
          storeOp: 'store',
        }],
      });
      passEncoder.setPipeline(passState.pipeline);
      passEncoder.setBindGroup(0, bindGroup);
      passEncoder.draw(3);
      passEncoder.end();

      if (passState.renderTarget) {
        passState.renderTarget.swap();
      }
    }

    this.device.queue.submit([encoder.finish()]);
  }

  private writeMultiUniforms(u: Uniforms, channels: (string | null)[]) {
    if (!this.device || !this.multiUniformBuffer) {
      return;
    }

    this.multiUniformFloats[0] = u.iTime;
    this.multiUniformFloats[1] = u.iTimeDelta;
    this.multiUniformInts[2] = u.iFrame | 0;
    this.multiUniformFloats[4] = u.iResolution[0];
    this.multiUniformFloats[5] = u.iResolution[1];
    this.multiUniformFloats[6] = u.iResolution[2];
    this.multiUniformFloats[7] = 0.0;
    this.multiUniformFloats[8] = u.iMouse[0];
    this.multiUniformFloats[9] = u.iMouse[1];
    this.multiUniformFloats[10] = u.iMouse[2];
    this.multiUniformFloats[11] = u.iMouse[3];
    this.multiUniformFloats[12] = u.iDate[0];
    this.multiUniformFloats[13] = u.iDate[1];
    this.multiUniformFloats[14] = u.iDate[2];
    this.multiUniformFloats[15] = u.iDate[3];

    for (let i = 0; i < 4; i++) {
      const base = 16 + i * 4;
      const channel = channels[i];
      if (channel && channel.toLowerCase() === 'keyboard') {
        this.multiUniformFloats[base] = 256.0;
        this.multiUniformFloats[base + 1] = 3.0;
        this.multiUniformFloats[base + 2] = 1.0;
        this.multiUniformFloats[base + 3] = 0.0;
        continue;
      }

      const target = channel ? this.multiTargets.get(channel) : null;
      const width = target?.width ?? u.iResolution[0];
      const height = target?.height ?? u.iResolution[1];
      this.multiUniformFloats[base] = width;
      this.multiUniformFloats[base + 1] = height;
      this.multiUniformFloats[base + 2] = width / Math.max(1, height);
      this.multiUniformFloats[base + 3] = 0.0;
    }

    this.device.queue.writeBuffer(this.multiUniformBuffer, 0, this.multiUniformFloats);
  }

  private resolveChannelView(channel: string | null): GPUTextureView {
    if (!channel) {
      return this.dummyTextureView!;
    }

    if (channel.toLowerCase() === 'keyboard') {
      return this.keyboardTextureView ?? this.dummyTextureView!;
    }

    return this.multiTargets.get(channel)?.readView ?? this.dummyTextureView!;
  }

  private resizeTargetsIfNeeded() {
    const width = Math.max(1, this.canvas.width);
    const height = Math.max(1, this.canvas.height);
    for (const target of this.multiTargets.values()) {
      target.resize(width, height);
    }
  }

  private disposeMultiPass() {
    for (const target of this.multiTargets.values()) {
      target.destroy();
    }
    this.multiPasses = [];
    this.multiTargets.clear();
    this.hasKeyboardChannel = false;
  }

  private captureValidation(label: string, module: GPUShaderModule) {
    if (!this.device) {
      return;
    }

    module.getCompilationInfo().then((info) => {
      const errors = info.messages.filter((message) => message.type === 'error');
      if (errors.length === 0) {
        return;
      }
      const text = errors
        .map((message) => `${message.lineNum}:${message.linePos} ${message.message}`)
        .join('\n');
      console.error(`${label} WGSL compile error:\n${text}`);
    }).catch(() => {});
  }
}

function wrapSinglePassSource(source: string): { code: string } {
  const hasUniformBlock = /\bvar<uniform>\s+u\s*:/.test(source) || /\bstruct\s+(?:Uniforms|ShaderUniforms)\b/.test(source);
  const hasFragmentEntry = /@fragment\b/.test(source) && /\bfn\s+fs_main\s*\(/.test(source);
  const hasMainImage = /\bfn\s+mainImage\s*\(/.test(source);

  if (hasUniformBlock && hasFragmentEntry) {
    return { code: source };
  }
  if (hasMainImage) {
    return { code: `${SINGLE_HEADER}\n${source}\n${SINGLE_FOOTER}` };
  }
  if (hasFragmentEntry) {
    return { code: `${SINGLE_HEADER}\n${source}` };
  }
  return { code: `${SINGLE_HEADER}\n${source}\n${SINGLE_FOOTER}` };
}

function wrapMultiPassSource(source: string): { code: string } {
  const hasUniformBlock = /\bvar<uniform>\s+u\s*:/.test(source) || /\bstruct\s+MultiPassUniforms\b/.test(source);
  const hasFragmentEntry = /@fragment\b/.test(source) && /\bfn\s+fs_main\s*\(/.test(source);
  const hasMainImage = /\bfn\s+mainImage\s*\(/.test(source);

  if (hasUniformBlock && hasFragmentEntry) {
    return { code: source };
  }
  if (hasMainImage) {
    return { code: `${MULTI_HEADER}\n${source}\n${MULTI_FOOTER}` };
  }
  if (hasFragmentEntry) {
    return { code: `${MULTI_HEADER}\n${source}` };
  }
  return { code: `${MULTI_HEADER}\n${source}\n${MULTI_FOOTER}` };
}

function formatGpuError(error: unknown): string {
  if (error instanceof Error) {
    return error.stack || error.message;
  }
  return String(error);
}
