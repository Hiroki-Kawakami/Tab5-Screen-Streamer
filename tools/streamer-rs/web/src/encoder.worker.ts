// Encodes one band of a frame as a JPEG for the video worker.

import init, { encodeBand } from "./wasm/streamer.js";
import type { BandJob } from "./messages";

const ready = init();

self.onmessage = async (event: MessageEvent<BandJob>) => {
  await ready;
  const job = event.data;
  const jpeg = encodeBand(
    job.picture,
    job.width,
    job.height,
    job.yuv,
    job.rotation,
    job.canvasWidth,
    job.canvasHeight,
    job.originX,
    job.originY,
    job.quality,
    job.row,
    job.rows,
  );
  postMessage(jpeg, { transfer: [jpeg.buffer] });
};
