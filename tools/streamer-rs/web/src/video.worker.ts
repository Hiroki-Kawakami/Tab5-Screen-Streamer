// Frames from the screen capture or the player, scaled, encoded and laid out
// for every device. Only the latest frame waits to be sent; older ones are
// dropped. The next frame is scaled while the last one is encoded.

import { drawFrame } from "./draw";
import { Scaler } from "./scaler";
import init, { Video } from "./wasm/streamer.js";
import type { BandJob, FrameMessage, FromVideo, ToVideo, VideoCommand, VideoStatus } from "./messages";

const YUV_FORMATS = new Set(["I420", "I420A", "NV12", "I422", "I422A", "I444", "I444A"]);
/** Frames handed to the device worker and not yet sent, per device. */
const INFLIGHT = 2;
const STATS_PERIOD = 1000;

const ready = init();
let video: Video | null = null;
let preview: OffscreenCanvasRenderingContext2D | null = null;
let device: MessagePort | null = null;
let pending: FrameMessage | null = null;
const inflight = new Map<number, number>();
let ingesting = false;
let encoding = false;
/** A frame was taken since the last one was sent. */
let fresh = false;
let resets = 0;
let timer = 0;
let scaler = Scaler.create();
let stats = { start: performance.now(), frames: 0, bytes: 0, quality: 0 };
const encoders: Worker[] = [];

type Prepared = Omit<BandJob, "row" | "rows"> & { bands: Uint32Array };

const status = (message: VideoStatus) => postMessage(message);

function matrixCode(space: VideoColorSpace): number {
  return space.matrix === "bt709" ? 1 : 6;
}

async function copy(frame: VideoFrame, size: number, options: VideoFrameCopyToOptions): Promise<PlaneLayout[]> {
  for (;;) {
    const data = video!.input(size);
    const layout = await frame.copyTo(data, options);
    // The view is detached, and nothing was copied, if the memory grew meanwhile.
    if (data.byteLength > 0) return layout;
  }
}

async function scaled(frame: VideoFrame, [width, height]: Uint32Array): Promise<boolean> {
  try {
    await scaler!.scale(frame, width, height, (size) => video!.input(size));
    return true;
  } catch (err) {
    scaler = null;
    status({ type: "error", message: `GPU scaling failed, scaling on the CPU: ${err instanceof Error ? err.message : err}` });
    return false;
  }
}

/** Whether `message` became the latest frame, rather than a reset coming
 * first. */
async function take(message: FrameMessage): Promise<boolean> {
  const { frame, display, turn, color } = message;
  const epoch = resets;
  try {
    const rect = frame.visibleRect!;
    const size = scaler && video!.size(...display, turn);
    if (size && (await scaled(frame, size))) {
      if (epoch !== resets) return false;
      const [w, h] = size;
      const planes = new Uint32Array([0, w, w * h, w / 2, (w * h * 5) / 4, w / 2]);
      video!.frameYuv("I420", w, h, planes, 6, true, ...display, turn);
    } else if (frame.format && YUV_FORMATS.has(frame.format)) {
      const layout = await copy(frame, frame.allocationSize({ rect }), { rect });
      if (epoch !== resets) return false;
      const planes = new Uint32Array(layout.flatMap((p) => [p.offset, p.stride]));
      const matrix = color?.matrix ?? matrixCode(frame.colorSpace);
      const fullRange = color?.fullRange ?? frame.colorSpace.fullRange ?? false;
      video!.frameYuv(frame.format, rect.width, rect.height, planes, matrix, fullRange, ...display, turn);
    } else {
      const options = { rect, format: "BGRX", colorSpace: "srgb" } as VideoFrameCopyToOptions;
      await copy(frame, rect.width * rect.height * 4, options);
      if (epoch !== resets) return false;
      video!.frameBgrx(rect.width, rect.height, ...display, turn);
    }
    return true;
  } finally {
    frame.close();
  }
}

function encode({ bands, picture, ...rest }: Prepared): Promise<Uint8Array[]> {
  const count = bands.length / 2;
  while (encoders.length < count) {
    encoders.push(new Worker(new URL("./encoder.worker.ts", import.meta.url), { type: "module" }));
  }
  return Promise.all(
    Array.from({ length: count }, (_, i) => {
      const encoder = encoders[i];
      return new Promise<Uint8Array>((resolve, reject) => {
        encoder.onmessage = (e: MessageEvent<Uint8Array>) => resolve(e.data);
        encoder.onerror = (e) => reject(new Error(`JPEG encoder: ${e.message}`));
        const job: BandJob = { ...rest, picture, row: bands[2 * i], rows: bands[2 * i + 1] };
        encoder.postMessage(job, i === count - 1 ? [picture.buffer] : []);
      });
    }),
  );
}

