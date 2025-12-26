enum LVGL {

    static func begin() throws(IDF.Error) {
        var config = lvgl_port_cfg_t(
            task_priority: 4,
            task_stack: 7168,
            task_affinity: 0,
            task_max_sleep_ms: 500,
            task_stack_caps: UInt32(MALLOC_CAP_INTERNAL | MALLOC_CAP_DEFAULT),
            timer_period_ms: 5
        )
        try IDF.Error.check(lvgl_port_init(&config))
    }

    static func withLock(_ proc: () -> Void) {
        lv_lock()
        proc()
        lv_unlock()
    }

    static func asyncCall(_ proc: @escaping () -> Void) {
        let wrapper = FFI.Wrapper(proc)
        lv_async_call({ ptr in
            let wrapper = Unmanaged<FFI.Wrapper<() -> Void>>.fromOpaque(ptr!).takeRetainedValue()
            wrapper.value()
        }, Unmanaged.passRetained(wrapper).toOpaque())
    }
}
