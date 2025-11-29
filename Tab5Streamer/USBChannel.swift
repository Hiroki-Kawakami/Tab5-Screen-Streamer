//
//  USBChannel.swift
//  Tab5Streamer
//
//  Created by hiroki on 2025/10/05.
//

import Foundation

class USBChannel {
    
    let VID: UInt16 = 0x303a
    let PID: UInt16 = 0x4020
    let EP_OUT: UInt8 = 0x01  // Bulk OUT
    let EP_IN:  UInt8 = 0x81  // Bulk IN
    
    var ctx: OpaquePointer?
    var devHandle: OpaquePointer?

    init() {
        guard libusb_init(&ctx) == 0 else { fatalError("libusb init failed") }
        devHandle = libusb_open_device_with_vid_pid(ctx, VID, PID)
        if devHandle == nil { fatalError("device not found") }
        _ = libusb_detach_kernel_driver(devHandle, 0)

        guard libusb_set_configuration(devHandle, 1) == 0 else { fatalError("set config failed") }
        guard libusb_claim_interface(devHandle, 0) == 0 else { fatalError("claim if failed") }
    }
    
    func send(data: NSMutableData) -> Int {
        var transferred: Int32 = 0
        let wret = libusb_bulk_transfer(devHandle, EP_OUT, data.mutableBytes, Int32(data.count), &transferred, 1000)
        guard wret == 0 else { fatalError("bulk OUT failed: \(wret)") }
        return Int(transferred)
    }
}
