// Plays one file at a time: demuxes it, decodes it with WebCodecs, and hands
// the frames to the video worker when they are due and the PCM to the device
// worker. The clock follows what the device plays when there is audio.

import { drawFrame } from "./draw";
import init, { AudioConverter, Media } from "./wasm/streamer.js";
import {
  type FrameColor,
  type FrameMessage,
  type FromPlayer,
  type PlayerCommand,
  type PlayerStatus,
  type ToPlayer,
  epochNow,
} from "./messages";

interface Track {
  index: number;
  kind: "video" | "audio" | "other";
  codec: string;
  codecString: string | null;
  width: number;
  height: number;
  displayWidth: number;
  displayHeight: number;
  rotation: number;
  matrix?: number;
  fullRange?: boolean;
  channels: number;
  sampleRate: number;
}

/** Undecoded video chunks held, so audio can be demuxed ahead of the frames. */
const MAX_CHUNKS = 240;
/** Decoded frames held plus frames being decoded; held frames can stall the
 * decoder. */
const FRAME_QUEUE = 4;
const AUDIO_QUEUE = 32;
const AUDIO_AHEAD_US = 1_000_000;
/** A frame later than this is dropped to catch up. */
const LATE_US = 50_000;
/** Steps smaller than this are jitter of the audio reports, not drift. */
const STEP_US = 15_000;
const POSITION_MS = 250;
const DEFAULT_RATE = 48000;
/** Seconds into the video the thumbnail is taken at, or the middle of a
 * video shorter than twice this. */
const THUMBNAIL_AT = 10;
const THUMBNAIL_SIZE: [number, number] = [192, 108];
/** Packets read looking for the thumbnail frame before giving up. */
const THUMBNAIL_PACKETS = 2000;

const ready = init();
const reader = new FileReaderSync();
let videoPort: MessagePort | null = null;
let audioPort: MessagePort | null = null;
let playback: Playback | null = null;
let wake: (() => void) | null = null;

const status = (message: PlayerStatus) => postMessage(message);
const toDevice = (message: FromPlayer, transfer: Transferable[] = []) => audioPort!.postMessage(message, transfer);

function poke() {
  wake?.();
}

function idle(ms: number): Promise<void> {
  return new Promise((resolve) => {
    let timer = 0;
    const done = () => {
      clearTimeout(timer);
      wake = null;
      resolve();
    };
    if (Number.isFinite(ms)) timer = setTimeout(done, Math.max(0, ms));
    wake = done;
  });
}

