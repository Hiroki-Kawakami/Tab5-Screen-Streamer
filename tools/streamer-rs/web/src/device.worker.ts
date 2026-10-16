// Runs the device sessions off the page, whose timers are throttled while it
// is hidden. Answers `ready` with usb false where workers have no WebUSB.

import { DeviceManager } from "./device";
import type { DeviceCommand, DeviceStatus } from "./messages";

const usb = navigator.usb;
const status = (message: DeviceStatus) => postMessage(message);
let manager: Promise<DeviceManager> | null = null;

self.onmessage = async (event: MessageEvent<DeviceCommand>) => {
  const command = event.data;
  if (command.type === "init") {
    manager = DeviceManager.create(usb!, status, command.video, command.audio);
    return;
  }
  (await manager)?.command(command);
};

status({ type: "ready", usb: usb !== undefined });
