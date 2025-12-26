extension LVGL {
    struct Object: ObjectProtocol {
        let obj: OpaquePointer

        init(obj: OpaquePointer) { self.obj = obj }
        init<T: ObjectProtocol>(parent: T?) { self.obj = lv_obj_create(parent?.obj) }

        struct Flag: OptionSet {
            let rawValue: UInt32
            static let hidden = Flag(rawValue: LV_OBJ_FLAG_HIDDEN.rawValue)
            static let clickable = Flag(rawValue: LV_OBJ_FLAG_CLICKABLE.rawValue)
            static let clickFocusable = Flag(rawValue: LV_OBJ_FLAG_CLICK_FOCUSABLE.rawValue)
            static let checkable = Flag(rawValue: LV_OBJ_FLAG_CHECKABLE.rawValue)
            static let scrollable = Flag(rawValue: LV_OBJ_FLAG_SCROLLABLE.rawValue)
            static let scrollElastic = Flag(rawValue: LV_OBJ_FLAG_SCROLL_ELASTIC.rawValue)
            static let scrollMomentum = Flag(rawValue: LV_OBJ_FLAG_SCROLL_MOMENTUM.rawValue)
            static let scrollOne = Flag(rawValue: LV_OBJ_FLAG_SCROLL_ONE.rawValue)
            static let scrollChainHor = Flag(rawValue: LV_OBJ_FLAG_SCROLL_CHAIN_HOR.rawValue)
            static let scrollChainVer = Flag(rawValue: LV_OBJ_FLAG_SCROLL_CHAIN_VER.rawValue)
            static let scrollChain = Flag(rawValue: LV_OBJ_FLAG_SCROLL_CHAIN.rawValue)
            static let scrollOnFocus = Flag(rawValue: LV_OBJ_FLAG_SCROLL_ON_FOCUS.rawValue)
            static let scrollWithArrow = Flag(rawValue: LV_OBJ_FLAG_SCROLL_WITH_ARROW.rawValue)
            static let snappable = Flag(rawValue: LV_OBJ_FLAG_SNAPPABLE.rawValue)
            static let pressLock = Flag(rawValue: LV_OBJ_FLAG_PRESS_LOCK.rawValue)
            static let eventBubble = Flag(rawValue: LV_OBJ_FLAG_EVENT_BUBBLE.rawValue)
            static let gestureBubble = Flag(rawValue: LV_OBJ_FLAG_GESTURE_BUBBLE.rawValue)
            static let advHittest = Flag(rawValue: LV_OBJ_FLAG_ADV_HITTEST.rawValue)
            static let ignoreLayout = Flag(rawValue: LV_OBJ_FLAG_IGNORE_LAYOUT.rawValue)
            static let floating = Flag(rawValue: LV_OBJ_FLAG_FLOATING.rawValue)
            static let sendDrawTaskEvents = Flag(rawValue: LV_OBJ_FLAG_SEND_DRAW_TASK_EVENTS.rawValue)
            static let overflowVisible = Flag(rawValue: LV_OBJ_FLAG_OVERFLOW_VISIBLE.rawValue)
            static let eventTrickle = Flag(rawValue: LV_OBJ_FLAG_EVENT_TRICKLE.rawValue)
            static let stateTrickle = Flag(rawValue: LV_OBJ_FLAG_STATE_TRICKLE.rawValue)

            static let layout1 = Flag(rawValue: LV_OBJ_FLAG_LAYOUT_1.rawValue)
            static let layout2 = Flag(rawValue: LV_OBJ_FLAG_LAYOUT_2.rawValue)
            static let widget1 = Flag(rawValue: LV_OBJ_FLAG_WIDGET_1.rawValue)
            static let widget2 = Flag(rawValue: LV_OBJ_FLAG_WIDGET_2.rawValue)
            static let user1 = Flag(rawValue: LV_OBJ_FLAG_USER_1.rawValue)
            static let user2 = Flag(rawValue: LV_OBJ_FLAG_USER_2.rawValue)
            static let user3 = Flag(rawValue: LV_OBJ_FLAG_USER_3.rawValue)
            static let user4 = Flag(rawValue: LV_OBJ_FLAG_USER_4.rawValue)
        }
    }

    protocol ObjectProtocol {
        var obj: OpaquePointer { get }
    }
}

extension LVGL.ObjectProtocol {
    func addFlag(_ flag: LVGL.Object.Flag) { lv_obj_add_flag(obj, lv_obj_flag_t(flag.rawValue)) }
    func removeFlag(_ flag: LVGL.Object.Flag) { lv_obj_remove_flag(obj, lv_obj_flag_t(flag.rawValue)) }
    func setFlag(_ flag: LVGL.Object.Flag, _ v: Bool) { lv_obj_set_flag(obj, lv_obj_flag_t(flag.rawValue), v) }
    func addState(_ state: LVGL.State) { lv_obj_add_state(obj, lv_state_t(state.rawValue)) }
    func removeState(_ state: LVGL.State) { lv_obj_remove_state(obj, lv_state_t(state.rawValue)) }
    func setState(_ state: LVGL.State, _ v: Bool) { lv_obj_set_state(obj, lv_state_t(state.rawValue), v) }
    func setUserData(_ data: UnsafeMutableRawPointer) { lv_obj_set_user_data(obj, data) }
    func hasFlag(_ flag: LVGL.Object.Flag) -> Bool { lv_obj_has_flag(obj, lv_obj_flag_t(flag.rawValue)) }
    func hasFlagAny(_ flag: LVGL.Object.Flag) -> Bool { lv_obj_has_flag_any(obj, lv_obj_flag_t(flag.rawValue)) }
    func getState() -> LVGL.State { LVGL.State(rawValue: lv_obj_get_state(obj).rawValue) }
    func hasState(_ state: LVGL.State) -> Bool { lv_obj_has_state(obj, lv_state_t(state.rawValue)) }
    func getGroup() -> OpaquePointer! { lv_obj_get_group(obj) }
    func getUserData() -> UnsafeMutableRawPointer { lv_obj_get_user_data(obj) }
    func allocateSpecAttr() { lv_obj_allocate_spec_attr(obj) }
    func checkType(_ class: OpaquePointer) -> Bool { lv_obj_check_type(obj, `class`) }
    func hasClass(_ class: OpaquePointer) -> Bool { lv_obj_has_class(obj, `class`) }
    func getClass() -> OpaquePointer { lv_obj_get_class(obj) }
    func isVaild() -> Bool { lv_obj_is_valid(obj) }
}
