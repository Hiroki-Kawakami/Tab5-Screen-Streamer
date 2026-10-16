import alertTriangle from "@tabler/icons/outline/alert-triangle.svg?raw";
import arrowsShuffle from "@tabler/icons/outline/arrows-shuffle.svg?raw";
import brandGithub from "@tabler/icons/outline/brand-github.svg?raw";
import chevronDown from "@tabler/icons/outline/chevron-down.svg?raw";
import deviceDesktop from "@tabler/icons/outline/device-desktop.svg?raw";
import layoutGrid from "@tabler/icons/outline/layout-grid.svg?raw";
import movie from "@tabler/icons/outline/movie.svg?raw";
import plugConnectedX from "@tabler/icons/outline/plug-connected-x.svg?raw";
import plus from "@tabler/icons/outline/plus.svg?raw";
import refresh from "@tabler/icons/outline/refresh.svg?raw";
import repeat from "@tabler/icons/outline/repeat.svg?raw";
import repeatOnce from "@tabler/icons/outline/repeat-once.svg?raw";
import screenShare from "@tabler/icons/outline/screen-share.svg?raw";
import sun from "@tabler/icons/outline/sun.svg?raw";
import upload from "@tabler/icons/outline/upload.svg?raw";
import usb from "@tabler/icons/outline/usb.svg?raw";
import volume from "@tabler/icons/outline/volume.svg?raw";
import volume2 from "@tabler/icons/outline/volume-2.svg?raw";
import volume3 from "@tabler/icons/outline/volume-3.svg?raw";
import x from "@tabler/icons/outline/x.svg?raw";
import pause from "@tabler/icons/filled/player-pause.svg?raw";
import play from "@tabler/icons/filled/player-play.svg?raw";
import skipBack from "@tabler/icons/filled/player-skip-back.svg?raw";
import skipForward from "@tabler/icons/filled/player-skip-forward.svg?raw";
import stop from "@tabler/icons/filled/player-stop.svg?raw";
import appIcon from "../../../../assets/app-icon.svg?raw";

export const icons = {
  alertTriangle,
  arrowsShuffle,
  brandGithub,
  chevronDown,
  deviceDesktop,
  layoutGrid,
  movie,
  plugConnectedX,
  plus,
  refresh,
  repeat,
  repeatOnce,
  screenShare,
  sun,
  upload,
  usb,
  volume,
  volume2,
  volume3,
  x,
  pause,
  play,
  skipBack,
  skipForward,
  stop,
  appIcon,
};

export type IconName = keyof typeof icons;

/** `name` as inline SVG markup, `size` px square. */
export function icon(name: IconName, size: number, strokeWidth?: number): string {
  let svg = icons[name]
    .replace(/<\?xml[^>]*\?>\s*/, "")
    .replace(/<title>[^<]*<\/title>/, "")
    .replace(/\s(width|height)="\d+"/g, "")
    .replace(/\s(class|id)="[^"]*"/g, "")
    .replace(/<svg/, `<svg aria-hidden="true" width="${size}" height="${size}"`);
  if (strokeWidth !== undefined) svg = svg.replace(/stroke-width="2"/, `stroke-width="${strokeWidth}"`);
  return svg;
}
