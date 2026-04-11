import { MultiPassRenderer } from '../renderer/MultiPassRenderer.js';
import { parseMultiPassShader, type ParsedPass } from '../renderer/shaderParser.js';
import type { Uniforms } from '../renderer/uniforms.js';
import { WebGL2Renderer } from '../renderer/WebGL2Renderer.js';
import { WebGPURenderer } from '../renderer/WebGPURenderer.js';
import type {
  PreviewBackend,
  PreviewCapabilities,
  PreviewCompileStatus,
  PreviewDocument,
  RenderPreset,
} from '../types/shader.js';
import type { PreviewHost, PreviewHostStatus } from './PreviewHost.js';

const NATIVE_BACKEND_REASONS: Partial<Record<PreviewBackend, string>> = {
  dx12: 'Native preview runtime is not active.',
  vulkan: 'Native preview runtime is not active.',
  metal: 'Native preview runtime is not active.',
  opengl: 'Native preview runtime is not active.',
};

export class BrowserPreviewHost implements PreviewHost {
  readonly runtime = 'browser' as const;
  readonly element: HTMLElement;

  private readonly stage: HTMLDivElement;
  private readonly loadingOverlay: HTMLDivElement;
  private readonly glCanvas: HTMLCanvasElement;
  private readonly gpuCanvas: HTMLCanvasElement;
  private readonly glRenderer: WebGL2Renderer;
  private readonly multiPassRenderer: MultiPassRenderer;
  private gpuRenderer: WebGPURenderer | null = null;
  private readonly compilerWorker: Worker;
  private currentPreset: RenderPreset;
  private currentDocument: PreviewDocument = { source: '', language: 'glsl' };
  private currentPasses: ParsedPass[] = [];
  private isMultiPass = false;
  private requestedBackend: PreviewBackend = 'auto';
  private resolvedBackend: PreviewBackend = 'webgl2';
  private compileStatus: PreviewCompileStatus = 'ready';
  private compileMessage = '';
  private compileIdCounter = 0;
  private activeCompileId = 0;
  private capabilities: PreviewCapabilities = {
    runtime: 'browser',
    availableBackends: ['auto', 'webgl2'],
    defaultBackend: 'auto',
    unavailableBackends: {
      webgpu: 'WebGPU is not available in this browser.',
      ...NATIVE_BACKEND_REASONS,
    },
  };

  constructor(initialPreset: RenderPreset) {
    this.currentPreset = initialPreset;

    this.element = document.createElement('div');
    this.element.className = 'canvas-stage';

    this.stage = this.element as HTMLDivElement;
    this.glCanvas = document.createElement('canvas');
    this.gpuCanvas = document.createElement('canvas');
    this.gpuCanvas.className = 'hidden';

    this.loadingOverlay = document.createElement('div');
    this.loadingOverlay.className = 'loading-overlay hidden';
    this.loadingOverlay.innerHTML = '<div class="spinner"></div> Transpiling WASM...';

    this.stage.append(this.glCanvas, this.gpuCanvas, this.loadingOverlay);

    this.glRenderer = new WebGL2Renderer(this.glCanvas);
    this.multiPassRenderer = new MultiPassRenderer(this.glCanvas);
    this.compilerWorker = new Worker(new URL('../worker/compiler.worker.ts', import.meta.url), { type: 'module' });

    this.setRenderPreset(initialPreset);
  }

  async initialize(): Promise<PreviewHostStatus> {
    const candidate = new WebGPURenderer(this.gpuCanvas);
    const ok = await candidate.init();

    if (ok) {
      this.gpuRenderer = candidate;
      this.capabilities = {
        runtime: 'browser',
        availableBackends: ['auto', 'webgl2', 'webgpu'],
        defaultBackend: 'auto',
        unavailableBackends: { ...NATIVE_BACKEND_REASONS },
      };
    } else {
      this.capabilities = {
        runtime: 'browser',
        availableBackends: ['auto', 'webgl2'],
        defaultBackend: 'auto',
        unavailableBackends: {
          webgpu: 'WebGPU is not available in this browser.',
          ...NATIVE_BACKEND_REASONS,
        },
      };
    }

    return this.getStatus();
  }

