fileprivate let Log = Logger(tag: "M5StackTab5")

class M5StackTab5<PixelFormat: Pixel> {
    static func begin(
        pixelFormat: PixelFormat.Type = RGB565.self,
        frameBufferNum: Int = 1,
        onRefreshDone: (() -> Void)? = nil,
        touchInterrupt: Bool = true
    ) throws(IDF.Error) -> M5StackTab5 {
        let i2c0 = try IDF.I2C(num: 0, scl: .gpio32, sda: .gpio31)
        let pi4ioe1 = try PI4IO(i2c: i2c0, address: 0x43, pins: (
            .output(defaultState: false), // RF_INT_EXT_SWITCH
            .output(defaultState: true),  // SPK_EN
            .output(defaultState: true),  // EXT5V_EN
            .none,
            .output(defaultState: true),  // LCD_RST
            .output(defaultState: true),  // TP_RST
            .output(defaultState: true),  // CAM_RST
            .input(pull: .none),          // HP_DET
        ))
        let pi4ioe2 = try PI4IO(i2c: i2c0, address: 0x44, pins: (
            .output(defaultState: true),  // WLAN_PWR_EN
            .none,
            .none,
            .output(defaultState: false), // USB5V_EN
            .output(defaultState: false), // PWROFF_PLUSE
            .output(defaultState: false), // nCHG_QC_EN
            .input(pull: .none),          // CHG_STAT
            .output(defaultState: false), // CHG_EN
        ))

        // Reset Touch Panel
        try IDF.GPIO.reset(pin: .gpio23)
        pi4ioe1.output &= ~(0b11 << 4)
        Task.delay(100)
        pi4ioe1.output |= (0b11 << 4)
        Task.delay(100)

        let display: Display
        let touch: Touch
        if i2c0.probe(address: 0x55) { // ST7123
            let st7123Display = try ST7123.Display(
                backlightGpio: .gpio22,
                size: Size(width: 720, height: 1280),
                pixelFormat: pixelFormat,
                fbNum: frameBufferNum,
                onRefreshDone: onRefreshDone
            )
            let st7123Touch = try ST7123.Touch(
                i2c: i2c0,
                size: (width: 720, height: 1280),
                int: touchInterrupt ? .gpio23 : nil,
                rst: nil,
                sclSpeedHz: 100000
            )
            display = .st7123(st7123Display)
            touch = .st7123(st7123Touch)
        } else if i2c0.probe(address: 0x14) { // ILI9881C + GT911
            let ili9881c = try ILI9881C(
                backlightGpio: .gpio22,
                size: Size(width: 720, height: 1280),
                pixelFormat: pixelFormat,
                fbNum: frameBufferNum
            )
            let gt911 = try GT911(
                i2c: i2c0,
                size: (width: 720, height: 1280),
                int: touchInterrupt ? .gpio23 : nil,
                rst: nil,
                sclSpeedHz: 100000
            )
            display = .ili9881c(ili9881c)
            touch = .gt911(gt911)
        } else {
            throw IDF.Error(ESP_ERR_NOT_FOUND)
        }

        // let audio = try Audio(
        //     num: 1,
        //     mclk: .gpio30, bclk: .gpio27, ws: .gpio29, dout: .gpio26, din: .gpio28,
        //     i2c: i2c
        // )
        // let sdcard = try SDCard(
        //     ldo: (channel: 4, voltageMv: 3300),
        //     slot: .default(
        //         busWidth: 4,
        //         clk: .gpio43, cmd: .gpio44,
        //         data0: .gpio39, data1: .gpio40, data2: .gpio41, data3: .gpio42,
        //     )
        // )
        return M5StackTab5(
            i2c0: i2c0,
            pi4ioe1: pi4ioe1,
            pi4ioe2: pi4ioe2,
            display: display,
            touch: touch,
            // audio: audio,
            // sdcard: sdcard
        )
    }

