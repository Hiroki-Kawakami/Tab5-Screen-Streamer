import "./style.css";

import { DeviceHost, PRODUCT_ID, VENDOR_ID } from "./device";
import { icon } from "./icons";
import type {
  DeviceCommand,
  DeviceInfo,
  DeviceStatus,
  FrameMessage,
  PlayerCommand,
  PlayerStatus,
  VideoCommand,
  VideoStatus,
} from "./messages";

type View = "landing" | "modes" | "screen" | "player";
type Repeat = "off" | "all" | "one";

interface Item {
  id: number;
  file: File;
  /** Seconds; undefined until probed. */
  duration: number | null | undefined;
  /** An object URL. */
  thumbnail: string | null;
  error: string | null;
}

/** Back to the start of the file rather than the previous one past this. */
const RESTART_US = 3_000_000;
const SLIDER_MS = 100;
const TOAST_MS = 6000;
const THUMBS = ["#2B3550", "#4A3426", "#23413D", "#3D2A47", "#3A3F2A", "#2E3A44", "#3F3524", "#352B3D"];
const SURFACES: Record<string, string> = { monitor: "Entire Screen", window: "Window", browser: "Browser Tab" };

document.getElementById("app")!.innerHTML = `
  <div id="landing" class="view landing">
    <main>
      <span class="logo">${icon("appIcon", 128)}</span>
      <div class="titles">
        <h1>Tab5 Streamer <span class="web">Web</span></h1>
        <p class="lead">Stream your PC screen or video files to M5Stack Tab5 over USB.</p>
      </div>
      <div id="connect" class="connect">
        <button id="connect-button" class="cta" type="button">${icon("usb", 22)}Connect to Tab5</button>
        <p class="hint">Connect your Tab5 with a USB cable first.</p>
      </div>
      <div id="unsupported" class="alert" role="alert" hidden>
        ${icon("alertTriangle", 22)}
        <div class="body">
          <p class="title">WebUSB is not supported in this browser</p>
          <p class="text">Open this page in the latest version of a Chromium-based browser, such as Google Chrome or Microsoft Edge.</p>
        </div>
      </div>
    </main>
    <footer>
      <a href="https://github.com/Hiroki-Kawakami/Tab5-Screen-Streamer">${icon("brandGithub", 16)}GitHub</a>
    </footer>
  </div>

  <div id="shell" class="view" hidden>
    <header class="header">
      <button id="home" class="home" type="button">
        ${icon("appIcon", 32)}
        <span class="home-title">Tab5 Streamer <span class="web">Web</span></span>
      </button>
      <div class="pill-anchor">
        <button id="pill" class="pill" type="button" aria-haspopup="menu" aria-expanded="false">
          <span id="pill-dot" class="dot"></span>
          <span id="pill-label" class="pill-label"></span>
          ${icon("chevronDown", 16)}
        </button>
        <div id="menu" class="menu" role="menu" aria-label="Tab5 connection menu" hidden>
          <div class="menu-device">
            <span class="menu-name">M5Stack Tab5</span>
            <span class="menu-state"><span id="menu-dot" class="dot"></span><span id="menu-state"></span></span>
          </div>
          <div class="menu-rule"></div>
          <div class="menu-controls">
            <div class="control-head">
              <label for="brightness" class="control-label">${icon("sun", 18)}Brightness</label>
              <span id="brightness-value" class="control-value"></span>
            </div>
            <input id="brightness" type="range" min="0" max="100" step="1" />
            <div id="menu-volume" class="menu-volume">
              <div class="control-head">
                <div class="control-label">
                  <button id="mute" class="mute" type="button"></button>
                  <label for="volume">Volume</label>
                </div>
                <span id="volume-value" class="control-value"></span>
              </div>
              <input id="volume" type="range" min="0" max="100" step="1" />
            </div>
          </div>
          <div class="menu-rule"></div>
          <div class="menu-actions">
            <button id="back" class="menu-item" role="menuitem" type="button">${icon("layoutGrid", 18)}Back to Mode Select</button>
            <button id="disconnect" class="menu-item danger" role="menuitem" type="button">${icon("plugConnectedX", 18)}Disconnect</button>
          </div>
        </div>
      </div>
    </header>

    <main id="modes" class="modes-main">
      <div class="modes-titles">
        <h1>Choose what to stream</h1>
        <p>The selected content will be shown on your Tab5.</p>
      </div>
      <div class="mode-cards">
        <button id="mode-screen" class="mode-card" type="button">
          <span class="mode-icon">${icon("screenShare", 40, 1.75)}</span>
          <span class="mode-text">
            <span class="mode-name">Screen Capture</span>
            <span class="mode-desc">Capture your screen or a window and stream it in real time.</span>
          </span>
        </button>
        <button id="mode-player" class="mode-card" type="button">
          <span class="mode-icon">${icon("movie", 40, 1.75)}</span>
          <span class="mode-text">
            <span class="mode-name">Video Files</span>
            <span class="mode-desc">Queue video files from your PC in a playlist and play them back.</span>
          </span>
        </button>
      </div>
    </main>

    <main id="screen" class="sc-main">
      <div class="sc-stage">
        <div class="sc-col">
          <section id="sc-preview" class="preview" aria-label="Preview"><video id="sc-video" muted playsinline></video></section>
          <div class="sc-bar panel">
            <div class="sc-source">
              <div class="sc-src-icon">${icon("deviceDesktop", 22)}</div>
              <div class="sc-src-text">
                <span class="sc-src-caption">Capture source</span>
                <span id="sc-source" class="sc-src-name"></span>
              </div>
              <button id="sc-change" class="sc-change" type="button" aria-label="Change Source">
                ${icon("refresh", 18)}<span class="sc-change-label">Change Source</span>
              </button>
            </div>
            <button id="sc-action" class="cta sc-action" type="button"></button>
          </div>
        </div>
      </div>
    </main>

    <main id="player" class="vp-main">
      <section class="vp-preview" aria-label="Preview">
        <div class="preview"><canvas id="vp-canvas" width="1280" height="720"></canvas></div>
        <div class="now">
          <span class="now-caption">Now playing</span>
          <span id="now-name" class="now-name"></span>
        </div>
      </section>
      <aside class="vp-side">
        <section class="transport panel" aria-label="Playback controls">
          <div class="transport-buttons">
            <button id="shuffle" class="icon-btn toggle" type="button" aria-label="Shuffle">${icon("arrowsShuffle", 22)}</button>
            <button id="prev" class="icon-btn skip" type="button" aria-label="Previous">${icon("skipBack", 24)}</button>
            <button id="play" class="play" type="button"></button>
            <button id="next" class="icon-btn skip" type="button" aria-label="Next">${icon("skipForward", 24)}</button>
            <button id="repeat" class="icon-btn toggle" type="button"></button>
          </div>
          <div class="seek">
            <label for="seek" class="sr-only">Seek</label>
            <input id="seek" type="range" min="0" max="0" step="0.1" value="0" />
            <div class="times"><span id="time"></span><span id="duration"></span></div>
          </div>
        </section>
        <section class="playlist panel" aria-label="Playlist">
          <div class="playlist-head">
            <div class="playlist-title">
              <h2>Playlist</h2>
              <span id="count" class="playlist-count"></span>
            </div>
            <button id="add" class="add" type="button">${icon("plus", 16)}Add Videos</button>
            <input id="files" type="file" multiple accept="video/*,.mkv,.webm,.mov,.mp4,.m4v" hidden />
          </div>
          <ul id="queue" class="vp-list"></ul>
          <div id="drop" class="vp-drop">${icon("upload", 16)}Drop video files here to add</div>
        </section>
      </aside>
    </main>
  </div>

  <div id="toast" class="alert toast" role="alert" hidden>
    ${icon("alertTriangle", 22)}
    <div class="body"><p id="toast-text" class="text"></p></div>
  </div>
`;