  dispose() {
    this.compilerWorker.terminate();
    this.glRenderer.dispose();
    this.multiPassRenderer.dispose();
    this.gpuRenderer?.dispose();
  }

  getActiveCanvas(): HTMLCanvasElement {
    return this.resolvedBackend === 'webgpu' ? this.gpuCanvas : this.glCanvas;
  }

  getStatus(): PreviewHostStatus {
    return {
      runtime: this.runtime,
      requestedBackend: this.requestedBackend,
      resolvedBackend: this.resolvedBackend,
      rendererLabel: this.getRendererLabel(),
      compileStatus: this.compileStatus,
      compileMessage: this.compileMessage,
      capabilities: this.capabilities,
    };
  }

  async setDocument(document: PreviewDocument): Promise<PreviewHostStatus> {
    this.currentDocument = { ...document };
    const parsed = parseMultiPassShader(document.source);
    this.isMultiPass = parsed.multiPass;
    this.currentPasses = parsed.passes;
    return await this.recompile();
  }

  async setBackend(backend: PreviewBackend): Promise<PreviewHostStatus> {
    this.requestedBackend = backend;
    return await this.recompile();
  }

  setRenderPreset(preset: RenderPreset) {
    this.currentPreset = preset;

    for (const canvas of [this.glCanvas, this.gpuCanvas]) {
      canvas.width = preset.width;
      canvas.height = preset.height;
    }
  }

  updateStageSize(containerWidth: number, containerHeight: number) {
    if (!containerWidth || !containerHeight) {
      return;
    }

    const scale = Math.min(
      containerWidth / this.currentPreset.width,
      containerHeight / this.currentPreset.height,
      1,
    );

    this.stage.style.width = `${Math.max(1, Math.floor(this.currentPreset.width * scale))}px`;
    this.stage.style.height = `${Math.max(1, Math.floor(this.currentPreset.height * scale))}px`;
  }

  getPointerPosition(event: MouseEvent): { x: number; y: number } | null {
    const rect = this.stage.getBoundingClientRect();
    if (!rect.width || !rect.height) {
      return null;
    }

    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    if (x < 0 || y < 0 || x > rect.width || y > rect.height) {
      return null;
    }

    const activeCanvas = this.getActiveCanvas();
    return {
      x: (x / rect.width) * activeCanvas.width,
      y: ((rect.height - y) / rect.height) * activeCanvas.height,
    };
  }

  usesKeyboard(): boolean {
    if (!this.isMultiPass) {
      return false;
    }

    if (this.resolvedBackend === 'webgpu') {
      return this.gpuRenderer?.usesKeyboard ?? false;
    }

    if (this.resolvedBackend === 'webgl2') {
      return this.multiPassRenderer.usesKeyboard;
    }

    return false;
  }

  render(uniforms: Uniforms, pressedKeys: Set<number>) {
    if (this.resolvedBackend === 'webgpu') {
      if (!this.gpuRenderer) {
        return;
      }
      if (this.isMultiPass) {
        this.gpuRenderer.updateKeyboard(pressedKeys);
      }
      this.gpuRenderer.render(uniforms);
      return;
    }

    if (this.resolvedBackend === 'webgl2') {
      if (this.isMultiPass) {
        this.multiPassRenderer.updateKeyboard(pressedKeys);
        this.multiPassRenderer.render(uniforms);
      } else {
        this.glRenderer.render(uniforms);
      }
    }
  }

