fileprivate let Log = Logger(tag: "ES8388")

class ES8388 {
    let i2s: IDF.I2S
    let outputDevice: esp_codec_dev_handle_t

    init(
        num: UInt32? = nil,
        mclk: IDF.GPIO.Pin, bclk: IDF.GPIO.Pin, ws: IDF.GPIO.Pin, dout: IDF.GPIO.Pin?, din: IDF.GPIO.Pin?,
        i2c: IDF.I2C,
    ) throws(IDF.Error) {
        // I2S Setup
        i2s = try IDF.I2S(
            num: 0,
            role: .master,
            autoClear: (beforeCb: false, afterCb: true),
            format: (
                tx: .std(
                    clock: .default(sampleRate: 48000),
                    slot: .philipsDefault(
                        dataBitWidth: I2S_DATA_BIT_WIDTH_16BIT,
                        slotMode: I2S_SLOT_MODE_MONO
                    ),
                    gpio: .init(mclk: mclk, bclk: bclk, ws: ws, dout: dout, din: din)
                ),
                rx: .tdm(
                    clock: .init(
                        sampleRate: 48000,
                        clkSrc: I2S_CLK_SRC_DEFAULT,
                        extClkFreq: 0,
                        mclkMultiple: I2S_MCLK_MULTIPLE_256,
                        bclkDiv: 0
                    ),
                    slot: .init(
                        dataBitWidth: I2S_DATA_BIT_WIDTH_16BIT,
                        slotBitWidth: I2S_SLOT_BIT_WIDTH_AUTO,
                        slotMode: I2S_SLOT_MODE_STEREO,
                        slotMask: [.slot0, .slot1, .slot2, .slot3],
                        wsWidth: UInt32(I2S_TDM_AUTO_WS_WIDTH),
                        wsPol: false,
                        bitShift: true,
                        leftAlign: false,
                        bigEndian: false,
                        bitOrderLsb: false,
                        skipMask: false,
                        totalSlot: UInt32(I2S_TDM_AUTO_SLOT_NUM)
                    ),
                    gpio: .init(mclk: mclk, bclk: bclk, ws: ws, dout: dout, din: din)
                ),
            )
        )
        outputDevice = ES8388.initSpeaker(i2s: i2s, i2c: i2c)
        volume = 0
        try reconfigOutput(rate: 48000, bps: 16, ch: 2)
    }

    static func initSpeaker(i2s: IDF.I2S, i2c: IDF.I2C) -> esp_codec_dev_handle_t {
        // let gpioInterface = audio_codec_new_gpio()
        //     .unwrap(errMsg: { "Failed to create audio codec GPIO Interface" })
        var i2cConfig = audio_codec_i2c_cfg_t(
            port: UInt8(i2c.portNumber),
            addr: UInt8(ES8388_CODEC_DEFAULT_ADDR),
            bus_handle: UnsafeMutableRawPointer(i2c.handle)
        )
        let i2cControlInterface = audio_codec_new_i2c_ctrl(&i2cConfig)
            .unwrap(errMsg: { "Failed to create Speaker codec I2C Interface" })

        // let gain = esp_codec_dev_hw_gain_t(
        //     pa_voltage: 5.0,
        //     codec_dac_voltage: 3.3,
        //     pa_gain: 0
        // )
        var es8388Config = es8388_codec_cfg_t()
        es8388Config.codec_mode = ESP_CODEC_DEV_WORK_MODE_DAC
        es8388Config.master_mode = false
        es8388Config.ctrl_if = i2cControlInterface
        es8388Config.pa_pin = -1
        let es8388Dev = es8388_codec_new(&es8388Config)
            .unwrap(errMsg: { "Failed to create ES8388 codec" })

        var codecDevConfig = esp_codec_dev_cfg_t(
            dev_type: ESP_CODEC_DEV_TYPE_OUT,
            codec_if: es8388Dev,
            data_if: i2s.interface
        )
        return esp_codec_dev_new(&codecDevConfig)
            .unwrap(errMsg: { "Failed to create Speaker codec device" })
    }

    func reconfigOutput(rate: UInt32, bps: UInt8, ch: UInt8) throws(IDF.Error) {
        var fs = esp_codec_dev_sample_info_t()
        fs.sample_rate = rate
        fs.channel = ch
        fs.bits_per_sample = bps
        try IDF.Error.check(esp_codec_dev_close(outputDevice))
        try IDF.Error.check(esp_codec_dev_open(outputDevice, &fs))
    }
    func write(_ data: UnsafeMutableRawBufferPointer) throws(IDF.Error) {
        try IDF.Error.check(esp_codec_dev_write(outputDevice, data.baseAddress!, Int32(data.count)))
    }

    var volume: Int = -1 {
        didSet {
            volume = max(0, min(100, volume))
            do throws(IDF.Error) {
                if (volume <= 0) {
                    try IDF.Error.check(esp_codec_dev_set_out_mute(outputDevice, true))
                } else {
                    try IDF.Error.check(esp_codec_dev_set_out_mute(outputDevice, false))
                    try IDF.Error.check(esp_codec_dev_set_out_vol(outputDevice, Int32(volume)))
                }
            } catch {
                Log.error("Failed to set volume: \(error)")
            }
        }
    }
}
