export type ShaderLanguage = 'glsl' | 'wgsl' | 'hlsl';
export type ShaderType = ShaderLanguage;
export type PreviewRuntime = 'browser' | 'native';
export type PreviewBackend = 'auto' | 'webgl2' | 'webgpu' | 'dx12' | 'vulkan' | 'metal' | 'opengl';

export interface PreviewBackendOption {
  value: PreviewBackend;
  label: string;
  disabled?: boolean;
  reason?: string;
}

export interface ShaderFile {
  name: string;
  provider: string;
  type: ShaderLanguage;
  modified: number;
}

export interface ShaderListing {
  [provider: string]: ShaderFile[];
}

export interface ShaderContent extends ShaderFile {
  content: string;
}

export interface RenderPreset {
  id: string;
  label: string;
  width: number;
  height: number;
}

export interface PreviewConfig {
  runtime: PreviewRuntime;
  backend: PreviewBackend;
  renderPresetId: string;
}

export interface PreviewDocument {
  source: string;
  language: ShaderLanguage;
}

export type PreviewCompileStatus = 'ready' | 'error' | 'offline' | 'transpiling';

export interface PreviewStats {
  runtime: PreviewRuntime;
  backend: PreviewBackend;
  rendererLabel: string;
  resolution: string;
  fps: number;
  frame: number;
  time: number;
  compileStatus: PreviewCompileStatus;
}

export interface PreviewDiagnostic {
  level: 'info' | 'success' | 'warning' | 'error';
  source: 'app' | 'library' | 'preview' | 'native';
  message: string;
  timestamp: number;
}

export interface PreviewCapabilities {
  runtime: PreviewRuntime;
  availableBackends: PreviewBackend[];
  defaultBackend: PreviewBackend;
  unavailableBackends: Partial<Record<PreviewBackend, string>>;
  adapterName?: string;
  deviceName?: string;
}

export interface AppState {
  currentSource: string;
  currentLanguage: ShaderLanguage;
  currentFile: { provider: string; filename: string } | null;
  preview: PreviewConfig;
  isPlaying: boolean;
  startTime: number;
  pausedTime: number;
  frameCount: number;
  mouse: { x: number; y: number; clickX: number; clickY: number; down: boolean };
}

export function detectShaderType(filename: string): ShaderLanguage {
  const ext = filename.split('.').pop()?.toLowerCase();
  if (ext === 'wgsl') return 'wgsl';
  if (ext === 'hlsl') return 'hlsl';
  return 'glsl';
}
