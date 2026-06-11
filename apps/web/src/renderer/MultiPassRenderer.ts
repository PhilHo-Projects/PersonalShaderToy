import type { Uniforms } from './uniforms.js';
import type { ParsedPass } from './shaderParser.js';
import { RenderTarget } from './RenderTarget.js';

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

uniform float     iTime;
uniform float     iTimeDelta;
uniform vec3      iResolution;
uniform vec3      iChannelResolution[4];
uniform vec4      iMouse;
uniform int       iFrame;
uniform vec4      iDate;
uniform sampler2D iChannel0;
uniform sampler2D iChannel1;
uniform sampler2D iChannel2;
uniform sampler2D iChannel3;

out vec4 outColor;
`;

const SHADERTOY_FOOTER = `
void main() {
  vec4 color = vec4(0.0);
  mainImage(color, gl_FragCoord.xy);
  outColor = color;
}`;

export interface CompileResult {
  success: boolean;
  errors?: string;
  passName?: string; // which pass failed
}

interface PassProgram {
  name: string;
  program: WebGLProgram;
  locs: Map<string, WebGLUniformLocation | null>;
  channels: (string | null)[]; // pass names for iChannel0-3
  renderTarget: RenderTarget | null; // null = render to screen (Image pass)
}

export class MultiPassRenderer {
  private gl: WebGL2RenderingContext;
  private vao: WebGLVertexArrayObject;
  private vsShader: WebGLShader | null = null;
  private passes: PassProgram[] = [];
  private renderTargets = new Map<string, RenderTarget>();
  private width = 0;
  private height = 0;
  private keyboardTexture: WebGLTexture | null = null;
  private hasKeyboardChannel = false;
  private keyboardData = new Uint8Array(256 * 3 * 4); // 256 wide, 3 rows, RGBA

  constructor(public canvas: HTMLCanvasElement) {
    const gl = canvas.getContext('webgl2', { antialias: false, preserveDrawingBuffer: false });
    if (!gl) throw new Error('WebGL2 not supported');
    this.gl = gl;

    // Enable float textures (needed for RGBA32F render targets)
    const ext = gl.getExtension('EXT_color_buffer_float');
    if (!ext) console.warn('EXT_color_buffer_float not available — multi-pass may not work');

    // Enable linear filtering of float textures (needed for texture() on RGBA32F)
    const floatLinear = gl.getExtension('OES_texture_float_linear');
    if (!floatLinear) console.warn('OES_texture_float_linear not available — texture sampling on float buffers may fail');

    this.vao = gl.createVertexArray()!;

    // Compile vertex shader once (shared by all passes)
    this.vsShader = gl.createShader(gl.VERTEX_SHADER)!;
    gl.shaderSource(this.vsShader, VERTEX_SHADER);
    gl.compileShader(this.vsShader);
  }

  /** Update keyboard texture data from external key state tracking. */
  updateKeyboard(keys: Set<number>) {
    // Row 0: current key state
    for (let i = 0; i < 256; i++) {
      this.keyboardData[i * 4] = keys.has(i) ? 255 : 0;
    }
  }

  get usesKeyboard(): boolean {
    return this.hasKeyboardChannel;
  }

  compile(parsedPasses: ParsedPass[]): CompileResult {
    const gl = this.gl;

    // Compile all passes first before committing any state
    const newPrograms: { name: string; program: WebGLProgram; channels: (string | null)[] }[] = [];

    for (const pass of parsedPasses) {
      const result = this.compilePass(pass);
      if (!result.program) {
        // Clean up any programs we already compiled in this batch
        for (const p of newPrograms) gl.deleteProgram(p.program);
        return { success: false, errors: result.errors, passName: pass.name };
      }
      newPrograms.push({ name: pass.name, program: result.program, channels: pass.channels });
    }

    // All passes compiled successfully — commit
    this.disposePasses();

    // Create render targets for buffer passes
    const w = gl.drawingBufferWidth || 1;
    const h = gl.drawingBufferHeight || 1;
    this.width = w;
    this.height = h;

    // Detect if any pass uses keyboard input
    this.hasKeyboardChannel = newPrograms.some(p =>
      p.channels.some(c => c?.toLowerCase() === 'keyboard')
    );

    // Create keyboard texture if needed
    if (this.hasKeyboardChannel && !this.keyboardTexture) {
      this.keyboardTexture = gl.createTexture()!;
      gl.bindTexture(gl.TEXTURE_2D, this.keyboardTexture);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, 256, 3, 0, gl.RGBA, gl.UNSIGNED_BYTE, this.keyboardData);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.bindTexture(gl.TEXTURE_2D, null);
    }

    this.passes = newPrograms.map(({ name, program, channels }) => {
      const isImage = name.toLowerCase() === 'image';
      let rt: RenderTarget | null = null;

      if (!isImage) {
        rt = new RenderTarget(gl, w, h);
        this.renderTargets.set(name, rt);
      }

      // Cache uniform locations
      const locs = new Map<string, WebGLUniformLocation | null>();
      for (const uname of ['iTime', 'iTimeDelta', 'iResolution', 'iChannelResolution',
                           'iMouse', 'iFrame', 'iDate',
                           'iChannel0', 'iChannel1', 'iChannel2', 'iChannel3']) {
        locs.set(uname, gl.getUniformLocation(program, uname));
      }

      return { name, program, locs, channels, renderTarget: rt };
    });

    return { success: true };
  }

  private compilePass(pass: ParsedPass): { program: WebGLProgram | null; errors?: string } {
    const gl = this.gl;
    const source = pass.source;

    // Detect if standalone
    const hasVersion = /^#version/m.test(source);
    const hasMain = /void\s+main\s*\(/m.test(source);
    const hasMainImage = /void\s+mainImage\s*\(/m.test(source);

    let fragSource: string;
    if (hasVersion && hasMain) {
      fragSource = source;
    } else if (hasMainImage) {
      fragSource = UNIFORM_HEADER + '\n' + source + '\n' + SHADERTOY_FOOTER;
    } else if (hasMain && !hasVersion) {
      fragSource = UNIFORM_HEADER + '\n' + source;
    } else {
      fragSource = UNIFORM_HEADER + '\n' + source + '\n' + SHADERTOY_FOOTER;
    }

    // Compile fragment shader
    const fs = gl.createShader(gl.FRAGMENT_SHADER)!;
    gl.shaderSource(fs, fragSource);
    gl.compileShader(fs);
    if (!gl.getShaderParameter(fs, gl.COMPILE_STATUS)) {
      let err = gl.getShaderInfoLog(fs) || 'Fragment shader error';
      gl.deleteShader(fs);
      if (!hasVersion) {
        const headerLines = UNIFORM_HEADER.split('\n').length;
        err = adjustLineNumbers(err, headerLines);
      }
      return { program: null, errors: `[${pass.name}] ${err}` };
    }

    // Link program
    const prog = gl.createProgram()!;
    gl.attachShader(prog, this.vsShader!);
    gl.attachShader(prog, fs);
    gl.linkProgram(prog);
    gl.deleteShader(fs);

    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
      const err = gl.getProgramInfoLog(prog) || 'Link error';
      gl.deleteProgram(prog);
      return { program: null, errors: `[${pass.name}] ${err}` };
    }

    return { program: prog };
  }

  render(u: Uniforms) {
    const gl = this.gl;
    if (this.passes.length === 0) return;


    // Resize render targets if canvas size changed
    const w = gl.drawingBufferWidth;
    const h = gl.drawingBufferHeight;
    if (w !== this.width || h !== this.height) {
      this.width = w;
      this.height = h;
      for (const rt of this.renderTargets.values()) {
        rt.resize(w, h);
      }
    }

    gl.bindVertexArray(this.vao);

    for (const pass of this.passes) {
      // Bind render target (or screen)
      if (pass.renderTarget) {
        gl.bindFramebuffer(gl.FRAMEBUFFER, pass.renderTarget.writeFramebuffer);
        gl.viewport(0, 0, pass.renderTarget.width, pass.renderTarget.height);
      } else {
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        gl.viewport(0, 0, w, h);
      }

      gl.useProgram(pass.program);

      // Bind standard uniforms
      const loc = (n: string) => pass.locs.get(n) ?? null;
      gl.uniform1f(loc('iTime'), u.iTime);
      gl.uniform1f(loc('iTimeDelta'), u.iTimeDelta);
      gl.uniform3fv(loc('iResolution'), u.iResolution);
      gl.uniform4fv(loc('iMouse'), u.iMouse);
      gl.uniform1i(loc('iFrame'), u.iFrame);
      gl.uniform4fv(loc('iDate'), u.iDate);

      // Set iChannelResolution — all buffer targets share canvas resolution
      const chanResLoc = loc('iChannelResolution');
      if (chanResLoc) {
        const res = u.iResolution;
        // keyboard texture is 256x3, buffer textures are canvas-sized
        const chanRes = new Float32Array(12);
        for (let i = 0; i < 4; i++) {
          const cn = pass.channels[i];
          if (cn && cn.toLowerCase() === 'keyboard') {
            chanRes[i * 3] = 256;
            chanRes[i * 3 + 1] = 3;
            chanRes[i * 3 + 2] = 1;
          } else {
            chanRes[i * 3] = res[0];
            chanRes[i * 3 + 1] = res[1];
            chanRes[i * 3 + 2] = 1;
          }
        }
        gl.uniform3fv(chanResLoc, chanRes);
      }

      // Bind channel textures
      for (let i = 0; i < 4; i++) {
        const chanName = pass.channels[i];
        if (!chanName) continue;

        gl.activeTexture(gl.TEXTURE0 + i);

        if (chanName.toLowerCase() === 'keyboard') {
          // Upload latest keyboard state and bind
          if (this.keyboardTexture) {
            gl.bindTexture(gl.TEXTURE_2D, this.keyboardTexture);
            gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, 256, 3, gl.RGBA, gl.UNSIGNED_BYTE, this.keyboardData);
          }
          gl.uniform1i(loc(`iChannel${i}`), i);
          continue;
        }

        const rt = this.renderTargets.get(chanName);
        if (!rt) continue;

        gl.bindTexture(gl.TEXTURE_2D, rt.readTexture);
        gl.uniform1i(loc(`iChannel${i}`), i);
      }


      // Draw
      gl.drawArrays(gl.TRIANGLES, 0, 3);


      // Swap ping-pong for buffer passes
      if (pass.renderTarget) {
        pass.renderTarget.swap();
      }

      // Unbind textures
      for (let i = 0; i < 4; i++) {
        gl.activeTexture(gl.TEXTURE0 + i);
        gl.bindTexture(gl.TEXTURE_2D, null);
      }
    }
  }

  private disposePasses() {
    const gl = this.gl;
    for (const pass of this.passes) {
      gl.deleteProgram(pass.program);
    }
    for (const rt of this.renderTargets.values()) {
      rt.dispose();
    }
    this.passes = [];
    this.renderTargets.clear();
  }

  dispose() {
    this.disposePasses();
    if (this.vsShader) {
      this.gl.deleteShader(this.vsShader);
      this.vsShader = null;
    }
    if (this.keyboardTexture) {
      this.gl.deleteTexture(this.keyboardTexture);
      this.keyboardTexture = null;
    }
  }

  get passCount(): number {
    return this.passes.length;
  }
}

function adjustLineNumbers(errorLog: string, offset: number): string {
  return errorLog.replace(/ERROR:\s*\d+:(\d+)/g, (_match, line) => {
    const adjusted = Math.max(1, parseInt(line) - offset);
    return `ERROR: 0:${adjusted}`;
  });
}
