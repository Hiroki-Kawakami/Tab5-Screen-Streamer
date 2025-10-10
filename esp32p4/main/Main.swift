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
    let tab5 = try M5StackTab5.begin()
    let frameBuffers = tab5.display.frameBuffers
    tab5.display.brightness = 100

    let multiTouch: MultiTouch = MultiTouch()
    multiTouch.task(xCoreID: 1) {
        tab5.touch.waitInterrupt()
        return try! tab5.touch.coordinates
    }

    let drawable = tab5.display.drawable(frameBuffer: frameBuffers[0])
    drawable.clear(color: .red)
    drawable.flush()

    try IDF.Error.check(usbd_init());
    Task(name: "TinyUSB", priority: 5, xCoreID: 1) { _ in
        usbd_task()
    }

    let jpegBufferSize = 512 * 1024
    let jpegBuffer = [UnsafeMutableBufferPointer<UInt8>]((0...2).map({ _ in
        Memory.allocate(type: UInt8.self, capacity: jpegBufferSize, capability: .spiram)!
    }))
    var jpegBufferIndex = 0
    let jpegDecoder = try IDF.JPEG.Decoder(outputFormat: .rgb888(elementOrder: .bgr, conversion: .bt601))

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
                    count: frameBuffers[frameBufferIndex].count * 3
                )
            ) else {
                continue
            }

            tab5.display.drawable(frameBuffer: frameBuffers[frameBufferIndex]).flush()
            frameBufferIndex = frameBufferIndex == 0 ? 1 : 0

            frameCount += 1
            let now = timer.count
            if (now - start) >= 1000000 {
                Log.info("\(frameCount)fps")
                frameCount = 0
                start = now
            }
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
                jpegBufferIndex = (jpegBufferIndex + 1) % 3
            } else {
                Log.warn("Frame drop!")
            }
        }
    }
}
