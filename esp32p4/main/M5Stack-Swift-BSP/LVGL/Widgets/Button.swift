extension LVGL {
    struct Button: ObjectProtocol {
        let obj: OpaquePointer

        init(obj: OpaquePointer) { self.obj = obj }
        init<T: ObjectProtocol>(parent: T?) { self.obj = lv_button_create(parent?.obj) }
    }
}
