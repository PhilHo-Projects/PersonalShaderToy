import type { ShaderContent, ShaderFile, ShaderListing } from '../types/shader.js';
import type { ShaderLibraryChangeEvent, ShaderLibraryService } from './ShaderLibraryService.js';

const OVERRIDE_PREFIX = 'pst:shader:';

function overrideKey(provider: string, filename: string): string {
  return `${OVERRIDE_PREFIX}${provider}/${filename}`;
}

/**
 * Production shader library. The corpus is baked into the build, so there is no
 * server: listing and loading are plain fetches, and a visitor's edits live only
 * in their own browser.
 */
export class StaticShaderLibraryService implements ShaderLibraryService {
  private manifest: ShaderListing | null = null;

  constructor(private base = '/shaders') {}

  async list(): Promise<ShaderListing> {
    const res = await fetch(`${this.base}/manifest.json`);
    if (!res.ok) {
      throw new Error(`Shader manifest unavailable (${res.status})`);
    }
    this.manifest = await res.json() as ShaderListing;
    return this.manifest;
  }

  async load(provider: string, filename: string): Promise<ShaderContent> {
    const meta = await this.findMeta(provider, filename);

    const override = this.readOverride(provider, filename);
    if (override !== null) {
      return { ...meta, content: override };
    }

    // Shader filenames contain spaces; both segments must be escaped.
    const url = `${this.base}/${encodeURIComponent(provider)}/${encodeURIComponent(filename)}`;
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error(`Shader unavailable (${res.status})`);
    }
    return { ...meta, content: await res.text() };
  }

  /** Persists to the visitor's browser only. Throws on quota so the caller can report it. */
  async save(provider: string, filename: string, content: string): Promise<void> {
    localStorage.setItem(overrideKey(provider, filename), content);
  }

  /** No file watching in production; the corpus is immutable for the life of the deploy. */
  watch(_onEvent: (event: ShaderLibraryChangeEvent) => void): () => void {
    return () => {};
  }

  hasOverride(provider: string, filename: string): boolean {
    return this.readOverride(provider, filename) !== null;
  }

  clearOverride(provider: string, filename: string): void {
    try {
      localStorage.removeItem(overrideKey(provider, filename));
    } catch {
      // A browser refusing storage has no override to clear.
    }
  }

  private readOverride(provider: string, filename: string): string | null {
    try {
      return localStorage.getItem(overrideKey(provider, filename));
    } catch {
      // Private browsing can throw on access rather than returning null.
      return null;
    }
  }

  private async findMeta(provider: string, filename: string): Promise<ShaderFile> {
    if (!this.manifest) {
      await this.list();
    }

    const entry = this.manifest?.[provider]?.find((f) => f.name === filename);
    if (entry) {
      return entry;
    }

    const ext = filename.split('.').pop()?.toLowerCase();
    return {
      name: filename,
      provider,
      type: ext === 'wgsl' ? 'wgsl' : ext === 'hlsl' ? 'hlsl' : 'glsl',
      modified: Date.now(),
    };
  }
}
