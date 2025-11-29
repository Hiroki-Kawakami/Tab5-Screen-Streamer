//
//  main.swift
//  Tab5Streamer
//
//  Created by hiroki on 2025/10/04.
//

import Foundation
@preconcurrency import ScreenCaptureKit
import IOKit
import IOKit.usb.IOUSBLib

final class FrameOutput: NSObject, SCStreamOutput {
    var frameCount: Int = 0
    let frames: AsyncStream<NSMutableData>
    private let cont: AsyncStream<NSMutableData>.Continuation
    let imageConverter = ImageConverter()

    override init() {
        var c: AsyncStream<NSMutableData>.Continuation!
        self.frames = AsyncStream(bufferingPolicy: .bufferingNewest(2)) { continuation in
            c = continuation
        }
        self.cont = c
        super.init()
    }

    func stream(_ stream: SCStream,
                didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
                of outputType: SCStreamOutputType) {
        guard sampleBuffer.isValid,
              CMSampleBufferDataIsReady(sampleBuffer) else { return }

        
        
        if let pb = CMSampleBufferGetImageBuffer(sampleBuffer),
           let jpegData = imageConverter?.scaleAndRotate(from: pb) {
            let sizeCount = (jpegData.count + 511) / 512
            let sizeBytes: [UInt8] = [UInt8(sizeCount & 0xff), UInt8((sizeCount >> 8) & 0xff)]
            jpegData.replaceBytes(in: NSRange(0..<2), withBytes: sizeBytes)
//            print("JPEG Data Size Count: \(sizeCount), data[0]=\(jpegData[0]), data[1]=\(jpegData[1])")
            
            if case .dropped(_) = cont.yield(jpegData) {
//                print("Frame drop!")
            }
            frameCount += 1
        }
    }
}

do {
    let usbChannel = USBChannel()
    
    let content = try await SCShareableContent.current
    let display = content.displays[1]
    let filter = SCContentFilter(display: display, excludingWindows: [])
    
    let config = SCStreamConfiguration()
    config.width  = 1280
    config.height = 720
    config.minimumFrameInterval = .zero
    config.pixelFormat = kCVPixelFormatType_32BGRA
    config.showsCursor = true
    config.capturesShadowsOnly = false
    let stream = SCStream(filter: filter, configuration: config, delegate: nil)

    let output = FrameOutput()
    let queue = DispatchQueue(label: "SCStream.sample.queue")
    try stream.addStreamOutput(output, type: .screen, sampleHandlerQueue: queue)
    
    try await stream.startCapture()
    print("Start capturing: \(display.width)x\(display.height) @ 60fps")

    var frameCount: Int = 0, transferred = 0
    var start = CFAbsoluteTimeGetCurrent()
    for await jpegData in output.frames {
        transferred += usbChannel.send(data: jpegData)
        frameCount += 1
        let now = CFAbsoluteTimeGetCurrent()
        if (now - start) >= 1.0 {
            let speed = transferred / 1000
            print("Capture: \(output.frameCount)fps, USB Tx: \(frameCount)fps, \(speed)kB/s")
            output.frameCount = 0
            frameCount = 0
            transferred = 0
            start = now
            
            if speed > 8000 {
                output.imageConverter?.quality = 0.4
            } else if speed < 5000 {
                output.imageConverter?.quality = 0.6
            }
        }
    }
} catch {
    print("Error: \(error)")
    exit(1)
}

