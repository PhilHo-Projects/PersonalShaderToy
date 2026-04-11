import type { NativePreviewCommand, NativePreviewEvent } from '../types/nativePreview.js';
import type { NativePreviewBridge } from './NativePreviewBridge.js';

export class TauriNativePreviewBridge implements NativePreviewBridge {
  private listeners = new Set<(event: NativePreviewEvent) => void>();
  private unlisten: (() => void) | null = null;
  private connected = false;
  private backend: string | undefined;

  constructor(backend?: string) {
    this.backend = backend;
  }

  async connect(): Promise<void> {
    if (this.connected) return;

    const { invoke } = await import('@tauri-apps/api/core');
    const { listen } = await import('@tauri-apps/api/event');

    // Start listening for sidecar events before spawning
    this.unlisten = await listen<string>('preview-sidecar-event', (event) => {
      try {
        const parsed = JSON.parse(event.payload) as NativePreviewEvent;
        for (const listener of this.listeners) {
          listener(parsed);
        }
      } catch {
        // ignore malformed JSON
      }
    });

    // Spawn the sidecar process via Tauri command
    await invoke('spawn_preview_sidecar', {
      backend: this.backend ?? null,
    });

    this.connected = true;
  }

  async disconnect(): Promise<void> {
    if (!this.connected) return;

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('stop_preview_sidecar');
    } catch {
      // ignore errors during shutdown
    }

    if (this.unlisten) {
      this.unlisten();
      this.unlisten = null;
    }

    this.connected = false;
  }

  async send(command: NativePreviewCommand): Promise<void> {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('send_preview_command', {
      command: JSON.stringify(command),
    });
  }

  onEvent(listener: (event: NativePreviewEvent) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }
}
