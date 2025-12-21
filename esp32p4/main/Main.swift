fileprivate let Log = Logger(tag: "main")

@_cdecl("app_main")
func app_main() {
    do {
        try main()
    } catch {
        Log.error("Main Function Exit with Error: \(error)")
    }
}

func main() throws(IDF.Error) {
    typealias PixelFormat = RGB888
    let tab5 = try M5StackTab5.begin(
        pixelFormat: PixelFormat.self,
        frameBufferNum: 3,
    )

    let frameBuffers = tab5.display.frameBuffers
    tab5.display.brightness = 100

    // let multiTouch: MultiTouch = MultiTouch()
    // multiTouch.task(xCoreID: 1) {
    //     tab5.touch.waitInterrupt()
    //     return try! tab5.touch.coordinates
    // }

    tab5.display.frameBuffers[0].initialize(repeating: PixelFormat(red: 255, green: 0, blue: 0))
    tab5.display.flush()

    try IDF.Error.check(usbd_init());
    Task(name: "TinyUSB", priority: 5, xCoreID: 1) { _ in
        usbd_task()
    }

    let jpegBufferSize = 512 * 1024
    let jpegBuffer = [UnsafeMutableBufferPointer<UInt8>]((0...3).map({ _ in
        Memory.allocate(type: UInt8.self, capacity: jpegBufferSize, capability: .spiram)!
    }))
    var jpegBufferIndex = 0
    let jpegDecoder = try IDF.JPEG.Decoder(outputFormat:
        PixelFormat.self == RGB888.self ? .rgb888(elementOrder: .bgr, conversion: .bt601) : .rgb565(elementOrder: .bgr, conversion: .bt601)
    )

    let timer = try IDF.Timer()
    let jpegDecoderQueue = Queue<UnsafeRawBufferPointer>(capacity: 1)!

    Task(name: "Decoder", priority: 15, xCoreID: 1) { _ in
        var frameBufferIndex = 0
        var frameCount = 0
        var start = timer.count
        for jpegData in jpegDecoderQueue {
            guard let _ = try? jpegDecoder.decode(
                inputBuffer: jpegData,
                outputBuffer: UnsafeMutableRawBufferPointer(
                    start: frameBuffers[frameBufferIndex].baseAddress!,
                    count: frameBuffers[frameBufferIndex].count * MemoryLayout<PixelFormat>.size
                )
            ) else {
                continue
            }

            tab5.display.flush(fbNum: frameBufferIndex)
            frameBufferIndex = (frameBufferIndex + 1) % frameBuffers.count

            frameCount += 1
            let now = timer.count
            if (now - start) >= 1000000 {
                Log.info("\(frameCount)fps")
                frameCount = 0
                start = now
            }
            Task.delay(2)
        }
    }

    Task(name: "Recv", priority: 4, xCoreID: 1) { _ in
        var mounted: Bool? = nil
        frameLoop: while (true) {
            let deviceMounted = usbd_mounted()
            if mounted != deviceMounted {
                Log.info("Device mounted: \(deviceMounted)")
                mounted = deviceMounted
            }
            if !deviceMounted {
                Task.delay(100)
                continue;
            }

            let availableSize = usbd_vendor_available()
            if availableSize < 2 { continue }

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
            if jpegDecoderQueue.send(jpegDataBuffer, timeout: 0) {
                jpegBufferIndex = (jpegBufferIndex + 1) % jpegBuffer.count
            } else {
                Log.warn("Frame drop!")
            }
        }
    }
}
