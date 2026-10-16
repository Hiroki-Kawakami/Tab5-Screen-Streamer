// The sessions with the devices over WebUSB (docs/protocol.md): opening them,
// keep-alive, events, and the video and audio endpoints.

import init, { audioFormat, errorName, message, parseEvents, setBrightness, setVolume } from "./wasm/streamer.js";
import {
  type AudioOutput,
  type DeviceCommand,
  type DeviceInfo,
  type DeviceStatus,
  type FromPlayer,
  type FromVideo,
  type ToPlayer,
  type ToVideo,
  epochNow,
} from "./messages";

export const VENDOR_ID = 0xf055;
export const PRODUCT_ID = 0x1118;
const INTERFACES = [0, 1, 2];
const CONTROL_EP = 1;
const VIDEO_EP = 2;
const AUDIO_EP = 3;

const OPEN = 0x01;
const CLOSE = 0x02;
const KEEPALIVE = 0x03;
const SET_VOLUME = 0x05;
const SET_BRIGHTNESS = 0x06;
const AUDIO_FORMAT = 0x20;
const AUDIO_DATA = 0x21;
const BULK = 1;

const IN_BYTES = 512;
const IN_QUEUE = 2;
const VIDEO_QUEUE = 2;
const TIMEOUT = 2000;
const OPEN_RETRY = 200;
const RETRY = 500;
/** Well inside the device's 3 s session timeout. */
const KEEPALIVE_MS = 1000;
const POLL = 100;

const CHUNKS_PER_SECOND = 50;
const TARGET_CHUNKS = 3;
const AUDIO_POLL = 2;
/** Longer than the device buffers, so every device runs dry and starts again
 * from the same chunk. */
const RESYNC_MS = 100;
/** Chunks a device in the host clock may have unsent before it skips some. */
const HOST_INFLIGHT = 4;
/** A device in the host clock that runs dry later than this is out of step. */
const SETTLE_MS = 500;

type DeviceEvent = { type: string; [key: string]: any };
type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

