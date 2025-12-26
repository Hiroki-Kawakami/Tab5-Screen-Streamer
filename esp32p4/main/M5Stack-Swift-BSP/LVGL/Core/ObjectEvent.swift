extension LVGL.ObjectProtocol {
    @discardableResult func addEventCb(_ eventCb: lv_event_cb_t, filter: lv_event_code_t, userData: UnsafeMutableRawPointer!) -> OpaquePointer! { lv_obj_add_event_cb(obj, eventCb, filter, userData) }
}
