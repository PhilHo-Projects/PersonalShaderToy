import type { Uniforms } from '../renderer/uniforms.js';
import type {
  PreviewBackend,
  PreviewCapabilities,
  PreviewCompileStatus,
  PreviewDocument,
  RenderPreset,
} from '../types/shader.js';
import type { NativePreviewEvent } from '../types/nativePreview.js';
import type { PreviewHost, PreviewHostStatus } from './PreviewHost.js';
import type { NativePreviewBridge } from './NativePreviewBridge.js';

const NATIVE_BACKENDS: PreviewBackend[] = ['auto', 'dx12', 'vulkan', 'metal', 'opengl'];

export class NativePreviewHost implements PreviewHost {
  readonly runtime = 'native' as const;
  readonly element: HTMLElement;

  private bridge: NativePreviewBridge;
  private statusEl: HTMLDivElement;
  private dummyCanvas: HTMLCanvasElement;
  private currentDocument: PreviewDocument = { source: '', language: 'glsl' };
  private requestedBackend: PreviewBackend = 'auto';
  private resolvedBackend = '';
  private adapterName = '';
  private compileStatus: PreviewCompileStatus = 'ready';
  private compileMessage = '';
  private nativeFps = 0;
  private nativeFrame = 0;
  private diagnosticListeners: ((level: string, message: string) => void)[] = [];

  constructor(bridge: NativePreviewBridge) {
    this.bridge = bridge;

    this.element = document.createElement('div');
    this.element.className = 'canvas-stage native-preview-stage';

    this.statusEl = document.createElement('div');
    this.statusEl.className = 'native-preview-status';
    this.statusEl.textContent = 'Native preview starting...';

    this.dummyCanvas = document.createElement('canvas');
    this.dummyCanvas.style.display = 'none';
    this.dummyCanvas.width = 1280;
    this.dummyCanvas.height = 720;

    this.element.append(this.statusEl, this.dummyCanvas);
  }

  onDiagnostic(listener: (level: string, message: string) => void): () => void {
    this.diagnosticListeners.push(listener);
    return () => {
      this.diagnosticListeners = this.diagnosticListeners.filter((l) => l !== listener);
    };
  }

  async initialize(): Promise<PreviewHostStatus> {
    this.bridge.onEvent((event) => this.handleSidecarEvent(event));

    try {
      await this.bridge.connect();
      this.statusEl.textContent = 'Native preview connected. Rendering in separate window.';
    } catch (err) {
      this.compileStatus = 'offline';
      this.compileMessage = `Failed to start native preview: ${err}`;
      this.statusEl.textContent = this.compileMessage;
    }

    return this.getStatus();
  }

  dispose(): void {
    void this.bridge.disconnect();
  }

  getActiveCanvas(): HTMLCanvasElement {
    return this.dummyCanvas;
  }

  getStatus(): PreviewHostStatus {
    return {
      runtime: this.runtime,
      requestedBackend: this.requestedBackend,
      resolvedBackend: (this.resolvedBackend || this.requestedBackend) as PreviewBackend,
      rendererLabel: this.getRendererLabel(),
      compileStatus: this.compileStatus,
      compileMessage: this.compileMessage,
      capabilities: this.getCapabilities(),
    };
  }

  async setDocument(document: PreviewDocument): Promise<PreviewHostStatus> {
    this.currentDocument = { ...document };

    if (document.language !== 'wgsl') {
      this.compileStatus = 'offline';
      this.compileMessage = `Native preview currently only supports WGSL. Got ${document.language.toUpperCase()}.`;
      return this.getStatus();
    }

    try {
      await this.bridge.send({ type: 'set_shader', source: document.source });
      this.compileStatus = 'ready';
      this.compileMessage = '';
    } catch (err) {
      this.compileStatus = 'error';
      this.compileMessage = `Failed to send shader to sidecar: ${err}`;
    }

    return this.getStatus();
  }

  async setBackend(backend: PreviewBackend): Promise<PreviewHostStatus> {
    this.requestedBackend = backend;

    try {
      await this.bridge.send({ type: 'set_backend', backend });
    } catch {
      // The sidecar may respond with backend_change_required
    }

    return this.getStatus();
  }

  setRenderPreset(preset: RenderPreset): void {
    this.dummyCanvas.width = preset.width;
    this.dummyCanvas.height = preset.height;

    void this.bridge
      .send({ type: 'set_resolution', width: preset.width, height: preset.height })
      .catch(() => {});
  }

  updateStageSize(_containerWidth: number, _containerHeight: number): void {
    // Native preview renders in its own window; no webview scaling needed
  }

  getPointerPosition(_event: MouseEvent): { x: number; y: number } | null {
    // Mouse input goes through the sidecar's own window
    return null;
  }

  usesKeyboard(): boolean {
    return false;
  }

  render(_uniforms: Uniforms, _pressedKeys: Set<number>): void {
    // No-op: the native sidecar renders independently at its own frame rate
  }

  getNativeFps(): number {
    return this.nativeFps;
  }

  getNativeFrame(): number {
    return this.nativeFrame;
  }

  private handleSidecarEvent(event: NativePreviewEvent): void {
    switch (event.type) {
      case 'started':
        this.resolvedBackend = event.resolved_backend;
        this.adapterName = event.adapter;
        this.compileStatus = 'ready';
        this.compileMessage = '';
        this.statusEl.textContent = `Native preview: ${event.adapter} (${event.resolved_backend})`;
        this.emitDiagnostic(
          'success',
          `Native preview started on ${event.adapter} (${event.resolved_backend})`,
        );
        break;

      case 'stats':
        this.nativeFps = event.fps;
        this.nativeFrame = event.frame;
        break;

      case 'shader_updated':
        if (event.success) {
          this.compileStatus = 'ready';
          this.compileMessage = '';
          this.emitDiagnostic('success', 'Shader compiled successfully (native)');
        }
        break;

      case 'diagnostic':
        if (event.level === 'error') {
          this.compileStatus = 'error';
          this.compileMessage = event.message;
        }
        this.emitDiagnostic(event.level, event.message);
        break;

      case 'backend_change_required':
        this.emitDiagnostic('warning', event.message);
        break;

      case 'pong':
        break;
    }
  }

  private emitDiagnostic(level: string, message: string): void {
    for (const listener of this.diagnosticListeners) {
      listener(level, message);
    }
  }

  private getRendererLabel(): string {
    if (this.resolvedBackend) {
      return `Native (${this.resolvedBackend})`;
    }
    return 'Native';
  }

  private getCapabilities(): PreviewCapabilities {
    return {
      runtime: 'native',
      availableBackends: NATIVE_BACKENDS,
      defaultBackend: 'auto',
      unavailableBackends: {
        webgl2: 'WebGL2 is a browser-only backend.',
        webgpu: 'WebGPU is a browser-only backend.',
      },
      adapterName: this.adapterName || undefined,
    };
  }
}
