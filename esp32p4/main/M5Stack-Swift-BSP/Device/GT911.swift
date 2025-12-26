class GT911 {
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
        _ESP_LCD_TOUCH_IO_I2C_GT911_CONFIG(&ioConfig)
        ioConfig.dev_addr = UInt32(ESP_LCD_TOUCH_IO_I2C_GT911_ADDRESS_BACKUP)
        ioConfig.scl_speed_hz = sclSpeedHz
        try IDF.Error.check(esp_lcd_new_panel_io_i2c_v2(i2c.handle, &ioConfig, &ioHandle))
        self.ioHandle = ioHandle!

        // Init GT911
        var config = esp_lcd_touch_config_t()
        config.x_max = size.width
        config.y_max = size.height
        config.rst_gpio_num = rst?.value ?? GPIO_NUM_NC
        config.int_gpio_num = int?.value ?? GPIO_NUM_NC

        var handle: esp_lcd_touch_handle_t?
        try IDF.Error.check(esp_lcd_touch_new_i2c_gt911(ioHandle, &config, &handle))
        self.handle = handle!

        if let intGpio = int {
            let semaphore = Semaphore.createBinary()
            if semaphore == nil {
                throw IDF.Error(ESP_ERR_NO_MEM)
            }
            self.interrupt = (semaphore: semaphore!, gpio: intGpio)

            try exitSleep()
            try IDF.Error.check(esp_lcd_touch_register_interrupt_callback_with_data(handle, {
                let semaphore = Unmanaged<Semaphore>.fromOpaque($0!.pointee.config.user_data!).takeUnretainedValue()
                semaphore.give()
            }, Unmanaged.passUnretained(semaphore!).toOpaque()))
        } else {
            self.interrupt = nil
            try exitSleep()
        }
    }

    private func exitSleep() throws(IDF.Error) {
        try IDF.Error.check(esp_lcd_touch_exit_sleep(handle))
        if let int = interrupt?.gpio {
            var gpioConfig = gpio_config_t()
            gpioConfig.mode = GPIO_MODE_INPUT
            gpioConfig.pin_bit_mask = 1 << int.rawValue
            gpioConfig.intr_type = GPIO_INTR_NEGEDGE
            try IDF.Error.check(gpio_config(&gpioConfig))
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
