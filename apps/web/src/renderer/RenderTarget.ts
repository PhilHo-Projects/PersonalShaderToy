/**
 * WebGL2 offscreen render target with ping-pong double buffering.
 * Needed for multi-pass rendering where a buffer can read from its own previous frame.
 */

export class RenderTarget {
  private gl: WebGL2RenderingContext;
  private framebuffers: [WebGLFramebuffer, WebGLFramebuffer];
  private textures: [WebGLTexture, WebGLTexture];
  private current = 0; // index of the texture that was LAST WRITTEN (readable)
  width: number;
  height: number;

  constructor(gl: WebGL2RenderingContext, width: number, height: number) {
    this.gl = gl;
    this.width = width;
    this.height = height;

    this.textures = [this.createTexture(width, height), this.createTexture(width, height)];
    this.framebuffers = [this.createFBO(this.textures[0]), this.createFBO(this.textures[1])];
  }

  private createTexture(w: number, h: number): WebGLTexture {
    const gl = this.gl;
    const tex = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, w, h, 0, gl.RGBA, gl.FLOAT, null);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.bindTexture(gl.TEXTURE_2D, null);
    return tex;
  }

  private createFBO(tex: WebGLTexture): WebGLFramebuffer {
    const gl = this.gl;
    const fbo = gl.createFramebuffer()!;
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    return fbo;
  }

  /** Texture to READ from (last completed frame). */
  get readTexture(): WebGLTexture {
    return this.textures[this.current];
  }

  /** Framebuffer to WRITE to (the other one). */
  get writeFramebuffer(): WebGLFramebuffer {
    return this.framebuffers[1 - this.current];
  }

  /** Call after rendering to this target to swap read/write. */
  swap() {
    this.current = 1 - this.current;
  }

  resize(width: number, height: number) {
    if (this.width === width && this.height === height) return;
    this.width = width;
    this.height = height;
    const gl = this.gl;

    for (let i = 0; i < 2; i++) {
      gl.bindTexture(gl.TEXTURE_2D, this.textures[i]);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, width, height, 0, gl.RGBA, gl.FLOAT, null);
    }
    gl.bindTexture(gl.TEXTURE_2D, null);
  }

  dispose() {
    const gl = this.gl;
    for (const fbo of this.framebuffers) gl.deleteFramebuffer(fbo);
    for (const tex of this.textures) gl.deleteTexture(tex);
  }
}
