extension LVGL {

    struct Indev {
        let indev: OpaquePointer
        init() { self.indev = lv_indev_create() }
        init(indev: OpaquePointer) { self.indev = indev }

        typealias `Type` = lv_indev_type_t
        typealias Data = lv_indev_data_t
        typealias State = lv_indev_state_t

        static func createPollingPointerDevice(callback: @escaping (Indev, UnsafeMutablePointer<lv_indev_data_t>) -> Void) -> Indev {
            let indev = Indev()
            let callbackBox = FFI.Wrapper(callback)
            indev.setType(.pointer)
            indev.setUserData(Unmanaged.passRetained(callbackBox).toOpaque())
            indev.setReadCallback { indev, data in
                let userData = lv_indev_get_user_data(indev)!
                let callback = Unmanaged<FFI.Wrapper<(LVGL.Indev, UnsafeMutablePointer<lv_indev_data_t>) -> Void>>.fromOpaque(userData).takeUnretainedValue()
                callback.value(Indev(indev: indev!), data!)
            }
            return indev
        }
    }
}

extension LVGL.Indev.`Type` {
    static let none = LV_INDEV_TYPE_NONE
    static let pointer = LV_INDEV_TYPE_POINTER
    static let keypad = LV_INDEV_TYPE_KEYPAD
    static let button = LV_INDEV_TYPE_BUTTON
    static let encoder = LV_INDEV_TYPE_ENCODER
}
extension LVGL.Indev.State {
    static let released = LV_INDEV_STATE_RELEASED
    static let pressed = LV_INDEV_STATE_PRESSED
}

extension LVGL.Indev {
    // MARK: - Lifecycle
    func delete() { lv_indev_delete(indev) }
    static func getNext(_ indev: OpaquePointer? = nil) -> OpaquePointer? { lv_indev_get_next(indev) }
    static func active() -> OpaquePointer? { lv_indev_active() }

    // MARK: - Read
    func read() { lv_indev_read(indev) }
    func enable(_ enable: Bool) { lv_indev_enable(indev, enable) }

    // MARK: - Configuration
    func setType(_ type: LVGL.Indev.`Type`) { lv_indev_set_type(indev, type) }
    func setReadCallback(_ callback: @escaping lv_indev_read_cb_t) { lv_indev_set_read_cb(indev, callback) }
    func setUserData(_ userData: UnsafeMutableRawPointer?) { lv_indev_set_user_data(indev, userData) }
    func setDriverData(_ driverData: UnsafeMutableRawPointer?) { lv_indev_set_driver_data(indev, driverData) }
    func setDisplay(_ display: OpaquePointer) { lv_indev_set_display(indev, display) }
    func setLongPressTime(_ time: UInt16) { lv_indev_set_long_press_time(indev, time) }
    func setLongPressRepeatTime(_ time: UInt16) { lv_indev_set_long_press_repeat_time(indev, time) }
    func setScrollLimit(_ limit: UInt8) { lv_indev_set_scroll_limit(indev, limit) }
    func setScrollThrow(_ throw: UInt8) { lv_indev_set_scroll_throw(indev, `throw`) }
    func setMode(_ mode: lv_indev_mode_t) { lv_indev_set_mode(indev, mode) }

    // MARK: - Getters
    func getType() -> lv_indev_type_t { lv_indev_get_type(indev) }
    func getReadCallback() -> lv_indev_read_cb_t? { lv_indev_get_read_cb(indev) }
    func getState() -> lv_indev_state_t { lv_indev_get_state(indev) }
    func getGroup() -> OpaquePointer? { lv_indev_get_group(indev) }
    func getDisplay() -> OpaquePointer? { lv_indev_get_display(indev) }
    func getUserData() -> UnsafeMutableRawPointer? { lv_indev_get_user_data(indev) }
    func getDriverData() -> UnsafeMutableRawPointer? { lv_indev_get_driver_data(indev) }
    func getPressMoved() -> Bool { lv_indev_get_press_moved(indev) }
    func getMode() -> lv_indev_mode_t { lv_indev_get_mode(indev) }
    func getReadTimer() -> OpaquePointer? { lv_indev_get_read_timer(indev) }

    // MARK: - Reset & Control
    func reset(obj: OpaquePointer? = nil) { lv_indev_reset(indev, obj) }
    func stopProcessing() { lv_indev_stop_processing(indev) }
    func resetLongPress() { lv_indev_reset_long_press(indev) }
    func waitRelease() { lv_indev_wait_release(indev) }

    // MARK: - Pointer/Touch
    func setCursor(_ cursorObj: OpaquePointer) { lv_indev_set_cursor(indev, cursorObj) }
    func getCursor() -> OpaquePointer? { lv_indev_get_cursor(indev) }
    func getPoint(_ point: UnsafeMutablePointer<lv_point_t>) { lv_indev_get_point(indev, point) }
    func getGestureDir() -> lv_dir_t { lv_indev_get_gesture_dir(indev) }
    func getScrollDir() -> lv_dir_t { lv_indev_get_scroll_dir(indev) }
    func getScrollObj() -> OpaquePointer? { lv_indev_get_scroll_obj(indev) }
    func getVect(_ point: UnsafeMutablePointer<lv_point_t>) { lv_indev_get_vect(indev, point) }
    func getShortClickStreak() -> UInt8 { lv_indev_get_short_click_streak(indev) }

    // MARK: - Keypad
    func setGroup(_ group: OpaquePointer) { lv_indev_set_group(indev, group) }
    func getKey() -> UInt32 { lv_indev_get_key(indev) }

    // MARK: - Button
    func setButtonPoints(_ points: UnsafePointer<lv_point_t>) { lv_indev_set_button_points(indev, points) }

    // MARK: - Events
    func addEventCallback(_ callback: @escaping lv_event_cb_t, filter: lv_event_code_t, userData: UnsafeMutableRawPointer? = nil) { lv_indev_add_event_cb(indev, callback, filter, userData) }
    func getEventCount() -> UInt32 { lv_indev_get_event_count(indev) }
    func getEventDescriptor(index: UInt32) -> OpaquePointer? { lv_indev_get_event_dsc(indev, index) }
    func removeEvent(index: UInt32) -> Bool { lv_indev_remove_event(indev, index) }
    func removeEventCallback(_ callback: lv_event_cb_t, userData: UnsafeMutableRawPointer?) -> UInt32 { lv_indev_remove_event_cb_with_user_data(indev, callback, userData) }
    func sendEvent(_ code: lv_event_code_t, param: UnsafeMutableRawPointer? = nil) -> lv_result_t { lv_indev_send_event(indev, code, param) }

    // MARK: - Static Functions
    static func getActiveObj() -> OpaquePointer? { lv_indev_get_active_obj() }
    static func searchObj(obj: OpaquePointer, point: UnsafeMutablePointer<lv_point_t>) -> OpaquePointer? { lv_indev_search_obj(obj, point) }
}
