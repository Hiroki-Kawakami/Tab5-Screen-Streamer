extension LVGL {
    struct State: OptionSet {
        let rawValue: UInt32
        static let `default` = State(rawValue: LV_STATE_DEFAULT.rawValue)
        static let checked = State(rawValue: LV_STATE_CHECKED.rawValue)
        static let focused = State(rawValue: LV_STATE_FOCUSED.rawValue)
        static let focusKey = State(rawValue: LV_STATE_FOCUS_KEY.rawValue)
        static let edited = State(rawValue: LV_STATE_EDITED.rawValue)
        static let hovered = State(rawValue: LV_STATE_HOVERED.rawValue)
        static let pressed = State(rawValue: LV_STATE_PRESSED.rawValue)
        static let scrolled = State(rawValue: LV_STATE_SCROLLED.rawValue)
        static let disabled = State(rawValue: LV_STATE_DISABLED.rawValue)
        static let user1 = State(rawValue: LV_STATE_USER_1.rawValue)
        static let user2 = State(rawValue: LV_STATE_USER_2.rawValue)
        static let user3 = State(rawValue: LV_STATE_USER_3.rawValue)
        static let user4 = State(rawValue: LV_STATE_USER_4.rawValue)
        static let any = State(rawValue: LV_STATE_ANY.rawValue)
    }

    struct Part: OptionSet {
        let rawValue: UInt32
        static let main = Part(rawValue: LV_PART_MAIN.rawValue)
        static let scrollbar = Part(rawValue: LV_PART_SCROLLBAR.rawValue)
        static let indicator = Part(rawValue: LV_PART_INDICATOR.rawValue)
        static let knob = Part(rawValue: LV_PART_KNOB.rawValue)
        static let selected = Part(rawValue: LV_PART_SELECTED.rawValue)
        static let items = Part(rawValue: LV_PART_ITEMS.rawValue)
        static let cursor = Part(rawValue: LV_PART_CURSOR.rawValue)
        static let customFirst = Part(rawValue: LV_PART_CUSTOM_FIRST.rawValue)
        static let any = Part(rawValue: LV_PART_ANY.rawValue)
    }

    struct StyleSelector: OptionSet {
        let rawValue: UInt32
        static let `default` = State(rawValue: LV_STATE_DEFAULT.rawValue)
        static let checked = State(rawValue: LV_STATE_CHECKED.rawValue)
        static let focused = State(rawValue: LV_STATE_FOCUSED.rawValue)
        static let focusKey = State(rawValue: LV_STATE_FOCUS_KEY.rawValue)
        static let edited = State(rawValue: LV_STATE_EDITED.rawValue)
        static let hovered = State(rawValue: LV_STATE_HOVERED.rawValue)
        static let pressed = State(rawValue: LV_STATE_PRESSED.rawValue)
        static let scrolled = State(rawValue: LV_STATE_SCROLLED.rawValue)
        static let disabled = State(rawValue: LV_STATE_DISABLED.rawValue)
        static let user1 = State(rawValue: LV_STATE_USER_1.rawValue)
        static let user2 = State(rawValue: LV_STATE_USER_2.rawValue)
        static let user3 = State(rawValue: LV_STATE_USER_3.rawValue)
        static let user4 = State(rawValue: LV_STATE_USER_4.rawValue)
        static let stateAny = State(rawValue: LV_STATE_ANY.rawValue)

        static let main = StyleSelector(rawValue: LV_PART_MAIN.rawValue)
        static let scrollbar = StyleSelector(rawValue: LV_PART_SCROLLBAR.rawValue)
        static let indicator = StyleSelector(rawValue: LV_PART_INDICATOR.rawValue)
        static let knob = StyleSelector(rawValue: LV_PART_KNOB.rawValue)
        static let selected = StyleSelector(rawValue: LV_PART_SELECTED.rawValue)
        static let items = StyleSelector(rawValue: LV_PART_ITEMS.rawValue)
        static let cursor = StyleSelector(rawValue: LV_PART_CURSOR.rawValue)
        static let customFirst = StyleSelector(rawValue: LV_PART_CUSTOM_FIRST.rawValue)
        static let partAny = StyleSelector(rawValue: LV_PART_ANY.rawValue)

        var state: State { State(rawValue: lv_obj_style_get_selector_state(rawValue).rawValue) }
        var part: Part { Part(rawValue: lv_obj_style_get_selector_part(rawValue).rawValue) }
    }

    struct StyleProp: RawRepresentable {
        let rawValue: UInt32
        static let inv = StyleProp(rawValue: LV_STYLE_PROP_INV.rawValue)
        static let width = StyleProp(rawValue: LV_STYLE_WIDTH.rawValue)
        static let height = StyleProp(rawValue: LV_STYLE_HEIGHT.rawValue)
        static let length = StyleProp(rawValue: LV_STYLE_LENGTH.rawValue)
        static let minWidth = StyleProp(rawValue: LV_STYLE_MIN_WIDTH.rawValue)
        static let maxWidth = StyleProp(rawValue: LV_STYLE_MAX_WIDTH.rawValue)
        static let minHeight = StyleProp(rawValue: LV_STYLE_MIN_HEIGHT.rawValue)
        static let maxHeight = StyleProp(rawValue: LV_STYLE_MAX_HEIGHT.rawValue)
        static let x = StyleProp(rawValue: LV_STYLE_X.rawValue)
        static let y = StyleProp(rawValue: LV_STYLE_Y.rawValue)
        static let align = StyleProp(rawValue: LV_STYLE_ALIGN.rawValue)
        static let radius = StyleProp(rawValue: LV_STYLE_RADIUS.rawValue)
        static let radialOffset = StyleProp(rawValue: LV_STYLE_RADIAL_OFFSET.rawValue)
        static let padRadial = StyleProp(rawValue: LV_STYLE_PAD_RADIAL.rawValue)
        static let padTop = StyleProp(rawValue: LV_STYLE_PAD_TOP.rawValue)
        static let padBottom = StyleProp(rawValue: LV_STYLE_PAD_BOTTOM.rawValue)
        static let padLeft = StyleProp(rawValue: LV_STYLE_PAD_LEFT.rawValue)
        static let padRight = StyleProp(rawValue: LV_STYLE_PAD_RIGHT.rawValue)
        static let padRow = StyleProp(rawValue: LV_STYLE_PAD_ROW.rawValue)
        static let padColumn = StyleProp(rawValue: LV_STYLE_PAD_COLUMN.rawValue)
        static let layout = StyleProp(rawValue: LV_STYLE_LAYOUT.rawValue)
        static let marginTop = StyleProp(rawValue: LV_STYLE_MARGIN_TOP.rawValue)
        static let marginBottom = StyleProp(rawValue: LV_STYLE_MARGIN_BOTTOM.rawValue)
        static let marginLeft = StyleProp(rawValue: LV_STYLE_MARGIN_LEFT.rawValue)
        static let marginRight = StyleProp(rawValue: LV_STYLE_MARGIN_RIGHT.rawValue)
        static let bgColor = StyleProp(rawValue: LV_STYLE_BG_COLOR.rawValue)
        static let bgOpa = StyleProp(rawValue: LV_STYLE_BG_OPA.rawValue)
        static let bgGradDir = StyleProp(rawValue: LV_STYLE_BG_GRAD_DIR.rawValue)
        static let bgMainStop = StyleProp(rawValue: LV_STYLE_BG_MAIN_STOP.rawValue)
        static let bgGradStop = StyleProp(rawValue: LV_STYLE_BG_GRAD_STOP.rawValue)
        static let bgGradColor = StyleProp(rawValue: LV_STYLE_BG_GRAD_COLOR.rawValue)
        static let bgMainOpa = StyleProp(rawValue: LV_STYLE_BG_MAIN_OPA.rawValue)
        static let bgGradOpa = StyleProp(rawValue: LV_STYLE_BG_GRAD_OPA.rawValue)
        static let bgGrad = StyleProp(rawValue: LV_STYLE_BG_GRAD.rawValue)
        static let baseDir = StyleProp(rawValue: LV_STYLE_BASE_DIR.rawValue)
        static let bgImageSrc = StyleProp(rawValue: LV_STYLE_BG_IMAGE_SRC.rawValue)
        static let bgImageOpa = StyleProp(rawValue: LV_STYLE_BG_IMAGE_OPA.rawValue)
        static let bgImageRecolor = StyleProp(rawValue: LV_STYLE_BG_IMAGE_RECOLOR.rawValue)
        static let bgImageRecolorOpa = StyleProp(rawValue: LV_STYLE_BG_IMAGE_RECOLOR_OPA.rawValue)
        static let bgImageTiled = StyleProp(rawValue: LV_STYLE_BG_IMAGE_TILED.rawValue)
        static let clipCorner = StyleProp(rawValue: LV_STYLE_CLIP_CORNER.rawValue)
        static let borderWidth = StyleProp(rawValue: LV_STYLE_BORDER_WIDTH.rawValue)
        static let borderColor = StyleProp(rawValue: LV_STYLE_BORDER_COLOR.rawValue)
        static let borderOpa = StyleProp(rawValue: LV_STYLE_BORDER_OPA.rawValue)
        static let borderSide = StyleProp(rawValue: LV_STYLE_BORDER_SIDE.rawValue)
        static let borderPost = StyleProp(rawValue: LV_STYLE_BORDER_POST.rawValue)
        static let outlineWidth = StyleProp(rawValue: LV_STYLE_OUTLINE_WIDTH.rawValue)
        static let outlineColor = StyleProp(rawValue: LV_STYLE_OUTLINE_COLOR.rawValue)
        static let outlineOpa = StyleProp(rawValue: LV_STYLE_OUTLINE_OPA.rawValue)
        static let outlinePad = StyleProp(rawValue: LV_STYLE_OUTLINE_PAD.rawValue)
        static let shadowWidth = StyleProp(rawValue: LV_STYLE_SHADOW_WIDTH.rawValue)
        static let shadowColor = StyleProp(rawValue: LV_STYLE_SHADOW_COLOR.rawValue)
        static let shadowOpa = StyleProp(rawValue: LV_STYLE_SHADOW_OPA.rawValue)
        static let shadowOffsetX = StyleProp(rawValue: LV_STYLE_SHADOW_OFFSET_X.rawValue)
        static let shadowOffsetY = StyleProp(rawValue: LV_STYLE_SHADOW_OFFSET_Y.rawValue)
        static let shadowSpread = StyleProp(rawValue: LV_STYLE_SHADOW_SPREAD.rawValue)
        static let imageOpa = StyleProp(rawValue: LV_STYLE_IMAGE_OPA.rawValue)
        static let imageRecolor = StyleProp(rawValue: LV_STYLE_IMAGE_RECOLOR.rawValue)
        static let imageRecolorOpa = StyleProp(rawValue: LV_STYLE_IMAGE_RECOLOR_OPA.rawValue)
        static let lineWidth = StyleProp(rawValue: LV_STYLE_LINE_WIDTH.rawValue)
        static let lineDashWidth = StyleProp(rawValue: LV_STYLE_LINE_DASH_WIDTH.rawValue)
        static let lineDashGap = StyleProp(rawValue: LV_STYLE_LINE_DASH_GAP.rawValue)
        static let lineRounded = StyleProp(rawValue: LV_STYLE_LINE_ROUNDED.rawValue)
        static let lineColor = StyleProp(rawValue: LV_STYLE_LINE_COLOR.rawValue)
        static let lineOpa = StyleProp(rawValue: LV_STYLE_LINE_OPA.rawValue)
        static let arcWidth = StyleProp(rawValue: LV_STYLE_ARC_WIDTH.rawValue)
        static let arcRounded = StyleProp(rawValue: LV_STYLE_ARC_ROUNDED.rawValue)
        static let arcColor = StyleProp(rawValue: LV_STYLE_ARC_COLOR.rawValue)
        static let arcOpa = StyleProp(rawValue: LV_STYLE_ARC_OPA.rawValue)
        static let arcImageSrc = StyleProp(rawValue: LV_STYLE_ARC_IMAGE_SRC.rawValue)
        static let textColor = StyleProp(rawValue: LV_STYLE_TEXT_COLOR.rawValue)
        static let textOpa = StyleProp(rawValue: LV_STYLE_TEXT_OPA.rawValue)
        static let textFont = StyleProp(rawValue: LV_STYLE_TEXT_FONT.rawValue)
        static let textLetterSpace = StyleProp(rawValue: LV_STYLE_TEXT_LETTER_SPACE.rawValue)
        static let textLineSpace = StyleProp(rawValue: LV_STYLE_TEXT_LINE_SPACE.rawValue)
        static let textDecor = StyleProp(rawValue: LV_STYLE_TEXT_DECOR.rawValue)
        static let textAlign = StyleProp(rawValue: LV_STYLE_TEXT_ALIGN.rawValue)
        static let textOutlineStrokeWidth = StyleProp(rawValue: LV_STYLE_TEXT_OUTLINE_STROKE_WIDTH.rawValue)
        static let textOutlineStrokeOpa = StyleProp(rawValue: LV_STYLE_TEXT_OUTLINE_STROKE_OPA.rawValue)
        static let textOutlineStrokeColor = StyleProp(rawValue: LV_STYLE_TEXT_OUTLINE_STROKE_COLOR.rawValue)
        static let opa = StyleProp(rawValue: LV_STYLE_OPA.rawValue)
        static let opaLayered = StyleProp(rawValue: LV_STYLE_OPA_LAYERED.rawValue)
        static let colorFilterDsc = StyleProp(rawValue: LV_STYLE_COLOR_FILTER_DSC.rawValue)
        static let colorFilterOpa = StyleProp(rawValue: LV_STYLE_COLOR_FILTER_OPA.rawValue)
        static let anim = StyleProp(rawValue: LV_STYLE_ANIM.rawValue)
        static let animDuration = StyleProp(rawValue: LV_STYLE_ANIM_DURATION.rawValue)
        static let transition = StyleProp(rawValue: LV_STYLE_TRANSITION.rawValue)
        static let blendMode = StyleProp(rawValue: LV_STYLE_BLEND_MODE.rawValue)
        static let transformWidth = StyleProp(rawValue: LV_STYLE_TRANSFORM_WIDTH.rawValue)
        static let transformHeight = StyleProp(rawValue: LV_STYLE_TRANSFORM_HEIGHT.rawValue)
        static let translateX = StyleProp(rawValue: LV_STYLE_TRANSLATE_X.rawValue)
        static let translateY = StyleProp(rawValue: LV_STYLE_TRANSLATE_Y.rawValue)
        static let transformScaleX = StyleProp(rawValue: LV_STYLE_TRANSFORM_SCALE_X.rawValue)
        static let transformScaleY = StyleProp(rawValue: LV_STYLE_TRANSFORM_SCALE_Y.rawValue)
        static let transformRotation = StyleProp(rawValue: LV_STYLE_TRANSFORM_ROTATION.rawValue)
        static let transformPivotX = StyleProp(rawValue: LV_STYLE_TRANSFORM_PIVOT_X.rawValue)
        static let transformPivotY = StyleProp(rawValue: LV_STYLE_TRANSFORM_PIVOT_Y.rawValue)
        static let transformSkewX = StyleProp(rawValue: LV_STYLE_TRANSFORM_SKEW_X.rawValue)
        static let transformSkewY = StyleProp(rawValue: LV_STYLE_TRANSFORM_SKEW_Y.rawValue)
        static let bitmapMaskSrc = StyleProp(rawValue: LV_STYLE_BITMAP_MASK_SRC.rawValue)
        static let rotarySensitivity = StyleProp(rawValue: LV_STYLE_ROTARY_SENSITIVITY.rawValue)
        static let translateRadial = StyleProp(rawValue: LV_STYLE_TRANSLATE_RADIAL.rawValue)
        static let recolor = StyleProp(rawValue: LV_STYLE_RECOLOR.rawValue)
        static let recolorOpa = StyleProp(rawValue: LV_STYLE_RECOLOR_OPA.rawValue)
        static let flexFlow = StyleProp(rawValue: LV_STYLE_FLEX_FLOW.rawValue)
        static let flexMainPlace = StyleProp(rawValue: LV_STYLE_FLEX_MAIN_PLACE.rawValue)
        static let flexCrossPlace = StyleProp(rawValue: LV_STYLE_FLEX_CROSS_PLACE.rawValue)
        static let flexTrackPlace = StyleProp(rawValue: LV_STYLE_FLEX_TRACK_PLACE.rawValue)
        static let flexGrow = StyleProp(rawValue: LV_STYLE_FLEX_GROW.rawValue)
        static let gridColumnAlign = StyleProp(rawValue: LV_STYLE_GRID_COLUMN_ALIGN.rawValue)
        static let gridRowAlign = StyleProp(rawValue: LV_STYLE_GRID_ROW_ALIGN.rawValue)
        static let gridRowDscArray = StyleProp(rawValue: LV_STYLE_GRID_ROW_DSC_ARRAY.rawValue)
        static let gridColumnDscArray = StyleProp(rawValue: LV_STYLE_GRID_COLUMN_DSC_ARRAY.rawValue)
        static let gridCellColumnPos = StyleProp(rawValue: LV_STYLE_GRID_CELL_COLUMN_POS.rawValue)
        static let gridCellColumnSpan = StyleProp(rawValue: LV_STYLE_GRID_CELL_COLUMN_SPAN.rawValue)
        static let gridCellXAlign = StyleProp(rawValue: LV_STYLE_GRID_CELL_X_ALIGN.rawValue)
        static let gridCellRowPos = StyleProp(rawValue: LV_STYLE_GRID_CELL_ROW_POS.rawValue)
        static let gridCellRowSpan = StyleProp(rawValue: LV_STYLE_GRID_CELL_ROW_SPAN.rawValue)
        static let gridCellYAlign = StyleProp(rawValue: LV_STYLE_GRID_CELL_Y_ALIGN.rawValue)
        static let imageColorkey = StyleProp(rawValue: LV_STYLE_IMAGE_COLORKEY.rawValue)
        static let lastBuiltInProp = StyleProp(rawValue: LV_STYLE_LAST_BUILT_IN_PROP.rawValue)
        static let numBuiltInProps = StyleProp(rawValue: LV_STYLE_NUM_BUILT_IN_PROPS.rawValue)
        static let any = StyleProp(rawValue: LV_STYLE_PROP_ANY.rawValue)
        static let const = StyleProp(rawValue: LV_STYLE_PROP_CONST.rawValue)
    }

    struct Style {
        let obj: UnsafeMutablePointer<lv_style_t>
    }

    typealias StyleValue = lv_style_value_t
    typealias StyleRes = lv_style_res_t
}

