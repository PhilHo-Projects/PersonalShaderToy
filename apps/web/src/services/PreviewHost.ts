import type { Uniforms } from '../renderer/uniforms.js';
import type {
  PreviewBackend,
  PreviewCapabilities,
  PreviewCompileStatus,
  PreviewDocument,
  PreviewRuntime,
  RenderPreset,
} from '../types/shader.js';

export interface PreviewHostStatus {
  runtime: PreviewRuntime;
  requestedBackend: PreviewBackend;
  resolvedBackend: PreviewBackend;
  rendererLabel: string;
  compileStatus: PreviewCompileStatus;
  compileMessage: string;
  capabilities: PreviewCapabilities;
}

export interface PreviewHost {
  readonly runtime: PreviewRuntime;
  readonly element: HTMLElement;
  initialize(): Promise<PreviewHostStatus>;
  dispose(): void;
  getActiveCanvas(): HTMLCanvasElement;
  getStatus(): PreviewHostStatus;
  setDocument(document: PreviewDocument): Promise<PreviewHostStatus>;
  setBackend(backend: PreviewBackend): Promise<PreviewHostStatus>;
  setRenderPreset(preset: RenderPreset): void;
  updateStageSize(containerWidth: number, containerHeight: number): void;
  getPointerPosition(event: MouseEvent): { x: number; y: number } | null;
  usesKeyboard(): boolean;
  render(uniforms: Uniforms, pressedKeys: Set<number>): void;
  takeScreenshot(): Promise<string | null>;
}
