extension LVGL {
    class Display {
        let disp: OpaquePointer!
        let flushCallback: (Display, UnsafeMutablePointer<UInt8>) -> Void
        init(disp: OpaquePointer!, flushCallback: @escaping (Display, UnsafeMutablePointer<UInt8>) -> Void) {
            self.disp = disp
            self.flushCallback = flushCallback
        }

        static func createDirectBufferDisplay(
            buffer: UnsafeMutableRawPointer?,
            buffer2: UnsafeMutableRawPointer? = nil,
            size: Size,
            flushCallback: @escaping (Display, UnsafeMutablePointer<UInt8>) -> Void
        ) -> Display {
            let disp = lv_display_create(Int32(size.width), Int32(size.height))
            let display = Display(disp: disp, flushCallback: flushCallback)

            lv_display_set_buffers(disp, buffer, nil, UInt32(size.area * MemoryLayout<lv_color_t>.size), LV_DISPLAY_RENDER_MODE_DIRECT)
            lv_display_set_user_data(disp, Unmanaged.passRetained(display).toOpaque())
            lv_display_set_flush_cb(disp, { disp, area, px_map in
                let user_data = lv_display_get_user_data(disp)
                let display = Unmanaged<Display>.fromOpaque(user_data!).takeUnretainedValue()
                display.flushCallback(display, px_map!)
            })
            return display
        }

        func setDefault() { lv_display_set_default(disp) }
        func flushReady() { lv_display_flush_ready(disp) }
    }

    struct Screen: ObjectProtocol {
        let obj: OpaquePointer

        static var active: Screen { Screen(obj: lv_screen_active()) }
    }
}