extension LVGL.ObjectProtocol {
    func addStyle(_ style: LVGL.Style, selector: LVGL.StyleSelector) { lv_obj_add_style(obj, style.obj, selector.rawValue) }
    func replaceStyle(old: LVGL.Style, new: LVGL.Style, selector: LVGL.StyleSelector) { lv_obj_replace_style(obj, old.obj, new.obj, selector.rawValue) }
    func removeStyle(_ style: LVGL.Style, selector: LVGL.StyleSelector) { lv_obj_remove_style(obj, style.obj, selector.rawValue) }
    func removeStyleAll() { lv_obj_remove_style_all(obj) }
    static func reportStyleChange(_ style: LVGL.Style) { lv_obj_report_style_change(style.obj) }
    func refreshStyle(part: LVGL.Part, prop: LVGL.StyleProp) { lv_obj_refresh_style(obj, lv_part_t(part.rawValue), lv_style_prop_t(prop.rawValue)) }
    func styleSetDisabled(_ style: LVGL.Style, selector: LVGL.StyleSelector, disabled: Bool) { lv_obj_style_set_disabled(obj, style.obj, selector.rawValue, disabled) }
    func styleGetDisabled(_ style: LVGL.Style, selector: LVGL.StyleSelector) -> Bool { lv_obj_style_get_disabled(obj, style.obj, selector.rawValue) }
    static func enableStyleRefresh(en: Bool) { lv_obj_enable_style_refresh(en) }
    func getStyleProp(part: LVGL.Part, prop: LVGL.StyleProp) -> LVGL.StyleValue { lv_obj_get_style_prop(obj, lv_part_t(part.rawValue), lv_style_prop_t(prop.rawValue)) }
    func hasStyleProp(selector: LVGL.StyleSelector, prop: LVGL.StyleProp) -> Bool { lv_obj_has_style_prop(obj, selector.rawValue, lv_style_prop_t(prop.rawValue)) }
    func setLocalStyleProp(prop: LVGL.StyleProp, value: LVGL.StyleValue, selector: LVGL.StyleSelector) { lv_obj_set_local_style_prop(obj, lv_style_prop_t(prop.rawValue), value, selector.rawValue) }
    func getLocalStyleProp(prop: LVGL.StyleProp, value: UnsafeMutablePointer<LVGL.StyleValue>, selector: LVGL.StyleSelector) -> LVGL.StyleRes { lv_obj_get_local_style_prop(obj, lv_style_prop_t(prop.rawValue), value, selector.rawValue) }
    func removeLocalStyleProp(prop: LVGL.StyleProp, selector: LVGL.StyleSelector) -> Bool { lv_obj_remove_local_style_prop(obj, lv_style_prop_t(prop.rawValue), selector.rawValue) }
    func styleApplyColorFilter(part: LVGL.Part, value: LVGL.StyleValue) -> LVGL.StyleValue { lv_obj_style_apply_color_filter(obj, lv_part_t(part.rawValue), value) }
    func fadeIn(time: UInt32, delay: UInt32) { lv_obj_fade_in(obj, time, delay) }
    func fadeOut(time: UInt32, delay: UInt32) { lv_obj_fade_out(obj, time, delay) }

