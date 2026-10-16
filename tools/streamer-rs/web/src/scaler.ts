// Scales a VideoFrame on the GPU to BT.601 full range I420, so only the
// scaled picture is read back.

const VERTEX = `#version 300 es
void main() {
  vec2 corner = vec2(gl_VertexID & 1, gl_VertexID >> 1);
  gl_Position = vec4(corner * 4.0 - 1.0, 0.0, 1.0);
}`;

// Each output texel packs four consecutive bytes of the I420 picture.
const FRAGMENT = `#version 300 es
precision highp float;
precision highp int;
uniform sampler2D source;
uniform ivec2 size;
uniform int columns;
uniform float lod;
out vec4 color;

float component(int index) {
  int w = size.x, h = size.y, luma = w * h, cw = w / 2, chroma = luma / 4;
  if (index < luma) {
    vec2 at = (vec2(index % w, index / w) + 0.5) / vec2(size);
    vec3 rgb = textureLod(source, at, lod).rgb;
    return dot(rgb, vec3(0.299, 0.587, 0.114));
  }
  index -= luma;
  int plane = index / chroma;
  if (plane > 1) return 0.0;
  index -= plane * chroma;
  vec2 at = (vec2(index % cw, index / cw) + 0.5) / vec2(cw, h / 2);
  vec3 rgb = textureLod(source, at, lod + 1.0).rgb;
  vec3 weights = plane == 0 ? vec3(-0.168736, -0.331264, 0.5) : vec3(0.5, -0.418688, -0.081312);
  return dot(rgb, weights) + 128.0 / 255.0;
}

void main() {
  ivec2 p = ivec2(gl_FragCoord.xy);
  int base = (p.y * columns + p.x) * 4;
  color = vec4(component(base), component(base + 1), component(base + 2), component(base + 3));
}`;

const POLL_MS = 1;

export class Scaler {
  private readonly source: WebGLTexture;
  private target: WebGLTexture | null = null;
  private readonly framebuffer: WebGLFramebuffer;
  private readonly pixels: WebGLBuffer;
  private readonly uniforms: Record<"size" | "columns" | "lod", WebGLUniformLocation>;
  private targetSize: [number, number] = [0, 0];

  private constructor(private readonly gl: WebGL2RenderingContext) {
    const program = gl.createProgram();
    for (const [type, text] of [
      [gl.VERTEX_SHADER, VERTEX],
      [gl.FRAGMENT_SHADER, FRAGMENT],
    ] as const) {
      const shader = gl.createShader(type)!;
      gl.shaderSource(shader, text);
      gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) ?? "shader");
      gl.attachShader(program, shader);
    }
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program) ?? "program");
    gl.useProgram(program);
    const uniform = (name: string) => gl.getUniformLocation(program, name)!;
    this.uniforms = { size: uniform("size"), columns: uniform("columns"), lod: uniform("lod") };

    this.source = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, this.source);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

    this.framebuffer = gl.createFramebuffer();
    this.pixels = gl.createBuffer();
  }

  /** None where WebGL 2 is missing or the shaders fail. */
  static create(): Scaler | null {
    const gl = new OffscreenCanvas(1, 1).getContext("webgl2", { antialias: false, depth: false });
    if (!gl) return null;
    try {
      return new Scaler(gl);
    } catch (err) {
      console.error("GPU scaler:", err);
      return null;
    }
  }

  /** `frame` as I420 at `width` x `height`, both even, written into the view
   * `into` returns for the picture's size in bytes. */
  async scale(frame: VideoFrame, width: number, height: number, into: (size: number) => Uint8Array): Promise<void> {
    const gl = this.gl;
    const columns = width / 2;
    const rows = Math.ceil((width * height * 3) / 2 / (columns * 4));
    this.resize(columns, rows);

    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.source);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, frame);
    gl.generateMipmap(gl.TEXTURE_2D);
    const ratio = Math.max(frame.displayWidth / width, frame.displayHeight / height);
    gl.uniform2i(this.uniforms.size, width, height);
    gl.uniform1i(this.uniforms.columns, columns);
    gl.uniform1f(this.uniforms.lod, Math.max(0, Math.log2(ratio)));

    gl.bindFramebuffer(gl.FRAMEBUFFER, this.framebuffer);
    gl.viewport(0, 0, columns, rows);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.bindBuffer(gl.PIXEL_PACK_BUFFER, this.pixels);
    gl.readPixels(0, 0, columns, rows, gl.RGBA, gl.UNSIGNED_BYTE, 0);
    const sync = gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0)!;
    gl.flush();
    try {
      while (gl.clientWaitSync(sync, 0, 0) === gl.TIMEOUT_EXPIRED) {
        await new Promise((resolve) => setTimeout(resolve, POLL_MS));
      }
    } finally {
      gl.deleteSync(sync);
    }
    if (gl.isContextLost()) throw new Error("WebGL context lost");
    gl.getBufferSubData(gl.PIXEL_PACK_BUFFER, 0, into((width * height * 3) / 2));
  }

  private resize(columns: number, rows: number) {
    if (this.targetSize[0] === columns && this.targetSize[1] === rows) return;
    const gl = this.gl;
    gl.deleteTexture(this.target);
    this.target = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, this.target);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA8, columns, rows);
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.framebuffer);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, this.target, 0);
    gl.bindBuffer(gl.PIXEL_PACK_BUFFER, this.pixels);
    gl.bufferData(gl.PIXEL_PACK_BUFFER, columns * rows * 4, gl.STREAM_READ);
    this.targetSize = [columns, rows];
  }
}
