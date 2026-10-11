//! Where frames land on the panel.

use crate::jpeg::Placement;

pub enum Layout {
    /// `frame` sized frames, rotated and centered on the panel-wide rows
    /// [y, y + height).
    Direct {
        frame: (u32, u32),
        rotation: u32,
        y: u32,
        height: u32,
    },
    /// `width` x `height` frames the device rotates by `rotation` and scales by
    /// `scale` / 16 into `area` (x, y, w, h).
    Blit {
        width: u32,
        height: u32,
        rotation: u32,
        scale: u32,
        area: (u32, u32, u32, u32),
    },
}

pub fn rotated(size: (u32, u32), rotation: u32) -> (u32, u32) {
    if rotation % 2 == 1 {
        (size.1, size.0)
    } else {
        size
    }
}

impl Layout {
    /// Fits `source` to the panel, or takes it as is when `scale` is false.
    pub fn direct(
        source: (u32, u32),
        rotation: u32,
        panel: (u32, u32),
        align: u32,
        scale: bool,
    ) -> Result<Self, String> {
        let (sw, sh) = rotated(source, rotation);
        let (pw, ph) = panel;
        let (cw, ch) = if scale {
            let fit = (pw as f64 / sw as f64).min(ph as f64 / sh as f64);
            let even = |v: f64, max: u32| ((v.round() as u32) / 2 * 2).clamp(2, max);
            (even(sw as f64 * fit, pw), even(sh as f64 * fit, ph))
        } else if sw > pw || sh > ph {
            return Err(format!("{sw}x{sh} is larger than the {pw}x{ph} panel"));
        } else {
            (sw, sh)
        };
        let height = ch.div_ceil(align) * align;
        let height = height.min(ph / align * align);
        Ok(Layout::Direct {
            frame: rotated((cw, ch), rotation),
            rotation,
            y: (ph - height) / 2 / align * align,
            height,
        })
    }

    /// Shrinks the source to fit `max` on both sides in multiples of 16, so any
    /// whole number of sixteenths scales it to whole pixels. Without `scale`
    /// the source has to be that size already.
    pub fn blit(
        source: (u32, u32),
        rotation: u32,
        panel: (u32, u32),
        max: (u32, u32),
        scale: bool,
    ) -> Result<Self, String> {
        let shrink = (max.0 as f64 / source.0 as f64)
            .min(max.1 as f64 / source.1 as f64)
            .min(1.0);
        let side = |length: u32, limit: u32| {
            (((length as f64 * shrink / 16.0).round() as u32) * 16).clamp(16, limit / 16 * 16)
        };
        let (width, height) = (side(source.0, max.0), side(source.1, max.1));
        if !scale && (width, height) != source {
            return Err(format!(
                "{}x{} does not blit; send {width}x{height}",
                source.0, source.1
            ));
        }
        let (rw, rh) = rotated((width, height), rotation);
        let (pw, ph) = panel;
        let scale = (16 * pw / rw).min(16 * ph / rh);
        if scale == 0 {
            return Err(format!("{rw}x{rh} does not fit the panel"));
        }
        let (dw, dh) = (rw * scale / 16, rh * scale / 16);
        Ok(Layout::Blit {
            width,
            height,
            rotation,
            scale,
            area: ((pw - dw) / 2, (ph - dh) / 2, dw, dh),
        })
    }

    /// The size of the frames taken.
    pub fn frame(&self) -> (u32, u32) {
        match *self {
            Layout::Direct { frame, .. } => frame,
            Layout::Blit { width, height, .. } => (width, height),
        }
    }

    /// The panel area the frames cover (x, y, w, h).
    pub fn content(&self, panel_width: u32) -> (u32, u32, u32, u32) {
        match *self {
            Layout::Direct { height, y, .. } => (0, y, panel_width, height),
            Layout::Blit { area, .. } => area,
        }
    }

    /// How a frame is drawn into the pixels sent: the rows of `content` for
    /// DIRECT, the frame as is for BLIT.
    pub fn placement(&self, panel_width: u32) -> Placement {
        match *self {
            Layout::Direct {
                frame,
                rotation,
                height,
                ..
            } => {
                let (rw, rh) = rotated(frame, rotation);
                let canvas = (panel_width as usize, height as usize);
                Placement {
                    rotation,
                    canvas,
                    origin: (
                        (panel_width - rw) as usize / 4 * 2,
                        (height - rh) as usize / 4 * 2,
                    ),
                }
            }
            Layout::Blit { width, height, .. } => Placement {
                rotation: 0,
                canvas: (width as usize, height as usize),
                origin: (0, 0),
            },
        }
    }

    /// Where source rows [row, row + rows) of a blit land: the same strip of
    /// the rotated, scaled source.
    pub fn blit_band(&self, row: u32, rows: u32) -> (u32, u32, u32, u32) {
        let Layout::Blit {
            rotation,
            scale,
            area: (x, y, w, h),
            ..
        } = *self
        else {
            unreachable!()
        };
        let start = row * scale / 16;
        let extent = rows * scale / 16;
        match rotation {
            1 => (x + start, y, extent, h),
            2 => (x, y + h - start - extent, w, extent),
            3 => (x + w - start - extent, y, extent, h),
            _ => (x, y + start, w, extent),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_fits_and_centers() {
        let layout = Layout::direct((1920, 1080), 1, (720, 1280), 16, true).unwrap();
        let Layout::Direct {
            frame, y, height, ..
        } = layout
        else {
            unreachable!()
        };
        assert_eq!(frame, (1280, 720));
        assert_eq!((y, height), (0, 1280));
        let layout = Layout::direct((1920, 1080), 0, (720, 1280), 16, true).unwrap();
        assert_eq!(layout.frame(), (720, 404));
        let placement = layout.placement(720);
        assert_eq!(placement.canvas, (720, 416));
        assert_eq!(placement.origin, (0, 6));
        assert_eq!(layout.content(720), (0, 432, 720, 416));
    }

    #[test]
    fn unscaled_direct_takes_the_source_or_fails() {
        let layout = Layout::direct((640, 360), 0, (720, 1280), 16, false).unwrap();
        assert_eq!(layout.frame(), (640, 360));
        assert_eq!(layout.placement(720).origin, (40, 4));
        assert!(Layout::direct((1280, 720), 0, (720, 1280), 16, false).is_err());
    }
}
