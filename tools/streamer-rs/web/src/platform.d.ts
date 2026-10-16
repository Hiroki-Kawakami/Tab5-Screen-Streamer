// The parts of WebUSB, MediaStreamTrackProcessor and FileReaderSync used here,
// which the DOM library does not declare.

interface USBEndpoint {
  readonly endpointNumber: number;
  readonly direction: "in" | "out";
  readonly packetSize: number;
}

interface USBAlternateInterface {
  readonly endpoints: USBEndpoint[];
}

interface USBInterface {
  readonly interfaceNumber: number;
  readonly alternate: USBAlternateInterface;
}

interface USBConfiguration {
  readonly interfaces: USBInterface[];
}

interface USBInTransferResult {
  readonly data?: DataView;
  readonly status: "ok" | "stall" | "babble";
}

interface USBOutTransferResult {
  readonly bytesWritten: number;
  readonly status: "ok" | "stall";
}

interface USBDevice {
  readonly vendorId: number;
  readonly productId: number;
  readonly serialNumber?: string | null;
  readonly opened: boolean;
  readonly configuration: USBConfiguration | null;
  open(): Promise<void>;
  close(): Promise<void>;
  selectConfiguration(value: number): Promise<void>;
  claimInterface(number: number): Promise<void>;
  transferIn(endpoint: number, length: number): Promise<USBInTransferResult>;
  transferOut(endpoint: number, data: BufferSource): Promise<USBOutTransferResult>;
}

interface USBConnectionEvent extends Event {
  readonly device: USBDevice;
}

interface USB extends EventTarget {
  getDevices(): Promise<USBDevice[]>;
  requestDevice(options: { filters: { vendorId?: number; productId?: number }[] }): Promise<USBDevice>;
}

interface Navigator {
  readonly usb?: USB;
}

interface WorkerNavigator {
  readonly usb?: USB;
}

declare class FileReaderSync {
  readAsArrayBuffer(blob: Blob): ArrayBuffer;
}

declare class MediaStreamTrackProcessor {
  constructor(init: { track: MediaStreamTrack });
  readonly readable: ReadableStream<VideoFrame>;
}
