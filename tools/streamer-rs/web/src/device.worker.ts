// Runs the device session off the page, whose timers are throttled while it
// is hidden. Answers `ready` with usb false where workers have no WebUSB.

import { DeviceHost } from "./device";
import type { DeviceCommand, DeviceStatus } from "./messages";

const usb = navigator.usb;
const status = (message: DeviceStatus) => postMessage(message);
let host: Promise<DeviceHost> | null = null;

self.onmessage = async (event: MessageEvent<DeviceCommand>) => {
  const command = event.data;
  if (command.type === "init") {
    host = DeviceHost.create(usb!, status, command.video, command.audio);
    return;
  }
  (await host)?.command(command);
};

status({ type: "ready", usb: usb !== undefined });
