import type { ShaderContent, ShaderListing } from '../types/shader.js';

export type ShaderLibraryEventType = 'file-added' | 'file-changed' | 'file-removed';

export interface ShaderLibraryChangeEvent {
  type: ShaderLibraryEventType;
  provider: string;
  filename: string;
}

export interface ShaderLibraryService {
  list(): Promise<ShaderListing>;
  load(provider: string, filename: string): Promise<ShaderContent>;
  save(provider: string, filename: string, content: string): Promise<void>;
  watch(onEvent: (event: ShaderLibraryChangeEvent) => void): () => void;
}
