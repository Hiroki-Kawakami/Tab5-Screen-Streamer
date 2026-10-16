// Frames from the screen capture or the player, scaled, encoded and laid out
// for the device. Only the latest frame waits to be sent; older ones are
// dropped.

import init, { Video } from "./wasm/streamer.js";
import type { FrameMessage, FromVideo, ToVideo, VideoCommand, VideoStatus } from "./messages";

const YUV_FORMATS = new Set(["I420", "I420A", "NV12", "I422", "I422A", "I444", "I444A"]);
/** Frames handed to the device worker and not yet sent. */
const INFLIGHT = 2;
const STATS_PERIOD = 1000;

const ready = init();
let video: Video | null = null;
let preview: OffscreenCanvasRenderingContext2D | null = null;
let device: MessagePort | null = null;
let pending: FrameMessage | null = null;
let inflight = 0;
let busy = false;
let timer = 0;
let stats = { start: performance.now(), frames: 0, bytes: 0, quality: 0 };

const status = (message: VideoStatus) => postMessage(message);

function matrixCode(space: VideoColorSpace): number {
  return space.matrix === "bt709" ? 1 : 6;
}

async function take(message: FrameMessage) {
  const { frame, display, turn, color } = message;
  try {
    const rect = frame.visibleRect!;
    if (frame.format && YUV_FORMATS.has(frame.format)) {
      const data = new Uint8Array(frame.allocationSize({ rect }));
      const layout = await frame.copyTo(data, { rect });
      const planes = new Uint32Array(layout.flatMap((p) => [p.offset, p.stride]));
      const matrix = color?.matrix ?? matrixCode(frame.colorSpace);
      const fullRange = color?.fullRange ?? frame.colorSpace.fullRange ?? false;
      video!.frameYuv(frame.format, rect.width, rect.height, data, planes, matrix, fullRange, ...display, turn);
    } else {
      const options = { rect, format: "BGRX", colorSpace: "srgb" } as VideoFrameCopyToOptions;
      const data = new Uint8Array(rect.width * rect.height * 4);
      await frame.copyTo(data, options);
      video!.frameBgrx(rect.width, rect.height, data, ...display, turn);
    }
  } finally {
    frame.close();
  }
}

function send(now: number) {
  const transfers = video!.encode(now) as Uint8Array<ArrayBuffer>[];
  if (transfers.length === 0) return;
  const buffers = transfers.map((t) => t.buffer);
  inflight += 1;
  device!.postMessage({ type: "frame", transfers: buffers } satisfies FromVideo, buffers);
  stats.frames += 1;
  stats.bytes += buffers.reduce((n, b) => n + b.byteLength, 0);
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

async function pump() {
  if (busy || !video) return;
  clearTimeout(timer);
  const now = performance.now();
  report(now);
  if (inflight >= INFLIGHT || !(pending || video.redraw())) return;
  if (!video.ready(now)) {
    const wait = video.untilDue(now);
    if (wait > 0) timer = setTimeout(pump, wait);
    return;
  }
  busy = true;
  try {
    const before = video.describe();
    if (pending) {
      const message = pending;
      pending = null;
      await take(message);
    }
    const after = video.describe();
    if (after && after !== before) status({ type: "layout", text: after });
    send(performance.now());
  } catch (err) {
    status({ type: "error", message: err instanceof Error ? err.message : String(err) });
  } finally {
    busy = false;
  }
  void pump();
}

function fromDevice(message: ToVideo) {
  try {
    apply(message);
  } catch (err) {
    status({ type: "error", message: err instanceof Error ? err.message : String(err) });
  }
  void pump();
}

function apply(message: ToVideo) {
  switch (message.type) {
    case "session":
      video!.connected(new Uint8Array(message.data), message.seq);
      inflight = 0;
      break;
    case "events":
      video!.events(new Uint8Array(message.data));
      break;
    case "lost":
      video!.lost();
      inflight = 0;
      break;
    case "sent":
      inflight = Math.max(0, inflight - 1);
      break;
  }
}

function draw(message: FrameMessage) {
  if (!preview || !message.preview) return;
  const { canvas } = preview;
  const [w, h] = message.display;
  const [rw, rh] = message.turn % 2 === 1 ? [h, w] : [w, h];
  const scale = Math.min(canvas.width / rw, canvas.height / rh);
  preview.clearRect(0, 0, canvas.width, canvas.height);
  preview.save();
  preview.translate(canvas.width / 2, canvas.height / 2);
  preview.rotate((-message.turn * Math.PI) / 2);
  preview.drawImage(message.frame, (-w * scale) / 2, (-h * scale) / 2, w * scale, h * scale);
  preview.restore();
}

function frame(message: FrameMessage) {
  draw(message);
  pending?.frame.close();
  pending = message;
  void pump();
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
      video?.reset();
      preview?.clearRect(0, 0, preview.canvas.width, preview.canvas.height);
      break;
  }
};
