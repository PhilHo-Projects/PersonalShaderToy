import * as monaco from 'monaco-editor';
import type { ShaderType } from '../types/shader.js';

// Configure Monaco workers
self.MonacoEnvironment = {
  getWorker(_workerId: string, _label: string) {
    return new Worker(
      new URL('monaco-editor/esm/vs/editor/editor.worker.js', import.meta.url),
      { type: 'module' }
    );
  },
};

// Register GLSL language
monaco.languages.register({ id: 'glsl' });
monaco.languages.setMonarchTokensProvider('glsl', {
  tokenizer: {
    root: [
      [/\/\/.*$/, 'comment'],
      [/\/\*/, 'comment', '@comment'],
      [/#\w+/, 'keyword.preprocessor'],
      [/\b(void|float|int|uint|bool|vec[234]|ivec[234]|uvec[234]|mat[234]|sampler2D|samplerCube|in|out|inout|uniform|varying|attribute|const|struct|return|if|else|for|while|do|break|continue|discard|precision|highp|mediump|lowp|layout|location)\b/, 'keyword'],
      [/\b(sin|cos|tan|asin|acos|atan|pow|exp|log|sqrt|abs|sign|floor|ceil|fract|mod|min|max|clamp|mix|step|smoothstep|length|distance|dot|cross|normalize|reflect|refract|texture|mainImage)\b/, 'support.function'],
      [/\b\d+\.\d*([eE][-+]?\d+)?\b/, 'number.float'],
      [/\b\d+\b/, 'number'],
      [/[{}()\[\]]/, '@brackets'],
      [/[;,.]/, 'delimiter'],
    ],
    comment: [
      [/[^/*]+/, 'comment'],
      [/\*\//, 'comment', '@pop'],
      [/[/*]/, 'comment'],
    ],
  },
});

// Register WGSL language
monaco.languages.register({ id: 'wgsl' });
monaco.languages.setMonarchTokensProvider('wgsl', {
  tokenizer: {
    root: [
      [/\/\/.*$/, 'comment'],
      [/\/\*/, 'comment', '@comment'],
      [/@\w+/, 'annotation'],
      [/\b(fn|var|let|const|struct|return|if|else|for|while|loop|break|continue|discard|switch|case|default|true|false|array|ptr|uniform|storage|workgroup|private|function)\b/, 'keyword'],
      [/\b(f32|i32|u32|bool|vec[234]|mat[234]x[234]|sampler|texture_2d)\b/, 'type'],
      [/\b(sin|cos|tan|asin|acos|atan2|pow|exp|log|sqrt|abs|sign|floor|ceil|fract|min|max|clamp|mix|step|smoothstep|length|distance|dot|cross|normalize|reflect|textureSample)\b/, 'support.function'],
      [/\b\d+\.\d*([eE][-+]?\d+)?\b/, 'number.float'],
      [/\b\d+[iu]?\b/, 'number'],
      [/[{}()\[\]]/, '@brackets'],
      [/[;,.:><=!&|^~+\-*/%]/, 'delimiter'],
    ],
    comment: [
      [/[^/*]+/, 'comment'],
      [/\*\//, 'comment', '@pop'],
      [/[/*]/, 'comment'],
    ],
  },
});

const LANG_MAP: Record<ShaderType, string> = {
  glsl: 'glsl',
  wgsl: 'wgsl',
  hlsl: 'glsl', // HLSL is close enough to GLSL for highlighting
};

export class ShaderEditor {
  private editor: monaco.editor.IStandaloneCodeEditor;
  private debounceTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    container: HTMLElement,
    private onChange: (source: string) => void,
    private onSave: () => void,
  ) {
    // Define a custom dark theme
    monaco.editor.defineTheme('shader-dark', {
      base: 'vs-dark',
      inherit: true,
      rules: [
        { token: 'keyword', foreground: 'cba6f7', fontStyle: 'bold' },
        { token: 'keyword.preprocessor', foreground: 'f38ba8' },
        { token: 'support.function', foreground: '89b4fa' },
        { token: 'number', foreground: 'fab387' },
        { token: 'number.float', foreground: 'fab387' },
        { token: 'comment', foreground: '6c7086', fontStyle: 'italic' },
        { token: 'type', foreground: 'a6e3a1' },
        { token: 'annotation', foreground: 'f9e2af' },
      ],
      colors: {
        'editor.background': '#1e1e2e',
        'editor.foreground': '#cdd6f4',
        'editor.lineHighlightBackground': '#2d2d44',
        'editorCursor.foreground': '#89b4fa',
        'editor.selectionBackground': '#45475a',
        'editorLineNumber.foreground': '#6c7086',
      },
    });

    this.editor = monaco.editor.create(container, {
      value: '',
      language: 'glsl',
      theme: 'shader-dark',
      minimap: { enabled: false },
      fontSize: 14,
      fontFamily: '"Fira Code", "Cascadia Code", "JetBrains Mono", monospace',
      fontLigatures: true,
      lineNumbers: 'on',
      scrollBeyondLastLine: false,
      automaticLayout: true,
      tabSize: 2,
      renderWhitespace: 'none',
      padding: { top: 12 },
    });

    // Debounced change
    this.editor.onDidChangeModelContent(() => {
      if (this.debounceTimer) clearTimeout(this.debounceTimer);
      this.debounceTimer = setTimeout(() => {
        onChange(this.editor.getValue());
      }, 500);
    });

    // Ctrl+Enter = immediate compile
    this.editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => {
      if (this.debounceTimer) clearTimeout(this.debounceTimer);
      onChange(this.editor.getValue());
    });

    // Ctrl+S = save
    this.editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      onSave();
    });
  }

  getValue(): string {
    return this.editor.getValue();
  }

  setValue(source: string) {
    this.editor.setValue(source);
  }

  setLanguage(type: ShaderType) {
    const model = this.editor.getModel();
    if (model) {
      monaco.editor.setModelLanguage(model, LANG_MAP[type]);
    }
  }

  focus() {
    this.editor.focus();
  }
}