function reason(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** The media time being presented, in µs. */
class Clock {
  private anchor: { at: number; pts: number } | null = null;
  private held: number | null = null;

  now(): number | null {
    if (this.held !== null) return this.held;
    return this.anchor && this.anchor.pts + (epochNow() - this.anchor.at) * 1000;
  }

  /** The current time, starting the clock at `pts` if it has not started. */
  nowOrStart(pts: number): number {
    if (this.held !== null) return this.held;
    this.anchor ??= { at: epochNow(), pts };
    return this.now()!;
  }

  set(pts: number, at: number) {
    if (this.held !== null) return;
    const now = this.anchor && this.anchor.pts + (at - this.anchor.at) * 1000;
    if (now === null || Math.abs(now - pts) > STEP_US) this.anchor = { at, pts };
  }

  pause() {
    this.held ??= this.now();
  }

  resume() {
    if (this.held !== null) this.anchor = { at: epochNow(), pts: this.held };
    this.held = null;
  }

  /** Starts again from the next frame, or stays at `pts` while paused. */
  reset(pts: number, paused: boolean) {
    this.anchor = null;
    this.held = paused ? pts : null;
  }
}

/** Counter-clockwise quarter turns that show `video` upright. */
function turnOf(video: Track): number {
  return ((360 - (video.rotation % 360)) % 360) / 90;
}

function openMedia(file: File): Media {
  return new Media(file.size, (offset: number, length: number) =>
    new Uint8Array(reader.readAsArrayBuffer(file.slice(offset, offset + length))),
  );
}

function videoConfig(media: Media, track: Track): VideoDecoderConfig | null {
  if (!track.codecString) return null;
  const extradata = media.extradata(track.index);
  const description = ["h264", "hevc", "av1"].includes(track.codec) && extradata.length > 0;
  return {
    codec: track.codecString,
    codedWidth: track.width,
    codedHeight: track.height,
    ...(description ? { description: extradata } : {}),
  };
}

function audioConfig(media: Media, track: Track): AudioDecoderConfig | null {
  if (!track.codecString) return null;
  const extradata = media.extradata(track.index);
  return {
    codec: track.codecString,
    sampleRate: track.sampleRate,
    numberOfChannels: track.channels,
    ...(extradata.length > 0 && track.codec !== "mp3" ? { description: extradata } : {}),
  };
}

async function supported<C>(config: C | null, check: (config: C) => Promise<{ supported?: boolean }>) {
  return config !== null && ((await check(config).catch(() => null))?.supported ?? false);
}

class Playback {
  private readonly decoder: VideoDecoder;
  private readonly audioDecoder: AudioDecoder | null;
  private readonly converter: AudioConverter | null;
  private readonly clock = new Clock();
  private readonly display: [number, number];
  private readonly turn: number;
  private readonly color: FrameColor | null;
  private chunks: EncodedVideoChunk[] = [];
  private frames: VideoFrame[] = [];
  private eof = false;
  private flushing = false;
  private flushed = false;
  private audioFlushed = false;
  private needKey = true;
  /** Frames and samples before this are dropped after decoding. */
  private target = 0;
  private audioEnd = 0;
  private paused: boolean;
  /** Paused after a seek: show the frame at the target. */
  private showOne = false;
  private failed = false;
  private done = false;
  private reported = 0;

  private constructor(
    private readonly media: Media,
    private readonly video: Track,
    private readonly videoDecoderConfig: VideoDecoderConfig,
    private readonly audio: { track: Track; config: AudioDecoderConfig; rate: number } | null,
    play: boolean,
  ) {
    this.paused = !play;
    this.showOne = !play;
    this.display = [video.displayWidth, video.displayHeight];
    this.turn = turnOf(video);
    this.color = video.matrix === undefined ? null : { matrix: video.matrix, fullRange: video.fullRange ?? false };
    this.decoder = new VideoDecoder({
      output: (frame) => this.decoded(frame),
      error: (e) => this.fail(e),
    });
    this.decoder.configure(videoDecoderConfig);
    if (audio) {
      this.converter = new AudioConverter(audio.track.sampleRate, audio.track.channels, audio.rate);
      this.audioDecoder = new AudioDecoder({
        output: (data) => this.decodedAudio(data),
        error: (e) => this.fail(e),
      });
      this.audioDecoder.configure(audio.config);
      toDevice({ type: "format", rate: audio.rate, channels: this.converter.outputChannels });
      toDevice({ type: "pause", paused: this.paused });
    } else {
      this.converter = null;
      this.audioDecoder = null;
    }
  }

  static async open(file: File, play: boolean, audioMaxRate: number): Promise<Playback> {
    const media = openMedia(file);
    try {
      const tracks = media.tracks() as Track[];
      const video = tracks.find((t) => t.kind === "video");
      if (!video) throw new Error("no video");
      const config = videoConfig(media, video);
      if (!(await supported(config, (c) => VideoDecoder.isConfigSupported(c)))) {
        throw new Error(`this browser cannot decode ${video.codec.toUpperCase()} video`);
      }
      const audioTrack = tracks.find((t) => t.kind === "audio");
      let audio = null;
      if (audioTrack) {
        const aconfig = audioConfig(media, audioTrack);
        if (await supported(aconfig, (c) => AudioDecoder.isConfigSupported(c))) {
          const rate = Math.min(audioTrack.sampleRate, audioMaxRate || DEFAULT_RATE);
          audio = { track: audioTrack, config: aconfig!, rate };
        } else {
          status({ type: "warning", message: `${audioTrack.codec.toUpperCase()} audio cannot be decoded; no sound` });
        }
      }
      media.select(new Uint32Array(audio ? [video.index, audio.track.index] : [video.index]));
      status({ type: "loaded", duration: media.duration() ?? null });
      return new Playback(media, video, config!, audio, play);
    } catch (err) {
      media.free();
      throw err;
    }
  }

  close() {
    for (const codec of [this.decoder, this.audioDecoder]) {
      if (codec && codec.state !== "closed") codec.close();
    }
    this.chunks = [];
    this.dropFrames();
    this.converter?.free();
    this.media.free();
    if (this.audio) toDevice({ type: "reset" });
  }

  play() {
    if (!this.paused) return;
    this.paused = false;
    this.showOne = false;
    this.clock.resume();
    if (this.audio) toDevice({ type: "pause", paused: false });
  }

  pause() {
    if (this.paused) return;
    this.paused = true;
    this.clock.pause();
    if (this.audio) toDevice({ type: "pause", paused: true });
  }

  seek(us: number) {
    this.media.seek(us);
    this.target = us;
    this.audioEnd = us;
    this.chunks = [];
    this.dropFrames();
    this.eof = false;
    this.flushing = false;
    this.flushed = false;
    this.audioFlushed = false;
    this.needKey = true;
    this.done = false;
    this.showOne = this.paused;
    this.decoder.reset();
    this.decoder.configure(this.videoDecoderConfig);
    if (this.audioDecoder && this.audio) {
      this.audioDecoder.reset();
      this.audioDecoder.configure(this.audio.config);
      toDevice({ type: "reset" });
    }
    this.clock.reset(us, this.paused);
  }

  clockReport(pts: number, at: number) {
    if (!this.paused) this.clock.set(pts, at);
  }

  private dropFrames() {
    for (const frame of this.frames) frame.close();
    this.frames = [];
  }

  private fail(err: unknown) {
    if (this.failed) return;
    this.failed = true;
    status({ type: "error", message: reason(err) });
  }

  private decoded(frame: VideoFrame) {
    if (frame.timestamp + (frame.duration ?? 0) <= this.target && frame.timestamp < this.target) {
      frame.close();
      return;
    }
    this.frames.push(frame);
    poke();
  }

  private decodedAudio(data: AudioData) {
    const rate = data.sampleRate;
    const skip = Math.max(0, Math.min(data.numberOfFrames, Math.round(((this.target - data.timestamp) * rate) / 1e6)));
    const frames = data.numberOfFrames - skip;
    if (frames <= 0) {
      data.close();
      return;
    }
    const channels = data.numberOfChannels;
    const planar = new Float32Array(frames * channels);
    for (let c = 0; c < channels; c++) {
      data.copyTo(planar.subarray(c * frames, (c + 1) * frames), {
        planeIndex: c,
        frameOffset: skip,
        frameCount: frames,
        format: "f32-planar",
      });
    }
    const pts = data.timestamp + (skip * 1e6) / rate;
    data.close();
    const pcm = this.converter!.process(planar);
    this.audioEnd = pts + (frames * 1e6) / rate;
    if (pcm.byteLength > 0) toDevice({ type: "pcm", pts, data: pcm.buffer as ArrayBuffer }, [pcm.buffer]);
    poke();
  }

  private audioAhead(): number {
    if (!this.audio) return 0;
    return this.audioEnd - (this.clock.now() ?? this.target);
  }

  private feed() {
    const audioQueue = () => this.audioDecoder?.decodeQueueSize ?? 0;
    while (
      !this.eof &&
      this.chunks.length < MAX_CHUNKS &&
      audioQueue() < AUDIO_QUEUE &&
      this.audioAhead() < AUDIO_AHEAD_US
    ) {
      const packet = this.media.nextPacket();
      if (!packet) {
        this.eof = true;
        break;
      }
      if (packet.track === this.video.index) {
        if (packet.key || !this.needKey) {
          this.needKey = false;
          const type = packet.key ? "key" : "delta";
          const { pts: timestamp, duration, data } = packet;
          this.chunks.push(new EncodedVideoChunk({ type, timestamp, duration, data }));
        }
      } else if (this.audioDecoder) {
        const { pts: timestamp, duration, data } = packet;
        this.audioDecoder.decode(new EncodedAudioChunk({ type: "key", timestamp, duration, data }));
      }
      packet.free();
    }
    while (this.chunks.length > 0 && this.decoder.decodeQueueSize + this.frames.length < FRAME_QUEUE) {
      this.decoder.decode(this.chunks.shift()!);
    }
    if (this.eof && this.chunks.length === 0 && !this.flushing) {
      this.flushing = true;
      this.decoder.flush().then(
        () => ((this.flushed = true), poke()),
        () => {},
      );
      const audio = this.audioDecoder?.flush() ?? Promise.resolve();
      audio.then(
        () => ((this.audioFlushed = true), poke()),
        () => {},
      );
    }
  }

  private send(frame: VideoFrame) {
    const message: FrameMessage = {
      type: "frame",
      frame,
      display: this.display,
      turn: this.turn,
      color: this.color,
      preview: true,
    };
    videoPort!.postMessage(message, [frame]);
  }

  /** Sends the frames that are due; how long until the next one is. */
  private present(): number {
    while (this.frames.length > 0) {
      const frame = this.frames[0];
      if (this.paused) {
        if (!this.showOne) return Infinity;
        this.showOne = false;
        this.frames.shift();
        this.send(frame);
        return Infinity;
      }
      const ahead = frame.timestamp - this.clock.nowOrStart(frame.timestamp);
      if (ahead > 0) return ahead / 1000;
      this.frames.shift();
      if (-ahead > LATE_US && this.frames.length > 0) {
        frame.close();
        continue;
      }
      this.send(frame);
    }
    return Infinity;
  }

  private finished(): boolean {
    if (this.paused || !this.flushed || this.frames.length > 0 || this.chunks.length > 0) return false;
    if (!this.audio) return true;
    return this.audioFlushed && (this.clock.now() ?? Infinity) >= this.audioEnd;
  }

  /** One round of work; how long until the next is needed. */
  step(): number {
    if (this.failed || this.done) return Infinity;
    try {
      this.feed();
    } catch (err) {
      this.fail(err);
      return Infinity;
    }
    const wait = this.present();
    const now = performance.now();
    if (now - this.reported >= POSITION_MS) {
      this.reported = now;
      status({ type: "position", us: this.clock.now() ?? this.target });
    }
    if (this.finished()) {
      this.done = true;
      status({ type: "ended" });
      return Infinity;
    }
    return Math.min(wait, this.paused ? Infinity : POSITION_MS);
  }
}

/** The frame at `us`, or the last one before the packets run out. */
async function frameAt(media: Media, video: Track, config: VideoDecoderConfig, us: number): Promise<VideoFrame | null> {
  let found: VideoFrame | null = null;
  let done = false;
  const decoder = new VideoDecoder({
    output: (frame) => {
      if (done) {
        frame.close();
        return;
      }
      found?.close();
      found = frame;
      done = frame.timestamp >= us;
    },
    error: () => (done = true),
  });
  try {
    decoder.configure(config);
    media.select(new Uint32Array([video.index]));
    media.seek(us);
    let key = false;
    for (let read = 0; !done && read < THUMBNAIL_PACKETS; read++) {
      const packet = media.nextPacket();
      if (!packet) break;
      key ||= packet.key;
      if (key) {
        const { pts: timestamp, duration, data } = packet;
        const type = packet.key ? "key" : "delta";
        decoder.decode(new EncodedVideoChunk({ type, timestamp, duration, data }));
      }
      packet.free();
      if (decoder.decodeQueueSize > FRAME_QUEUE) {
        await new Promise((resolve) => decoder.addEventListener("dequeue", resolve, { once: true }));
      }
    }
    if (!done) await decoder.flush().catch(() => {});
  } finally {
    done = true;
    if (decoder.state !== "closed") decoder.close();
  }
  return found;
}

async function thumbnail(media: Media, duration: number | null): Promise<Blob | null> {
  const video = (media.tracks() as Track[]).find((t) => t.kind === "video");
  const config = video ? videoConfig(media, video) : null;
  if (!video || !(await supported(config, (c) => VideoDecoder.isConfigSupported(c)))) return null;
  const at = duration !== null && duration <= 2 * THUMBNAIL_AT ? duration / 2 : THUMBNAIL_AT;
  const frame = await frameAt(media, video, config!, at * 1e6);
  if (!frame) return null;
  const canvas = new OffscreenCanvas(...THUMBNAIL_SIZE);
  const context = canvas.getContext("2d")!;
  drawFrame(context, frame, [video.displayWidth, video.displayHeight], turnOf(video));
  frame.close();
  return canvas.convertToBlob({ type: "image/jpeg", quality: 0.8 });
}

async function probe(id: number, file: File) {
  let duration: number | null = null;
  let picture: Blob | null = null;
  let media: Media | null = null;
  try {
    media = openMedia(file);
    duration = media.duration() ?? null;
  } catch {}
  status({ type: "probed", id, duration, thumbnail: null });
  try {
    if (media) picture = await thumbnail(media, duration);
  } catch {}
  media?.free();
  if (picture) status({ type: "probed", id, duration, thumbnail: picture });
}

let probing: Promise<void> = Promise.resolve();

async function load(file: File, play: boolean, start: number, audioMaxRate: number) {
  playback?.close();
  playback = null;
  try {
    const opened = await Playback.open(file, play, audioMaxRate);
    playback = opened;
    if (start > 0) opened.seek(start);
  } catch (err) {
    status({ type: "error", message: reason(err) });
  }
  poke();
}

let loading: Promise<void> = Promise.resolve();

self.onmessage = async (event: MessageEvent<PlayerCommand>) => {
  await ready;
  const command = event.data;
  switch (command.type) {
    case "init":
      videoPort = command.video;
      audioPort = command.audio;
      audioPort.onmessage = (e: MessageEvent<ToPlayer>) => playback?.clockReport(e.data.pts, e.data.at);
      void run();
      return;
    case "load":
      loading = loading.then(() => load(command.file, command.play, command.start, command.audioMaxRate));
      return;
    case "probe":
      probing = probing.then(() => probe(command.id, command.file));
      return;
  }
  await loading;
  switch (command.type) {
    case "play":
      playback?.play();
      break;
    case "pause":
      playback?.pause();
      break;
    case "seek":
      try {
        playback?.seek(command.us);
      } catch (err) {
        status({ type: "error", message: reason(err) });
      }
      break;
    case "stop":
      playback?.close();
      playback = null;
      break;
  }
  poke();
};

async function run() {
  for (;;) await idle(playback ? playback.step() : Infinity);
}
