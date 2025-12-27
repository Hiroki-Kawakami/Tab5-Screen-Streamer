fileprivate let Log = Logger(tag: "main")

@_cdecl("app_main")
func app_main() {
    do {
        try main(pixelFormat: RGB888.self)
    } catch {
        Log.error("Main Function Exit with Error: \(error)")
    }
}

func main<PixelFormat: Pixel>(pixelFormat: PixelFormat.Type) throws(IDF.Error) {
    let tab5 = try M5StackTab5.begin(
        pixelFormat: PixelFormat.self,
        frameBufferNum: 3,
    )
    try LVGL.begin()
    tab5.display.brightness = 100
    let controlView = try ControlView(tab5: tab5)
    let frameBuffers = tab5.display.frameBuffers

    try IDF.Error.check(usbd_init());
    Task(name: "TinyUSB", priority: 5, xCoreID: 1) { _ in
        usbd_task()
    }

    let jpegBufferSize = 512 * 1024
    let jpegBuffer = [UnsafeMutableBufferPointer<UInt8>]((0..<8).map({ _ in
        Memory.allocate(type: UInt8.self, capacity: jpegBufferSize, capability: .spiram)!
    }))
    var jpegBufferIndex = 0
    let jpegDecoder = try IDF.JPEG.Decoder(outputFormat:
        PixelFormat.self == RGB888.self ? .rgb888(elementOrder: .bgr, conversion: .bt601) : .rgb565(elementOrder: .bgr, conversion: .bt601)
    )
    let jpegDecoderQueue = Queue<UnsafeRawBufferPointer>(capacity: 1)!

    let timer = try IDF.Timer()
    Task(name: "Decoder", priority: 15, xCoreID: 0) { _ in
        var frameBufferIndex = 0
        var frameCount = 0
        var start = timer.count
        var decodeDurationMax: UInt64 = 0
        while true {
            guard let jpegData = jpegDecoderQueue.receive(timeout: 10) else {
                if controlView.visible {
                    LVGL.withLock {
                        controlView.push(fbIndex: frameBufferIndex)
                    }
                }
                continue
            }

            let nextFrameBufferIndex = (frameBufferIndex + 1) % frameBuffers.count
            let decodeStart = timer.count
            guard let _ = try? jpegDecoder.decode(
                inputBuffer: jpegData,
                outputBuffer: UnsafeMutableRawBufferPointer(
                    start: frameBuffers[nextFrameBufferIndex].baseAddress!,
                    count: frameBuffers[nextFrameBufferIndex].count * MemoryLayout<PixelFormat>.size
                )
            ) else {
                continue
            }
            let decodeDuration = timer.duration(from: decodeStart)
            if decodeDuration > decodeDurationMax { decodeDurationMax = decodeDuration }

            if controlView.visible {
                LVGL.withLock {
                    controlView.push(fbIndex: nextFrameBufferIndex)
                }
            }

            tab5.display.flush(fbNum: nextFrameBufferIndex)
            frameBufferIndex = nextFrameBufferIndex

            frameCount += 1
            let now = timer.count
            if (now - start) >= 1000000 {
                Log.info("\(frameCount)fps, decode: \(decodeDurationMax)")
                frameCount = 0
                start = now
                decodeDurationMax = 0
            }
        }
    }

    Task(name: "Recv", priority: 4, xCoreID: 1) { _ in
        var mounted: Bool? = nil
        var firstFrameReceived = false
        frameLoop: while (true) {
            let deviceMounted = usbd_mounted()
            if mounted != deviceMounted {
                Log.info("Device mounted: \(deviceMounted)")
                mounted = deviceMounted
                controlView.usbMounted = deviceMounted
            }
            if !deviceMounted {
                Task.delay(100)
                continue;
            }

            let availableSize = usbd_vendor_available()
            if availableSize < 2 { continue }
            if !firstFrameReceived {
                controlView.visible = false
                firstFrameReceived = true
            }

            var bufferAddress = jpegBuffer[jpegBufferIndex].baseAddress!
            let readSize = usbd_vendor_read(bufferAddress, min(availableSize, 512))
            var jpegDataSize = UnsafeRawPointer(bufferAddress).load(as: UInt32.self).littleEndian
            bufferAddress = bufferAddress.advanced(by: Int(readSize))
            // Log.info("Start Receive: \(jpegDataSize)")

            while jpegDataSize > readSize {
                jpegDataSize -= readSize
                var waitCount = 0
                while usbd_vendor_available() == 0 {
                    if waitCount >= 1000 { continue frameLoop }
                    waitCount += 1
                }

                let readSize = usbd_vendor_read(bufferAddress, min(usbd_vendor_available(), jpegDataSize, 512))
                // Log.info("Receive: \(readSize), \(i)/\(sizeCount)")
                bufferAddress = bufferAddress.advanced(by: Int(readSize))
            }

            let jpegDataBuffer = UnsafeRawBufferPointer(start: jpegBuffer[jpegBufferIndex].baseAddress!.advanced(by: 4), count: jpegBuffer[jpegBufferIndex].count - 4)
            jpegDecoderQueue.overwrite(jpegDataBuffer)
            jpegBufferIndex = (jpegBufferIndex + 1) % jpegBuffer.count
        }
    }
}
