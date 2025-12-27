class ControlView<PixelFormat: Pixel> {

    var visible: Bool = true

    private var currentBuffer: UnsafeMutablePointer<UInt8>?
    private var guiBuffers: [UnsafeMutableBufferPointer<lv_color_t>]
    private var tab5: M5StackTab5<PixelFormat>
    private let ppa: IDF.PPAClient

    private var usbStatusRect: LVGL.Object!
    private var usbStatusLabel: LVGL.Label!
    var usbMounted: Bool = false {
        didSet {
            LVGL.asyncCall {
                if self.usbMounted {
                    self.usbStatusRect.setStyleBgColor(LVGL.Color(hex: 0x0EBC00))
                    self.usbStatusLabel.setText("USB Connected")
                } else {
                    self.usbStatusRect.setStyleBgColor(LVGL.Color(hex: 0xC20000))
                    self.usbStatusLabel.setText("USB Disconnected")
                }
            }
        }
    }

    init(tab5: M5StackTab5<PixelFormat>) throws(IDF.Error) {
        self.guiBuffers = [
            Memory.allocate(type: lv_color_t.self, capacity: 320 * 480, capability: .spiram)!,
        ]
        self.tab5 = tab5
        ppa = try IDF.PPAClient(operType: .srm)
        LVGL.withLock { createDisplay() }
    }

    private func createDisplay() {
        let display = LVGL.Display.createDirectBufferDisplay(
            buffer: guiBuffers[0].baseAddress,
            size: Size(width: 320, height: 480)
        ) { display, buffer in
            self.currentBuffer = buffer
            display.flushReady()
        }
        display.setDefault()

        let touch = TouchStateMachine()
        touch.onEvent { event in
            guard case .tap(_) = event else { return }
            self.visible = !self.visible
            print("Control Visible: \(self.visible)")
        }
        let _ = LVGL.Indev.createPollingPointerDevice { indev, data in
            guard let point = (try? self.tab5.touch.coordinates)?.first else {
                data.pointee.state = .released
                touch.onTouch(coordinates: [])
                return
            }
            if self.visible && point.y < 480 {
                data.pointee.point.x = 320 - Int32(point.y) * 320 / 480
                data.pointee.point.y = Int32(point.x) * 480 / 720
                data.pointee.state = .pressed
            } else {
                touch.onTouch(coordinates: [point])
            }
        }

        let screen = LVGL.Screen.active
        screen.setStyleBgColor(LVGL.Color(hex: 0xCCCCCC))

        usbStatusRect = LVGL.Object(parent: screen)
        usbStatusRect.setSize(width: 280, height: 100)
        usbStatusRect.align(.topMid, yOffset: 20)
        usbStatusRect.setStyleRadius(15)
        usbStatusRect.setStyleBgColor(LVGL.Color(hex: 0x0EBC00))
        usbStatusRect.setStyleBgColor(LVGL.Color(hex: 0xC20000))
        usbStatusRect.setStyleBgOpa(.cover)
        usbStatusRect.setStyleBorderOpa(.transp)
        usbStatusLabel = LVGL.Label(parent: usbStatusRect)
        usbStatusLabel.setStyleTextFont(lv_font_montserrat_20_ptr)
        usbStatusLabel.setText("USB Disconnected")
        usbStatusLabel.setStyleTextColor(.white)
        usbStatusLabel.center()

        let brightnessLabel = LVGL.Label(parent: screen)
        brightnessLabel.setText("Brightness")
        brightnessLabel.setWidth(280)
        brightnessLabel.alignTo(base: usbStatusRect, align: .outBottomMid, yOffset: 40)
        let brightnessSlider = LVGL.Slider(parent: screen)
        brightnessSlider.setWidth(280)
        brightnessSlider.alignTo(base: brightnessLabel, align: .outBottomMid, yOffset: 20)
        brightnessSlider.setRange(min: 1, max: 100)
        brightnessSlider.setValue(Int32(tab5.display.brightness), anim: false)
        let callbackWrapper = FFI.Wrapper<() -> Void> {
            self.tab5.display.brightness = Int(brightnessSlider.getValue())
        }
        brightnessSlider.addEventCb({ obj in
            let event = LVGL.Event(e: obj!)
            Unmanaged<FFI.Wrapper<() -> Void>>.fromOpaque(event.getUserData()).takeUnretainedValue().value()
        }, filter: LV_EVENT_VALUE_CHANGED, userData: Unmanaged.passRetained(callbackWrapper).toOpaque())
    }

    func push(fbIndex: Int) {
        let colorMode: IDF.PPAClient.SRMColorMode = MemoryLayout<PixelFormat>.size == 2 ? .rgb565 : .rgb888
        try? ppa.srm(
            input: (buffer: UnsafeRawBufferPointer(start: currentBuffer, count: self.guiBuffers[0].count), size: Size(width: 320, height: 480), block: nil, colorMode: .rgb565),
            output: (buffer: UnsafeMutableRawBufferPointer(tab5.display.frameBuffers[fbIndex]), size: Size(width: 720, height: 1280), block: Rect(x: 0, y: 0, width: 720, height: 480), colorMode: colorMode),
            rotate: 90
        )
    }
}
