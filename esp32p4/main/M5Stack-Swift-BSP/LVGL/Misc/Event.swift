
extension LVGL {
    struct Event {
        let e: OpaquePointer

        // Get event information
        func getTarget() -> UnsafeMutableRawPointer! { lv_event_get_target(e) }
        func getCurrentTarget() -> UnsafeMutableRawPointer! { lv_event_get_current_target(e) }
        func getCode() -> lv_event_code_t { lv_event_get_code(e) }
        func getParam() -> UnsafeMutableRawPointer! { lv_event_get_param(e) }
        func getUserData() -> UnsafeMutableRawPointer! { lv_event_get_user_data(e) }

        // Event flow control
        func stopBubbling() { lv_event_stop_bubbling(e) }
        func stopTrickling() { lv_event_stop_trickling(e) }
        func stopProcessing() { lv_event_stop_processing(e) }
    }
}