function within<T>(promise: Promise<T>, what: string): Promise<T> {
  let timer = 0;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what} timed out`)), TIMEOUT);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

function reason(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

class Link {
  private readonly packetSizes = new Map<number, number>();

  constructor(readonly device: USBDevice) {
    for (const iface of device.configuration?.interfaces ?? []) {
      for (const endpoint of iface.alternate.endpoints) {
        if (endpoint.direction === "out") this.packetSizes.set(endpoint.endpointNumber, endpoint.packetSize);
      }
    }
  }

  /** One transfer, ended with a short or zero-length packet. */
  async out(endpoint: number, data: Uint8Array<ArrayBuffer>, what: string): Promise<void> {
    const packet = this.packetSizes.get(endpoint) ?? 512;
    const zlp = data.byteLength === 0 || data.byteLength % packet === 0;
    const sent = [this.device.transferOut(endpoint, data)];
    if (zlp && data.byteLength > 0) sent.push(this.device.transferOut(endpoint, new Uint8Array(0)));
    for (const result of await within(Promise.all(sent), what)) {
      if (result.status !== "ok") throw new Error(`${what}: ${result.status}`);
    }
  }

  async in(): Promise<Uint8Array<ArrayBuffer>> {
    const result = await this.device.transferIn(CONTROL_EP, IN_BYTES);
    if (result.status !== "ok" || !result.data) throw new Error(`control in: ${result.status}`);
    const view = result.data;
    return new Uint8Array(view.buffer.slice(view.byteOffset, view.byteOffset + view.byteLength) as ArrayBuffer);
  }
}

interface Format {
  rate: number;
  channels: number;
}

interface Chunk {
  pts: number;
  data: Uint8Array;
}

/** The samples of 16-bit interleaved `data` that `output` plays. */
function pick(data: Uint8Array, channels: number, output: AudioOutput): Uint8Array {
  if (output === "lr" || channels === 1) return data;
  const input = new Int16Array(data.buffer, data.byteOffset, data.byteLength / 2);
  const out = new Int16Array(input.length / channels);
  const channel = output === "l" ? 0 : 1;
  for (let i = 0; i < out.length; i++) out[i] = input[i * channels + channel];
  return new Uint8Array(out.buffer);
}

/** One device's part of the audio. */
class AudioOut {
  output: AudioOutput = "lr";
  formatSent = false;
  inflight = 0;
  buffered = 0;
  /** When the latest report came; frames sent since then are `sent`. */
  reportedAt = 0;
  sent = 0;
  sentEnd: number | null = null;
  private seq = 0;

  constructor(
    readonly id: number,
    readonly device: DeviceHost,
  ) {}

  get audible(): boolean {
    return this.output !== "off" && this.device.open;
  }

  /** A new session: the device knows no format and holds nothing. */
  opened() {
    this.formatSent = false;
    this.buffered = 0;
    this.sent = 0;
    this.reportedAt = performance.now();
    this.sentEnd = null;
  }

  lost() {
    this.inflight = 0;
    this.sentEnd = null;
  }

  send(chunk: Chunk, format: Format, frames: number, hostClock: boolean): Promise<void> {
    const parts: Uint8Array[] = [];
    if (!this.formatSent) {
      const channels = this.output === "lr" ? format.channels : 1;
      parts.push(message(AUDIO_FORMAT, ++this.seq, 0, audioFormat(format.rate, channels, 16, hostClock)));
      this.formatSent = true;
    }
    parts.push(message(AUDIO_DATA, ++this.seq, 0, pick(chunk.data, format.channels, this.output)));
    const transfer = new Uint8Array(parts.reduce((n, p) => n + p.byteLength, 0));
    parts.reduce((at, p) => (transfer.set(p, at), at + p.byteLength), 0);
    this.inflight += 1;
    this.sent += frames;
    this.sentEnd = chunk.pts + (frames * 1e6) / format.rate;
    return this.device
      .send(AUDIO_EP, transfer, "audio")
      .finally(() => (this.inflight = Math.max(0, this.inflight - 1)));
  }
}

/** The player's audio to every device that plays it. A single device plays in
 * its own clock, paced by EV_AUDIO to keep about TARGET_CHUNKS queued so its
 * endpoint never fills and stalls the others. Several play in the host clock:
 * each chunk goes to all of them when it is due, and they resample to that
 * pace. */
class AudioBus {
  private format: Format | null = null;
  private pending = new Uint8Array(0);
  private pendingPts = 0;
  private queue: Chunk[] = [];
  private paused = false;
  private primed = false;
  private readonly outs: AudioOut[] = [];
  /** The devices being fed, the first giving the clock to the player. */
  private playing: AudioOut[] = [];
  private hostClock = false;
  /** In the host clock, chunk `index` is due at start + (index - TARGET_CHUNKS
   * + 1) chunks, so the first TARGET_CHUNKS fill the devices' buffers. */
  private start = 0;
  private index = 0;
  private holdUntil = 0;
  private blockedSince: number | null = null;
  private timer = 0;

  constructor(private readonly player: MessagePort) {}

  private get frames(): number {
    return Math.round(this.format!.rate / CHUNKS_PER_SECOND);
  }

  attach(id: number, device: DeviceHost): AudioOut {
    const out = new AudioOut(id, device);
    this.outs.push(out);
    this.outs.sort((a, b) => a.id - b.id);
    return out;
  }

  detach(out: AudioOut) {
    const at = this.outs.indexOf(out);
    if (at >= 0) this.outs.splice(at, 1);
    this.refresh();
  }

  setOutput(id: number, output: AudioOutput) {
    const out = this.outs.find((o) => o.id === id);
    if (!out || out.output === output) return;
    out.output = output;
    out.formatSent = false;
    this.refresh();
  }

  opened(out: AudioOut) {
    out.opened();
    this.refresh();
  }

  lost(out: AudioOut) {
    out.lost();
    this.refresh();
  }

  handle(message: FromPlayer) {
    switch (message.type) {
      case "format":
        if (this.format?.rate !== message.rate || this.format.channels !== message.channels) {
          this.format = { rate: message.rate, channels: message.channels };
          for (const out of this.outs) out.formatSent = false;
          this.reset();
        }
        break;
      case "pcm":
        if (this.outs.some((o) => o.device.open)) this.add(message.pts, new Uint8Array(message.data));
        break;
      case "pause":
        this.paused = message.paused;
        this.primed = false;
        break;
      case "reset":
        this.reset();
        break;
    }
    this.schedule(0);
  }

  private reset() {
    this.queue = [];
    this.pending = new Uint8Array(0);
    this.primed = false;
    for (const out of this.outs) out.sentEnd = null;
  }

  /** Starts every device again together when the devices playing change. */
  private refresh() {
    const playing = this.outs.filter((o) => o.audible);
    const hostClock = playing.length >= 2;
    if (
      hostClock === this.hostClock &&
      playing.length === this.playing.length &&
      playing.every((out, i) => out === this.playing[i])
    ) {
      return;
    }
    if (hostClock !== this.hostClock) for (const out of playing) out.formatSent = false;
    this.playing = playing;
    this.hostClock = hostClock;
    this.resync();
  }

  private resync() {
    this.primed = false;
    this.holdUntil = performance.now() + RESYNC_MS;
    this.schedule(0);
  }

  report(out: AudioOut, buffered: number) {
    const now = performance.now();
    out.buffered = buffered;
    out.reportedAt = now;
    out.sent = 0;
    if (!this.format || this.paused || !this.primed || !this.playing.includes(out)) return;
    if (this.hostClock && buffered === 0 && now - this.start >= SETTLE_MS) {
      this.resync();
      return;
    }
    if (out !== this.playing[0] || out.sentEnd === null || buffered === 0) return;
    const pts = out.sentEnd - (buffered * 1e6) / this.format.rate;
    this.player.postMessage({ type: "clock", pts, at: epochNow() } satisfies ToPlayer);
  }

  private add(pts: number, data: Uint8Array) {
    if (!this.format) return;
    if (this.pending.byteLength === 0) this.pendingPts = pts;
    const joined = new Uint8Array(this.pending.byteLength + data.byteLength);
    joined.set(this.pending);
    joined.set(data, this.pending.byteLength);
    const chunkBytes = this.frames * this.format.channels * 2;
    const chunkUs = (this.frames * 1e6) / this.format.rate;
    let at = 0;
    for (; at + chunkBytes <= joined.byteLength; at += chunkBytes) {
      this.queue.push({ pts: this.pendingPts, data: joined.subarray(at, at + chunkBytes) });
      this.pendingPts += chunkUs;
    }
    this.pending = joined.slice(at);
  }

  private schedule(ms: number) {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.tick(), ms);
  }

  private tick() {
    if (this.paused || !this.format || this.queue.length === 0 || this.playing.length === 0) return;
    const now = performance.now();
    if (now < this.holdUntil) {
      this.schedule(this.holdUntil - now);
      return;
    }
    if (!this.primed) {
      if (this.queue.length < TARGET_CHUNKS) return;
      this.primed = true;
      this.start = now;
      this.index = 0;
    }
    if (this.hostClock) this.tickHost(now);
    else this.tickDevice(now, this.playing[0]);
  }

  /** Between reports the device is taken to drain at its rate, so sparse
   * reports do not starve it. A chunk the device has not made room for
   * within TIMEOUT is dropped, so the input keeps its pace. */
  private tickDevice(now: number, out: AudioOut) {
    if (out.inflight > 0) return;
    const drained = ((now - out.reportedAt) * this.format!.rate) / 1000;
    if (out.buffered + out.sent - drained >= this.frames * TARGET_CHUNKS) {
      this.blockedSince ??= now;
      if (now - this.blockedSince >= TIMEOUT) {
        this.queue.shift();
        this.blockedSince = now;
      }
      this.schedule(AUDIO_POLL);
      return;
    }
    this.blockedSince = null;
    const chunk = this.queue.shift()!;
    out
      .send(chunk, this.format!, this.frames, false)
      .then(() => this.schedule(0))
      .catch(() => {});
    this.schedule(AUDIO_POLL);
  }

  /** Sends the chunks that are due to every device. Behind by more than the
   * devices buffer, they have run dry, and start again together. */
  private tickHost(now: number) {
    const chunkMs = (this.frames * 1000) / this.format!.rate;
    for (;;) {
      const due = this.start + (this.index - TARGET_CHUNKS + 1) * chunkMs;
      if (due > now) {
        this.schedule(due - now);
        return;
      }
      if (now - due > TARGET_CHUNKS * chunkMs) {
        this.primed = false;
        this.schedule(0);
        return;
      }
      const chunk = this.queue.shift();
      if (!chunk) return;
      this.index += 1;
      for (const out of this.playing) {
        if (out.inflight < HOST_INFLIGHT) void out.send(chunk, this.format!, this.frames, true).catch(() => {});
      }
    }
  }
}

/** Keeps a session with the device with `serial` while started, opening a new
 * one whenever the device comes back. */
class DeviceHost {
  private running = false;
  private looping = false;
  private link: Link | null = null;
  private session: { info: DeviceInfo } | null = null;
  private controlSeq = 0;
  private lastActivity = 0;
  private opening: { seq: number; resolve: (data: Uint8Array<ArrayBuffer>) => void } | null = null;
  private ended: (() => void) | null = null;
  private releasing = false;
  private failure: string | null = null;
  private videoQueue: { data: ArrayBuffer; last: boolean }[] = [];
  private videoInflight = 0;
  private readonly audio: AudioOut;

  constructor(
    private readonly usb: USB,
    private readonly id: number,
    private readonly serial: string | null,
    private readonly report: (status: DeviceStatus) => void,
    private readonly video: MessagePort,
    private readonly bus: AudioBus,
  ) {
    this.audio = bus.attach(id, this);
  }

  get open(): boolean {
    return this.session !== null;
  }

  private status(status: DistributiveOmit<DeviceStatus, "device">) {
    this.report({ ...status, device: this.id } as DeviceStatus);
  }

  private toVideo(message: DistributiveOmit<ToVideo, "device">, transfer: Transferable[] = []) {
    this.video.postMessage({ ...message, device: this.id }, transfer);
  }

  start() {
    this.running = true;
    if (!this.looping) void this.loop();
  }

  stop() {
    this.running = false;
    this.ended?.();
    this.bus.detach(this.audio);
  }

  release() {
    if (!this.session) return;
    this.releasing = true;
    this.ended?.();
  }

  brightness(value: number) {
    if (this.session) void this.control(SET_BRIGHTNESS, setBrightness(value)).catch((e) => this.fail(e));
  }

  volume(value: number, mute: boolean) {
    if (this.session) void this.control(SET_VOLUME, setVolume(BULK, mute, value)).catch((e) => this.fail(e));
  }

  private async loop() {
    this.looping = true;
    let last = "";
    const waiting = (reason: string) => {
      if (reason !== last) this.status({ type: "waiting", reason });
      last = reason;
    };
    while (this.running) {
      try {
        const device = (await this.usb.getDevices()).find(
          (d) => d.vendorId === VENDOR_ID && d.productId === PRODUCT_ID && (d.serialNumber ?? null) === this.serial,
        );
        if (device) {
          await this.serve(device);
          last = "";
        } else {
          waiting("device not found");
        }
      } catch (err) {
        waiting(reason(err));
      }
      await this.closeDevice();
      if (this.running) await sleep(RETRY);
    }
    this.looping = false;
    this.status({ type: "stopped" });
  }

  private async closeDevice() {
    const device = this.link?.device;
    this.link = null;
    if (device?.opened) await device.close().catch(() => {});
  }

  /** Serves sessions on `device` until it is lost or stopped. */
  private async serve(device: USBDevice) {
    await device.open();
    if (!device.configuration) await device.selectConfiguration(1);
    for (const number of INTERFACES) await device.claimInterface(number);
    const link = new Link(device);
    this.link = link;
    this.failure = null;
    for (let i = 0; i < IN_QUEUE; i++) void this.read(link);
    const keepAlive = setInterval(() => this.keepAlive(), POLL);
    try {
      while (this.running) await this.runSession();
    } finally {
      clearInterval(keepAlive);
    }
  }

  /** One session, until it is lost, stopped or released; a release gives the
   * panel back to the device, which a session holds from its first frame. */
  private async runSession() {
    const ended = new Promise<void>((resolve) => (this.ended = resolve));
    this.releasing = false;
    try {
      const { data, seq } = await this.openSession();
      const info = (parseEvents(data) as DeviceEvent[]).find((e) => e.type === "info" && e.seq === seq)!.info;
      this.session = { info };
      this.bus.opened(this.audio);
      this.toVideo({ type: "session", data: data.buffer, seq }, [data.buffer]);
      this.status({ type: "connected", info });
      if (this.running && this.failure === null && !this.releasing) await ended;
      if (this.failure !== null) throw new Error(this.failure);
      await this.flushVideo();
      await this.control(CLOSE, new Uint8Array(0));
    } finally {
      this.ended = null;
      this.opening = null;
      if (this.session) {
        this.session = null;
        this.bus.lost(this.audio);
        this.videoQueue = [];
        this.videoInflight = 0;
        this.toVideo({ type: "lost" });
      }
    }
  }

  /** Sends OPEN until it is answered with INFO; events before it are from an
   * earlier session. A retry gets past a transfer an earlier session cut
   * short, which the device drops along with the OPEN after it. */
  private async openSession(): Promise<{ data: Uint8Array<ArrayBuffer>; seq: number }> {
    const deadline = performance.now() + TIMEOUT;
    while (performance.now() < deadline) {
      const seq = this.controlSeq + 1;
      const answered = new Promise<Uint8Array<ArrayBuffer>>((resolve) => {
        this.opening = { seq, resolve };
      });
      await this.control(OPEN, new Uint8Array(0));
      const data = await Promise.race([answered, sleep(OPEN_RETRY).then(() => null)]);
      if (data) {
        this.opening = null;
        return { data, seq };
      }
      if (this.failure !== null) throw new Error(this.failure);
    }
    throw new Error("no INFO");
  }

  private fail(err: unknown) {
    this.failure ??= reason(err);
    this.ended?.();
  }

  async send(endpoint: number, data: Uint8Array<ArrayBuffer>, what: string): Promise<void> {
    const link = this.link;
    if (!link) throw new Error("no device");
    this.lastActivity = performance.now();
    try {
      await link.out(endpoint, data, what);
    } catch (err) {
      this.fail(err);
      throw err;
    }
  }

  private control(kind: number, payload: Uint8Array): Promise<void> {
    const data = message(kind, ++this.controlSeq, 0, payload) as Uint8Array<ArrayBuffer>;
    return this.send(CONTROL_EP, data, "control out");
  }

  private keepAlive() {
    if (!this.session || performance.now() - this.lastActivity < KEEPALIVE_MS) return;
    void this.control(KEEPALIVE, new Uint8Array(0)).catch(() => {});
  }

  private async read(link: Link) {
    while (this.link === link) {
      let data: Uint8Array<ArrayBuffer>;
      try {
        data = await link.in();
      } catch (err) {
        if (this.link === link) this.fail(err);
        return;
      }
      this.events(data);
    }
  }

  private events(data: Uint8Array<ArrayBuffer>) {
    for (const event of parseEvents(data) as DeviceEvent[]) {
      switch (event.type) {
        case "info":
          if (this.opening && this.opening.seq === event.seq) this.opening.resolve(data);
          break;
        case "closed":
          if (this.session) this.fail("the device closed the session");
          break;
        case "error":
          this.status({
            type: "warning",
            message: `device: ${errorName(event.code)} on channel ${event.channel} seq ${event.seq}`,
          });
          break;
        case "audio":
          this.bus.report(this.audio, event.buffered);
          this.status({ type: "audio", buffered: event.buffered, capacity: event.capacity });
          break;
        case "brightness":
          this.status({ type: "brightness", value: event.brightness });
          break;
        case "volume":
          if (event.stream === BULK) this.status({ type: "volume", value: event.volume, mute: event.mute });
          break;
      }
    }
    if (this.session) this.toVideo({ type: "events", data: data.buffer }, [data.buffer]);
  }

  frame(transfers: ArrayBuffer[]) {
    if (!this.session) {
      this.toVideo({ type: "sent" });
      return;
    }
    transfers.forEach((data, i) => this.videoQueue.push({ data, last: i === transfers.length - 1 }));
    this.pumpVideo();
  }

  private pumpVideo() {
    while (this.session && this.videoInflight < VIDEO_QUEUE && this.videoQueue.length > 0) {
      const { data, last } = this.videoQueue.shift()!;
      this.videoInflight += 1;
      this.send(VIDEO_EP, new Uint8Array(data), "video")
        .then(() => {
          this.videoInflight -= 1;
          if (last) this.toVideo({ type: "sent" });
          this.pumpVideo();
        })
        .catch(() => {});
    }
  }

  private async flushVideo() {
    const deadline = performance.now() + TIMEOUT;
    while ((this.videoQueue.length > 0 || this.videoInflight > 0) && this.failure === null) {
      if (performance.now() > deadline) throw new Error("video flush timed out");
      await sleep(10);
    }
  }
}

/** The devices the page added, each in its own session. */
export class DeviceManager {
  private readonly hosts = new Map<number, DeviceHost>();
  private readonly bus: AudioBus;

  private constructor(
    private readonly usb: USB,
    private readonly status: (status: DeviceStatus) => void,
    private readonly video: MessagePort,
    player: MessagePort,
  ) {
    this.bus = new AudioBus(player);
    video.onmessage = (event: MessageEvent<FromVideo>) => this.frame(event.data);
    player.onmessage = (event: MessageEvent<FromPlayer>) => this.bus.handle(event.data);
  }

  static async create(
    usb: USB,
    status: (status: DeviceStatus) => void,
    video: MessagePort,
    player: MessagePort,
  ): Promise<DeviceManager> {
    await init();
    return new DeviceManager(usb, status, video, player);
  }

  command(command: DeviceCommand) {
    switch (command.type) {
      case "add": {
        if (this.hosts.has(command.device)) break;
        const host = new DeviceHost(this.usb, command.device, command.serial, this.status, this.video, this.bus);
        this.hosts.set(command.device, host);
        host.start();
        break;
      }
      case "remove":
        this.hosts.get(command.device)?.stop();
        this.hosts.delete(command.device);
        break;
      case "release":
        for (const host of this.hosts.values()) host.release();
        break;
      case "brightness":
        this.hosts.get(command.device)?.brightness(command.value);
        break;
      case "volume":
        this.hosts.get(command.device)?.volume(command.value, command.mute);
        break;
      case "output":
        this.bus.setOutput(command.device, command.output);
        break;
    }
  }

  private frame(message: FromVideo) {
    const host = this.hosts.get(message.device);
    if (host) host.frame(message.transfers);
    else this.video.postMessage({ type: "sent", device: message.device } satisfies ToVideo);
  }
}