function element<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

const ui = {
  landing: element<HTMLDivElement>("landing"),
  connect: element<HTMLDivElement>("connect"),
  connectButton: element<HTMLButtonElement>("connect-button"),
  unsupported: element<HTMLDivElement>("unsupported"),
  shell: element<HTMLDivElement>("shell"),
  home: element<HTMLButtonElement>("home"),
  pill: element<HTMLButtonElement>("pill"),
  pillDot: element<HTMLSpanElement>("pill-dot"),
  pillLabel: element<HTMLSpanElement>("pill-label"),
  menu: element<HTMLDivElement>("menu"),
  menuDot: element<HTMLSpanElement>("menu-dot"),
  menuState: element<HTMLSpanElement>("menu-state"),
  brightness: element<HTMLInputElement>("brightness"),
  brightnessValue: element<HTMLSpanElement>("brightness-value"),
  menuVolume: element<HTMLDivElement>("menu-volume"),
  mute: element<HTMLButtonElement>("mute"),
  volume: element<HTMLInputElement>("volume"),
  volumeValue: element<HTMLSpanElement>("volume-value"),
  back: element<HTMLButtonElement>("back"),
  disconnect: element<HTMLButtonElement>("disconnect"),
  modes: element<HTMLElement>("modes"),
  modeScreen: element<HTMLButtonElement>("mode-screen"),
  modePlayer: element<HTMLButtonElement>("mode-player"),
  screen: element<HTMLElement>("screen"),
  scPreview: element<HTMLElement>("sc-preview"),
  scVideo: element<HTMLVideoElement>("sc-video"),
  scSource: element<HTMLSpanElement>("sc-source"),
  scChange: element<HTMLButtonElement>("sc-change"),
  scAction: element<HTMLButtonElement>("sc-action"),
  player: element<HTMLElement>("player"),
  canvas: element<HTMLCanvasElement>("vp-canvas"),
  nowName: element<HTMLSpanElement>("now-name"),
  shuffle: element<HTMLButtonElement>("shuffle"),
  prev: element<HTMLButtonElement>("prev"),
  play: element<HTMLButtonElement>("play"),
  next: element<HTMLButtonElement>("next"),
  repeat: element<HTMLButtonElement>("repeat"),
  seek: element<HTMLInputElement>("seek"),
  time: element<HTMLSpanElement>("time"),
  duration: element<HTMLSpanElement>("duration"),
  count: element<HTMLSpanElement>("count"),
  add: element<HTMLButtonElement>("add"),
  files: element<HTMLInputElement>("files"),
  queue: element<HTMLUListElement>("queue"),
  drop: element<HTMLDivElement>("drop"),
  toast: element<HTMLDivElement>("toast"),
  toastText: element<HTMLParagraphElement>("toast-text"),
};