async function send(now: number, fresh: boolean) {
  const jobs = video!.prepare(now, fresh) as Prepared[] | null;
  if (!jobs) return;
  const jpegs = [];
  for (const job of jobs) jpegs.push(await encode(job));
  const frames = video!.finish(jpegs) as { device: number; transfers: Uint8Array<ArrayBuffer>[] }[];
  if (frames.length === 0) return;
  for (const frame of frames) {
    const buffers = frame.transfers.map((t) => t.buffer);
    inflight.set(frame.device, (inflight.get(frame.device) ?? 0) + 1);
    device!.postMessage({ type: "frame", device: frame.device, transfers: buffers } satisfies FromVideo, buffers);
    stats.bytes += buffers.reduce((n, b) => n + b.byteLength, 0);
  }
  stats.frames += 1;
  stats.quality = video!.quality;
}

function report(now: number) {
  const elapsed = now - stats.start;
  if (elapsed < STATS_PERIOD) return;
  if (stats.frames > 0) {
    const seconds = elapsed / 1000;
    status({ type: "stats", fps: stats.frames / seconds, rate: stats.bytes / seconds, quality: stats.quality });
  }
  stats = { start: now, frames: 0, bytes: 0, quality: 0 };
}

function pump() {
  if (!video) return;
  if (pending && !ingesting) void ingest();
  if (encoding) return;
  clearTimeout(timer);
  const now = performance.now();
  report(now);
  if ([...inflight.values()].some((n) => n >= INFLIGHT) || !(fresh || video.redraw())) return;
  if (!video.ready(now, fresh)) {
    const wait = video.untilDue(now);
    if (wait > 0) timer = setTimeout(pump, wait);
    return;
  }
  void deliver();
}

async function ingest() {
  ingesting = true;
  const message = pending!;
  pending = null;
  try {
    const before = video!.describe();
    if (await take(message)) fresh = true;
    const after = video!.describe();
    if (after && after !== before) status({ type: "layout", text: after });
  } catch (err) {
    status({ type: "error", message: err instanceof Error ? err.message : String(err) });
  } finally {
    ingesting = false;
  }
  pump();
}

async function deliver() {
  encoding = true;
  const all = fresh;
  fresh = false;
  try {
    await send(performance.now(), all);
  } catch (err) {
    status({ type: "error", message: err instanceof Error ? err.message : String(err) });
  } finally {
    encoding = false;
  }
  pump();
}

function fromDevice(message: ToVideo) {
  try {
    apply(message);
  } catch (err) {
    status({ type: "error", message: err instanceof Error ? err.message : String(err) });
  }
  pump();
}

function apply(message: ToVideo) {
  switch (message.type) {
    case "session":
      video!.connected(message.device, new Uint8Array(message.data), message.seq);
      inflight.set(message.device, 0);
      break;
    case "events":
      video!.events(message.device, new Uint8Array(message.data));
      break;
    case "lost":
      video!.lost(message.device);
      inflight.delete(message.device);
      break;
    case "sent":
      if (inflight.has(message.device)) inflight.set(message.device, Math.max(0, inflight.get(message.device)! - 1));
      break;
  }
}

function draw(message: FrameMessage) {
  if (!preview || !message.preview) return;
  preview.clearRect(0, 0, preview.canvas.width, preview.canvas.height);
  drawFrame(preview, message.frame, message.display, message.turn);
}

function frame(message: FrameMessage) {
  draw(message);
  pending?.frame.close();
  pending = message;
  pump();
}

self.onmessage = async (event: MessageEvent<VideoCommand>) => {
  await ready;
  const command = event.data;
  switch (command.type) {
    case "init":
      video = new Video();
      device = command.device;
      preview = command.preview.getContext("2d");
      device.onmessage = (e: MessageEvent<ToVideo>) => fromDevice(e.data);
      command.player.onmessage = (e: MessageEvent<FrameMessage>) => frame(e.data);
      break;
    case "frame":
      frame(command);
      break;
    case "reset":
      pending?.frame.close();
      pending = null;
      fresh = false;
      resets += 1;
      video?.reset();
      preview?.clearRect(0, 0, preview.canvas.width, preview.canvas.height);
      break;
  }
};
