// What the page and its workers send each other. Times called `at` are
// milliseconds since the epoch (performance.timeOrigin + performance.now()),
// the same clock in every worker; `pts` and `us` are µs of the media.

export interface Volume {
  mute: boolean;
  volume: number;
  min: number;
  max: number;
}

export interface DeviceInfo {
  panelWidth: number;
  panelHeight: number;
  orientation: number;
  maxFps: number;
  maxMessage: number[];
  audioMaxRate: number;
  brightness: number;
  volume: Volume;
  overlay: boolean;
}

/** Which channels of the sound a device plays. */
export type AudioOutput = "off" | "l" | "r" | "lr";

/** `device` is an id the page gives each Tab5 it adds. */
export type DeviceCommand =
  | { type: "init"; video: MessagePort; audio: MessagePort }
  /** Keeps a session with the USB device with `serial` until removed. */
  | { type: "add"; device: number; serial: string | null }
  | { type: "remove"; device: number }
  /** Ends every session and opens a new one, so the devices show their own screen. */
  | { type: "release" }
  | { type: "brightness"; device: number; value: number }
  | { type: "volume"; device: number; value: number; mute: boolean }
  | { type: "output"; device: number; output: AudioOutput };

export type DeviceStatus =
  | { type: "ready"; usb: boolean }
  | { type: "waiting"; device: number; reason: string }
  | { type: "connected"; device: number; info: DeviceInfo }
  | { type: "stopped"; device: number }
  | { type: "brightness"; device: number; value: number }
  | { type: "volume"; device: number; value: number; mute: boolean }
  | { type: "audio"; device: number; buffered: number; capacity: number }
  | { type: "warning"; device: number; message: string };

/** Device worker to video worker. */
export type ToVideo =
  | { type: "session"; device: number; data: ArrayBuffer; seq: number }
  | { type: "events"; device: number; data: ArrayBuffer }
  | { type: "lost"; device: number }
  | { type: "sent"; device: number };

/** Video worker to device worker: one frame's transfers to a device. */
export type FromVideo = { type: "frame"; device: number; transfers: ArrayBuffer[] };

/** Device worker to player worker. */
export type ToPlayer = { type: "clock"; pts: number; at: number };

/** Player worker to device worker. PCM is 16-bit interleaved. */
export type FromPlayer =
  | { type: "format"; rate: number; channels: number }
  | { type: "pcm"; pts: number; data: ArrayBuffer }
  | { type: "pause"; paused: boolean }
  | { type: "reset" };

export interface FrameColor {
  /** 1 for BT.709, otherwise BT.601. */
  matrix: number;
  fullRange: boolean;
}

export interface FrameMessage {
  type: "frame";
  frame: VideoFrame;
  /** The size it is shown at, before `turn`. */
  display: [number, number];
  /** Counter-clockwise quarter turns it needs. */
  turn: number;
  /** Overrides the frame's own colour space. */
  color: FrameColor | null;
  /** Draw it on the preview canvas too. */
  preview: boolean;
}

export type VideoCommand =
  | { type: "init"; device: MessagePort; player: MessagePort; preview: OffscreenCanvas }
  | FrameMessage
  | { type: "reset" };

/** Video worker to an encoder worker: rows [row, row + rows) of a frame's
 * canvas to encode as a JPEG, answered with its bytes. */
export interface BandJob {
  picture: Uint8Array;
  width: number;
  height: number;
  yuv: boolean;
  rotation: number;
  canvasWidth: number;
  canvasHeight: number;
  originX: number;
  originY: number;
  quality: number;
  row: number;
  rows: number;
}

export type VideoStatus =
  | { type: "stats"; fps: number; rate: number; quality: number }
  | { type: "layout"; text: string }
  | { type: "error"; message: string };

export type PlayerCommand =
  | { type: "init"; video: MessagePort; audio: MessagePort }
  | { type: "load"; file: File; play: boolean; start: number; audioMaxRate: number }
  | { type: "play" }
  | { type: "pause" }
  | { type: "seek"; us: number }
  | { type: "stop" }
  | { type: "probe"; id: number; file: File };

export type PlayerStatus =
  | { type: "loaded"; duration: number | null }
  | { type: "probed"; id: number; duration: number | null; thumbnail: Blob | null }
  | { type: "position"; us: number }
  | { type: "ended" }
  | { type: "warning"; message: string }
  | { type: "error"; message: string };

export function epochNow(): number {
  return performance.timeOrigin + performance.now();
}
