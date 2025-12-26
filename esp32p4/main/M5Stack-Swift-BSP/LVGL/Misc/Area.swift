extension LVGL {
    struct Align {
        let rawValue: UInt32
        static let `default` = Align(rawValue: LV_ALIGN_DEFAULT.rawValue)
        static let topLeft = Align(rawValue: LV_ALIGN_TOP_LEFT.rawValue)
        static let topMid = Align(rawValue: LV_ALIGN_TOP_MID.rawValue)
        static let topRight = Align(rawValue: LV_ALIGN_TOP_RIGHT.rawValue)
        static let bottomLeft = Align(rawValue: LV_ALIGN_BOTTOM_LEFT.rawValue)
        static let bottomMid = Align(rawValue: LV_ALIGN_BOTTOM_MID.rawValue)
        static let bottomRight = Align(rawValue: LV_ALIGN_BOTTOM_RIGHT.rawValue)
        static let leftMid = Align(rawValue: LV_ALIGN_LEFT_MID.rawValue)
        static let rightMid = Align(rawValue: LV_ALIGN_RIGHT_MID.rawValue)
        static let center = Align(rawValue: LV_ALIGN_CENTER.rawValue)
        static let outTopLeft = Align(rawValue: LV_ALIGN_OUT_TOP_LEFT.rawValue)
        static let outTopMid = Align(rawValue: LV_ALIGN_OUT_TOP_MID.rawValue)
        static let outTopRight = Align(rawValue: LV_ALIGN_OUT_TOP_RIGHT.rawValue)
        static let outBottomLeft = Align(rawValue: LV_ALIGN_OUT_BOTTOM_LEFT.rawValue)
        static let outBottomMid = Align(rawValue: LV_ALIGN_OUT_BOTTOM_MID.rawValue)
        static let outBottomRight = Align(rawValue: LV_ALIGN_OUT_BOTTOM_RIGHT.rawValue)
        static let outLeftTop = Align(rawValue: LV_ALIGN_OUT_LEFT_TOP.rawValue)
        static let outLeftMid = Align(rawValue: LV_ALIGN_OUT_LEFT_MID.rawValue)
        static let outLeftBottom = Align(rawValue: LV_ALIGN_OUT_LEFT_BOTTOM.rawValue)
        static let outRightTop = Align(rawValue: LV_ALIGN_OUT_RIGHT_TOP.rawValue)
        static let outRightMid = Align(rawValue: LV_ALIGN_OUT_RIGHT_MID.rawValue)
        static let outRightBottom = Align(rawValue: LV_ALIGN_OUT_RIGHT_BOTTOM.rawValue)
    }
}