    // MARK: - Style Getters (from lv_obj_style_gen.h)

    // Size
    func getStyleWidth(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_width(obj, lv_part_t(part.rawValue)) }
    func getStyleMinWidth(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_min_width(obj, lv_part_t(part.rawValue)) }
    func getStyleMaxWidth(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_max_width(obj, lv_part_t(part.rawValue)) }
    func getStyleHeight(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_height(obj, lv_part_t(part.rawValue)) }
    func getStyleMinHeight(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_min_height(obj, lv_part_t(part.rawValue)) }
    func getStyleMaxHeight(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_max_height(obj, lv_part_t(part.rawValue)) }
    func getStyleLength(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_length(obj, lv_part_t(part.rawValue)) }

    // Position
    func getStyleX(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_x(obj, lv_part_t(part.rawValue)) }
    func getStyleY(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_y(obj, lv_part_t(part.rawValue)) }
    func getStyleAlign(part: LVGL.Part = .main) -> LVGL.Align { LVGL.Align(rawValue: lv_obj_get_style_align(obj, lv_part_t(part.rawValue)).rawValue) }

    // Transform
    func getStyleTransformWidth(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_transform_width(obj, lv_part_t(part.rawValue)) }
    func getStyleTransformHeight(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_transform_height(obj, lv_part_t(part.rawValue)) }
    func getStyleTranslateX(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_translate_x(obj, lv_part_t(part.rawValue)) }
    func getStyleTranslateY(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_translate_y(obj, lv_part_t(part.rawValue)) }
    func getStyleTranslateRadial(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_translate_radial(obj, lv_part_t(part.rawValue)) }
    func getStyleTransformScaleX(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_transform_scale_x(obj, lv_part_t(part.rawValue)) }
    func getStyleTransformScaleY(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_transform_scale_y(obj, lv_part_t(part.rawValue)) }
    func getStyleTransformRotation(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_transform_rotation(obj, lv_part_t(part.rawValue)) }
    func getStyleTransformPivotX(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_transform_pivot_x(obj, lv_part_t(part.rawValue)) }
    func getStyleTransformPivotY(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_transform_pivot_y(obj, lv_part_t(part.rawValue)) }
    func getStyleTransformSkewX(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_transform_skew_x(obj, lv_part_t(part.rawValue)) }
    func getStyleTransformSkewY(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_transform_skew_y(obj, lv_part_t(part.rawValue)) }

