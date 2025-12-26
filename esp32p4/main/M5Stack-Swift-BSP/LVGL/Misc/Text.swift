extension LVGL {
    enum Text {

        struct Flag: OptionSet {
            let rawValue: UInt32
            static let none = Flag(rawValue: LV_TEXT_FLAG_NONE.rawValue)
            static let expand = Flag(rawValue: LV_TEXT_FLAG_EXPAND.rawValue)
            static let fit = Flag(rawValue: LV_TEXT_FLAG_FIT.rawValue)
            static let breakAll = Flag(rawValue: LV_TEXT_FLAG_BREAK_ALL.rawValue)
            static let recolor = Flag(rawValue: LV_TEXT_FLAG_RECOLOR.rawValue)
        }

        struct Align {
            let rawValue: UInt32
            static let auto = Align(rawValue: LV_TEXT_ALIGN_AUTO.rawValue)
            static let left = Align(rawValue: LV_TEXT_ALIGN_LEFT.rawValue)
            static let center = Align(rawValue: LV_TEXT_ALIGN_CENTER.rawValue)
            static let right = Align(rawValue: LV_TEXT_ALIGN_RIGHT.rawValue)
        }

        func getSize(text: String, font: UnsafePointer<lv_font_t>, letterSpace: Int32, lineSpace: Int32, maxWidth: Int32, flag: Flag) -> Size {
            text.withCString {
                var point = lv_point_t()
                lv_text_get_size(&point, $0, font, letterSpace, lineSpace, maxWidth, lv_text_flag_t(flag.rawValue))
                return Size(width: Int(point.x), height: Int(point.y))
            }
        }
    }
}
