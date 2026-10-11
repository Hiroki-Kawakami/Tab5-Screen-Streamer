//! Where frames land on the panel.

pub enum Layout {
    /// Panel-wide rows, `height` tall from row `y`; the source is rotated,
    /// scaled to `scaled_width` and padded to the panel width before sending.
    Direct {
        scaled_width: u32,
        height: u32,
        y: u32,
        rotation: u32,
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
    pub fn direct(source: (u32, u32), rotation: u32, panel: (u32, u32), align: u32) -> Self {
        let (sw, sh) = rotated(source, rotation);
        let (pw, ph) = panel;
        let scale = (pw as f64 / sw as f64).min(ph as f64 / sh as f64);
        let scaled_width = (((sw as f64 * scale).round() as u32) / 2 * 2).clamp(2, pw);
        let height = (((sh as f64 * scale) / align as f64).round() as u32 * align)
            .clamp(align, ph / align * align);
        Layout::Direct {
            scaled_width,
            height,
            y: (ph - height) / 2 / align * align,
            rotation,
        }
    }

    /// Shrinks the source to fit `max` on both sides in multiples of 16, so any
    /// whole number of sixteenths scales it to whole pixels.
    pub fn blit(
        source: (u32, u32),
        rotation: u32,
        panel: (u32, u32),
        max: (u32, u32),
    ) -> Result<Self, String> {
        let shrink = (max.0 as f64 / source.0 as f64)
            .min(max.1 as f64 / source.1 as f64)
            .min(1.0);
        let side = |length: u32, limit: u32| {
            (((length as f64 * shrink / 16.0).round() as u32) * 16).clamp(16, limit / 16 * 16)
        };
        let (width, height) = (side(source.0, max.0), side(source.1, max.1));
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

    /// The size of the frames sent.
    pub fn frame(&self, panel_width: u32) -> (u32, u32) {
        match *self {
            Layout::Direct { height, .. } => (panel_width, height),
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