    // Padding
    func getStylePadTop(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_pad_top(obj, lv_part_t(part.rawValue)) }
    func getStylePadBottom(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_pad_bottom(obj, lv_part_t(part.rawValue)) }
    func getStylePadLeft(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_pad_left(obj, lv_part_t(part.rawValue)) }
    func getStylePadRight(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_pad_right(obj, lv_part_t(part.rawValue)) }
    func getStylePadRow(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_pad_row(obj, lv_part_t(part.rawValue)) }
    func getStylePadColumn(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_pad_column(obj, lv_part_t(part.rawValue)) }
    func getStylePadRadial(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_pad_radial(obj, lv_part_t(part.rawValue)) }

    // Margin
    func getStyleMarginTop(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_margin_top(obj, lv_part_t(part.rawValue)) }
    func getStyleMarginBottom(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_margin_bottom(obj, lv_part_t(part.rawValue)) }
    func getStyleMarginLeft(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_margin_left(obj, lv_part_t(part.rawValue)) }
    func getStyleMarginRight(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_margin_right(obj, lv_part_t(part.rawValue)) }

    // Background
    func getStyleBgColor(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_bg_color(obj, lv_part_t(part.rawValue))) }
    func getStyleBgColorFiltered(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_bg_color_filtered(obj, lv_part_t(part.rawValue))) }
    func getStyleBgOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_bg_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleBgGradColor(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_bg_grad_color(obj, lv_part_t(part.rawValue))) }
    func getStyleBgGradColorFiltered(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_bg_grad_color_filtered(obj, lv_part_t(part.rawValue))) }
    func getStyleBgGradDir(part: LVGL.Part = .main) -> lv_grad_dir_t { lv_obj_get_style_bg_grad_dir(obj, lv_part_t(part.rawValue)) }
    func getStyleBgMainStop(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_bg_main_stop(obj, lv_part_t(part.rawValue)) }
    func getStyleBgGradStop(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_bg_grad_stop(obj, lv_part_t(part.rawValue)) }
    func getStyleBgMainOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_bg_main_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleBgGradOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_bg_grad_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleBgGrad(part: LVGL.Part = .main) -> UnsafePointer<lv_grad_dsc_t>? { lv_obj_get_style_bg_grad(obj, lv_part_t(part.rawValue)) }
    func getStyleBgImageSrc(part: LVGL.Part = .main) -> UnsafeRawPointer? { lv_obj_get_style_bg_image_src(obj, lv_part_t(part.rawValue)) }
    func getStyleBgImageOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_bg_image_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleBgImageRecolor(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_bg_image_recolor(obj, lv_part_t(part.rawValue))) }
    func getStyleBgImageRecolorFiltered(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_bg_image_recolor_filtered(obj, lv_part_t(part.rawValue))) }
    func getStyleBgImageRecolorOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_bg_image_recolor_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleBgImageTiled(part: LVGL.Part = .main) -> Bool { lv_obj_get_style_bg_image_tiled(obj, lv_part_t(part.rawValue)) }

    // Border
    func getStyleBorderColor(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_border_color(obj, lv_part_t(part.rawValue))) }
    func getStyleBorderColorFiltered(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_border_color_filtered(obj, lv_part_t(part.rawValue))) }
    func getStyleBorderOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_border_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleBorderWidth(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_border_width(obj, lv_part_t(part.rawValue)) }
    func getStyleBorderSide(part: LVGL.Part = .main) -> lv_border_side_t { lv_obj_get_style_border_side(obj, lv_part_t(part.rawValue)) }
    func getStyleBorderPost(part: LVGL.Part = .main) -> Bool { lv_obj_get_style_border_post(obj, lv_part_t(part.rawValue)) }

    // Outline
    func getStyleOutlineWidth(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_outline_width(obj, lv_part_t(part.rawValue)) }
    func getStyleOutlineColor(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_outline_color(obj, lv_part_t(part.rawValue))) }
    func getStyleOutlineColorFiltered(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_outline_color_filtered(obj, lv_part_t(part.rawValue))) }
    func getStyleOutlineOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_outline_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleOutlinePad(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_outline_pad(obj, lv_part_t(part.rawValue)) }

    // Shadow
    func getStyleShadowWidth(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_shadow_width(obj, lv_part_t(part.rawValue)) }
    func getStyleShadowOffsetX(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_shadow_offset_x(obj, lv_part_t(part.rawValue)) }
    func getStyleShadowOffsetY(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_shadow_offset_y(obj, lv_part_t(part.rawValue)) }
    func getStyleShadowSpread(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_shadow_spread(obj, lv_part_t(part.rawValue)) }
    func getStyleShadowColor(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_shadow_color(obj, lv_part_t(part.rawValue))) }
    func getStyleShadowColorFiltered(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_shadow_color_filtered(obj, lv_part_t(part.rawValue))) }
    func getStyleShadowOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_shadow_opa(obj, lv_part_t(part.rawValue))) }

    // Image
    func getStyleImageOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_image_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleImageRecolor(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_image_recolor(obj, lv_part_t(part.rawValue))) }
    func getStyleImageRecolorFiltered(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_image_recolor_filtered(obj, lv_part_t(part.rawValue))) }
    func getStyleImageRecolorOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_image_recolor_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleImageColorkey(part: LVGL.Part = .main) -> UnsafePointer<lv_image_colorkey_t>? { lv_obj_get_style_image_colorkey(obj, lv_part_t(part.rawValue)) }

    // Line
    func getStyleLineWidth(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_line_width(obj, lv_part_t(part.rawValue)) }
    func getStyleLineDashWidth(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_line_dash_width(obj, lv_part_t(part.rawValue)) }
    func getStyleLineDashGap(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_line_dash_gap(obj, lv_part_t(part.rawValue)) }
    func getStyleLineRounded(part: LVGL.Part = .main) -> Bool { lv_obj_get_style_line_rounded(obj, lv_part_t(part.rawValue)) }
    func getStyleLineColor(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_line_color(obj, lv_part_t(part.rawValue))) }
    func getStyleLineColorFiltered(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_line_color_filtered(obj, lv_part_t(part.rawValue))) }
    func getStyleLineOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_line_opa(obj, lv_part_t(part.rawValue))) }

    // Arc
    func getStyleArcWidth(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_arc_width(obj, lv_part_t(part.rawValue)) }
    func getStyleArcRounded(part: LVGL.Part = .main) -> Bool { lv_obj_get_style_arc_rounded(obj, lv_part_t(part.rawValue)) }
    func getStyleArcColor(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_arc_color(obj, lv_part_t(part.rawValue))) }
    func getStyleArcColorFiltered(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_arc_color_filtered(obj, lv_part_t(part.rawValue))) }
    func getStyleArcOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_arc_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleArcImageSrc(part: LVGL.Part = .main) -> UnsafeRawPointer? { lv_obj_get_style_arc_image_src(obj, lv_part_t(part.rawValue)) }

    // Text
    func getStyleTextColor(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_text_color(obj, lv_part_t(part.rawValue))) }
    func getStyleTextColorFiltered(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_text_color_filtered(obj, lv_part_t(part.rawValue))) }
    func getStyleTextOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_text_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleTextFont(part: LVGL.Part = .main) -> UnsafePointer<lv_font_t>? { lv_obj_get_style_text_font(obj, lv_part_t(part.rawValue)) }
    func getStyleTextLetterSpace(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_text_letter_space(obj, lv_part_t(part.rawValue)) }
    func getStyleTextLineSpace(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_text_line_space(obj, lv_part_t(part.rawValue)) }
    func getStyleTextDecor(part: LVGL.Part = .main) -> lv_text_decor_t { lv_obj_get_style_text_decor(obj, lv_part_t(part.rawValue)) }
    func getStyleTextAlign(part: LVGL.Part = .main) -> lv_text_align_t { lv_obj_get_style_text_align(obj, lv_part_t(part.rawValue)) }
    func getStyleTextOutlineStrokeColor(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_text_outline_stroke_color(obj, lv_part_t(part.rawValue))) }
    func getStyleTextOutlineStrokeColorFiltered(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_text_outline_stroke_color_filtered(obj, lv_part_t(part.rawValue))) }
    func getStyleTextOutlineStrokeWidth(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_text_outline_stroke_width(obj, lv_part_t(part.rawValue)) }
    func getStyleTextOutlineStrokeOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_text_outline_stroke_opa(obj, lv_part_t(part.rawValue))) }

    // Misc
    func getStyleRadius(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_radius(obj, lv_part_t(part.rawValue)) }
    func getStyleRadialOffset(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_radial_offset(obj, lv_part_t(part.rawValue)) }
    func getStyleClipCorner(part: LVGL.Part = .main) -> Bool { lv_obj_get_style_clip_corner(obj, lv_part_t(part.rawValue)) }
    func getStyleOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleOpaLayered(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_opa_layered(obj, lv_part_t(part.rawValue))) }
    func getStyleColorFilterDsc(part: LVGL.Part = .main) -> UnsafePointer<lv_color_filter_dsc_t>? { lv_obj_get_style_color_filter_dsc(obj, lv_part_t(part.rawValue)) }
    func getStyleColorFilterOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_color_filter_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleRecolor(part: LVGL.Part = .main) -> LVGL.Color { LVGL.Color(lv_color: lv_obj_get_style_recolor(obj, lv_part_t(part.rawValue))) }
    func getStyleRecolorOpa(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_recolor_opa(obj, lv_part_t(part.rawValue))) }
    func getStyleAnim(part: LVGL.Part = .main) -> UnsafePointer<lv_anim_t>? { lv_obj_get_style_anim(obj, lv_part_t(part.rawValue)) }
    func getStyleAnimDuration(part: LVGL.Part = .main) -> UInt32 { lv_obj_get_style_anim_duration(obj, lv_part_t(part.rawValue)) }
    func getStyleTransition(part: LVGL.Part = .main) -> UnsafePointer<lv_style_transition_dsc_t>? { lv_obj_get_style_transition(obj, lv_part_t(part.rawValue)) }
    func getStyleBlendMode(part: LVGL.Part = .main) -> lv_blend_mode_t { lv_obj_get_style_blend_mode(obj, lv_part_t(part.rawValue)) }
    func getStyleLayout(part: LVGL.Part = .main) -> UInt16 { lv_obj_get_style_layout(obj, lv_part_t(part.rawValue)) }
    func getStyleBaseDir(part: LVGL.Part = .main) -> lv_base_dir_t { lv_obj_get_style_base_dir(obj, lv_part_t(part.rawValue)) }
    func getStyleBitmapMaskSrc(part: LVGL.Part = .main) -> UnsafeRawPointer? { lv_obj_get_style_bitmap_mask_src(obj, lv_part_t(part.rawValue)) }
    func getStyleRotarySensitivity(part: LVGL.Part = .main) -> UInt32 { lv_obj_get_style_rotary_sensitivity(obj, lv_part_t(part.rawValue)) }

    // MARK: - Style Setters (from lv_obj_style_gen.h)

    // Size
    func setStyleWidth(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_width(obj, value, selector.rawValue) }
    func setStyleMinWidth(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_min_width(obj, value, selector.rawValue) }
    func setStyleMaxWidth(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_max_width(obj, value, selector.rawValue) }
    func setStyleHeight(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_height(obj, value, selector.rawValue) }
    func setStyleMinHeight(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_min_height(obj, value, selector.rawValue) }
    func setStyleMaxHeight(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_max_height(obj, value, selector.rawValue) }
    func setStyleLength(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_length(obj, value, selector.rawValue) }

    // Position
    func setStyleX(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_x(obj, value, selector.rawValue) }
    func setStyleY(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_y(obj, value, selector.rawValue) }
    func setStyleAlign(_ value: LVGL.Align, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_align(obj, lv_align_t(value.rawValue), selector.rawValue) }

    // Transform
    func setStyleTransformWidth(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_transform_width(obj, value, selector.rawValue) }
    func setStyleTransformHeight(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_transform_height(obj, value, selector.rawValue) }
    func setStyleTranslateX(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_translate_x(obj, value, selector.rawValue) }
    func setStyleTranslateY(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_translate_y(obj, value, selector.rawValue) }
    func setStyleTranslateRadial(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_translate_radial(obj, value, selector.rawValue) }
    func setStyleTransformScaleX(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_transform_scale_x(obj, value, selector.rawValue) }
    func setStyleTransformScaleY(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_transform_scale_y(obj, value, selector.rawValue) }
    func setStyleTransformRotation(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_transform_rotation(obj, value, selector.rawValue) }
    func setStyleTransformPivotX(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_transform_pivot_x(obj, value, selector.rawValue) }
    func setStyleTransformPivotY(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_transform_pivot_y(obj, value, selector.rawValue) }
    func setStyleTransformSkewX(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_transform_skew_x(obj, value, selector.rawValue) }
    func setStyleTransformSkewY(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_transform_skew_y(obj, value, selector.rawValue) }

    // Padding
    func setStylePadTop(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_pad_top(obj, value, selector.rawValue) }
    func setStylePadBottom(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_pad_bottom(obj, value, selector.rawValue) }
    func setStylePadLeft(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_pad_left(obj, value, selector.rawValue) }
    func setStylePadRight(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_pad_right(obj, value, selector.rawValue) }
    func setStylePadRow(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_pad_row(obj, value, selector.rawValue) }
    func setStylePadColumn(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_pad_column(obj, value, selector.rawValue) }
    func setStylePadRadial(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_pad_radial(obj, value, selector.rawValue) }

    // Margin
    func setStyleMarginTop(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_margin_top(obj, value, selector.rawValue) }
    func setStyleMarginBottom(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_margin_bottom(obj, value, selector.rawValue) }
    func setStyleMarginLeft(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_margin_left(obj, value, selector.rawValue) }
    func setStyleMarginRight(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_margin_right(obj, value, selector.rawValue) }

    // Background
    func setStyleBgColor(_ value: LVGL.Color, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_color(obj, value.lv_color, selector.rawValue) }
    func setStyleBgOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleBgGradColor(_ value: LVGL.Color, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_grad_color(obj, value.lv_color, selector.rawValue) }
    func setStyleBgGradDir(_ value: lv_grad_dir_t, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_grad_dir(obj, value, selector.rawValue) }
    func setStyleBgMainStop(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_main_stop(obj, value, selector.rawValue) }
    func setStyleBgGradStop(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_grad_stop(obj, value, selector.rawValue) }
    func setStyleBgMainOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_main_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleBgGradOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_grad_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleBgGrad(_ value: UnsafePointer<lv_grad_dsc_t>, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_grad(obj, value, selector.rawValue) }
    func setStyleBgImageSrc(_ value: UnsafeRawPointer, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_image_src(obj, value, selector.rawValue) }
    func setStyleBgImageOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_image_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleBgImageRecolor(_ value: LVGL.Color, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_image_recolor(obj, value.lv_color, selector.rawValue) }
    func setStyleBgImageRecolorOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_image_recolor_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleBgImageTiled(_ value: Bool, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bg_image_tiled(obj, value, selector.rawValue) }

    // Border
    func setStyleBorderColor(_ value: LVGL.Color, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_border_color(obj, value.lv_color, selector.rawValue) }
    func setStyleBorderOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_border_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleBorderWidth(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_border_width(obj, value, selector.rawValue) }
    func setStyleBorderSide(_ value: lv_border_side_t, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_border_side(obj, value, selector.rawValue) }
    func setStyleBorderPost(_ value: Bool, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_border_post(obj, value, selector.rawValue) }

    // Outline
    func setStyleOutlineWidth(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_outline_width(obj, value, selector.rawValue) }
    func setStyleOutlineColor(_ value: LVGL.Color, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_outline_color(obj, value.lv_color, selector.rawValue) }
    func setStyleOutlineOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_outline_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleOutlinePad(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_outline_pad(obj, value, selector.rawValue) }

    // Shadow
    func setStyleShadowWidth(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_shadow_width(obj, value, selector.rawValue) }
    func setStyleShadowOffsetX(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_shadow_offset_x(obj, value, selector.rawValue) }
    func setStyleShadowOffsetY(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_shadow_offset_y(obj, value, selector.rawValue) }
    func setStyleShadowSpread(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_shadow_spread(obj, value, selector.rawValue) }
    func setStyleShadowColor(_ value: LVGL.Color, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_shadow_color(obj, value.lv_color, selector.rawValue) }
    func setStyleShadowOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_shadow_opa(obj, value.rawValue, selector.rawValue) }

    // Image
    func setStyleImageOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_image_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleImageRecolor(_ value: LVGL.Color, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_image_recolor(obj, value.lv_color, selector.rawValue) }
    func setStyleImageRecolorOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_image_recolor_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleImageColorkey(_ value: UnsafePointer<lv_image_colorkey_t>, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_image_colorkey(obj, value, selector.rawValue) }

    // Line
    func setStyleLineWidth(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_line_width(obj, value, selector.rawValue) }
    func setStyleLineDashWidth(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_line_dash_width(obj, value, selector.rawValue) }
    func setStyleLineDashGap(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_line_dash_gap(obj, value, selector.rawValue) }
    func setStyleLineRounded(_ value: Bool, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_line_rounded(obj, value, selector.rawValue) }
    func setStyleLineColor(_ value: LVGL.Color, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_line_color(obj, value.lv_color, selector.rawValue) }
    func setStyleLineOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_line_opa(obj, value.rawValue, selector.rawValue) }

    // Arc
    func setStyleArcWidth(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_arc_width(obj, value, selector.rawValue) }
    func setStyleArcRounded(_ value: Bool, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_arc_rounded(obj, value, selector.rawValue) }
    func setStyleArcColor(_ value: LVGL.Color, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_arc_color(obj, value.lv_color, selector.rawValue) }
    func setStyleArcOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_arc_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleArcImageSrc(_ value: UnsafeRawPointer, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_arc_image_src(obj, value, selector.rawValue) }

    // Text
    func setStyleTextColor(_ value: LVGL.Color, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_text_color(obj, value.lv_color, selector.rawValue) }
    func setStyleTextOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_text_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleTextFont(_ value: UnsafePointer<lv_font_t>, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_text_font(obj, value, selector.rawValue) }
    func setStyleTextLetterSpace(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_text_letter_space(obj, value, selector.rawValue) }
    func setStyleTextLineSpace(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_text_line_space(obj, value, selector.rawValue) }
    func setStyleTextDecor(_ value: lv_text_decor_t, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_text_decor(obj, value, selector.rawValue) }
    func setStyleTextAlign(_ value: lv_text_align_t, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_text_align(obj, value, selector.rawValue) }
    func setStyleTextOutlineStrokeColor(_ value: LVGL.Color, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_text_outline_stroke_color(obj, value.lv_color, selector.rawValue) }
    func setStyleTextOutlineStrokeWidth(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_text_outline_stroke_width(obj, value, selector.rawValue) }
    func setStyleTextOutlineStrokeOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_text_outline_stroke_opa(obj, value.rawValue, selector.rawValue) }

    // Misc
    func setStyleRadius(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_radius(obj, value, selector.rawValue) }
    func setStyleRadialOffset(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_radial_offset(obj, value, selector.rawValue) }
    func setStyleClipCorner(_ value: Bool, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_clip_corner(obj, value, selector.rawValue) }
    func setStyleOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleOpaLayered(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_opa_layered(obj, value.rawValue, selector.rawValue) }
    func setStyleColorFilterDsc(_ value: UnsafePointer<lv_color_filter_dsc_t>, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_color_filter_dsc(obj, value, selector.rawValue) }
    func setStyleColorFilterOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_color_filter_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleRecolor(_ value: LVGL.Color, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_recolor(obj, value.lv_color, selector.rawValue) }
    func setStyleRecolorOpa(_ value: LVGL.Opacity, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_recolor_opa(obj, value.rawValue, selector.rawValue) }
    func setStyleAnim(_ value: UnsafePointer<lv_anim_t>, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_anim(obj, value, selector.rawValue) }
    func setStyleAnimDuration(_ value: UInt32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_anim_duration(obj, value, selector.rawValue) }
    func setStyleTransition(_ value: UnsafePointer<lv_style_transition_dsc_t>, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_transition(obj, value, selector.rawValue) }
    func setStyleBlendMode(_ value: lv_blend_mode_t, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_blend_mode(obj, value, selector.rawValue) }
    func setStyleLayout(_ value: UInt16, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_layout(obj, value, selector.rawValue) }
    func setStyleBaseDir(_ value: lv_base_dir_t, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_base_dir(obj, value, selector.rawValue) }
    func setStyleBitmapMaskSrc(_ value: UnsafeRawPointer, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_bitmap_mask_src(obj, value, selector.rawValue) }
    func setStyleRotarySensitivity(_ value: UInt32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_rotary_sensitivity(obj, value, selector.rawValue) }

    // MARK: - Convenience Style Setters (from lv_obj_style.h after lv_obj_style_gen.h)

    func setStylePadAll(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_pad_all(obj, value, selector.rawValue) }
    func setStylePadHor(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_pad_hor(obj, value, selector.rawValue) }
    func setStylePadVer(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_pad_ver(obj, value, selector.rawValue) }
    func setStylePadGap(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_pad_gap(obj, value, selector.rawValue) }
    func setStyleMarginAll(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_margin_all(obj, value, selector.rawValue) }
    func setStyleMarginHor(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_margin_hor(obj, value, selector.rawValue) }
    func setStyleMarginVer(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_margin_ver(obj, value, selector.rawValue) }
    func setStyleSize(width: Int32, height: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_size(obj, width, height, selector.rawValue) }
    func setStyleTransformScale(_ value: Int32, selector: LVGL.StyleSelector = .main) { lv_obj_set_style_transform_scale(obj, value, selector.rawValue) }

    // MARK: - Convenience Style Getters (from lv_obj_style.h after lv_obj_style_gen.h)

    func getStyleSpaceLeft(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_space_left(obj, lv_part_t(part.rawValue)) }
    func getStyleSpaceRight(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_space_right(obj, lv_part_t(part.rawValue)) }
    func getStyleSpaceTop(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_space_top(obj, lv_part_t(part.rawValue)) }
    func getStyleSpaceBottom(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_space_bottom(obj, lv_part_t(part.rawValue)) }
    func getStyleTransformScaleXSafe(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_transform_scale_x_safe(obj, lv_part_t(part.rawValue)) }
    func getStyleTransformScaleYSafe(part: LVGL.Part = .main) -> Int32 { lv_obj_get_style_transform_scale_y_safe(obj, lv_part_t(part.rawValue)) }

    // MARK: - Additional Style Functions (from lv_obj_style.h)

    func calculateStyleTextAlign(part: LVGL.Part = .main, text: String) -> lv_text_align_t {
        text.withCString { lv_obj_calculate_style_text_align(obj, lv_part_t(part.rawValue), $0) }
    }

    func getStyleOpaRecursive(part: LVGL.Part = .main) -> LVGL.Opacity { LVGL.Opacity(rawValue: lv_obj_get_style_opa_recursive(obj, lv_part_t(part.rawValue))) }
    func styleApplyRecolor(part: LVGL.Part = .main, color: lv_color32_t) -> lv_color32_t { lv_obj_style_apply_recolor(obj, lv_part_t(part.rawValue), color) }
    func getStyleRecolorRecursive(part: LVGL.Part = .main) -> lv_color32_t { lv_obj_get_style_recolor_recursive(obj, lv_part_t(part.rawValue)) }
}