    let i2c0: IDF.I2C
    let pi4ioe1: PI4IO
    let pi4ioe2: PI4IO
    var display: Display
    let touch: Touch
    // let audio: Audio
    // let sdcard: SDCard
    private init(
        i2c0: IDF.I2C,
        pi4ioe1: PI4IO,
        pi4ioe2: PI4IO,
        display: Display,
        touch: Touch
        // audio: Audio,
        // sdcard: SDCard
    ) {
        self.i2c0 = i2c0
        self.pi4ioe1 = pi4ioe1
        self.pi4ioe2 = pi4ioe2
        self.display = display
        self.touch = touch
        // self.audio = audio
        // self.sdcard = sdcard
    }

    // MARK: Display
    enum Display {
        case ili9881c(ILI9881C<PixelFormat>)
        case st7123(ST7123.Display<PixelFormat>)

        var brightness: Int {
            get {
                switch (self) {
                    case .ili9881c(let ili9881c): ili9881c.brightness
                    case .st7123(let st7123): st7123.brightness
                }
            }
            set {
                switch (self) {
                    case .ili9881c(let ili9881c): ili9881c.brightness = newValue
                    case .st7123(let st7123): st7123.brightness = newValue
                }
            }
        }

        var frameBuffers: [UnsafeMutableBufferPointer<PixelFormat>] {
            get {
                switch (self) {
                    case .ili9881c(let ili9881c): ili9881c.frameBuffers
                    case .st7123(let st7123): st7123.frameBuffers
                }
            }
        }

        func drawBitmap(rect: Rect, data: UnsafeRawPointer) {
            switch (self) {
                case .ili9881c(let ili9881c): ili9881c.drawBitmap(rect: rect, data: data)
                case .st7123(let st7123): st7123.drawBitmap(rect: rect, data: data)
            }
        }

        func flush(fbNum: Int = 0) {
            switch (self) {
                case .ili9881c(let ili9881c): ili9881c.flush(fbNum: fbNum)
                case .st7123(let st7123): st7123.flush(fbNum: fbNum)
            }
        }
    }

    // MARK: Touch
    enum Touch {
        case gt911(GT911)
        case st7123(ST7123.Touch)

        func waitInterrupt() {
            switch (self) {
                case .gt911(let gt911): gt911.waitInterrupt()
                case .st7123(let st7123): st7123.waitInterrupt()
            }
        }

        var coordinates: [Point] {
            get throws(IDF.Error) {
                switch (self) {
                    case .gt911(let gt911): try gt911.coordinates
                    case .st7123(let st7123): try st7123.coordinates
                }
            }
        }
    }


    /*
     * MARK: Audio (ES8388)
     */


    /*
     * MARK: SDCard
     */
    // class SDCard {
    //     let powerControl: sd_pwr_ctrl_handle_t
    //     var sdmmc: IDF.SDMMC? = nil
    //     let slotConfig: IDF.SDMMC.SlotConfig

    //     init(
    //         ldo: (channel: Int32, voltageMv: Int32),
    //         slot: IDF.SDMMC.SlotConfig
    //     ) throws(IDF.Error) {
    //         // Setup SDCard Power (LDO)
    //         var ldoConfig = sd_pwr_ctrl_ldo_config_t(ldo_chan_id: ldo.channel)
    //         var powerControl: sd_pwr_ctrl_handle_t?
    //         try IDF.Error.check(sd_pwr_ctrl_new_on_chip_ldo(&ldoConfig, &powerControl))
    //         self.powerControl = powerControl!
    //         self.slotConfig = slot
    //     }

    //     var isMounted: Bool {
    //         return sdmmc != nil
    //     }

    //     func mount(path: String, maxFiles: Int32) throws(IDF.Error) {
    //         var host = IDF.SDMMC.HostConfig.default
    //         host.slot = SDMMC_HOST_SLOT_0
    //         host.max_freq_khz = SDMMC_FREQ_HIGHSPEED
    //         sdmmc = try IDF.SDMMC.mount(
    //             path: path,
    //             host: host,
    //             slot: slotConfig,
    //             maxFiles: maxFiles
    //         )
    //         sdmmc!.printInfo()
    //     }
    // }
}
