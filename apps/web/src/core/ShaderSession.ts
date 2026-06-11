import { parseMultiPassShader, serializeMultiPassShader, type ParsedPass } from '../renderer/shaderParser.js';
import type { PreviewDocument, ShaderLanguage } from '../types/shader.js';

export interface ShaderFileRef {
  provider: string;
  filename: string;
}

export interface ShaderSessionSnapshot {
  document: PreviewDocument;
  currentFile: ShaderFileRef | null;
  isMultiPass: boolean;
  passNames: string[];
  activePassIndex: number;
  activeSource: string;
}

export class ShaderSession {
  private document: PreviewDocument = { source: '', language: 'glsl' };
  private currentFile: ShaderFileRef | null = null;
  private parsedPasses: ParsedPass[] = [];
  private isMultiPass = false;
  private activePassIndex = 0;

  load(document: PreviewDocument) {
    this.document = { ...document };
    this.reparse(document.source);
  }

  setLanguage(language: ShaderLanguage) {
    this.document.language = language;
  }

  setCurrentFile(file: ShaderFileRef | null) {
    this.currentFile = file;
  }

  updateActiveSource(source: string) {
    if (this.isMultiPass && this.parsedPasses[this.activePassIndex]) {
      this.parsedPasses[this.activePassIndex].source = source;
      this.document.source = serializeMultiPassShader(this.parsedPasses);
      return;
    }

    this.document.source = source;
  }

  selectPass(index: number): string {
    this.activePassIndex = Math.max(0, Math.min(index, this.parsedPasses.length - 1));
    return this.getActiveSource();
  }

  getActiveSource(): string {
    if (!this.isMultiPass) {
      return this.document.source;
    }
    return this.parsedPasses[this.activePassIndex]?.source || this.document.source;
  }

  getSerializedSource(): string {
    return this.document.source;
  }

  getSnapshot(): ShaderSessionSnapshot {
    return {
      document: { ...this.document },
      currentFile: this.currentFile ? { ...this.currentFile } : null,
      isMultiPass: this.isMultiPass,
      passNames: this.parsedPasses.map((pass) => pass.name),
      activePassIndex: this.activePassIndex,
      activeSource: this.getActiveSource(),
    };
  }

  private reparse(source: string) {
    const parsed = parseMultiPassShader(source);
    this.isMultiPass = parsed.multiPass;
    this.parsedPasses = parsed.passes;
    this.activePassIndex = 0;
  }
}