let toastTimer = 0;

function say(text: string) {
  ui.toastText.textContent = text;
  ui.toast.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (ui.toast.hidden = true), TOAST_MS);
}

function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const pad = (v: number) => String(v).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s % 60)}` : `${pad(m)}:${pad(s % 60)}`;
}

/** Calls `send` at most every SLIDER_MS while a slider moves, and once more
 * with its final value. */
function throttled(send: () => void): () => void {
  let last = 0;
  let timer = 0;
  return () => {
    clearTimeout(timer);
    const wait = last + SLIDER_MS - performance.now();
    if (wait <= 0) {
      last = performance.now();
      send();
    } else {
      timer = setTimeout(() => {
        last = performance.now();
        send();
      }, wait);
    }
  };
}

const videoWorker = new Worker(new URL("./video.worker.ts", import.meta.url), { type: "module" });
const playerWorker = new Worker(new URL("./player.worker.ts", import.meta.url), { type: "module" });
const deviceWorker = new Worker(new URL("./device.worker.ts", import.meta.url), { type: "module" });
const deviceVideo = new MessageChannel();
const devicePlayer = new MessageChannel();
const playerVideo = new MessageChannel();

const toVideo = (command: VideoCommand, transfer: Transferable[] = []) => videoWorker.postMessage(command, transfer);
const toPlayer = (command: PlayerCommand) => playerWorker.postMessage(command);
let toDevice: (command: DeviceCommand) => void = () => {};

const preview = ui.canvas.transferControlToOffscreen();
toVideo({ type: "init", device: deviceVideo.port2, player: playerVideo.port2, preview }, [
  deviceVideo.port2,
  playerVideo.port2,
  preview,
]);
playerWorker.postMessage({ type: "init", video: playerVideo.port1, audio: devicePlayer.port2 } satisfies PlayerCommand, [
  playerVideo.port1,
  devicePlayer.port2,
]);

deviceWorker.onmessage = async (event: MessageEvent<DeviceStatus>) => {
  if (event.data.type !== "ready") {
    deviceStatus(event.data);
    return;
  }
  if (event.data.usb) {
    const init: DeviceCommand = { type: "init", video: deviceVideo.port1, audio: devicePlayer.port1 };
    deviceWorker.postMessage(init, [deviceVideo.port1, devicePlayer.port1]);
    toDevice = (command) => deviceWorker.postMessage(command);
  } else if (navigator.usb) {
    deviceWorker.terminate();
    const host = await DeviceHost.create(navigator.usb, deviceStatus, deviceVideo.port1, devicePlayer.port1);
    toDevice = (command) => host.command(command);
  }
  if (wantConnected) toDevice({ type: "start" });
};

let view: View = "landing";
let wantConnected = false;

function show(next: View) {
  if (view === "screen" && next !== "screen") leaveScreen();
  if (view === "player" && next !== "player") stopPlayer();
  view = next;
  ui.landing.hidden = view !== "landing";
  ui.shell.hidden = view === "landing";
  ui.modes.hidden = view !== "modes";
  ui.screen.hidden = view !== "screen";
  ui.player.hidden = view !== "player";
  ui.shell.classList.toggle("stream-view", view === "screen" || view === "player");
  ui.shell.classList.toggle("screen", view === "screen");
  ui.shell.classList.toggle("player", view === "player");
  ui.back.hidden = view === "modes";
  ui.menuVolume.hidden = view !== "player";
  closeMenu();
  if (view === "screen") renderScreen();
  if (view === "player") render();
}

if (!navigator.usb) {
  ui.connect.hidden = true;
  ui.unsupported.hidden = false;
}

ui.connectButton.onclick = async () => {
  const devices = await navigator.usb!.getDevices();
  if (!devices.some((d) => d.vendorId === VENDOR_ID && d.productId === PRODUCT_ID)) {
    try {
      await navigator.usb!.requestDevice({ filters: [{ vendorId: VENDOR_ID, productId: PRODUCT_ID }] });
    } catch {
      return;
    }
  }
  info = null;
  deviceStatus({ type: "waiting", reason: "" });
  wantConnected = true;
  toDevice({ type: "start" });
  show("modes");
};

let info: DeviceInfo | null = null;
let draggingSlider = false;

function percent(volume: number): number {
  const { min, max } = info!.volume;
  return max > min ? Math.round(((volume - min) * 100) / (max - min)) : 0;
}

function showBrightness(value: number) {
  if (!draggingSlider) ui.brightness.value = String(value);
  ui.brightnessValue.textContent = `${value}%`;
}

function showVolume() {
  if (!info) return;
  const { volume, mute } = info.volume;
  const shown = mute ? 0 : percent(volume);
  if (!draggingSlider) ui.volume.value = String(shown);
  ui.volumeValue.textContent = mute ? "Muted" : `${shown}%`;
  ui.volumeValue.classList.toggle("off", mute);
  ui.volume.classList.toggle("off", mute);
  ui.mute.innerHTML = icon(shown >= 50 ? "volume" : shown > 0 ? "volume2" : "volume3", 18);
  ui.mute.classList.toggle("off", shown === 0);
  ui.mute.setAttribute("aria-label", mute ? "Unmute" : "Mute");
  ui.mute.setAttribute("aria-pressed", String(mute));
}

function deviceStatus(status: DeviceStatus) {
  switch (status.type) {
    case "waiting":
      info = null;
      ui.menuState.textContent = status.reason ? `Waiting for Tab5: ${status.reason}` : "Connecting…";
      break;
    case "connected":
      info = status.info;
      showBrightness(info.brightness);
      showVolume();
      break;
    case "stopped":
      info = null;
      break;
    case "brightness":
      showBrightness(status.value);
      break;
    case "volume":
      if (info) {
        info.volume = { ...info.volume, volume: status.value, mute: status.mute };
        showVolume();
      }
      break;
    case "warning":
      say(status.message);
      break;
  }
  const connected = info !== null;
  ui.pillDot.classList.toggle("on", connected);
  ui.menuDot.classList.toggle("on", connected);
  ui.pillLabel.textContent = connected ? "Tab5 Connected" : "Connecting…";
  ui.pill.setAttribute("aria-label", ui.pillLabel.textContent);
  if (connected) ui.menuState.textContent = "Connected via USB";
  for (const control of [ui.brightness, ui.volume, ui.mute]) control.disabled = !connected;
}

function closeMenu() {
  ui.menu.hidden = true;
  ui.pill.setAttribute("aria-expanded", "false");
}

ui.pill.onclick = (e) => {
  e.stopPropagation();
  const open = ui.menu.hidden;
  ui.menu.hidden = !open;
  ui.pill.setAttribute("aria-expanded", String(open));
};
ui.menu.onclick = (e) => e.stopPropagation();
document.addEventListener("click", closeMenu);

for (const slider of [ui.brightness, ui.volume]) {
  slider.addEventListener("pointerdown", () => (draggingSlider = true));
  slider.addEventListener("pointerup", () => (draggingSlider = false));
}
const sendBrightness = throttled(() => toDevice({ type: "brightness", value: Number(ui.brightness.value) }));
ui.brightness.oninput = () => {
  ui.brightnessValue.textContent = `${ui.brightness.value}%`;
  sendBrightness();
};
const sendVolume = throttled(() => {
  if (!info) return;
  const { min, max } = info.volume;
  const value = min + Math.round((Number(ui.volume.value) * (max - min)) / 100);
  toDevice({ type: "volume", value, mute: false });
});
ui.volume.oninput = () => {
  ui.volumeValue.textContent = `${ui.volume.value}%`;
  sendVolume();
};
ui.mute.onclick = () => {
  if (info) toDevice({ type: "volume", value: info.volume.volume, mute: !info.volume.mute });
};

ui.home.onclick = () => show("modes");
ui.back.onclick = () => show("modes");
ui.disconnect.onclick = () => {
  show("landing");
  wantConnected = false;
  toDevice({ type: "stop" });
  info = null;
};
ui.modeScreen.onclick = () => show("screen");
ui.modePlayer.onclick = () => show("player");

videoWorker.onmessage = (event: MessageEvent<VideoStatus>) => {
  const status = event.data;
  switch (status.type) {
    case "stats":
      console.debug(`${status.fps.toFixed(1)} frames/s, ${(status.rate / 1e6).toFixed(1)} MB/s, quality ${status.quality}`);
      break;
    case "layout":
      console.info(status.text);
      break;
    case "error":
      say(status.message);
      break;
  }
};

let source: { stream: MediaStream; track: MediaStreamTrack } | null = null;
let streaming: { track: MediaStreamTrack; reader: ReadableStreamDefaultReader<VideoFrame> } | null = null;

function renderScreen() {
  const surface = source?.track.getSettings().displaySurface;
  ui.scSource.textContent = source ? (SURFACES[surface ?? ""] ?? source.track.label) : "Not selected";
  ui.scSource.classList.toggle("none", !source);
  ui.scPreview.classList.toggle("live", streaming !== null);
  ui.scAction.innerHTML = streaming
    ? `${icon("stop", 18)}Stop Streaming`
    : `${icon("play", 20)}Start Streaming`;
}

async function pickSource(): Promise<boolean> {
  let stream: MediaStream;
  try {
    stream = await navigator.mediaDevices.getDisplayMedia({ video: { frameRate: { ideal: 60 } }, audio: false });
  } catch {
    return false;
  }
  const wasStreaming = streaming !== null;
  stopReading();
  dropSource();
  const track = stream.getVideoTracks()[0];
  source = { stream, track };
  track.addEventListener("ended", () => {
    if (source?.track !== track) return;
    if (streaming) stopStreaming();
    dropSource();
    renderScreen();
  });
  ui.scVideo.srcObject = stream;
  void ui.scVideo.play().catch(() => {});
  if (wasStreaming) startReading();
  renderScreen();
  return true;
}

function dropSource() {
  if (!source) return;
  for (const track of source.stream.getTracks()) track.stop();
  source = null;
  ui.scVideo.srcObject = null;
}

/** Reads a clone of the source, so stopping the stream leaves the preview
 * running. */
function startReading() {
  if (!source) return;
  const track = source.track.clone();
  const reader = new MediaStreamTrackProcessor({ track }).readable.getReader();
  streaming = { track, reader };
  void (async () => {
    for (;;) {
      const { value: frame, done } = await reader.read().catch(() => ({ value: undefined, done: true as const }));
      if (done || !frame) break;
      const message: FrameMessage = {
        type: "frame",
        frame,
        display: [frame.displayWidth, frame.displayHeight],
        turn: 0,
        color: null,
        preview: false,
      };
      toVideo(message, [frame]);
    }
  })();
}

function stopReading() {
  if (!streaming) return;
  void streaming.reader.cancel().catch(() => {});
  streaming.track.stop();
  streaming = null;
}

async function startStreaming() {
  if (!source && !(await pickSource())) return;
  startReading();
  renderScreen();
}

function stopStreaming() {
  stopReading();
  toVideo({ type: "reset" });
  toDevice({ type: "release" });
  renderScreen();
}

function leaveScreen() {
  if (streaming) stopStreaming();
  dropSource();
}

ui.scChange.onclick = () => void pickSource();
ui.scAction.onclick = () => {
  if (streaming) stopStreaming();
  else void startStreaming();
};

let items: Item[] = [];
let nextId = 1;
let current: number | null = null;
let shuffled: number[] = [];
let shuffle = false;
let repeat: Repeat = "off";
let playing = false;
let playerActive = false;
let position = 0;
let duration: number | null = null;
let seeking = false;
let failures = 0;

function order(): number[] {
  return shuffle ? shuffled : items.map((item) => item.id);
}

function reshuffle() {
  const rest = items.map((item) => item.id).filter((id) => id !== current);
  for (let i = rest.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [rest[i], rest[j]] = [rest[j], rest[i]];
  }
  shuffled = current === null ? rest : [current, ...rest];
}

/** The item `step` places from the current one, wrapping with repeat all. */
function neighbour(step: number): number | null {
  const ids = order();
  if (ids.length === 0) return null;
  const at = current === null ? -1 : ids.indexOf(current);
  let next = at + step;
  if (next < 0 || next >= ids.length) {
    if (repeat !== "all") return null;
    next = (next + ids.length) % ids.length;
  }
  return ids[next];
}

function playItem(id: number, play = true, start = 0) {
  const item = items.find((i) => i.id === id);
  if (!item) return;
  current = id;
  playing = play;
  position = start;
  duration = item.duration ?? null;
  playerActive = true;
  toPlayer({ type: "load", file: item.file, play, start, audioMaxRate: info?.audioMaxRate ?? 0 });
  render();
}

function stopPlayer() {
  if (!playerActive) return;
  toPlayer({ type: "stop" });
  toVideo({ type: "reset" });
  toDevice({ type: "release" });
  playerActive = false;
  current = null;
  playing = false;
  position = 0;
  duration = null;
  render();
}

function advance() {
  const next = neighbour(1);
  if (next === null) stopPlayer();
  else playItem(next);
}

function add(files: Iterable<File>) {
  const added: Item[] = [...files].map((file) => ({
    id: nextId++,
    file,
    duration: undefined,
    thumbnail: null,
    error: null,
  }));
  if (added.length === 0) return;
  items.push(...added);
  for (const { id, file } of added) toPlayer({ type: "probe", id, file });
  if (shuffle) {
    for (const { id } of added) {
      const at = 1 + Math.floor(Math.random() * shuffled.length);
      shuffled.splice(Math.min(at, shuffled.length), 0, id);
    }
  }
  if (current === null) playItem(order().find((id) => added.some((a) => a.id === id))!);
  render();
}

function remove(id: number) {
  const thumbnail = items.find((item) => item.id === id)?.thumbnail;
  if (thumbnail) URL.revokeObjectURL(thumbnail);
  if (id === current) {
    const next = neighbour(1);
    items = items.filter((item) => item.id !== id);
    shuffled = shuffled.filter((s) => s !== id);
    if (next === null || next === id) stopPlayer();
    else playItem(next);
  } else {
    items = items.filter((item) => item.id !== id);
    shuffled = shuffled.filter((s) => s !== id);
  }
  render();
}

function row(item: Item): HTMLLIElement {
  const li = document.createElement("li");
  li.className = "pl-row";
  li.classList.toggle("current", item.id === current);
  li.draggable = true;
  const select = document.createElement("button");
  select.type = "button";
  select.className = "pl-select";
  select.setAttribute("aria-label", `Play ${item.file.name}`);
  select.innerHTML = `
    <span class="pl-thumb" style="background: ${THUMBS[item.id % THUMBS.length]}">${
      item.thumbnail ? `<img src="${item.thumbnail}" alt="" />` : icon("movie", 20)
    }</span>
    <span class="pl-text"><span class="pl-name"></span><span class="pl-meta"></span></span>`;
  select.querySelector(".pl-name")!.textContent = item.file.name;
  const meta = select.querySelector<HTMLSpanElement>(".pl-meta")!;
  meta.textContent = item.error ?? (item.duration ? clock(item.duration) : "--:--");
  meta.classList.toggle("error", item.error !== null);
  select.onclick = () => playItem(item.id);
  const close = document.createElement("button");
  close.type = "button";
  close.className = "icon-btn pl-remove";
  close.setAttribute("aria-label", `Remove ${item.file.name} from playlist`);
  close.innerHTML = icon("x", 18);
  close.onclick = () => remove(item.id);
  li.append(select, close);
  li.ondragstart = (e) => e.dataTransfer?.setData("text/x-queue", String(item.id));
  li.ondragover = (e) => {
    if (e.dataTransfer?.types.includes("text/x-queue")) e.preventDefault();
  };
  li.ondrop = (e) => {
    const from = Number(e.dataTransfer?.getData("text/x-queue"));
    if (!from || from === item.id) return;
    e.preventDefault();
    e.stopPropagation();
    const moved = items.find((i) => i.id === from)!;
    items = items.filter((i) => i.id !== from);
    items.splice(items.indexOf(item), 0, moved);
    render();
  };
  return li;
}

function render() {
  ui.queue.replaceChildren(...items.map(row));
  ui.count.textContent = `${items.length} ${items.length === 1 ? "video" : "videos"}`;
  ui.nowName.textContent = items.find((i) => i.id === current)?.file.name ?? "—";
  ui.play.innerHTML = playing ? icon("pause", 28) : icon("play", 28);
  ui.play.classList.toggle("paused", !playing);
  ui.play.setAttribute("aria-label", playing ? "Pause" : "Play");
  ui.shuffle.classList.toggle("on", shuffle);
  ui.shuffle.setAttribute("aria-pressed", String(shuffle));
  ui.repeat.innerHTML = icon(repeat === "one" ? "repeatOnce" : "repeat", 22);
  ui.repeat.classList.toggle("on", repeat !== "off");
  ui.repeat.setAttribute("aria-label", { off: "Repeat: off", all: "Repeat: all", one: "Repeat: one" }[repeat]);
  const empty = items.length === 0;
  for (const button of [ui.prev, ui.play, ui.next]) button.disabled = empty;
  ui.seek.disabled = current === null || !duration;
  ui.seek.max = String(duration ?? 0);
  if (!seeking) ui.seek.value = String(position / 1e6);
  ui.time.textContent = clock(seeking ? Number(ui.seek.value) : position / 1e6);
  ui.duration.textContent = clock(duration ?? 0);
}

playerWorker.onmessage = (event: MessageEvent<PlayerStatus>) => {
  const status = event.data;
  switch (status.type) {
    case "loaded":
      duration = status.duration;
      failures = 0;
      break;
    case "probed": {
      const item = items.find((i) => i.id === status.id);
      if (!item) break;
      item.duration = status.duration;
      if (status.thumbnail) item.thumbnail = URL.createObjectURL(status.thumbnail);
      break;
    }
    case "position":
      position = status.us;
      break;
    case "ended":
      if (repeat === "one" && current !== null) playItem(current);
      else advance();
      return;
    case "warning":
      say(status.message);
      break;
    case "error": {
      const item = items.find((i) => i.id === current);
      if (item) item.error = status.message;
      say(`${item?.file.name ?? "Playback"}: ${status.message}`);
      failures += 1;
      if (failures >= items.length) stopPlayer();
      else advance();
      return;
    }
  }
  render();
};

ui.add.onclick = () => ui.files.click();
ui.files.onchange = () => {
  add(ui.files.files ?? []);
  ui.files.value = "";
};
ui.player.addEventListener("dragover", (e) => {
  if (e.dataTransfer?.types.includes("Files")) {
    e.preventDefault();
    ui.drop.classList.add("over");
  }
});
ui.player.addEventListener("dragleave", () => ui.drop.classList.remove("over"));
ui.player.addEventListener("drop", (e) => {
  ui.drop.classList.remove("over");
  if (!e.dataTransfer?.files.length) return;
  e.preventDefault();
  add(e.dataTransfer.files);
});

ui.play.onclick = () => {
  if (current === null) {
    const first = order()[0];
    if (first !== undefined) playItem(first);
    return;
  }
  playing = !playing;
  toPlayer({ type: playing ? "play" : "pause" });
  render();
};
ui.next.onclick = () => {
  const next = neighbour(1);
  if (next !== null) playItem(next, playing || current === null);
};
ui.prev.onclick = () => {
  const previous = neighbour(-1);
  if (current !== null && (position > RESTART_US || previous === null)) {
    toPlayer({ type: "seek", us: 0 });
    position = 0;
    render();
  } else if (previous !== null) {
    playItem(previous, playing || current === null);
  }
};
ui.repeat.onclick = () => {
  repeat = repeat === "off" ? "all" : repeat === "all" ? "one" : "off";
  render();
};
ui.shuffle.onclick = () => {
  shuffle = !shuffle;
  if (shuffle) reshuffle();
  render();
};
ui.seek.oninput = () => {
  seeking = true;
  ui.time.textContent = clock(Number(ui.seek.value));
};
ui.seek.onchange = () => {
  seeking = false;
  position = Number(ui.seek.value) * 1e6;
  toPlayer({ type: "seek", us: position });
  render();
};

show("landing");
