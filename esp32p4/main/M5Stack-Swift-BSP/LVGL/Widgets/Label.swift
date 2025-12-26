extension LVGL {
    struct Label: ObjectProtocol {
        let obj: OpaquePointer

        init(obj: OpaquePointer) { self.obj = obj }
        init<T: ObjectProtocol>(parent: T?) { self.obj = lv_label_create(parent?.obj) }

        func setText(_ text: String) { text.withCString { lv_label_set_text(obj, $0) } }
        func setTextStatic(_ text: StaticString) { text.withUTF8Buffer { lv_label_set_text_static(obj, $0.baseAddress) } }
    }
}
