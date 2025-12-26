extension LVGL.ObjectProtocol {
    // MARK: - Position
    func setPos(x: Int32, y: Int32) { lv_obj_set_pos(obj, x, y) }
    func setX(_ x: Int32) { lv_obj_set_x(obj, x) }
    func setY(_ y: Int32) { lv_obj_set_y(obj, y) }

    // MARK: - Size
    func setSize(width: Int32, height: Int32) { lv_obj_set_size(obj, width, height) }
    func refreshSize() -> Bool { lv_obj_refr_size(obj) }
    func setWidth(_ width: Int32) { lv_obj_set_width(obj, width) }
    func setHeight(_ height: Int32) { lv_obj_set_height(obj, height) }
    func setContentWidth(_ width: Int32) { lv_obj_set_content_width(obj, width) }
    func setContentHeight(_ height: Int32) { lv_obj_set_content_height(obj, height) }

    // MARK: - Layout
    func setLayout(_ layout: UInt32) { lv_obj_set_layout(obj, layout) }
    func isLayoutPositioned() -> Bool { lv_obj_is_layout_positioned(obj) }
    func markLayoutAsDirty() { lv_obj_mark_layout_as_dirty(obj) }
    func updateLayout() { lv_obj_update_layout(obj) }

    // MARK: - Alignment
    func setAlign(_ align: LVGL.Align) { lv_obj_set_align(obj, lv_align_t(align.rawValue)) }
    func align(_ align: LVGL.Align, xOffset: Int32 = 0, yOffset: Int32 = 0) { lv_obj_align(obj, lv_align_t(align.rawValue), xOffset, yOffset) }
    func alignTo<T: LVGL.ObjectProtocol>(base: T, align: LVGL.Align, xOffset: Int32 = 0, yOffset: Int32 = 0) { lv_obj_align_to(obj, base.obj, lv_align_t(align.rawValue), xOffset, yOffset) }
    func center() { lv_obj_center(obj) }

    // MARK: - Transform
    func setTransform(_ matrix: OpaquePointer) { lv_obj_set_transform(obj, matrix) }
    func resetTransform() { lv_obj_reset_transform(obj) }
    func getTransform() -> OpaquePointer? { lv_obj_get_transform(obj) }
    func transformPoint(_ point: UnsafeMutablePointer<lv_point_t>, flags: lv_obj_point_transform_flag_t) { lv_obj_transform_point(obj, point, flags) }
    func transformPointArray(_ points: UnsafeMutablePointer<lv_point_t>, count: Int, flags: lv_obj_point_transform_flag_t) { lv_obj_transform_point_array(obj, points, count, flags) }
    func getTransformedArea(_ area: UnsafeMutablePointer<lv_area_t>, flags: lv_obj_point_transform_flag_t) { lv_obj_get_transformed_area(obj, area, flags) }

    // MARK: - Get Coordinates
    func getCoords(_ coords: UnsafeMutablePointer<lv_area_t>) { lv_obj_get_coords(obj, coords) }
    func getX() -> Int32 { lv_obj_get_x(obj) }
    func getX2() -> Int32 { lv_obj_get_x2(obj) }
    func getY() -> Int32 { lv_obj_get_y(obj) }
    func getY2() -> Int32 { lv_obj_get_y2(obj) }
    func getXAligned() -> Int32 { lv_obj_get_x_aligned(obj) }
    func getYAligned() -> Int32 { lv_obj_get_y_aligned(obj) }
    func getWidth() -> Int32 { lv_obj_get_width(obj) }
    func getHeight() -> Int32 { lv_obj_get_height(obj) }
    func getContentWidth() -> Int32 { lv_obj_get_content_width(obj) }
    func getContentHeight() -> Int32 { lv_obj_get_content_height(obj) }
    func getContentCoords(_ area: UnsafeMutablePointer<lv_area_t>) { lv_obj_get_content_coords(obj, area) }
    func getSelfWidth() -> Int32 { lv_obj_get_self_width(obj) }
    func getSelfHeight() -> Int32 { lv_obj_get_self_height(obj) }
    func refreshSelfSize() -> Bool { lv_obj_refresh_self_size(obj) }

    // MARK: - Move
    func refreshPos() { lv_obj_refr_pos(obj) }
    func moveTo(x: Int32, y: Int32) { lv_obj_move_to(obj, x, y) }
    func moveChildrenBy(xDiff: Int32, yDiff: Int32, ignoreFloating: Bool) { lv_obj_move_children_by(obj, xDiff, yDiff, ignoreFloating) }

    // MARK: - Invalidate
    func invalidateArea(_ area: UnsafePointer<lv_area_t>) { lv_obj_invalidate_area(obj, area) }
    func invalidate() { lv_obj_invalidate(obj) }
    func areaIsVisible(_ area: UnsafeMutablePointer<lv_area_t>) -> Bool { lv_obj_area_is_visible(obj, area) }
    func isVisible() -> Bool { lv_obj_is_visible(obj) }

    // MARK: - Click Area
    func setExtClickArea(_ size: Int32) { lv_obj_set_ext_click_area(obj, size) }
    func getClickArea(_ area: UnsafeMutablePointer<lv_area_t>) { lv_obj_get_click_area(obj, area) }
    func hitTest(_ point: UnsafePointer<lv_point_t>) -> Bool { lv_obj_hit_test(obj, point) }
}

// MARK: - Global Functions
extension LVGL {
    static func clampWidth(_ width: Int32, min: Int32, max: Int32, ref: Int32) -> Int32 { lv_clamp_width(width, min, max, ref) }
    static func clampHeight(_ height: Int32, min: Int32, max: Int32, ref: Int32) -> Int32 { lv_clamp_height(height, min, max, ref) }
}
