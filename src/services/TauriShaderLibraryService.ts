import type { ShaderContent, ShaderListing } from '../types/shader.js';
import type { ShaderLibraryChangeEvent, ShaderLibraryService } from './ShaderLibraryService.js';

export class TauriShaderLibraryService implements ShaderLibraryService {
  async list(): Promise<ShaderListing> {
    const { invoke } = await import('@tauri-apps/api/core');
    return await invoke<ShaderListing>('list_shaders');
  }

  async load(provider: string, filename: string): Promise<ShaderContent> {
    const { invoke } = await import('@tauri-apps/api/core');
    return await invoke<ShaderContent>('load_shader', { provider, filename });
  }

  async save(provider: string, filename: string, content: string): Promise<void> {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('save_shader', { provider, filename, content });
  }

  watch(onEvent: (event: ShaderLibraryChangeEvent) => void): () => void {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void import('@tauri-apps/api/event')
      .then(({ listen }) => listen<ShaderLibraryChangeEvent>('shader-library-event', (event) => {
        if (!disposed) {
          onEvent(event.payload);
        }
      }))
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      if (unlisten) {
        unlisten();
      }
    };
  }
}
