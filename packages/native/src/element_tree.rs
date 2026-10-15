/// Event types for Rust → JS communication.
/// Element IDs are f64 (JS numbers) — lossless for integers up to 2^53.
///
/// EventPayload is the single struct sent across the napi boundary for ALL
/// event types. Fields are optional — each event type populates only the
/// fields it needs. This avoids N different napi structs while keeping the
/// FFI surface small.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use napi_derive::napi;

/// Event payload sent back to JS when a user interacts with an element.
#[derive(Debug, Clone)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(serde::Serialize)
)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    serde(rename_all = "camelCase")
)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct EventPayload {
    /// Numeric element ID (matches the ID assigned in JS via createElement).
    pub element_id: f64,

    /// Event type string — matches the key used in EVENT_PROPS on the JS side.
    /// e.g. "click", "mousedown", "mouseenter", "keyDown", "scroll", etc. Pointer,
    /// mouse and drag types are the lowercase DOM names.
    pub event_type: String,

    // ── Mouse position ───────────────────────────────────────────────
    /// Mouse X position in window coordinates (pixels).
    pub x: Option<f64>,
    /// Mouse Y position in window coordinates (pixels).
    pub y: Option<f64>,

    // ── Mouse button ─────────────────────────────────────────────────
    /// Which mouse button: 0=left, 1=middle, 2=right.
    /// Populated for: mousedown, mouseup, click, auxclick, contextmenu, mousedownoutside.
    pub button: Option<u32>,

    /// Number of consecutive clicks (1=single, 2=double, 3=triple).
    /// Populated for: the click-family and down/up events; `detail` carries the same count.
    pub click_count: Option<u32>,

    /// Whether this is a right-click (convenience for click events).
    /// true when button==2 or ClickEvent::is_right_click().
    pub is_right_click: Option<bool>,

    /// Which mouse button is currently held during a mouseMove.
    /// Same encoding as `button`: 0=left, 1=middle, 2=right.
    /// Populated for: mousemove.
    pub pressed_button: Option<u32>,

    // ── Keyboard ─────────────────────────────────────────────────────
    /// Key name, e.g. "a", "enter", "escape", "down", "left", "f1".
    /// Populated for: keyDown, keyUp.
    pub key: Option<String>,

    /// The character produced by the key press (e.g. "ß" for option-s).
    /// May differ from `key` when modifiers are active.
    /// Populated for: keyDown, keyUp.
    pub key_char: Option<String>,

    /// Whether this is a key-repeat event (key held down).
    /// Populated for: keyDown.
    pub is_held: Option<bool>,

    /// Id of the GPUI key dispatch that produced this event. Every element
    /// `keyDown` and the trailing `windowKeyDown` of one keystroke share it, so
    /// JS can apply `preventDefault` / `stopPropagation` across them.
    /// Populated for: keyDown, keyUp, windowKeyDown, windowKeyUp.
    pub keystroke_id: Option<f64>,

    // ── Scroll ───────────────────────────────────────────────────────
    /// Scroll delta on the X axis (pixels or lines, see `precise`).
    /// Populated for: scroll.
    pub delta_x: Option<f64>,

    /// Scroll delta on the Y axis (pixels or lines, see `precise`).
    /// Populated for: scroll.
    pub delta_y: Option<f64>,

    /// true = pixel-precise (trackpad), false = line-based (mouse wheel).
    /// Populated for: scroll.
    pub precise: Option<bool>,

    /// Touch phase for scroll: "started", "moved", "ended".
    /// Populated for: scroll (trackpad gestures).
    pub touch_phase: Option<String>,

    // ── Hover ────────────────────────────────────────────────────────
    /// true = mouse entered element, false = mouse left element.
    /// Populated for: mouseenter, mouseleave.
    pub hovered: Option<bool>,

    // ── Custom element payloads ──────────────────────────────────────
    /// Element-defined string payload.
    /// Populated for: `<diff>` toggleFile (the file path), showMore (the
    /// hidden line count), and lineClick (the line text); `<markdown>`
    /// linkClick (the URL); `selectionChange` (joined selected text, or
    /// absent when the selection is empty).
    pub value: Option<String>,

    /// Line number on the pre-change side. Populated for: `<diff>` lineClick.
    pub old_line: Option<f64>,

    /// Line number on the post-change side. Populated for: `<diff>` lineClick.
    pub new_line: Option<f64>,

    /// First visible logical index. Populated for: `<virtual-list>` visibleRange.
    pub start_index: Option<f64>,

    /// Exclusive end of the visible logical range. Populated for: visibleRange.
    pub end_index: Option<f64>,

    /// Border-box size of the element, in pixels. With `x` and `y`, its position
    /// in window coordinates. Populated for: layout.
    pub width: Option<f64>,
    pub height: Option<f64>,

    /// Scroll offset of the element, the pair `getScrollOffset` returns: 0 at the
    /// start and negative as the content moves. Populated for: scrollChange.
    pub scroll_x: Option<f64>,
    pub scroll_y: Option<f64>,

    /// Matches found by this element's `highlight` prop. Counted once per match
    /// even when it is split across several painted runs, and it counts every
    /// retained match, not only the ones currently on screen.
    /// Populated for: highlight.
    pub match_count: Option<f64>,

    /// Logical native motion target that reached completion.
    /// Populated for: motionComplete.
    pub motion_generation: Option<f64>,

    /// The files of an OS file drag (`dataTransfer.files`). Sent once per drag,
    /// with its first event, a `dragenter`.
    pub files: Option<Vec<DroppedFile>>,

    // ── DOM event propagation ────────────────────────────────────────
    /// Element the event was aimed at (the DOM `event.target`). Absent means
    /// `element_id` is the target. Ids are `f64` like `element_id`: `u64` would
    /// reach JS as a `bigint` and never match the `number` keys of the handler map.
    pub target: Option<f64>,

    /// Propagation path, target first and then each ancestor up to the root.
    /// JS runs capture over it root to target and bubble target to root. Absent
    /// means the event is handled at `element_id` only.
    pub path: Option<Vec<f64>>,

    /// The other element of a boundary event (the DOM `event.relatedTarget`).
    /// Populated for: mouseover, mouseout, mouseenter, mouseleave and the pointer equivalents.
    pub related_target: Option<f64>,

    // ── DOM pointer and mouse event fields ───────────────────────────
    /// Pointer position in window coordinates, the DOM `clientX`/`clientY`.
    /// Same values as `x`/`y`, which stay for existing handlers.
    pub client_x: Option<f64>,
    pub client_y: Option<f64>,

    /// Pointer position relative to the target's padding edge (its border box
    /// minus its border widths), the DOM `offsetX`/`offsetY`.
    pub offset_x: Option<f64>,
    pub offset_y: Option<f64>,

    /// Pointer motion since the previous move, in logical pixels, the DOM
    /// `movementX`/`movementY`. Populated for: pointermove, mousemove. Zero where
    /// the platform reports no relative motion.
    pub movement_x: Option<f64>,
    pub movement_y: Option<f64>,

    /// Buttons held while the event fires, as the DOM bitmask: 1 primary,
    /// 2 secondary, 4 auxiliary, 8 back, 16 forward.
    pub buttons: Option<u32>,

    /// The DOM `detail`: the consecutive click count on click-family and
    /// down/up events.
    pub detail: Option<u32>,

    /// The DOM `pointerId`. 1 for the mouse, -1 for a click no pointer made.
    pub pointer_id: Option<i32>,

    /// The DOM `pointerType`. "mouse" for the mouse, empty for a click no pointer made.
    pub pointer_type: Option<String>,

    // ── Modifiers ────────────────────────────────────────────────────
    pub modifiers: Option<EventModifiers>,
}

