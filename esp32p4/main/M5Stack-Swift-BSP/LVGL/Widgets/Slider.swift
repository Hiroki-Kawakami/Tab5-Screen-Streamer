extension LVGL {
    struct Slider: ObjectProtocol {
        let obj: OpaquePointer

        init(obj: OpaquePointer) { self.obj = obj }
        init<T: ObjectProtocol>(parent: T?) { self.obj = lv_slider_create(parent?.obj) }

        // Setter functions
        func setValue(_ value: Int32, anim: lv_anim_enable_t) { lv_slider_set_value(obj, value, anim) }
        func setStartValue(_ value: Int32, anim: lv_anim_enable_t) { lv_slider_set_start_value(obj, value, anim) }
        func setRange(min: Int32, max: Int32) { lv_slider_set_range(obj, min, max) }
        func setMinValue(_ min: Int32) { lv_slider_set_min_value(obj, min) }
        func setMaxValue(_ max: Int32) { lv_slider_set_max_value(obj, max) }
        func setMode(_ mode: lv_slider_mode_t) { lv_slider_set_mode(obj, mode) }
        func setOrientation(_ orientation: lv_slider_orientation_t) { lv_slider_set_orientation(obj, orientation) }

        // Getter functions
        func getValue() -> Int32 { lv_slider_get_value(obj) }
        func getLeftValue() -> Int32 { lv_slider_get_left_value(obj) }
        func getMinValue() -> Int32 { lv_slider_get_min_value(obj) }
        func getMaxValue() -> Int32 { lv_slider_get_max_value(obj) }
        func isDragged() -> Bool { lv_slider_is_dragged(obj) }
        func getMode() -> lv_slider_mode_t { lv_slider_get_mode(obj) }
        func getOrientation() -> lv_slider_orientation_t { lv_slider_get_orientation(obj) }
        func isSymmetrical() -> Bool { lv_slider_is_symmetrical(obj) }
    }
}

extension lv_slider_mode_t {
    static let normal = LV_SLIDER_MODE_NORMAL
    static let symmetrical = LV_SLIDER_MODE_SYMMETRICAL
    static let range = LV_SLIDER_MODE_RANGE
}

extension lv_slider_orientation_t {
    static let auto = LV_SLIDER_ORIENTATION_AUTO
    static let horizontal = LV_SLIDER_ORIENTATION_HORIZONTAL
    static let vertical = LV_SLIDER_ORIENTATION_VERTICAL
}
