import type { Uniforms } from './uniforms.js';

const VERTEX_SHADER = `#version 300 es
void main() {
  vec2 pos = vec2(
    gl_VertexID == 1 ? 3.0 : -1.0,
    gl_VertexID == 2 ? 3.0 : -1.0
  );
  gl_Position = vec4(pos, 0.0, 1.0);
}`;

const UNIFORM_HEADER = `#version 300 es
precision highp float;

uniform float iTime;
uniform vec3  iResolution;
uniform vec4  iMouse;
uniform int   iFrame;
uniform vec4  iDate;

out vec4 outColor;
`;

const SHADERTOY_FOOTER = `
void main() {
  vec4 color = vec4(0.0);
  mainImage(color, gl_FragCoord.xy);
  outColor = color;
}`;

const STANDALONE_FOOTER = '';

export interface CompileResult {
  success: boolean;
  errors?: string;
}

export class WebGL2Renderer {
  private gl: WebGL2RenderingContext;
  private program: WebGLProgram | null = null;
  private vao: WebGLVertexArrayObject;
  private locs = new Map<string, WebGLUniformLocation | null>();

  constructor(public canvas: HTMLCanvasElement) {
    const gl = canvas.getContext('webgl2', { antialias: false, preserveDrawingBuffer: false });
    if (!gl) throw new Error('WebGL2 not supported');
    this.gl = gl;
    this.vao = gl.createVertexArray()!;
  }

  compile(source: string): CompileResult {
    const gl = this.gl;

    // Detect if standalone (has its own main and #version)
    const hasVersion = /^#version/m.test(source);
    const hasMain = /void\s+main\s*\(/m.test(source);
    const hasMainImage = /void\s+mainImage\s*\(/m.test(source);

    let fragSource: string;
    if (hasVersion && hasMain) {
      // Standalone shader — use as-is, but ensure it has an output
      fragSource = source;
    } else if (hasMainImage) {
      // ShaderToy-style: wrap with uniforms and main
      fragSource = UNIFORM_HEADER + '\n' + source + '\n' + SHADERTOY_FOOTER;
    } else if (hasMain && !hasVersion) {
      // Has main but no version — prepend header
      fragSource = UNIFORM_HEADER + '\n' + source;
    } else {
      // Assume ShaderToy-style
      fragSource = UNIFORM_HEADER + '\n' + source + '\n' + SHADERTOY_FOOTER;
    }

    // Compile vertex shader
    const vs = gl.createShader(gl.VERTEX_SHADER)!;
    gl.shaderSource(vs, VERTEX_SHADER);
    gl.compileShader(vs);
    if (!gl.getShaderParameter(vs, gl.COMPILE_STATUS)) {
      const err = gl.getShaderInfoLog(vs) || 'Vertex shader error';
      gl.deleteShader(vs);
      return { success: false, errors: err };
    }

    // Compile fragment shader
    const fs = gl.createShader(gl.FRAGMENT_SHADER)!;
    gl.shaderSource(fs, fragSource);
    gl.compileShader(fs);
    if (!gl.getShaderParameter(fs, gl.COMPILE_STATUS)) {
      let err = gl.getShaderInfoLog(fs) || 'Fragment shader error';
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      // Adjust line numbers if we wrapped the shader
      if (!hasVersion) {
        const headerLines = UNIFORM_HEADER.split('\n').length;
        err = adjustLineNumbers(err, headerLines);
      }
      return { success: false, errors: err };
    }

    // Link program
    const prog = gl.createProgram()!;
    gl.attachShader(prog, vs);
    gl.attachShader(prog, fs);
    gl.linkProgram(prog);
    gl.deleteShader(vs);
    gl.deleteShader(fs);

    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
      const err = gl.getProgramInfoLog(prog) || 'Link error';
      gl.deleteProgram(prog);
      return { success: false, errors: err };
    }

    // Success — swap program
    if (this.program) gl.deleteProgram(this.program);
    this.program = prog;

    // Cache uniform locations
    this.locs.clear();
    for (const name of ['iTime', 'iResolution', 'iMouse', 'iFrame', 'iDate']) {
      this.locs.set(name, gl.getUniformLocation(prog, name));
    }

    return { success: true };
  }

  render(u: Uniforms) {
    const gl = this.gl;
    if (!this.program) return;

    gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
    gl.useProgram(this.program);

    const loc = (n: string) => this.locs.get(n) ?? null;
    gl.uniform1f(loc('iTime'), u.iTime);
    gl.uniform3fv(loc('iResolution'), u.iResolution);
    gl.uniform4fv(loc('iMouse'), u.iMouse);
    gl.uniform1i(loc('iFrame'), u.iFrame);
    gl.uniform4fv(loc('iDate'), u.iDate);

    gl.bindVertexArray(this.vao);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  dispose() {
    if (this.program) {
      this.gl.deleteProgram(this.program);
      this.program = null;
    }
  }
}

function adjustLineNumbers(errorLog: string, offset: number): string {
  return errorLog.replace(/ERROR:\s*\d+:(\d+)/g, (_match, line) => {
    const adjusted = Math.max(1, parseInt(line) - offset);
    return `ERROR: 0:${adjusted}`;
  });
}
