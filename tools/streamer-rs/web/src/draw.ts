/** Draws `frame` fitted and centred on the whole canvas, shown at `display`
 * after `turn` counter-clockwise quarter turns. */
export function drawFrame(
  context: OffscreenCanvasRenderingContext2D,
  frame: VideoFrame,
  display: [number, number],
  turn: number,
) {
  const { canvas } = context;
  const [w, h] = display;
  const [rw, rh] = turn % 2 === 1 ? [h, w] : [w, h];
  const scale = Math.min(canvas.width / rw, canvas.height / rh);
  context.save();
  context.translate(canvas.width / 2, canvas.height / 2);
  context.rotate((-turn * Math.PI) / 2);
  context.drawImage(frame, (-w * scale) / 2, (-h * scale) / 2, w * scale, h * scale);
  context.restore();
}
