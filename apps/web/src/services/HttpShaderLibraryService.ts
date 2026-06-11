import type { ShaderContent, ShaderListing } from '../types/shader.js';
import type { ShaderLibraryChangeEvent, ShaderLibraryService } from './ShaderLibraryService.js';

function getDefaultWsUrl(): string {
  const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws';
  return `${protocol}://${window.location.hostname}:4781`;
}

export class HttpShaderLibraryService implements ShaderLibraryService {
  private socket: WebSocket | null = null;
  private reconnectTimer: number | null = null;
  private listeners = new Set<(event: ShaderLibraryChangeEvent) => void>();

  constructor(
    private apiBase = '/api',
    private wsUrl = getDefaultWsUrl(),
  ) {}

  async list(): Promise<ShaderListing> {
    const res = await fetch(`${this.apiBase}/shaders`);
    return await res.json() as ShaderListing;
  }

  async load(provider: string, filename: string): Promise<ShaderContent> {
    const res = await fetch(`${this.apiBase}/shaders/${provider}/${filename}`);
    return await res.json() as ShaderContent;
  }

  async save(provider: string, filename: string, content: string): Promise<void> {
    await fetch(`${this.apiBase}/shaders/${provider}/${filename}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ content }),
    });
  }

  watch(onEvent: (event: ShaderLibraryChangeEvent) => void): () => void {
    this.listeners.add(onEvent);
    this.ensureSocket();

    return () => {
      this.listeners.delete(onEvent);
      if (this.listeners.size === 0) {
        this.teardownSocket();
      }
    };
  }

  private ensureSocket() {
    if (this.socket || this.listeners.size === 0) {
      return;
    }

    try {
      this.socket = new WebSocket(this.wsUrl);
      this.socket.onmessage = (event) => {
        try {
          const payload = JSON.parse(event.data) as ShaderLibraryChangeEvent;
          for (const listener of this.listeners) {
            listener(payload);
          }
        } catch {
          // Ignore malformed payloads so a single bad event does not break watchers.
        }
      };
      this.socket.onclose = () => {
        this.socket = null;
        if (this.listeners.size > 0) {
          this.scheduleReconnect();
        }
      };
      this.socket.onerror = () => {
        this.socket?.close();
      };
    } catch {
      this.scheduleReconnect();
    }
  }

  private scheduleReconnect() {
    if (this.reconnectTimer !== null) {
      return;
    }

    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      this.ensureSocket();
    }, 3000);
  }

  private teardownSocket() {
    if (this.reconnectTimer !== null) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }

    if (this.socket) {
      this.socket.close();
      this.socket = null;
    }
  }
}
