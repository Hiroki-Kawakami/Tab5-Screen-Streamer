//
//  ImageConverter.swift
//  Tab5Streamer
//
//  Created by hiroki on 2025/10/05.
//

import Foundation
import AVFoundation
@preconcurrency import ScreenCaptureKit

class ImageConverter {
    private let ciContext: CIContext
    var scale: CGFloat = 1
    var quality: CGFloat = 0.4

    init?() {
        guard let device = MTLCreateSystemDefaultDevice() else { return nil }
        self.ciContext = CIContext(mtlDevice: device, options: [.cacheIntermediates: true])
    }
    
    func scaleAndRotate(from imageBuffer: CVImageBuffer) -> NSMutableData? {
        let ci = CIImage(cvPixelBuffer: imageBuffer, options: [.applyOrientationProperty: false])
        let img = ci.transformed(by: CGAffineTransform(rotationAngle: .pi / 2))
//        img = img.transformed(by: .init(scaleX: scale, y: scale))
//        guard let translated = ciContext.createCGImage(img, from: img.extent) else { return nil }
        
        if let data = ciContext.jpegRepresentation(of: img, colorSpace: img.colorSpace ?? CGColorSpaceCreateDeviceRGB(), options: [
            kCGImageDestinationLossyCompressionQuality as CIImageRepresentationOption: quality,
            kCGImageDestinationPreserveGainMap as CIImageRepresentationOption: false,
        ]) {
            let mutData = NSMutableData(data: data)
            mutData.padTo512()
            return mutData
        }
        return nil
    }
}

extension NSMutableData {
    func padTo512() {
        let blockSize = 512
        let length = self.length
        let remainder = length % blockSize
        if remainder != 0 {
            let padding = blockSize - remainder
            var zeros = [UInt8](repeating: 0, count: padding)
            self.append(&zeros, length: padding)
        }
    }
}
