import type { PreviewBackend, PreviewRuntime } from './shader.js';

export type NativePreviewCommand =
  | { type: 'ping' }
  | { type: 'shutdown' }
  | { type: 'set_shader'; source: string }
  | { type: 'set_resolution'; width: number; height: number }
  | { type: 'set_backend'; backend: PreviewBackend };

export type NativePreviewEvent =
  | {
      type: 'started';
      requested_backend: PreviewBackend | string;
      resolved_backend: string;
      adapter: string;
      device: string;
      runtime?: PreviewRuntime;
    }
  | {
      type: 'stats';
      fps: number;
      frame_time_ms: number;
      frame: number;
    }
  | {
      type: 'diagnostic';
      level: 'info' | 'success' | 'warning' | 'error';
      message: string;
    }
  | { type: 'pong' }
  | { type: 'shader_updated'; success: boolean }
  | { type: 'backend_change_required'; message: string };
