import type { Uniforms } from "./uniforms.js";
import type { ParsedPass } from "./uniforms.js";
import { RenderTarget } from "./RenderTarget.js";

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
  private blank: WebGLTexture;
  private vao: WebGLVertexArrayObject;
  private vsShader: WebGLShader | null = null;
  private passes: PassProgram[] = [];
  private renderTargets = new Map<string, RenderTarget>();
  timings: number[] | null = null;
  private timer: { TIME_ELAPSED_EXT: number; GPU_DISJOINT_EXT: number } | null;
  private queries: WebGLQuery[] = [];
  private output: WebGLFramebuffer | null = null;
  private color: WebGLTexture | null = null;
  private width = 0;
  private height = 0;
  private keyboardTexture: WebGLTexture | null = null;
  private hasKeyboardChannel = false;
  private keyboardData = new Uint8Array(256 * 3 * 4); // 256 wide, 3 rows, RGBA

  constructor(public canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", {
      antialias: false,
      preserveDrawingBuffer: true,
    });
    if (!gl) throw new Error("WebGL2 not supported");
    this.gl = gl;
    this.timer = gl.getExtension("EXT_disjoint_timer_query_webgl2");

    this.blank = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, this.blank);
    gl.texImage2D(
      gl.TEXTURE_2D,
      0,
      gl.RGBA,
      1,
      1,
      0,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      new Uint8Array([0, 0, 0, 255]),
    );
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.bindTexture(gl.TEXTURE_2D, null);

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
      const k = i * 4,
        down = keys.has(i) && !this.keyboardData[k];
      this.keyboardData[1024 + k] = down ? 255 : 0;
      if (down) this.keyboardData[2048 + k] ^= 255;
      this.keyboardData[k] = keys.has(i) ? 255 : 0;
    }
  }

  get usesKeyboard(): boolean {
    return this.hasKeyboardChannel;
  }

  compile(parsedPasses: ParsedPass[]): CompileResult {
    const gl = this.gl;

    // Compile all passes first before committing any state
    if (
      parsedPasses.length > 1 &&
      (!gl.getExtension("EXT_color_buffer_float") ||
        !gl.getExtension("OES_texture_float_linear"))
    ) {
      return {
        success: false,
        errors:
          "Floating-point buffer rendering and linear filtering are required for this shader.",
      };
    }
    const newPrograms: {
      name: string;
      program: WebGLProgram;
      channels: (string | null)[];
    }[] = [];

    for (const pass of parsedPasses) {
      const result = this.compilePass(pass);
      if (!result.program) {
        // Clean up any programs we already compiled in this batch
        for (const p of newPrograms) gl.deleteProgram(p.program);
        return { success: false, errors: result.errors, passName: pass.name };
      }
      newPrograms.push({
        name: pass.name,
        program: result.program,
        channels: pass.channels,
      });
    }

    // All passes compiled successfully — commit
    this.disposePasses();

    // Create render targets for buffer passes
    const w = gl.drawingBufferWidth || 1;
    const h = gl.drawingBufferHeight || 1;
    this.width = 0;
    this.height = 0;

    // Detect if any pass uses keyboard input
    this.hasKeyboardChannel = newPrograms.some((p) =>
      p.channels.some((c) => c?.toLowerCase() === "keyboard"),
    );

    // Create keyboard texture if needed
    if (this.hasKeyboardChannel && !this.keyboardTexture) {
      this.keyboardTexture = gl.createTexture()!;
      gl.bindTexture(gl.TEXTURE_2D, this.keyboardTexture);
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA8,
        256,
        3,
        0,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        this.keyboardData,
      );
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.bindTexture(gl.TEXTURE_2D, null);
    }

    this.passes = newPrograms.map(({ name, program, channels }) => {
      const isImage = name.toLowerCase() === "image";
      let rt: RenderTarget | null = null;

      if (!isImage) {
        rt = new RenderTarget(gl, w, h);
        this.renderTargets.set(name, rt);
      }

      // Cache uniform locations
      const locs = new Map<string, WebGLUniformLocation | null>();
      for (const uname of [
        "iTime",
        "iTimeDelta",
        "iResolution",
        "iChannelResolution",
        "iMouse",
        "iFrame",
        "iDate",
        "iChannel0",
        "iChannel1",
        "iChannel2",
        "iChannel3",
      ]) {
        locs.set(uname, gl.getUniformLocation(program, uname));
      }

      return { name, program, locs, channels, renderTarget: rt };
    });

    return { success: true };
  }

  private compilePass(pass: ParsedPass): {
    program: WebGLProgram | null;
    errors?: string;
  } {
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
      fragSource = UNIFORM_HEADER + "\n" + source + "\n" + SHADERTOY_FOOTER;
    } else if (hasMain && !hasVersion) {
      fragSource = UNIFORM_HEADER + "\n" + source;
    } else {
      fragSource = UNIFORM_HEADER + "\n" + source + "\n" + SHADERTOY_FOOTER;
    }

    // Compile fragment shader
    const fs = gl.createShader(gl.FRAGMENT_SHADER)!;
    gl.shaderSource(fs, fragSource);
    gl.compileShader(fs);
    if (!gl.getShaderParameter(fs, gl.COMPILE_STATUS)) {
      let err = gl.getShaderInfoLog(fs) || "Fragment shader error";
      gl.deleteShader(fs);
      if (!hasVersion) {
        const headerLines = UNIFORM_HEADER.split("\n").length;
        err = adjustLineNumbers(err, headerLines - pass.line + 1);
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
      const err = gl.getProgramInfoLog(prog) || "Link error";
      gl.deleteProgram(prog);
      return { program: null, errors: `[${pass.name}] ${err}` };
    }

    return { program: prog };
  }

  render(u: Uniforms) {
    if (this.gl.isContextLost())
      throw Error("WebGL context lost. Select a shader to restart playback.");
    const gl = this.gl;
    if (this.passes.length === 0) return;

    if (this.timer && this.queries.length) {
      const disjoint = gl.getParameter(this.timer.GPU_DISJOINT_EXT);
      if (
        disjoint ||
        this.queries.every((q) =>
          gl.getQueryParameter(q, gl.QUERY_RESULT_AVAILABLE),
        )
      ) {
        this.timings = disjoint
          ? null
          : this.queries.map(
              (q) => gl.getQueryParameter(q, gl.QUERY_RESULT) / 1e6,
            );
        this.queries.forEach((q) => gl.deleteQuery(q));
        this.queries = [];
      }
    }
    const sample = this.timer && !this.queries.length;
    // Resize render targets if canvas size changed
    const w = gl.drawingBufferWidth;
    const h = gl.drawingBufferHeight;
    if (w !== this.width || h !== this.height) {
      this.width = w;
      this.height = h;
      if (!this.output) this.output = gl.createFramebuffer();
      if (!this.color) this.color = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, this.color);
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA8,
        w,
        h,
        0,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        null,
      );
      gl.bindFramebuffer(gl.FRAMEBUFFER, this.output);
      gl.framebufferTexture2D(
        gl.FRAMEBUFFER,
        gl.COLOR_ATTACHMENT0,
        gl.TEXTURE_2D,
        this.color,
        0,
      );
      gl.bindTexture(gl.TEXTURE_2D, null);
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
        gl.bindFramebuffer(gl.FRAMEBUFFER, this.output);
        gl.viewport(0, 0, w, h);
      }

      gl.useProgram(pass.program);

      // Bind standard uniforms
      const loc = (n: string) => pass.locs.get(n) ?? null;
      gl.uniform1f(loc("iTime"), u.iTime);
      gl.uniform1f(loc("iTimeDelta"), u.iTimeDelta);
      gl.uniform3fv(loc("iResolution"), u.iResolution);
      gl.uniform4fv(loc("iMouse"), u.iMouse);
      gl.uniform1i(loc("iFrame"), u.iFrame);
      gl.uniform4fv(loc("iDate"), u.iDate);

      // Set iChannelResolution — all buffer targets share canvas resolution
      const chanResLoc = loc("iChannelResolution");
      if (chanResLoc) {
        const res = u.iResolution;
        // keyboard texture is 256x3, buffer textures are canvas-sized
        const chanRes = new Float32Array(12);
        for (let i = 0; i < 4; i++) {
          const cn = pass.channels[i];
          if (cn && cn.toLowerCase() === "keyboard") {
            chanRes[i * 3] = 256;
            chanRes[i * 3 + 1] = 3;
            chanRes[i * 3 + 2] = 1;
          } else {
            chanRes[i * 3] = cn ? res[0] : 1;
            chanRes[i * 3 + 1] = cn ? res[1] : 1;
            chanRes[i * 3 + 2] = 1;
          }
        }
        gl.uniform3fv(chanResLoc, chanRes);
      }

      // Bind channel textures
      for (let i = 0; i < 4; i++) {
        const chanName = pass.channels[i];
        gl.activeTexture(gl.TEXTURE0 + i);
        if (!chanName) {
          gl.bindTexture(gl.TEXTURE_2D, this.blank);
          gl.uniform1i(loc(`iChannel${i}`), i);
          continue;
        }

        if (chanName.toLowerCase() === "keyboard") {
          // Upload latest keyboard state and bind
          if (this.keyboardTexture) {
            gl.bindTexture(gl.TEXTURE_2D, this.keyboardTexture);
            gl.texSubImage2D(
              gl.TEXTURE_2D,
              0,
              0,
              0,
              256,
              3,
              gl.RGBA,
              gl.UNSIGNED_BYTE,
              this.keyboardData,
            );
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
      const query = sample ? gl.createQuery() : null;
      if (query) gl.beginQuery(this.timer!.TIME_ELAPSED_EXT, query);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      if (query) {
        gl.endQuery(this.timer!.TIME_ELAPSED_EXT);
        this.queries.push(query);
      }

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
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, this.output);
    gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
    gl.blitFramebuffer(0, 0, w, h, 0, 0, w, h, gl.COLOR_BUFFER_BIT, gl.NEAREST);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
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

  reset() {
    this.timings = null;
    this.queries.forEach((q) => this.gl.deleteQuery(q));
    this.queries = [];
    for (const pass of this.passes) {
      if (!pass.renderTarget) continue;
      pass.renderTarget.dispose();
      pass.renderTarget = new RenderTarget(
        this.gl,
        this.canvas.width,
        this.canvas.height,
      );
      this.renderTargets.set(pass.name, pass.renderTarget);
    }
  }

  dispose() {
    this.queries.forEach((q) => this.gl.deleteQuery(q));
    this.gl.deleteFramebuffer(this.output);
    this.gl.deleteTexture(this.color);
    this.gl.deleteVertexArray(this.vao);
    this.gl.deleteTexture(this.blank);
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
