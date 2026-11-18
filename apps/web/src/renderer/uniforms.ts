export interface Uniforms {
  iTime: number;
  iTimeDelta: number;
  iFrame: number;
  iResolution: [number, number, number];
  iMouse: [number, number, number, number];
  iDate: [number, number, number, number];
}
export interface ParsedPass {
  name: string;
  source: string;
  channels: (string | null)[];
  line: number;
}
export interface PreparedPass {
  name: string;
  channels: (string | null)[];
  wgsl: string;
  entry: string;
  legacy: boolean;
}
export interface Player {
  canvas: HTMLCanvasElement;
  render(u: Uniforms): void;
  updateKeyboard(keys: Set<number>): void;
  reset(): void;
  dispose(): void;
  readonly timings: number[] | null;
}
export type Language = "glsl" | "wgsl" | "hlsl";
export interface ShaderFile {
  title: string;
  source: string;
  language: Language;
  id?: string;
  passes?: ParsedPass[];
  prepared?: PreparedPass[];
  hlsl?: string[];
}
export interface CatalogEntry {
  id: string;
  title: string;
  language: Language;
  path: string;
  credit: string;
}
