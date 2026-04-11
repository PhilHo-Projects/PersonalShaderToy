import type { NativePreviewCommand, NativePreviewEvent } from '../types/nativePreview.js';

export interface NativePreviewBridge {
  connect(): Promise<void>;
  disconnect(): Promise<void>;
  send(command: NativePreviewCommand): Promise<void>;
  onEvent(listener: (event: NativePreviewEvent) => void): () => void;
}

export class UnavailableNativePreviewBridge implements NativePreviewBridge {
  async connect(): Promise<void> {}

  async disconnect(): Promise<void> {}

  async send(_command: NativePreviewCommand): Promise<void> {
    throw new Error('Native preview bridge is not wired into the desktop shell yet.');
  }

  onEvent(_listener: (event: NativePreviewEvent) => void): () => void {
    return () => {};
  }
}