impl Default for EventPayload {
    fn default() -> Self {
        Self {
            element_id: 0.0,
            event_type: String::new(),
            x: None,
            y: None,
            button: None,
            click_count: None,
            is_right_click: None,
            pressed_button: None,
            key: None,
            key_char: None,
            is_held: None,
            keystroke_id: None,
            delta_x: None,
            delta_y: None,
            precise: None,
            touch_phase: None,
            hovered: None,
            value: None,
            old_line: None,
            new_line: None,
            start_index: None,
            end_index: None,
            width: None,
            height: None,
            scroll_x: None,
            scroll_y: None,
            match_count: None,
            motion_generation: None,
            files: None,
            target: None,
            path: None,
            related_target: None,
            client_x: None,
            client_y: None,
            offset_x: None,
            offset_y: None,
            movement_x: None,
            movement_y: None,
            buttons: None,
            detail: None,
            pointer_id: None,
            pointer_type: None,
            modifiers: None,
        }
    }
}

/// One file of an OS file drag, as the DOM `File` plus the path the browser
/// would hide.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(serde::Serialize)
)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    serde(rename_all = "camelCase")
)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct DroppedFile {
    pub path: String,
    pub name: String,
    pub size: f64,
    /// MIME type guessed from the extension, empty when unknown.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        serde(rename = "type")
    )]
    #[cfg_attr(
        not(all(target_arch = "wasm32", target_os = "unknown")),
        napi(js_name = "type")
    )]
    pub mime_type: String,
    /// Milliseconds since the Unix epoch.
    pub last_modified: f64,
}

#[derive(Debug, Clone)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(serde::Serialize)
)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct EventModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub cmd: bool,
}

impl Default for EventModifiers {
    fn default() -> Self {
        Self {
            shift: false,
            ctrl: false,
            alt: false,
            cmd: false,
        }
    }
}

/// One highlight wash painted in the last frame, with the boxes it drew.
///
/// The rects matter: a quad never lands in `getPaintedText()`, and a match that
/// soft-wraps must produce one box per visual row. Without the geometry the only
/// way to assert either is a screenshot.
#[derive(Debug, Clone)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(serde::Serialize)
)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    serde(rename_all = "camelCase")
)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct HighlightMatch {
    /// Numeric id of the element that painted the run.
    pub element_id: f64,
    /// Index of the run within that element. 0 for a plain `<text>`.
    pub sub: f64,
    /// The full string of the run, so `text.slice(start, end)` is the match.
    pub text: String,
    /// UTF-16 code-unit offsets into `text`, the units JS strings use.
    pub start: f64,
    pub end: f64,
    pub active: bool,
    pub rects: Vec<HighlightRect>,
}

#[derive(Debug, Clone)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(serde::Serialize)
)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct HighlightRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl From<crate::text::PaintedHighlight> for HighlightMatch {
    fn from(painted: crate::text::PaintedHighlight) -> Self {
        Self {
            element_id: painted.element_id as f64,
            sub: painted.sub as f64,
            text: painted.text.to_string(),
            start: painted.start as f64,
            end: painted.end as f64,
            active: painted.active,
            rects: painted
                .rects
                .into_iter()
                .map(|(x, y, width, height)| HighlightRect {
                    x: x as f64,
                    y: y as f64,
                    width: width as f64,
                    height: height as f64,
                })
                .collect(),
        }
    }
}

/// Convert GPUI Modifiers → our napi EventModifiers.
impl From<gpui::Modifiers> for EventModifiers {
    fn from(m: gpui::Modifiers) -> Self {
        Self {
            shift: m.shift,
            ctrl: m.control,
            alt: m.alt,
            cmd: m.platform, // platform = Cmd on macOS, Win on Windows
        }
    }
}
