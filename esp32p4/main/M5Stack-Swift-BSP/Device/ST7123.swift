enum ST7123 {
    class Display<PixelFormat: Pixel> {
        private let ledcTimer: IDF.LEDControl.Timer
        private let backlight: IDF.LEDControl
        private let phyPowerChannel: esp_ldo_channel_handle_t?
        private let mipiDsiBus: esp_lcd_dsi_bus_handle_t
        let io: esp_lcd_panel_io_handle_t
        let panel: esp_lcd_panel_handle_t
        let size: Size
        let frameBuffers: [UnsafeMutableBufferPointer<PixelFormat>]
        private let onRefreshDone: FFI.Wrapper<() -> Void>?

        init(
            backlightGpio: IDF.GPIO.Pin,
            size: Size,
            pixelFormat: PixelFormat.Type = RGB565.self,
            fbNum: Int = 1,
            onRefreshDone: (() -> Void)? = nil
        ) throws(IDF.Error) {
            let rgb888 = MemoryLayout<PixelFormat>.size == 3

            // Setup Backlight
            ledcTimer = try IDF.LEDControl.makeTimer(dutyResolution: 12, freqHz: 5000)
            backlight = try IDF.LEDControl(gpio: backlightGpio, timer: ledcTimer)

            // Enable DSI PHY power
            var ldoConfig = esp_ldo_channel_config_t(
                chan_id: 3,
                voltage_mv: 2500,
                flags: ldo_extra_flags()
            )
            var phyPowerChannel: esp_ldo_channel_handle_t?
            try IDF.Error.check(esp_ldo_acquire_channel(&ldoConfig, &phyPowerChannel))
            self.phyPowerChannel = phyPowerChannel

            // Create MIPI DSI Bus
            var busConfig = esp_lcd_dsi_bus_config_t(
                bus_id: 0,
                num_data_lanes: 2,
                phy_clk_src: MIPI_DSI_PHY_PLLREF_CLK_SRC_PLL_F20M,
                lane_bit_rate_mbps: 965,
            )
            var mipiDsiBus: esp_lcd_dsi_bus_handle_t?
            try IDF.Error.check(esp_lcd_new_dsi_bus(&busConfig, &mipiDsiBus))
            self.mipiDsiBus = mipiDsiBus!

            // Install MIPI DSI LCD control panel
            var dbiConfig = esp_lcd_dbi_io_config_t(virtual_channel: 0, lcd_cmd_bits: 8, lcd_param_bits: 8)
            var io: esp_lcd_panel_io_handle_t?
            try IDF.Error.check(esp_lcd_new_panel_io_dbi(mipiDsiBus, &dbiConfig, &io))
            self.io = io!

            // Install LCD Driver of ST7123
            var dpiConfig = esp_lcd_dpi_panel_config_t(
                virtual_channel: 0,
                dpi_clk_src: MIPI_DSI_DPI_CLK_SRC_DEFAULT,
                dpi_clock_freq_mhz: 70,
                pixel_format: rgb888 ? LCD_COLOR_PIXEL_FORMAT_RGB888 : LCD_COLOR_PIXEL_FORMAT_RGB565,
                in_color_format: lcd_color_format_t(0),
                out_color_format: lcd_color_format_t(0),
                num_fbs: UInt8(fbNum),
                video_timing: esp_lcd_video_timing_t(
                    h_size: UInt32(size.width),
                    v_size: UInt32(size.height),
                    hsync_pulse_width: 2,
                    hsync_back_porch: 40,
                    hsync_front_porch: 40,
                    vsync_pulse_width: 2,
                    vsync_back_porch: 8,
                    vsync_front_porch: 220
                ),
                flags: extra_dpi_panel_flags(use_dma2d: 1, disable_lp: 0)
            )
            self.panel = try withUnsafePointer(to: &dpiConfig) { ptr throws(IDF.Error) -> esp_lcd_panel_handle_t in
                var vendorConfig = st7123_vendor_config_t(
                    init_cmds: bsp_lcd_st7123_specific_init_code_default_ptr,
                    init_cmds_size: bsp_lcd_st7123_specific_init_code_default_num,
                    mipi_config: st7123_vendor_config_t.__Unnamed_struct_mipi_config(
                        dsi_bus: mipiDsiBus,
                        dpi_config: ptr
                    )
                )
                return try withUnsafeMutablePointer(to: &vendorConfig) { ptr throws(IDF.Error) -> esp_lcd_panel_handle_t in
                    var lcdDevConfig = esp_lcd_panel_dev_config_t(
                        reset_gpio_num: -1,
                        esp_lcd_panel_dev_config_t.__Unnamed_union___Anonymous_field1(
                            rgb_ele_order: LCD_RGB_ELEMENT_ORDER_RGB
                        ),
                        data_endian: LCD_RGB_DATA_ENDIAN_LITTLE,
                        bits_per_pixel: 24,
                        flags: esp_lcd_panel_dev_config_t.__Unnamed_struct_flags(),
                        vendor_config: ptr
                    )

                    var dispPanel: esp_lcd_panel_handle_t?
                    try IDF.Error.check(esp_lcd_new_panel_st7123(io, &lcdDevConfig, &dispPanel))
                    try IDF.Error.check(esp_lcd_panel_reset(dispPanel))
                    try IDF.Error.check(esp_lcd_panel_init(dispPanel))
                    try IDF.Error.check(esp_lcd_panel_disp_on_off(dispPanel, true))
                    return dispPanel!
                }
            }
            self.size = size

            var fb0: UnsafeMutableRawPointer?, fb1: UnsafeMutableRawPointer?, fb2: UnsafeMutableRawPointer?
            esp_lcd_dpi_panel_get_frame_buffers(panel, UInt32(fbNum), &fb0, &fb1, &fb2)
            self.frameBuffers = [fb0, fb1, fb2].enumerated().compactMap({ i, ptr in
                if i >= fbNum { return nil }
                let typedPointer = ptr!.bindMemory(to: PixelFormat.self, capacity: size.area)
                return UnsafeMutableBufferPointer<PixelFormat>(start: typedPointer, count: size.area)
            })

            if let onRefreshDone = onRefreshDone {
                let wrapper = FFI.Wrapper<() -> Void>(onRefreshDone)
                var callbacks = esp_lcd_dpi_panel_event_callbacks_t()
                callbacks.on_refresh_done = { (panel, edata, user_ctx) in
                    let cb = Unmanaged<FFI.Wrapper<() -> Void>>.fromOpaque(user_ctx!).takeUnretainedValue()
                    cb.value()
                    return false
                }
                esp_lcd_dpi_panel_register_event_callbacks(panel, &callbacks, Unmanaged.passUnretained(wrapper).toOpaque())
                self.onRefreshDone = wrapper
            } else {
                self.onRefreshDone = nil
            }
        }

        var brightness: Int = 0 {
            didSet {
                backlight.setDutyFloat(Float(brightness) / 100.0)
            }
        }

        func drawBitmap(rect: Rect, data: UnsafeRawPointer) {
            esp_lcd_panel_draw_bitmap(panel, Int32(rect.minX), Int32(rect.minY), Int32(rect.maxX), Int32(rect.maxY), data)
        }

        func flush(fbNum: Int = 0) {
            drawBitmap(rect: Rect(origin: .zero, size: size), data: frameBuffers[fbNum].baseAddress!)
        }
    }

    class Touch {
        var ioHandle: esp_lcd_panel_io_handle_t
        var handle: esp_lcd_touch_handle_t
        let interrupt: (semaphore: Semaphore, gpio: IDF.GPIO.Pin)?

        init(
            i2c: IDF.I2C,
            size: (width: UInt16, height: UInt16),
            int: IDF.GPIO.Pin?,
            rst: IDF.GPIO.Pin?,
            sclSpeedHz: UInt32,
        ) throws(IDF.Error) {
            // Setup IO
            var ioHandle: esp_lcd_panel_handle_t?
            var ioConfig = esp_lcd_panel_io_i2c_config_t()
            _ESP_LCD_TOUCH_IO_I2C_ST7123_CONFIG(&ioConfig)
            ioConfig.dev_addr = UInt32(ESP_LCD_TOUCH_IO_I2C_ST7123_ADDRESS)
            ioConfig.scl_speed_hz = sclSpeedHz
            try IDF.Error.check(esp_lcd_new_panel_io_i2c_v2(i2c.handle, &ioConfig, &ioHandle))
            self.ioHandle = ioHandle!

            // Init ST7123 Touch
            var config = esp_lcd_touch_config_t()
            config.x_max = size.width
            config.y_max = size.height
            config.rst_gpio_num = rst?.value ?? GPIO_NUM_NC
            config.int_gpio_num = int?.value ?? GPIO_NUM_NC

            var handle: esp_lcd_touch_handle_t?
            try IDF.Error.check(esp_lcd_touch_new_i2c_st7123(ioHandle, &config, &handle))
            self.handle = handle!

            if let intGpio = int {
                let semaphore = Semaphore.createBinary()
                if semaphore == nil {
                    throw IDF.Error(ESP_ERR_NO_MEM)
                }
                self.interrupt = (semaphore: semaphore!, gpio: intGpio)

                try IDF.Error.check(esp_lcd_touch_register_interrupt_callback_with_data(handle, {
                    let semaphore = Unmanaged<Semaphore>.fromOpaque($0!.pointee.config.user_data!).takeUnretainedValue()
                    semaphore.give()
                }, Unmanaged.passUnretained(semaphore!).toOpaque()))
            } else {
                self.interrupt = nil
            }
        }

        func waitInterrupt() {
            interrupt?.semaphore.take()
        }

        var coordinates: [Point] {
            get throws(IDF.Error) {
                try IDF.Error.check(esp_lcd_touch_read_data(handle))
                return withUnsafeTemporaryAllocation(of: esp_lcd_touch_point_data_t.self, capacity: 5) { ptr in
                    var touchCount: UInt8 = 0
                    esp_lcd_touch_get_data(handle, ptr.baseAddress, &touchCount, 5)
                    return (0..<Int(touchCount)).map { Point(x: Int(ptr[$0].x), y: Int(ptr[$0].y)) }
                }
            }
        }
    }
}