  private async recompile(): Promise<PreviewHostStatus> {
    this.resolvedBackend = this.resolveBackend();
    this.syncCanvasVisibility();

    if (!this.currentDocument.source) {
      this.setCompileState('ready');
      return this.getStatus();
    }

    if (this.resolvedBackend === 'webgl2') {
      if (this.currentDocument.language !== 'glsl') {
        this.setCompileState(
          'offline',
          `${this.currentDocument.language.toUpperCase()} is not supported by the WebGL2 preview. Switch to WebGPU or choose a GLSL shader.`,
        );
        return this.getStatus();
      }

      const result = this.isMultiPass
        ? this.multiPassRenderer.compile(this.currentPasses)
        : this.glRenderer.compile(this.currentDocument.source);
      this.setCompileState(result.success ? 'ready' : 'error', result.errors || '');
      return this.getStatus();
    }

    if (this.resolvedBackend === 'webgpu') {
      if (!this.gpuRenderer) {
        this.setCompileState(
          'offline',
          'WebGPU is not available in this browser. Use Chrome 113+ / Edge 113+ or switch to WebGL2.',
        );
        return this.getStatus();
      }

      if (this.currentDocument.language === 'glsl') {
        this.setCompileState(
          'offline',
          'GLSL is not yet translated for the WebGPU preview. Use WebGL2 for GLSL content.',
        );
        return this.getStatus();
      }

      if (this.currentDocument.language === 'hlsl') {
        if (this.isMultiPass) {
          this.setCompileState(
            'error',
            'The browser WebGPU path only supports single-pass HLSL transpilation right now.',
          );
          return this.getStatus();
        }

        await this.compileHlsl(this.currentDocument.source);
        return this.getStatus();
      }

      const result = this.isMultiPass
        ? this.gpuRenderer.compileMultiPass(this.currentPasses)
        : this.gpuRenderer.compile(this.currentDocument.source);
      this.setCompileState(result.success ? 'ready' : 'error', result.errors || '');
      return this.getStatus();
    }

    this.setCompileState('offline', `${this.resolvedBackend} requires the native preview runtime.`);
    return this.getStatus();
  }

  private resolveBackend(): PreviewBackend {
    if (this.requestedBackend !== 'auto') {
      return this.requestedBackend;
    }

    return this.currentDocument.language === 'glsl' ? 'webgl2' : 'webgpu';
  }

  private syncCanvasVisibility() {
    if (this.resolvedBackend === 'webgpu') {
      this.glCanvas.classList.add('hidden');
      this.gpuCanvas.classList.remove('hidden');
      return;
    }

    this.gpuCanvas.classList.add('hidden');
    this.glCanvas.classList.remove('hidden');
  }

  private async compileHlsl(source: string) {
    const compileId = ++this.compileIdCounter;
    this.activeCompileId = compileId;
    this.setCompileState('transpiling');
    this.loadingOverlay.classList.remove('hidden');

    const workerResult = await new Promise<{ success: boolean; compiledTemplate?: string; error?: string }>((resolve) => {
      const handler = (event: MessageEvent) => {
        if (event.data.id === compileId) {
          this.compilerWorker.removeEventListener('message', handler);
          resolve(event.data);
        }
      };

      this.compilerWorker.addEventListener('message', handler);
      this.compilerWorker.postMessage({
        id: compileId,
        source,
        fromLanguage: 'hlsl',
        toLanguage: 'wgsl',
      });
    });

    if (this.activeCompileId !== compileId) {
      return;
    }

    this.loadingOverlay.classList.add('hidden');

    if (workerResult.success && workerResult.compiledTemplate && this.gpuRenderer) {
      const result = this.gpuRenderer.compile(workerResult.compiledTemplate);
      this.setCompileState(result.success ? 'ready' : 'error', result.errors || '');
      return;
    }

    this.setCompileState('error', workerResult.error || 'WASM transpilation failed.');
  }

  private setCompileState(status: PreviewCompileStatus, message = '') {
    this.compileStatus = status;
    this.compileMessage = status === 'ready' ? '' : message;
  }

  private getRendererLabel() {
    if (this.resolvedBackend === 'webgpu') {
      if (this.currentDocument.language === 'hlsl') {
        return 'HLSL -> WebGPU';
      }
      return this.isMultiPass ? 'WebGPU Multi-pass' : 'WebGPU';
    }

    if (this.resolvedBackend === 'webgl2') {
      return this.isMultiPass ? 'WebGL2 Multi-pass' : 'WebGL2';
    }

    return `${this.resolvedBackend.toUpperCase()} (native)`;
  }
}
