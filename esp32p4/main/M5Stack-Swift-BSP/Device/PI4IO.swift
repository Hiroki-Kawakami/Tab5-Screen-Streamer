class PI4IO {
    private enum Register: UInt8 {
        case CHIP_RESET = 0x01
        case IO_DIR = 0x03
        case OUT_SET = 0x05
        case OUT_H_IM = 0x07
        case IN_DEF_STA = 0x09
        case PULL_EN = 0x0B
        case PULL_SEL = 0x0D
        case IN_STA = 0x0F
        case INT_MASK = 0x11
        case IRQ_STA = 0x13
    }

    enum Pin {
        case none
        case input(defaultState: Bool = false, pull: Pull = .down, interrupt: Bool = true)
        case output(defaultState: Bool = false, pull: Pull = .up)

        enum Pull {
            case none
            case up
            case down
        }
    }

    private let device: IDF.I2C.Device
    var direction: UInt8 {
        didSet {
            try! device.transmit([Register.IO_DIR.rawValue, output])
        }
    }
    var output: UInt8 {
        didSet {
            try! device.transmit([Register.OUT_SET.rawValue, output])
        }
    }
    var input: UInt8 {
        let data = try! device.transmitReceive([Register.IN_STA.rawValue], readSize: 1)
        return data[0]
    }

    init(i2c: IDF.I2C, address: UInt8, pins: (Pin, Pin, Pin, Pin, Pin, Pin, Pin, Pin)) throws(IDF.Error) {
        let combine = { (bit: ((Pin) -> Bool)) in
            [pins.7, pins.6, pins.5, pins.4, pins.3, pins.2, pins.1, pins.0]
                .reduce(UInt8(0), { r, b in r << 1 | (bit(b) ? 1 : 0) })
        }
        let ioDir = combine({ pin in
            if case .output = pin { return true }
            return false
        })
        let ioHm: UInt8 = 0b00000000
        let pullSel = combine({ pin in
            if case .input(_, let pull, _) = pin { return pull == .up }
            if case .output(_, let pull) = pin { return pull == .up }
            return true
        })
        let pullEn = combine({ pin in
            if case .input(_, let pull, _) = pin { return pull != .none }
            if case .output(_, let pull) = pin { return pull != .none }
            return false
        })
        let inDefSta = combine({ pin in
            if case .input(let defaultState, _, _) = pin { return defaultState }
            return false
        })
        let intMask = combine({ pin in
            if case .input(_, _, let interrupt) = pin { return !interrupt }
            return true
        })
        let outSet = combine({ pin in
            if case .output(let defaultState, _) = pin { return defaultState }
            return false
        })

        device = try i2c.addDevice(address: address, sclSpeedHz: 400000)
        try device.transmit([Register.CHIP_RESET.rawValue, 0xFF])
        let _ = try device.transmitReceive([Register.CHIP_RESET.rawValue], readSize: 1)
        if ioDir    != 0b00000000 { try device.transmit([Register.IO_DIR    .rawValue, ioDir   ]) }
        if ioHm     != 0b11111111 { try device.transmit([Register.OUT_H_IM  .rawValue, ioHm    ]) }
        if pullSel  != 0b00000000 { try device.transmit([Register.PULL_SEL  .rawValue, pullSel ]) }
        if pullEn   != 0b11111111 { try device.transmit([Register.PULL_EN   .rawValue, pullEn  ]) }
        if inDefSta != 0b00000000 { try device.transmit([Register.IN_DEF_STA.rawValue, inDefSta]) }
        if intMask  != 0b00000000 { try device.transmit([Register.INT_MASK  .rawValue, intMask ]) }
        if outSet   != 0b00000000 { try device.transmit([Register.OUT_SET   .rawValue, outSet  ]) }
        self.direction = ioDir
        self.output = outSet
    }

    struct GPIO: Device.GPIO {
        fileprivate var pi4io: PI4IO
        fileprivate var mask: UInt8

        var value: Bool {
            get {
                if pi4io.direction & mask != 0 {
                    return pi4io.output & mask != 0
                } else {
                    return pi4io.input & mask != 0
                }
            }
            set {
                if pi4io.direction & mask == 0 || (pi4io.output & mask != 0) == newValue { return }
                if newValue { pi4io.output |= mask }
                else { pi4io.output &= ~mask }
            }
        }
    }

    subscript(index: Int) -> some Device.GPIO {
        return GPIO(pi4io: self, mask: 1 << index)
    }
}
