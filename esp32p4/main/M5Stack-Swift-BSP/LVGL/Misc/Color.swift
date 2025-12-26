extension LVGL {
    struct Opacity {
        let rawValue: lv_opa_t
        static func percent(_ value: UInt8) -> Opacity { Opacity(rawValue: lv_opa_t(UInt32(value) * 255 / 100)) }
        static let transp = Opacity(rawValue: lv_opa_t(LV_OPA_TRANSP.rawValue))
        static let cover = Opacity(rawValue: lv_opa_t(LV_OPA_COVER.rawValue))
    }

    struct Color {
        let lv_color: lv_color_t

        init(lv_color: lv_color_t) {
            self.lv_color = lv_color
        }

        init(red: UInt8, green: UInt8, blue: UInt8) {
            self.lv_color = lv_color_make(red, green, blue)
        }

        init(hex: UInt32) {
            self.lv_color = lv_color_hex(hex)
        }

        static let white = Color(hex: 0xFFFFFF)
        static let black = Color(hex: 0x000000)
        static let red = Color(hex: 0xFF0000)
        static let green = Color(hex: 0x00FF00)
        static let blue = Color(hex: 0x0000FF)
    }
}
