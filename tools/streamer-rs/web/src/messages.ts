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

export type DeviceCommand =
  | { type: "init"; video: MessagePort; audio: MessagePort }
  | { type: "start" }
  | { type: "stop" }
  /** Ends the session and opens a new one, so the device shows its own screen. */
  | { type: "release" }
  | { type: "brightness"; value: number }
  | { type: "volume"; value: number; mute: boolean };

export type DeviceStatus =
  | { type: "ready"; usb: boolean }
  | { type: "waiting"; reason: string }
  | { type: "connected"; info: DeviceInfo }
  | { type: "stopped" }
  | { type: "brightness"; value: number }
  | { type: "volume"; value: number; mute: boolean }
  | { type: "audio"; buffered: number; capacity: number }
  | { type: "warning"; message: string };

/** Device worker to video worker. */
export type ToVideo =
  | { type: "session"; data: ArrayBuffer; seq: number }
  | { type: "events"; data: ArrayBuffer }
  | { type: "lost" }
  | { type: "sent" };

/** Video worker to device worker: one frame's transfers. */
export type FromVideo = { type: "frame"; transfers: ArrayBuffer[] };

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
