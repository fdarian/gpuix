//! Native single-line and multiline text editors with platform IME support.
//!
//! The editor follows GPUI's input example:
//! https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/input.rs
//! Caret blinking, double-click, drag autoscroll, and bounded undo follow
//! Comet's composer (MIT). Use Comet only as a generic editor behavior
//! reference; its composer contains app-specific code.
//! Upstream: https://github.com/zeronsh/comet/blob/main/crates/ui/src/composer.rs
//! Reviewed at: https://github.com/zeronsh/comet/blob/b3fa51872f70c8f973c241b659cf0c166766f4f5/crates/ui/src/composer.rs

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, size, App, Bounds, ClipboardEntry,
    ClipboardItem, Context, CursorStyle, DispatchPhase, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, GlobalElementId, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ScrollWheelEvent,
    SharedString, Style, Task, TextRun, TextStyle, UTF16Selection, UnderlineStyle, Window,
    WrappedLine,
};
use unicode_segmentation::UnicodeSegmentation;
use web_time::Instant;

use super::input_rules::{
    content_to_masked, masked, masked_to_content, number_from_json, step_from_json, step_number,
    truncate_to_utf16, InputType, NumberRules,
};
use super::text_selection::{
    describe, resolve, utf16_len, SelectionRequest, TextSelection,
};
use super::{CustomElement, CustomElementFactory, CustomRenderContext};
use crate::renderer::{emit_bubbling_event, emit_event_full, EventCallback, GpuixView};
use crate::retained_tree::RetainedTree;
use crate::theme::Theme;

actions!(
    gpuix_text_editor,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectAll,
        Home,
        End,
        DocStart,
        DocEnd,
        SelectHome,
        SelectEnd,
        SelectDocStart,
        SelectDocEnd,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteToLineStart,
        DeleteToLineEnd,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Newline,
        Submit,
        StepUp,
        StepDown,
    ]
);

const INPUT_KEY_CONTEXT: &str = "GpuixInput";
/// `type="number"` is an input that also owns ArrowUp and ArrowDown.
const NUMBER_KEY_CONTEXT: &str = "GpuixNumberInput";
const NUMBER_INPUT_KEY_CONTEXT: &str = "GpuixInput GpuixNumberInput";
const TEXTAREA_KEY_CONTEXT: &str = "GpuixTextarea";
const CARET_BLINK_MS: u64 = 500;
const CARET_WIDTH: Pixels = px(2.0);
const DRAG_SCROLL_FRAME_MS: u64 = 16;
const UNDO_COALESCE: Duration = Duration::from_millis(700);
const UNDO_LIMIT: usize = 200;

fn caret_visible(ms_since_activity: u64) -> bool {
    (ms_since_activity / CARET_BLINK_MS) % 2 == 0
}

fn clipboard_text(item: ClipboardItem) -> Option<String> {
    if item
        .entries
        .iter()
        .any(|entry| matches!(entry, ClipboardEntry::ExternalPaths(_)))
    {
        return None;
    }
    item.text()
}

// Chrome paints the caret at line height, not font size. Firefox uses the em
// square after you type. Match Chrome so a 14/20 composer bar is 20px tall.
fn caret_rect(origin: Point<Pixels>, line_height: Pixels) -> Bounds<Pixels> {
    Bounds::new(origin, size(CARET_WIDTH, line_height))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PressIntent {
    SelectAll,
    SelectWord,
    ExtendSelection,
    PlaceCaret,
}

impl PressIntent {
    fn arms_drag(self) -> bool {
        matches!(self, Self::ExtendSelection | Self::PlaceCaret)
    }
}

fn press_intent(click_count: usize, shift: bool) -> PressIntent {
    match click_count {
        n if n >= 3 => PressIntent::SelectAll,
        2 => PressIntent::SelectWord,
        _ if shift => PressIntent::ExtendSelection,
        _ => PressIntent::PlaceCaret,
    }
}

fn drag_scroll_delta(
    pointer_y: f32,
    viewport_top: f32,
    viewport_bottom: f32,
    line_height: f32,
) -> f32 {
    let distance = if pointer_y < viewport_top {
        pointer_y - viewport_top
    } else if pointer_y > viewport_bottom {
        pointer_y - viewport_bottom
    } else {
        return 0.0;
    };
    distance.signum() * (distance.abs() * 0.2).clamp(1.0, line_height)
}

fn utf16_offset_to_utf8(text: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for character in text.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += character.len_utf16();
        utf8_offset += character.len_utf8();
    }
    utf8_offset
}

fn single_line_text(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

pub fn init(cx: &mut App) {
    let word_navigation_uses_alt = word_navigation_uses_alt();
    let bind_paste_shortcut = !cfg!(all(target_arch = "wasm32", target_os = "unknown"));
    let mut bindings = text_editor_bindings(
        INPUT_KEY_CONTEXT,
        false,
        word_navigation_uses_alt,
        bind_paste_shortcut,
    );
    bindings.extend(text_editor_bindings(
        TEXTAREA_KEY_CONTEXT,
        true,
        word_navigation_uses_alt,
        bind_paste_shortcut,
    ));
    bindings.extend([
        KeyBinding::new("up", StepUp, Some(NUMBER_KEY_CONTEXT)),
        KeyBinding::new("down", StepDown, Some(NUMBER_KEY_CONTEXT)),
    ]);
    cx.bind_keys(bindings);
}

fn text_editor_bindings(
    context: &'static str,
    multiline: bool,
    word_navigation_uses_alt: bool,
    bind_paste_shortcut: bool,
) -> Vec<KeyBinding> {
    let context = Some(context);
    let mut bindings = vec![
        if multiline {
            KeyBinding::new("enter", Newline, context)
        } else {
            KeyBinding::new("enter", Submit, context)
        },
        KeyBinding::new("shift-enter", Newline, context),
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("shift-home", SelectHome, context),
        KeyBinding::new("shift-end", SelectEnd, context),
        KeyBinding::new("cmd-left", Home, context),
        KeyBinding::new("cmd-right", End, context),
        KeyBinding::new("cmd-backspace", DeleteToLineStart, context),
        KeyBinding::new("cmd-delete", DeleteToLineEnd, context),
        KeyBinding::new("cmd-up", DocStart, context),
        KeyBinding::new("cmd-down", DocEnd, context),
        KeyBinding::new("shift-cmd-left", SelectHome, context),
        KeyBinding::new("shift-cmd-right", SelectEnd, context),
        KeyBinding::new("shift-cmd-up", SelectDocStart, context),
        KeyBinding::new("shift-cmd-down", SelectDocEnd, context),
    ];
    if multiline {
        bindings.extend([
            KeyBinding::new("up", Up, context),
            KeyBinding::new("down", Down, context),
            KeyBinding::new("shift-up", SelectUp, context),
            KeyBinding::new("shift-down", SelectDown, context),
        ]);
    }

    let word_prefix = if word_navigation_uses_alt {
        "alt"
    } else {
        "ctrl"
    };
    bindings.extend([
        KeyBinding::new(&format!("{word_prefix}-backspace"), DeleteWordLeft, context),
        KeyBinding::new(&format!("{word_prefix}-delete"), DeleteWordRight, context),
        KeyBinding::new(&format!("{word_prefix}-left"), WordLeft, context),
        KeyBinding::new(&format!("{word_prefix}-right"), WordRight, context),
        KeyBinding::new(
            &format!("shift-{word_prefix}-left"),
            SelectWordLeft,
            context,
        ),
        KeyBinding::new(
            &format!("shift-{word_prefix}-right"),
            SelectWordRight,
            context,
        ),
    ]);
    for prefix in ["cmd", "ctrl"] {
        bindings.extend([
            KeyBinding::new(&format!("{prefix}-a"), SelectAll, context),
            KeyBinding::new(&format!("{prefix}-c"), Copy, context),
            KeyBinding::new(&format!("{prefix}-x"), Cut, context),
            KeyBinding::new(&format!("{prefix}-z"), Undo, context),
            KeyBinding::new(&format!("shift-{prefix}-z"), Redo, context),
        ]);
        if bind_paste_shortcut {
            bindings.push(KeyBinding::new(&format!("{prefix}-v"), Paste, context));
        }
    }
    bindings
}

fn browser_platform_is_macos(platform: &str, user_agent: &str) -> bool {
    platform.starts_with("Mac")
        || user_agent.contains("Macintosh")
        || user_agent.contains("Mac OS X")
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn word_navigation_uses_alt() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let navigator = window.navigator();
    let platform = navigator.platform().unwrap_or_default();
    let user_agent = navigator.user_agent().unwrap_or_default();
    browser_platform_is_macos(&platform, &user_agent)
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn word_navigation_uses_alt() -> bool {
    cfg!(target_os = "macos")
}

pub struct InputFactory;

impl CustomElementFactory for InputFactory {
    fn element_type(&self) -> &str {
        "input"
    }

    fn create(&self, _id: u64) -> Box<dyn CustomElement> {
        Box::new(TextEditorElement::new(false))
    }
}

pub struct TextareaFactory;

impl CustomElementFactory for TextareaFactory {
    fn element_type(&self) -> &str {
        "textarea"
    }

    fn create(&self, _id: u64) -> Box<dyn CustomElement> {
        Box::new(TextEditorElement::new(true))
    }
}

struct TextEditorElement {
    multiline: bool,
    value: String,
    placeholder: String,
    read_only: bool,
    disabled: bool,
    input_type: InputType,
    max_length: Option<usize>,
    min: Option<f64>,
    max: Option<f64>,
    step: f64,
    min_rows: usize,
    max_rows: usize,
    last_prop_value: Option<String>,
    theme: Theme,
    state: Option<Entity<TextEditorState>>,
    /// Stands in for the real focus handle while disabled: the editor keeps one
    /// but never tracks it, so nothing can focus it.
    inert_focus: Option<FocusHandle>,
    /// A selection requested before the first render created the state.
    pending_selection: Option<SelectionRequest>,
}

impl TextEditorElement {
    fn new(multiline: bool) -> Self {
        Self {
            multiline,
            value: String::new(),
            placeholder: String::new(),
            read_only: false,
            disabled: false,
            input_type: InputType::Text,
            max_length: None,
            min: None,
            max: None,
            step: 1.0,
            min_rows: 1,
            max_rows: if multiline { 10 } else { 1 },
            last_prop_value: None,
            theme: Theme::dark(),
            state: None,
            inert_focus: None,
            pending_selection: None,
        }
    }

    /// `type` is ignored on `<textarea>`, as in the DOM.
    fn effective_type(&self) -> InputType {
        if self.multiline {
            InputType::Text
        } else {
            self.input_type
        }
    }

    fn number_rules(&self) -> Option<NumberRules> {
        (self.effective_type() == InputType::Number).then_some(NumberRules {
            min: self.min,
            max: self.max,
            step: self.step,
        })
    }

    /// Push a changed `value` prop into the editor now, rather than at the next
    /// frame, so an imperative call right after a controlled update sees it.
    fn apply_value(&mut self, cx: &mut App) {
        let Some(state) = self.state.clone() else {
            return;
        };
        if self.last_prop_value.as_ref() == Some(&self.value) {
            return;
        }
        let value = self.value.clone();
        state.update(cx, |state, cx| state.sync_prop_value(value, cx));
        self.last_prop_value = Some(self.value.clone());
    }
}

/// The role of an `<input>` of this type, or of a `<textarea>`.
pub(crate) fn default_role(input_type: InputType, multiline: bool) -> gpui::Role {
    if multiline {
        return gpui::Role::MultilineTextInput;
    }
    match input_type {
        InputType::Search => gpui::Role::SearchInput,
        InputType::Password => gpui::Role::PasswordInput,
        InputType::Number => gpui::Role::SpinButton,
        _ => gpui::Role::TextInput,
    }
}

impl CustomElement for TextEditorElement {
    fn render(
        &mut self,
        ctx: CustomRenderContext,
        _window: &mut Window,
        cx: &mut Context<crate::renderer::GpuixView>,
    ) -> gpui::AnyElement {
        // Inert takes the same path as disabled: no focus handle, no mouse.
        let disabled = self.disabled || ctx.inert;
        let input_type = self.effective_type();
        let number_rules = self.number_rules();
        let focus_handle = if disabled {
            self.inert_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone()
        } else {
            ctx.focus_handle
                .cloned()
                .unwrap_or_else(|| cx.focus_handle())
        };
        let emits_change = ctx.events.contains("change");
        let callback = ctx.event_callback.clone();
        let tree = ctx.activation.tree.clone();
        let view = cx.weak_entity();

        let state = self
            .state
            .get_or_insert_with(|| {
                let value = self.value.clone();
                let placeholder = self.placeholder.clone();
                let multiline = self.multiline;
                let read_only = self.read_only;
                let disabled = self.disabled;
                let max_length = self.max_length;
                let muted_color = self.theme.text_muted;
                let initial_selection = self
                    .pending_selection
                    .take()
                    .map(|request| resolve(&value, request));
                let min_rows = self.min_rows;
                let max_rows = self.max_rows;
                let caret_color = self.theme.caret;
                let callback = callback.clone();
                let tree = tree.clone();
                let view = view.clone();
                let id = ctx.id;
                let cursor = value.len();
                let (selected_range, selection_reversed, directionless_range) =
                    match initial_selection {
                        Some(selection) => (
                            selection.range.clone(),
                            selection.reversed,
                            selection.directionless.then_some(selection.range),
                        ),
                        None => (cursor..cursor, false, None),
                    };
                let state_focus_handle = focus_handle.clone();
                cx.new(move |cx| TextEditorState {
                    element_id: id,
                    view,
                    callback,
                    tree,
                    emits_change,
                    focus_handle: state_focus_handle,
                    content: value,
                    placeholder: placeholder.into(),
                    multiline,
                    read_only,
                    disabled,
                    input_type,
                    max_length,
                    number_rules,
                    muted_color,
                    min_rows,
                    max_rows,
                    selected_range,
                    selection_reversed,
                    directionless_range,
                    marked_range: None,
                    is_selecting: false,
                    drag_position: None,
                    drag_generation: 0,
                    drag_autoscroll_active: false,
                    scroll_top: 0.0,
                    scroll_left: 0.0,
                    follow_cursor: true,
                    last_lines: Vec::new(),
                    line_starts: vec![0],
                    last_bounds: None,
                    line_height: px(20.0),
                    font_size: px(16.0),
                    content_height: 20.0,
                    content_width: 0.0,
                    display_is_placeholder: false,
                    caret_color,
                    blink_anchor: cx.background_executor().now(),
                    blink_task: None,
                    pending_values: VecDeque::new(),
                    undo_stack: VecDeque::new(),
                    redo_stack: Vec::new(),
                    last_edit: None,
                })
            })
            .clone();

        self.apply_value(cx);
        state.update(cx, |state, cx| {
            if state.focus_handle != focus_handle {
                state.focus_handle = focus_handle.clone();
            }
            if state.disabled != disabled {
                state.disabled = disabled;
                state.is_selecting = false;
                state.drag_position = None;
                cx.notify();
            }
            state.input_type = input_type;
            state.max_length = self.max_length;
            state.number_rules = number_rules;
            state.muted_color = self.theme.text_muted;
            state.callback = callback;
            state.emits_change = emits_change;
            state.placeholder = self.placeholder.clone().into();
            state.read_only = self.read_only;
            state.min_rows = self.min_rows.max(1);
            state.max_rows = self.max_rows.max(state.min_rows);
            if state.caret_color != self.theme.caret {
                state.caret_color = self.theme.caret;
                cx.notify();
            }
        });

        let mut editor = div().id(crate::events::ids::host_element_id(ctx.id)).flex().min_w_0().w_full();
        if !disabled {
            editor = editor.track_focus(&focus_handle);
        }
        let mut editor = editor.child(state);
        // Single-line inputs center text vertically when given extra height.
        if !self.multiline {
            editor = editor.items_center();
        }
        editor = crate::events::ids::host_hitbox(editor, ctx.pointer_events_none);
        editor = crate::events::app_region::mark(editor, ctx.app_region);
        if let Some(style) = ctx.style {
            editor = crate::renderer::apply_interactive_styles(editor, style, &ctx.colors, &ctx.lengths);
            // Clip text to rounded corners, matching HTML input behavior.
            if style.border_radius.is_some() {
                editor = editor.overflow_hidden();
            }
        }
        // The editor does not go through `custom_surface`, which is where other
        // custom elements pick these up.
        if let Some(features) = ctx.font_features.clone() {
            editor = editor.font_features(features);
        }
        if let Some(variations) = ctx.font_variations.clone() {
            editor = editor.font_variations(variations);
        }
        editor = crate::focus::apply_focus_styles(
            editor,
            ctx.id,
            ctx.style,
            &ctx.colors,
            &ctx.lengths,
            &ctx.focus_styles,
        );
        if ctx.resolved.position == crate::layout::containing_block::Position::Static {
            // A containing block for the editor's own parts, not an author's `position`: it must
            // not rise above its later siblings.
            editor = editor.relative().unlifted();
        }
        let default_role = default_role(input_type, self.multiline);
        // Before `apply_accessibility`, so an explicit `aria-valuenow`, `-valuemin`
        // or `-valuemax` from the app wins.
        if let Some(rules) = number_rules {
            if let Ok(value) = self.value.trim().parse::<f64>() {
                if value.is_finite() {
                    editor = editor.aria_numeric_value(value);
                }
            }
            if let Some(min) = rules.min {
                editor = editor.aria_min_numeric_value(min);
            }
            if let Some(max) = rules.max {
                editor = editor.aria_max_numeric_value(max);
            }
            editor = editor.aria_numeric_value_step(rules.step);
        }
        editor = crate::accessibility::apply_accessibility(
            editor,
            ctx.id,
            ctx.props,
            Some(default_role),
        );
        if input_type != InputType::Password
            && ctx.props.get("aria-valuetext").is_none()
            && !self.value.is_empty()
        {
            editor = editor.aria_value(self.value.clone());
        }
        if !self.placeholder.is_empty() {
            editor = editor.aria_placeholder(self.placeholder.clone());
        }
        // Custom elements paint themselves, so nothing registers their box for
        // automation unless the builder does it. Without this, a locator on an
        // editor fails with "Element has no painted bounds" and `click()` has
        // no target. `<div>` and `<text>` get this from `build_element`.
        //
        // `Some(false)` also claims the same box as a non-selectable
        // selection-start region: a drag inside an editor must move the caret,
        // not start a document selection.
        editor = crate::automation::track_host_bounds(editor, ctx.id, Some(false), false, false, None);
        // A disabled control gets no events, like the DOM.
        if !disabled {
            editor = super::wire_host_events(editor, &ctx);
        }
        editor.into_any_element()
    }

    fn set_prop(&mut self, key: &str, value: serde_json::Value) {
        match key {
            "value" => self.value = value.as_str().unwrap_or_default().to_string(),
            "placeholder" => self.placeholder = value.as_str().unwrap_or_default().to_string(),
            "readOnly" => self.read_only = value.as_bool().unwrap_or(false),
            "disabled" => self.disabled = value.as_bool().unwrap_or(false),
            "type" => self.input_type = InputType::parse(value.as_str().unwrap_or_default()),
            "maxLength" => {
                self.max_length = value.as_u64().and_then(|length| usize::try_from(length).ok())
            }
            "min" => self.min = number_from_json(&value),
            "max" => self.max = number_from_json(&value),
            "step" => self.step = step_from_json(Some(&value)),
            "minRows" => self.min_rows = value.as_u64().unwrap_or(1) as usize,
            "maxRows" => {
                self.max_rows = value
                    .as_u64()
                    .unwrap_or(if self.multiline { 10 } else { 1 })
                    as usize
            }
            "theme" => self.theme = Theme::from_prop(Some(&value)),
            _ => {}
        }
    }

    fn supported_props(&self) -> &'static [&'static str] {
        &[
            "value",
            "placeholder",
            "readOnly",
            "disabled",
            "type",
            "maxLength",
            "min",
            "max",
            "step",
            "minRows",
            "maxRows",
            "theme",
        ]
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &[
            "change", "keydown", "keyup", "focus", "blur",
        ]
    }

    fn destroy(&mut self) {
        self.state = None;
        self.inert_focus = None;
    }

    fn text_selection(&mut self, cx: &mut App) -> Option<TextSelection> {
        self.apply_value(cx);
        Some(match &self.state {
            Some(state) => state.read(cx).selection(),
            None => {
                let cursor = self.value.len();
                match self.pending_selection {
                    Some(request) => {
                        let selection = resolve(&self.value, request);
                        describe(
                            &self.value,
                            &selection.range,
                            selection.reversed,
                            selection.directionless.then_some(&selection.range),
                        )
                    }
                    None => describe(&self.value, &(cursor..cursor), false, None),
                }
            }
        })
    }

    fn text_value(&mut self, cx: &mut App) -> Option<String> {
        self.apply_value(cx);
        Some(match &self.state {
            Some(state) => state.read(cx).content.clone(),
            None => self.value.clone(),
        })
    }

    fn set_text_selection(&mut self, request: SelectionRequest, cx: &mut App) -> bool {
        self.apply_value(cx);
        match self.state.clone() {
            Some(state) => state.update(cx, |state, cx| state.select_request(request, cx)),
            None => self.pending_selection = Some(request),
        }
        true
    }
}

#[derive(Clone)]
struct EditSnapshot {
    content: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Insert,
    DeleteBackward,
    DeleteForward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CoalescingEdit {
    kind: EditKind,
    anchor: usize,
}

#[derive(Clone, Copy)]
struct LastEdit {
    edit: CoalescingEdit,
    when: Instant,
}

/// What an edit of the text is, named as `InputEvent.inputType` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputKind {
    InsertText,
    InsertFromPaste,
    InsertLineBreak,
    InsertCompositionText,
    DeleteCompositionText,
    DeleteContentBackward,
    DeleteContentForward,
    DeleteWordBackward,
    DeleteWordForward,
    DeleteSoftLineBackward,
    DeleteSoftLineForward,
    DeleteByCut,
    HistoryUndo,
    HistoryRedo,
    /// The number stepper: the DOM fires a plain `Event` there, which has no `inputType`.
    Stepped,
}

impl InputKind {
    fn input_type(self) -> &'static str {
        match self {
            InputKind::InsertText => "insertText",
            InputKind::InsertFromPaste => "insertFromPaste",
            InputKind::InsertLineBreak => "insertLineBreak",
            InputKind::InsertCompositionText => "insertCompositionText",
            InputKind::DeleteCompositionText => "deleteCompositionText",
            InputKind::DeleteContentBackward => "deleteContentBackward",
            InputKind::DeleteContentForward => "deleteContentForward",
            InputKind::DeleteWordBackward => "deleteWordBackward",
            InputKind::DeleteWordForward => "deleteWordForward",
            InputKind::DeleteSoftLineBackward => "deleteSoftLineBackward",
            InputKind::DeleteSoftLineForward => "deleteSoftLineForward",
            InputKind::DeleteByCut => "deleteByCut",
            InputKind::HistoryUndo => "historyUndo",
            InputKind::HistoryRedo => "historyRedo",
            InputKind::Stepped => "",
        }
    }

    /// `InputEvent.data` is the inserted text for these and null for the rest,
    /// a line break included.
    fn carries_text(self) -> bool {
        matches!(
            self,
            InputKind::InsertText | InputKind::InsertFromPaste | InputKind::InsertCompositionText
        )
    }

    /// The kind an edit reports once its outcome is known: an edit of marked
    /// text is the composition's own, and an insertion that inserted nothing
    /// deleted the text it replaced.
    fn for_outcome(self, in_composition: bool, inserted: &str) -> InputKind {
        if in_composition {
            return if inserted.is_empty() {
                InputKind::DeleteCompositionText
            } else {
                InputKind::InsertCompositionText
            };
        }
        if self.carries_text() && inserted.is_empty() {
            return InputKind::DeleteContentBackward;
        }
        self
    }
}

fn coalescing_edit(
    range: &Range<usize>,
    new_text: &str,
    selection_reversed: bool,
) -> Option<CoalescingEdit> {
    if new_text.is_empty() {
        if range.is_empty() {
            return None;
        }
        return Some(CoalescingEdit {
            kind: if selection_reversed {
                EditKind::DeleteBackward
            } else {
                EditKind::DeleteForward
            },
            anchor: range.start,
        });
    }

    let mut characters = new_text.chars();
    let character = characters.next()?;
    (range.is_empty() && characters.next().is_none() && !character.is_whitespace()).then_some(
        CoalescingEdit {
            kind: EditKind::Insert,
            anchor: range.start + new_text.len(),
        },
    )
}

fn edits_coalesce(
    previous: CoalescingEdit,
    current: Option<CoalescingEdit>,
    range: &Range<usize>,
    elapsed: Duration,
) -> bool {
    let Some(current) = current else {
        return false;
    };
    if previous.kind != current.kind || elapsed >= UNDO_COALESCE {
        return false;
    }
    match current.kind {
        EditKind::Insert | EditKind::DeleteForward => range.start == previous.anchor,
        EditKind::DeleteBackward => range.end == previous.anchor,
    }
}

fn push_undo_snapshot(history: &mut VecDeque<EditSnapshot>, snapshot: EditSnapshot) {
    if history.len() == UNDO_LIMIT {
        history.pop_front();
    }
    history.push_back(snapshot);
}

struct TextEditorState {
    element_id: u64,
    view: gpui::WeakEntity<GpuixView>,
    callback: Option<EventCallback>,
    /// For the propagation path of a composition event. Never locked from the
    /// code that builds the frame, which holds it: only input handlers emit.
    tree: Arc<Mutex<RetainedTree>>,
    emits_change: bool,
    focus_handle: FocusHandle,
    content: String,
    placeholder: SharedString,
    multiline: bool,
    read_only: bool,
    disabled: bool,
    input_type: InputType,
    max_length: Option<usize>,
    number_rules: Option<NumberRules>,
    muted_color: gpui::Hsla,
    min_rows: usize,
    max_rows: usize,
    selected_range: Range<usize>,
    selection_reversed: bool,
    /// The range a programmatic selection set without a direction, which the
    /// DOM reports as `"none"` until the selection changes.
    directionless_range: Option<Range<usize>>,
    marked_range: Option<Range<usize>>,
    is_selecting: bool,
    drag_position: Option<Point<Pixels>>,
    drag_generation: u64,
    drag_autoscroll_active: bool,
    scroll_top: f32,
    scroll_left: f32,
    follow_cursor: bool,
    last_lines: Vec<WrappedLine>,
    line_starts: Vec<usize>,
    last_bounds: Option<Bounds<Pixels>>,
    line_height: Pixels,
    font_size: Pixels,
    content_height: f32,
    content_width: f32,
    display_is_placeholder: bool,
    caret_color: gpui::Hsla,
    blink_anchor: Instant,
    blink_task: Option<Task<()>>,
    pending_values: VecDeque<String>,
    undo_stack: VecDeque<EditSnapshot>,
    redo_stack: Vec<EditSnapshot>,
    last_edit: Option<LastEdit>,
}

impl TextEditorState {
    fn reset_blink(&mut self, cx: &Context<Self>) {
        self.blink_anchor = cx.background_executor().now();
    }

    fn caret_shown(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        if !self.focus_handle.is_focused(window) || !window.is_window_active() {
            self.blink_task = None;
            return false;
        }
        if self.blink_task.is_none() {
            self.reset_blink(cx);
            self.blink_task = Some(cx.spawn(async move |this, cx| loop {
                cx.background_executor()
                    .timer(Duration::from_millis(CARET_BLINK_MS))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }));
        }
        caret_visible(self.blink_anchor.elapsed().as_millis() as u64)
    }

    fn masks_text(&self) -> bool {
        self.input_type == InputType::Password && !self.multiline
    }

    /// What is painted and measured: the content, bullets for a password, or
    /// the placeholder while empty.
    fn display_text(&self) -> (SharedString, bool) {
        if self.content.is_empty() {
            (self.placeholder.clone(), true)
        } else if self.masks_text() {
            (SharedString::from(masked(&self.content)), false)
        } else {
            (SharedString::from(self.content.clone()), false)
        }
    }

    fn content_to_display(&self, offset: usize) -> usize {
        if self.masks_text() {
            content_to_masked(&self.content, offset)
        } else {
            offset
        }
    }

    fn display_to_content(&self, offset: usize) -> usize {
        if self.masks_text() {
            masked_to_content(&self.content, offset)
        } else {
            offset
        }
    }

    fn selection(&self) -> TextSelection {
        describe(
            &self.content,
            &self.selected_range,
            self.selection_reversed,
            self.directionless_range.as_ref(),
        )
    }

    fn select_request(&mut self, request: SelectionRequest, cx: &mut Context<Self>) {
        let selection = resolve(&self.content, request);
        self.selected_range = selection.range.clone();
        self.selection_reversed = selection.reversed;
        self.directionless_range = selection.directionless.then_some(selection.range);
        self.follow_cursor = true;
        self.reset_blink(cx);
        cx.notify();
    }

    fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            content: self.content.clone(),
            selected_range: self.selected_range.clone(),
            selection_reversed: self.selection_reversed,
        }
    }

    fn sync_prop_value(&mut self, value: String, cx: &mut Context<Self>) {
        if let Some(index) = self
            .pending_values
            .iter()
            .rposition(|pending| pending == &value)
        {
            self.pending_values.drain(..=index);
            return;
        }
        self.pending_values.clear();
        self.set_external_text(value, cx);
    }

    fn set_external_text(&mut self, value: String, cx: &mut Context<Self>) {
        if self.content == value {
            return;
        }
        self.content = value;
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.scroll_top = 0.0;
        self.scroll_left = 0.0;
        self.follow_cursor = true;
        self.reset_blink(cx);
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_edit = None;
        cx.notify();
    }

    fn replace_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        kind: InputKind,
        cx: &mut Context<Self>,
    ) {
        if self.read_only || self.disabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.edit(range, new_text, true, kind, cx);
    }

    fn emit_composition(&self, event_type: &str, data: &str) {
        let tree = self.tree.lock().unwrap();
        emit_bubbling_event(&self.callback, &tree, self.element_id, event_type, |payload| {
            payload.data = Some(data.to_string());
        });
    }

    fn emit_change(&mut self, kind: InputKind, inserted: &str, is_composing: bool) {
        if self.emits_change {
            self.pending_values.push_back(self.content.clone());
            while self.pending_values.len() > 32 {
                self.pending_values.pop_front();
            }
            emit_event_full(&self.callback, self.element_id, "change", |payload| {
                payload.value = Some(self.content.clone());
                payload.input_type = Some(kind.input_type().to_string());
                payload.data = kind.carries_text().then(|| inserted.to_string());
                payload.is_composing = Some(is_composing);
            });
        }
    }

    fn restore(&mut self, snapshot: EditSnapshot, kind: InputKind, cx: &mut Context<Self>) {
        self.content = snapshot.content;
        self.selected_range = snapshot.selected_range;
        self.selection_reversed = snapshot.selection_reversed;
        self.marked_range = None;
        self.follow_cursor = true;
        self.last_edit = None;
        self.reset_blink(cx);
        self.emit_change(kind, "", false);
        cx.notify();
    }

    fn record_edit(&mut self, range: &Range<usize>, new_text: &str, now: Instant) {
        let current = coalescing_edit(range, new_text, self.selection_reversed);
        let mergeable = self.last_edit.is_some_and(|previous| {
            edits_coalesce(
                previous.edit,
                current,
                range,
                now.duration_since(previous.when),
            )
        });
        if !mergeable {
            let snapshot = self.snapshot();
            push_undo_snapshot(&mut self.undo_stack, snapshot);
        }
        self.redo_stack.clear();
        self.last_edit = current.map(|edit| LastEdit { edit, when: now });
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.content.len());
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.follow_cursor = true;
        self.reset_blink(cx);
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.content.len());
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.follow_cursor = true;
        self.reset_blink(cx);
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    fn previous_word_boundary(&self, offset: usize) -> usize {
        self.content
            .split_word_bound_indices()
            .rev()
            .find_map(|(index, word)| (index < offset && !word.trim().is_empty()).then_some(index))
            .unwrap_or(0)
    }

    fn next_word_boundary(&self, offset: usize) -> usize {
        self.content
            .split_word_bound_indices()
            .find_map(|(index, word)| {
                let end = index + word.len();
                (end > offset && !word.trim().is_empty()).then_some(end)
            })
            .unwrap_or(self.content.len())
    }

    fn line_range_at(&self, offset: usize) -> Range<usize> {
        let start = self.content[..offset]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let end = self.content[offset..]
            .find('\n')
            .map(|index| offset + index)
            .unwrap_or(self.content.len());
        start..end
    }

    fn visual_line_boundary(&self, end: bool) -> usize {
        let Some(cursor) = self.point_for_index(self.cursor_offset()) else {
            let line = self.line_range_at(self.cursor_offset());
            return if end { line.end } else { line.start };
        };
        self.index_for_point(point(
            if end { px(1_000_000.0) } else { px(0.0) },
            cursor.y + px(0.5),
        ))
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            let previous = self.previous_boundary(self.cursor_offset());
            if previous == self.cursor_offset() {
                return;
            }
            self.select_to(previous, cx);
        }
        self.replace_range(None, "", InputKind::DeleteContentBackward, cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if next == self.cursor_offset() {
                return;
            }
            self.select_to(next, cx);
        }
        self.replace_range(None, "", InputKind::DeleteContentForward, cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let offset = if self.selected_range.is_empty() {
            self.previous_boundary(self.cursor_offset())
        } else {
            self.selected_range.start
        };
        self.move_to(offset, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let offset = if self.selected_range.is_empty() {
            self.next_boundary(self.cursor_offset())
        } else {
            self.selected_range.end
        };
        self.move_to(offset, cx);
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.vertical_target(-1.0) {
            self.move_to(offset, cx);
        }
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.vertical_target(1.0) {
            self.move_to(offset, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.vertical_target(-1.0) {
            self.select_to(offset, cx);
        }
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.vertical_target(1.0) {
            self.select_to(offset, cx);
        }
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        // Non-directional, as a browser reports it.
        self.directionless_range = Some(self.selected_range.clone());
        self.reset_blink(cx);
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.visual_line_boundary(false), cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.visual_line_boundary(true), cx);
    }

    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.visual_line_boundary(false), cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.visual_line_boundary(true), cx);
    }

    fn select_doc_start(&mut self, _: &SelectDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_doc_end(&mut self, _: &SelectDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.previous_word_boundary(self.cursor_offset()), cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.next_word_boundary(self.cursor_offset()), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_word_boundary(self.cursor_offset()), cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_word_boundary(self.cursor_offset()), cx);
    }

    fn delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.previous_word_boundary(self.cursor_offset()), cx);
        }
        self.replace_range(None, "", InputKind::DeleteWordBackward, cx);
    }

    fn delete_word_right(
        &mut self,
        _: &DeleteWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.next_word_boundary(self.cursor_offset()), cx);
        }
        self.replace_range(None, "", InputKind::DeleteWordForward, cx);
    }

    fn delete_to_line_start(
        &mut self,
        _: &DeleteToLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            let start = self.line_range_at(self.cursor_offset()).start;
            if start == self.cursor_offset() {
                return;
            }
            self.select_to(start, cx);
        }
        self.replace_range(None, "", InputKind::DeleteSoftLineBackward, cx);
    }

    fn delete_to_line_end(
        &mut self,
        _: &DeleteToLineEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            let end = self.line_range_at(self.cursor_offset()).end;
            if end == self.cursor_offset() {
                return;
            }
            self.select_to(end, cx);
        }
        self.replace_range(None, "", InputKind::DeleteSoftLineForward, cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        // A password never reaches the clipboard.
        if self.masks_text() {
            return;
        }
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.masks_text() || self.selected_range.is_empty() {
            return;
        }
        self.copy(&Copy, window, cx);
        self.replace_range(None, "", InputKind::DeleteByCut, cx);
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            cx.propagate();
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(clipboard_text) else {
            cx.propagate();
            return;
        };
        self.replace_range(None, &text, InputKind::InsertFromPaste, cx);
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some(previous) = self.undo_stack.pop_back() {
            self.redo_stack.push(self.snapshot());
            self.restore(previous, InputKind::HistoryUndo, cx);
        }
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some(next) = self.redo_stack.pop() {
            let snapshot = self.snapshot();
            push_undo_snapshot(&mut self.undo_stack, snapshot);
            self.restore(next, InputKind::HistoryRedo, cx);
        }
    }

    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline && !self.read_only {
            self.replace_range(None, "\n", InputKind::InsertLineBreak, cx);
        }
    }

    fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        crate::forms::implicit_submission(self.view.clone(), self.element_id, window, cx);
    }

    /// Replace `range` and put the caret after the new text. `user_input`
    /// applies `maxLength`; a programmatic edit such as the number stepper does not.
    /// `kind` is what the `change` event reports unless the edit ends a composition.
    fn edit(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        user_input: bool,
        kind: InputKind,
        cx: &mut Context<Self>,
    ) {
        let replacement = self.single_line_if_needed(new_text);
        let replacement = if user_input {
            let Some(replacement) = self.fit_max_length(&range, replacement) else {
                return;
            };
            replacement
        } else {
            replacement
        };
        if range.is_empty() && replacement.is_empty() {
            return;
        }
        let ends_composition = self.marked_range.is_some();
        if !ends_composition {
            self.record_edit(&range, &replacement, cx.background_executor().now());
        }
        self.content =
            self.content[..range.start].to_owned() + &replacement + &self.content[range.end..];
        let cursor = range.start + replacement.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.follow_cursor = true;
        self.reset_blink(cx);
        // The input event is still part of the composition, so it comes first, as in Chrome.
        self.emit_change(
            kind.for_outcome(ends_composition, &replacement),
            &replacement,
            ends_composition,
        );
        if ends_composition {
            self.emit_composition("compositionend", &replacement);
        }
        cx.notify();
    }

    fn single_line_if_needed(&self, text: &str) -> String {
        if self.multiline {
            text.to_string()
        } else {
            single_line_text(text)
        }
    }

    /// Cut `replacement` so the content stays within `maxLength` UTF-16 units.
    /// `None` rejects the edit: there is no room for any of a non-empty
    /// `replacement`, so even the selection it would have replaced stays, as in
    /// browsers. Content a program set beyond the limit is kept; it only blocks
    /// insertion.
    fn fit_max_length(&self, range: &Range<usize>, replacement: String) -> Option<String> {
        let Some(max_length) = self.max_length else {
            return Some(replacement);
        };
        let kept = utf16_len(&self.content[..range.start]) + utf16_len(&self.content[range.end..]);
        let room = max_length.saturating_sub(kept);
        if room == 0 && !replacement.is_empty() {
            return None;
        }
        Some(truncate_to_utf16(&replacement, room).to_string())
    }

    fn step_up(&mut self, _: &StepUp, _: &mut Window, cx: &mut Context<Self>) {
        self.step_number(1.0, cx);
    }

    fn step_down(&mut self, _: &StepDown, _: &mut Window, cx: &mut Context<Self>) {
        self.step_number(-1.0, cx);
    }

    fn step_number(&mut self, direction: f64, cx: &mut Context<Self>) {
        if self.read_only || self.disabled {
            return;
        }
        let Some(rules) = self.number_rules else {
            return;
        };
        let Some(next) = step_number(&self.content, direction, rules) else {
            return;
        };
        // Not user typing, so `maxLength` does not apply.
        self.edit(0..self.content.len(), &next, false, InputKind::Stepped, cx);
    }

    fn vertical_target(&self, direction: f32) -> Option<usize> {
        let current = self.point_for_index(self.cursor_offset())?;
        let target_y = f32::from(current.y) + direction * f32::from(self.line_height);
        if target_y < 0.0 {
            return Some(0);
        }
        if target_y >= self.content_height {
            return Some(self.content.len());
        }
        Some(self.index_for_point(point(current.x, px(target_y))))
    }

    fn point_for_index(&self, index: usize) -> Option<Point<Pixels>> {
        let index = self.content_to_display(index);
        for (line_index, line) in self.last_lines.iter().enumerate() {
            let line_start = *self.line_starts.get(line_index)?;
            if index < line_start || index > line_start + line.len() {
                continue;
            }
            let local = line.position_for_index(index - line_start, self.line_height)?;
            let y_offset: Pixels = self
                .last_lines
                .iter()
                .take(line_index)
                .map(|line| line.size(self.line_height).height)
                .sum();
            return Some(point(local.x, local.y + y_offset));
        }
        None
    }

    fn index_for_point(&self, position: Point<Pixels>) -> usize {
        if self.display_is_placeholder {
            return 0;
        }
        let mut y = f32::from(position.y).max(0.0);
        for (line_index, line) in self.last_lines.iter().enumerate() {
            let height = f32::from(line.size(self.line_height).height);
            let line_start = self.line_starts.get(line_index).copied().unwrap_or(0);
            if y < height || line_index + 1 == self.last_lines.len() {
                let local = point(position.x, px(y.min(height - 1.0).max(0.0)));
                let index = line
                    .closest_index_for_position(local, self.line_height)
                    .unwrap_or_else(|index| index);
                return self
                    .display_to_content(line_start + index)
                    .min(self.content.len());
            }
            y -= height;
        }
        self.content.len()
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else {
            return 0;
        };
        self.index_for_point(point(
            position.x - bounds.left() + px(self.scroll_left),
            position.y - bounds.top() + px(self.scroll_top),
        ))
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A prevented mousedown moves neither focus nor the caret, as in the DOM.
        if self.disabled || window.default_prevented() {
            return;
        }
        if !self.read_only {
            window.request_text_input();
        }
        window.focus(&self.focus_handle, cx);
        let intent = press_intent(event.click_count, event.modifiers.shift);
        self.is_selecting = intent.arms_drag();
        self.drag_position = intent.arms_drag().then_some(event.position);
        self.drag_generation = self.drag_generation.wrapping_add(1);
        self.drag_autoscroll_active = false;
        match intent {
            PressIntent::SelectAll => {
                self.move_to(0, cx);
                self.select_to(self.content.len(), cx);
                self.directionless_range = Some(self.selected_range.clone());
            }
            // A browser selects the whole password: word breaks would leak its shape.
            PressIntent::SelectWord if self.masks_text() => {
                self.move_to(0, cx);
                self.select_to(self.content.len(), cx);
            }
            PressIntent::SelectWord => {
                let index = self.index_for_mouse_position(event.position);
                let range = crate::text::selection::word_range(&self.content, index);
                self.move_to(range.start, cx);
                self.select_to(range.end, cx);
            }
            PressIntent::ExtendSelection => {
                self.select_to(self.index_for_mouse_position(event.position), cx);
            }
            PressIntent::PlaceCaret => {
                self.move_to(self.index_for_mouse_position(event.position), cx);
            }
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
        self.drag_position = None;
        self.drag_generation = self.drag_generation.wrapping_add(1);
        self.drag_autoscroll_active = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.drag_position = Some(event.position);
            let position = self.drag_selection_position(event.position);
            self.select_to(self.index_for_mouse_position(position), cx);
            if self.multiline
                && self.drag_scroll_delta(event.position) != 0.0
                && !self.drag_autoscroll_active
            {
                self.start_drag_autoscroll(cx);
            }
        }
    }

    fn start_drag_autoscroll(&mut self, cx: &mut Context<Self>) {
        self.drag_autoscroll_active = true;
        let generation = self.drag_generation;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(DRAG_SCROLL_FRAME_MS))
                .await;
            let keep_running = this
                .update(cx, |input, cx| input.drag_autoscroll_tick(generation, cx))
                .unwrap_or(false);
            if !keep_running {
                break;
            }
        })
        .detach();
    }

    fn drag_selection_position(&self, position: Point<Pixels>) -> Point<Pixels> {
        let Some(bounds) = self.last_bounds else {
            return position;
        };
        let x = if self.multiline {
            position.x.clamp(bounds.left(), bounds.right() - px(0.5))
        } else {
            position.x
        };
        point(x, position.y.clamp(bounds.top(), bounds.bottom() - px(0.5)))
    }

    fn drag_scroll_delta(&self, position: Point<Pixels>) -> f32 {
        let Some(bounds) = self.last_bounds else {
            return 0.0;
        };
        drag_scroll_delta(
            f32::from(position.y),
            f32::from(bounds.top()),
            f32::from(bounds.bottom()),
            f32::from(self.line_height),
        )
    }

    fn drag_autoscroll_tick(&mut self, generation: u64, cx: &mut Context<Self>) -> bool {
        if !self.multiline || !self.is_selecting || self.drag_generation != generation {
            return false;
        }
        let (Some(position), Some(bounds)) = (self.drag_position, self.last_bounds) else {
            self.drag_autoscroll_active = false;
            return false;
        };
        let delta = self.drag_scroll_delta(position);
        if delta == 0.0 {
            self.drag_autoscroll_active = false;
            return false;
        }
        let max_scroll = (self.content_height - f32::from(bounds.size.height)).max(0.0);
        let next = (self.scroll_top + delta).clamp(0.0, max_scroll);
        if next == self.scroll_top {
            self.drag_autoscroll_active = false;
            return false;
        }
        self.scroll_top = next;
        let edge_position = self.drag_selection_position(position);
        self.select_to(self.index_for_mouse_position(edge_position), cx);
        self.follow_cursor = false;
        true
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.default_prevented() {
            return;
        }
        let Some(bounds) = self.last_bounds else {
            return;
        };
        let viewport_height = f32::from(bounds.size.height);
        let max_scroll = (self.content_height - viewport_height).max(0.0);
        if max_scroll == 0.0 {
            return;
        }
        let delta = f32::from(event.delta.pixel_delta(self.line_height).y);
        let next = (self.scroll_top - delta).clamp(0.0, max_scroll);
        if next == self.scroll_top {
            if delta != 0.0 {
                cx.stop_propagation();
            }
            return;
        }
        self.scroll_top = next;
        self.follow_cursor = false;
        cx.stop_propagation();
        cx.notify();
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for character in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += character.len_utf16();
            utf8_offset += character.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for character in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += character.len_utf8();
            utf16_offset += character.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn layout_text(&mut self, width: Pixels, style: &TextStyle, window: &mut Window) -> f32 {
        let (display, is_placeholder) = self.display_text();
        let rem_size = window.rem_size();
        let font_size = style.font_size.to_pixels(rem_size);
        self.font_size = font_size;
        // Taffy measures after parent `with_text_style` is gone, so
        // `window.line_height()` is always 16×φ. Use the style captured during
        // request_layout instead.
        self.line_height = window
            .text_system()
            .line_height(style, rem_size, window.scale_factor());
        let color = if is_placeholder {
            gpui::rgba(0x8f8f8fff).into()
        } else if self.disabled {
            self.muted_color
        } else {
            style.color
        };
        let run = |len: usize, underline: bool| TextRun {
            len,
            font: style.font(),
            color,
            font_smoothing: style.font_smoothing,
            background_color: None,
            underline: underline.then_some(UnderlineStyle {
                color: Some(color),
                thickness: px(1.0),
                ..Default::default()
            }),
            strikethrough: None,
            font_size: None,
            line_height: None,
            overline: None,
            extra_decorations: Vec::new(),
            group: 0,
        };
        let marked = self.marked_range.as_ref().map(|marked| {
            self.content_to_display(marked.start)..self.content_to_display(marked.end)
        });
        let runs = match marked {
            Some(marked) if !is_placeholder => vec![
                run(marked.start, false),
                run(marked.len(), true),
                run(display.len() - marked.end, false),
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            _ => vec![run(display.len(), false)],
        };
        let wrap_width = self.multiline.then_some(width);
        let lines = window
            .text_system()
            .shape_text(display, font_size, &runs, wrap_width, None)
            .map(|lines| lines.into_vec())
            .unwrap_or_default();
        let mut line_starts = Vec::with_capacity(lines.len());
        let mut offset = 0;
        for line in &lines {
            line_starts.push(offset);
            offset += line.len() + 1;
        }
        if line_starts.is_empty() {
            line_starts.push(0);
        }
        self.content_height = lines
            .iter()
            .map(|line| f32::from(line.size(self.line_height).height))
            .sum::<f32>()
            .max(f32::from(self.line_height));
        self.content_width = lines
            .iter()
            .map(|line| f32::from(line.unwrapped_layout.width))
            .fold(0.0, f32::max);
        self.display_is_placeholder = is_placeholder;
        self.last_lines = lines;
        self.line_starts = line_starts;
        self.content_height
    }

    fn clamp_scroll(&mut self, viewport_width: f32, viewport_height: f32) {
        if self.follow_cursor {
            if let Some(cursor) = self.point_for_index(self.cursor_offset()) {
                let cursor_top = f32::from(cursor.y);
                if cursor_top < self.scroll_top {
                    self.scroll_top = cursor_top;
                } else if cursor_top + f32::from(self.line_height)
                    > self.scroll_top + viewport_height
                {
                    self.scroll_top = cursor_top + f32::from(self.line_height) - viewport_height;
                }
                if !self.multiline {
                    let cursor_left = f32::from(cursor.x);
                    if cursor_left < self.scroll_left {
                        self.scroll_left = cursor_left;
                    } else if cursor_left + 2.0 > self.scroll_left + viewport_width {
                        self.scroll_left = cursor_left + 2.0 - viewport_width;
                    }
                }
            }
        }
        self.scroll_top = self
            .scroll_top
            .clamp(0.0, (self.content_height - viewport_height).max(0.0));
        self.scroll_left = if self.multiline {
            0.0
        } else {
            self.scroll_left
                .clamp(0.0, (self.content_width + 2.0 - viewport_width).max(0.0))
        };
    }
}

impl EntityInputHandler for TextEditorState {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        self.content.get(range).map(str::to_string)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(marked) = self.marked_range.take() {
            let text = self.content[marked].to_string();
            self.emit_composition("compositionend", &text);
        }
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_range(range_utf16, new_text, InputKind::InsertText, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only || self.disabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let replacement = self.single_line_if_needed(new_text);
        let Some(replacement) = self.fit_max_length(&range, replacement) else {
            return;
        };
        let was_composing = self.marked_range.is_some();
        // What compositionstart reports: the text the composition takes over.
        let replaced = self.content[range.clone()].to_string();
        if !was_composing {
            let snapshot = self.snapshot();
            push_undo_snapshot(&mut self.undo_stack, snapshot);
            self.redo_stack.clear();
            self.last_edit = None;
        }
        self.content =
            self.content[..range.start].to_owned() + &replacement + &self.content[range.end..];
        self.marked_range =
            (!replacement.is_empty()).then_some(range.start..range.start + replacement.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|selected| {
                range.start + utf16_offset_to_utf8(&replacement, selected.start)
                    ..range.start + utf16_offset_to_utf8(&replacement, selected.end)
            })
            .unwrap_or_else(|| range.start + replacement.len()..range.start + replacement.len());
        self.follow_cursor = true;
        self.reset_blink(cx);
        let is_composing = self.marked_range.is_some();
        match (was_composing, is_composing) {
            (false, true) => {
                self.emit_composition("compositionstart", &replaced);
                self.emit_composition("compositionupdate", &replacement);
            }
            (true, true) => self.emit_composition("compositionupdate", &replacement),
            _ => {}
        }
        self.emit_change(
            InputKind::InsertCompositionText.for_outcome(was_composing, &replacement),
            &replacement,
            was_composing || is_composing,
        );
        // Marking nothing cancels the composition; its input event is still part of it.
        if was_composing && !is_composing {
            self.emit_composition("compositionend", "");
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let start = self.point_for_index(range.start)?;
        Some(caret_rect(
            point(
                bounds.left() + start.x - px(self.scroll_left),
                bounds.top() + start.y - px(self.scroll_top),
            ),
            self.line_height,
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.offset_to_utf16(self.index_for_mouse_position(position)))
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_range = self.range_from_utf16(&range_utf16);
        self.selection_reversed = false;
        self.follow_cursor = true;
        self.reset_blink(cx);
        cx.notify();
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.content.encode_utf16().count())
    }

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        !self.read_only && !self.disabled
    }
}

impl gpui::Render for TextEditorState {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(if self.number_rules.is_some() {
                NUMBER_INPUT_KEY_CONTEXT
            } else if !self.multiline {
                INPUT_KEY_CONTEXT
            } else {
                TEXTAREA_KEY_CONTEXT
            })
            .when(!self.disabled, |editor| editor.track_focus(&self.focus_handle))
            .cursor(if self.disabled {
                CursorStyle::Arrow
            } else {
                CursorStyle::IBeam
            })
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::select_doc_start))
            .on_action(cx.listener(Self::select_doc_end))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::delete_to_line_start))
            .on_action(cx.listener(Self::delete_to_line_end))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::step_up))
            .on_action(cx.listener(Self::step_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .w_full()
            .min_w_0()
            .child(EditorTextElement {
                input: cx.entity(),
                min_rows: self.min_rows,
                max_rows: self.max_rows,
            })
    }
}

struct EditorTextElement {
    input: Entity<TextEditorState>,
    min_rows: usize,
    max_rows: usize,
}

struct EditorPrepaint {
    caret: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
}

impl gpui::Element for EditorTextElement {
    type RequestLayoutState = ();
    type PrepaintState = EditorPrepaint;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        let input = self.input.clone();
        let text_style = window.text_style();
        let min_rows = self.min_rows;
        let max_rows = self.max_rows.max(min_rows);
        let layout = window.request_measured_layout(style, move |known, available, _, window, cx| {
            let width = known.width.unwrap_or(match available.width {
                gpui::AvailableSpace::Definite(width) => width,
                _ => px(320.0),
            });
            let (content_height, line_height) = input.update(cx, |input, _| {
                let content_height = input.layout_text(width, &text_style, window);
                (content_height, f32::from(input.line_height))
            });
            let height =
                content_height.clamp(min_rows as f32 * line_height, max_rows as f32 * line_height);
            size(width, px(height))
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        cx: &mut App,
    ) -> EditorPrepaint {
        self.input.update(cx, |input, _| {
            input.last_bounds = Some(bounds);
            input.clamp_scroll(f32::from(bounds.size.width), f32::from(bounds.size.height));
        });
        let input = self.input.read(cx);
        let origin = point(
            bounds.left() - px(input.scroll_left),
            bounds.top() - px(input.scroll_top),
        );
        let mut selection = Vec::new();
        let mut caret = None;
        if input.disabled {
            // No caret and no selection wash on a control that cannot be edited.
        } else if input.selected_range.is_empty() || input.display_is_placeholder {
            let caret_point = input
                .point_for_index(input.cursor_offset())
                .unwrap_or(point(px(0.0), px(0.0)));
            caret = Some(fill(
                caret_rect(
                    point(origin.x + caret_point.x, origin.y + caret_point.y),
                    input.line_height,
                ),
                input.caret_color,
            ));
        } else if let (Some(start), Some(end)) = (
            input.point_for_index(input.selected_range.start),
            input.point_for_index(input.selected_range.end),
        ) {
            let color = gpui::rgba(0x7c86ff59);
            if start.y == end.y {
                selection.push(fill(
                    Bounds::from_corners(
                        point(origin.x + start.x, origin.y + start.y),
                        point(origin.x + end.x, origin.y + start.y + input.line_height),
                    ),
                    color,
                ));
            } else {
                selection.push(fill(
                    Bounds::from_corners(
                        point(origin.x + start.x, origin.y + start.y),
                        point(bounds.right(), origin.y + start.y + input.line_height),
                    ),
                    color,
                ));
                if end.y > start.y + input.line_height {
                    selection.push(fill(
                        Bounds::from_corners(
                            point(origin.x, origin.y + start.y + input.line_height),
                            point(bounds.right(), origin.y + end.y),
                        ),
                        color,
                    ));
                }
                selection.push(fill(
                    Bounds::from_corners(
                        point(origin.x, origin.y + end.y),
                        point(origin.x + end.x, origin.y + end.y + input.line_height),
                    ),
                    color,
                ));
            }
        }
        EditorPrepaint { caret, selection }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut EditorPrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        if !self.input.read(cx).disabled {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.input.clone()),
                cx,
            );
        }
        let input = self.input.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && event.pressed_button == Some(MouseButton::Left) {
                input.update(cx, |input, cx| input.on_mouse_move(event, cx));
            }
        });
        window.with_content_mask(Some(gpui::ContentMask::from_bounds(bounds)), |window| {
            for quad in prepaint.selection.drain(..) {
                window.paint_quad(quad);
            }
            let (lines, line_height, scroll_top, scroll_left, display) =
                self.input.update(cx, |input, _| {
                    let display = input.display_text().0;
                    (
                        std::mem::take(&mut input.last_lines),
                        input.line_height,
                        input.scroll_top,
                        input.scroll_left,
                        display,
                    )
                });
            if window.visibility() == gpui::Visibility::Visible {
                crate::text::log_painted_text(window.window_handle().window_id(), display);
            }
            let mut y = bounds.top() - px(scroll_top);
            for line in &lines {
                let height = line.size(line_height).height;
                line.paint(
                    point(bounds.left() - px(scroll_left), y),
                    line_height,
                    gpui::TextAlign::Left,
                    Some(bounds),
                    window,
                    cx,
                )
                .ok();
                y += height;
            }
            self.input.update(cx, |input, _| input.last_lines = lines);
            let caret_shown = self
                .input
                .update(cx, |input, cx| input.caret_shown(window, cx));
            if caret_shown {
                if let Some(caret) = prepaint.caret.take() {
                    window.paint_quad(caret);
                }
            }
        });
    }
}

impl gpui::IntoElement for EditorTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_binding(bindings: &[KeyBinding], keystroke: &str, action: &dyn gpui::Action) -> bool {
        let keystroke = gpui::Keystroke::parse(keystroke).unwrap();
        bindings.iter().any(|binding| {
            binding.match_keystrokes(std::slice::from_ref(&keystroke)) == Some(false)
                && binding.action().partial_eq(action)
        })
    }

    #[test]
    fn macos_word_navigation_uses_alt() {
        let bindings = text_editor_bindings(INPUT_KEY_CONTEXT, false, true, true);

        assert!(has_binding(&bindings, "alt-left", &WordLeft));
        assert!(has_binding(&bindings, "alt-right", &WordRight));
        assert!(!has_binding(&bindings, "ctrl-left", &WordLeft));
        assert!(!has_binding(&bindings, "ctrl-right", &WordRight));
    }

    #[test]
    fn non_macos_word_navigation_uses_control() {
        let bindings = text_editor_bindings(INPUT_KEY_CONTEXT, false, false, true);

        assert!(has_binding(&bindings, "ctrl-left", &WordLeft));
        assert!(has_binding(&bindings, "ctrl-right", &WordRight));
        assert!(!has_binding(&bindings, "alt-left", &WordLeft));
        assert!(!has_binding(&bindings, "alt-right", &WordRight));
    }

    #[test]
    fn browser_paste_stays_with_the_dom_event() {
        let bindings = text_editor_bindings(INPUT_KEY_CONTEXT, false, true, false);

        assert!(!has_binding(&bindings, "cmd-v", &Paste));
        assert!(!has_binding(&bindings, "ctrl-v", &Paste));
    }

    #[test]
    fn desktop_paste_uses_the_platform_clipboard_action() {
        let bindings = text_editor_bindings(INPUT_KEY_CONTEXT, false, true, true);

        assert!(has_binding(&bindings, "cmd-v", &Paste));
        assert!(has_binding(&bindings, "ctrl-v", &Paste));
    }

    #[test]
    fn textarea_enter_inserts_a_newline_and_input_enter_submits() {
        let textarea = text_editor_bindings(TEXTAREA_KEY_CONTEXT, true, true, true);
        assert!(has_binding(&textarea, "enter", &Newline));
        assert!(has_binding(&textarea, "shift-enter", &Newline));
        assert!(!has_binding(&textarea, "enter", &Submit));

        let input = text_editor_bindings(INPUT_KEY_CONTEXT, false, true, true);
        assert!(has_binding(&input, "enter", &Submit));
        assert!(!has_binding(&input, "enter", &Newline));
    }

    #[test]
    fn browser_platform_detection_recognizes_macos() {
        assert!(browser_platform_is_macos("MacIntel", ""));
        assert!(browser_platform_is_macos(
            "Unknown",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"
        ));
        assert!(!browser_platform_is_macos(
            "Win32",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64)"
        ));
        assert!(!browser_platform_is_macos(
            "Linux x86_64",
            "Mozilla/5.0 (X11; Linux x86_64)"
        ));
    }

    #[test]
    fn ime_offsets_are_relative_to_replacement_text() {
        assert_eq!(utf16_offset_to_utf8("é🙂", 0), 0);
        assert_eq!(utf16_offset_to_utf8("é🙂", 1), "é".len());
        assert_eq!(utf16_offset_to_utf8("é🙂", 3), "é🙂".len());
    }

    #[test]
    fn single_line_newlines_become_one_space() {
        assert_eq!(single_line_text("a\r\nb\nc\rd"), "a b c d");
    }

    #[test]
    fn caret_blink_phase() {
        assert!(caret_visible(0));
        assert!(caret_visible(CARET_BLINK_MS - 1));
        assert!(!caret_visible(CARET_BLINK_MS));
        assert!(!caret_visible(2 * CARET_BLINK_MS - 1));
        assert!(caret_visible(2 * CARET_BLINK_MS));
    }

    #[test]
    fn caret_matches_the_line_height() {
        let bounds = caret_rect(point(px(10.0), px(4.0)), px(20.0));
        assert_eq!(bounds.origin, point(px(10.0), px(4.0)));
        assert_eq!(bounds.size, size(px(2.0), px(20.0)));
    }

    #[test]
    fn external_paths_are_not_text_editor_paste() {
        let item = ClipboardItem {
            entries: vec![
                ClipboardEntry::ExternalPaths(gpui::ExternalPaths(
                    [std::path::PathBuf::from("/tmp/image.png")]
                        .into_iter()
                        .collect(),
                )),
                ClipboardEntry::String(gpui::ClipboardString::new("/tmp/image.png".to_string())),
            ],
        };
        assert_eq!(clipboard_text(item), None);
    }

    #[test]
    fn text_still_pastes_when_the_clipboard_also_has_an_image() {
        let item = ClipboardItem {
            entries: vec![
                ClipboardEntry::String(gpui::ClipboardString::new("caption".to_string())),
                ClipboardEntry::Image(gpui::Image::empty()),
            ],
        };
        assert_eq!(clipboard_text(item), Some("caption".to_string()));
    }

    #[test]
    fn caret_color_comes_from_the_input_theme() {
        let mut input = TextEditorElement::new(false);
        input.set_prop("theme", serde_json::json!({ "caret": "#22c55e" }));
        assert_eq!(input.theme.caret, gpui::rgba(0x22c55eff).into());
    }

    #[test]
    fn input_kinds_are_the_input_types_of_chrome() {
        let types = [
            (InputKind::InsertText, "insertText"),
            (InputKind::InsertFromPaste, "insertFromPaste"),
            (InputKind::InsertLineBreak, "insertLineBreak"),
            (InputKind::InsertCompositionText, "insertCompositionText"),
            (InputKind::DeleteCompositionText, "deleteCompositionText"),
            (InputKind::DeleteContentBackward, "deleteContentBackward"),
            (InputKind::DeleteContentForward, "deleteContentForward"),
            (InputKind::DeleteWordBackward, "deleteWordBackward"),
            (InputKind::DeleteWordForward, "deleteWordForward"),
            (InputKind::DeleteSoftLineBackward, "deleteSoftLineBackward"),
            (InputKind::DeleteSoftLineForward, "deleteSoftLineForward"),
            (InputKind::DeleteByCut, "deleteByCut"),
            (InputKind::HistoryUndo, "historyUndo"),
            (InputKind::HistoryRedo, "historyRedo"),
            (InputKind::Stepped, ""),
        ];
        for (kind, input_type) in types {
            assert_eq!(kind.input_type(), input_type);
        }
    }

    #[test]
    fn only_insertions_carry_text_and_a_line_break_does_not() {
        for kind in [
            InputKind::InsertText,
            InputKind::InsertFromPaste,
            InputKind::InsertCompositionText,
        ] {
            assert!(kind.carries_text(), "{kind:?}");
        }
        for kind in [
            InputKind::InsertLineBreak,
            InputKind::DeleteCompositionText,
            InputKind::DeleteContentBackward,
            InputKind::DeleteByCut,
            InputKind::HistoryUndo,
            InputKind::HistoryRedo,
            InputKind::Stepped,
        ] {
            assert!(!kind.carries_text(), "{kind:?}");
        }
    }

    #[test]
    fn an_edit_of_marked_text_is_the_compositions_and_an_empty_insertion_is_a_deletion() {
        assert_eq!(
            InputKind::InsertText.for_outcome(true, "か"),
            InputKind::InsertCompositionText
        );
        assert_eq!(
            InputKind::InsertText.for_outcome(true, ""),
            InputKind::DeleteCompositionText
        );
        assert_eq!(
            InputKind::InsertText.for_outcome(false, ""),
            InputKind::DeleteContentBackward
        );
        assert_eq!(
            InputKind::InsertFromPaste.for_outcome(false, ""),
            InputKind::DeleteContentBackward
        );
        assert_eq!(
            InputKind::InsertFromPaste.for_outcome(false, "x"),
            InputKind::InsertFromPaste
        );
        assert_eq!(
            InputKind::DeleteWordBackward.for_outcome(false, ""),
            InputKind::DeleteWordBackward
        );
        assert_eq!(InputKind::Stepped.for_outcome(false, "5"), InputKind::Stepped);
    }

    #[test]
    fn insertion_undo_coalescing_requires_one_contiguous_non_whitespace_character() {
        let insert_at_one = CoalescingEdit {
            kind: EditKind::Insert,
            anchor: 1,
        };

        assert_eq!(coalescing_edit(&(0..0), "a", false), Some(insert_at_one));
        assert!(edits_coalesce(
            insert_at_one,
            coalescing_edit(&(1..1), "b", false),
            &(1..1),
            Duration::from_millis(699),
        ));
        assert!(!edits_coalesce(
            insert_at_one,
            coalescing_edit(&(2..2), "b", false),
            &(2..2),
            Duration::from_millis(699),
        ));
        assert_eq!(coalescing_edit(&(0..1), "a", false), None);
        assert_eq!(coalescing_edit(&(1..1), "ab", false), None);
        assert_eq!(coalescing_edit(&(1..1), " ", false), None);
        assert_eq!(coalescing_edit(&(1..1), "\n", false), None);
        assert_eq!(coalescing_edit(&(1..1), "\t", false), None);
        assert_eq!(coalescing_edit(&(1..1), "\u{2003}", false), None);
        assert!(!edits_coalesce(
            insert_at_one,
            coalescing_edit(&(1..1), "b", false),
            &(1..1),
            UNDO_COALESCE,
        ));
        assert!(!edits_coalesce(
            CoalescingEdit {
                kind: EditKind::DeleteBackward,
                anchor: 1,
            },
            coalescing_edit(&(1..1), "b", false),
            &(1..1),
            Duration::from_millis(1),
        ));
        assert!(!edits_coalesce(
            insert_at_one,
            None,
            &(1..1),
            Duration::from_millis(1),
        ));
    }

    #[test]
    fn backward_and_forward_deletions_use_their_own_contiguity_rules() {
        let backward = CoalescingEdit {
            kind: EditKind::DeleteBackward,
            anchor: 3,
        };
        assert_eq!(
            coalescing_edit(&(2..3), "", true),
            Some(CoalescingEdit {
                kind: EditKind::DeleteBackward,
                anchor: 2,
            })
        );
        assert!(edits_coalesce(
            backward,
            coalescing_edit(&(2..3), "", true),
            &(2..3),
            Duration::from_millis(699),
        ));
        assert!(!edits_coalesce(
            backward,
            coalescing_edit(&(1..2), "", true),
            &(1..2),
            Duration::from_millis(699),
        ));

        let forward = CoalescingEdit {
            kind: EditKind::DeleteForward,
            anchor: 2,
        };
        assert_eq!(coalescing_edit(&(2..3), "", false), Some(forward));
        assert!(edits_coalesce(
            forward,
            coalescing_edit(&(2..3), "", false),
            &(2..3),
            Duration::from_millis(699),
        ));
        assert!(!edits_coalesce(
            forward,
            coalescing_edit(&(3..4), "", false),
            &(3..4),
            Duration::from_millis(699),
        ));
        assert!(!edits_coalesce(
            forward,
            coalescing_edit(&(2..3), "", false),
            &(2..3),
            UNDO_COALESCE,
        ));
        assert_eq!(coalescing_edit(&(2..2), "", false), None);
    }

    #[test]
    fn undo_history_discards_only_the_oldest_snapshot_at_the_limit() {
        let mut history = VecDeque::new();
        for index in 0..=UNDO_LIMIT {
            push_undo_snapshot(
                &mut history,
                EditSnapshot {
                    content: index.to_string(),
                    selected_range: index..index,
                    selection_reversed: false,
                },
            );
        }

        assert_eq!(history.len(), UNDO_LIMIT);
        assert_eq!(history.front().unwrap().content, "1");
        assert_eq!(history.back().unwrap().content, UNDO_LIMIT.to_string());
    }

    #[test]
    fn drag_autoscroll_is_edge_proportional_and_capped_to_one_line() {
        let line_height = 20.0;
        assert_eq!(drag_scroll_delta(200.0, 100.0, 300.0, line_height), 0.0);
        assert_eq!(drag_scroll_delta(90.0, 100.0, 300.0, line_height), -2.0);
        assert_eq!(drag_scroll_delta(315.0, 100.0, 300.0, line_height), 3.0);
        assert_eq!(drag_scroll_delta(-100.0, 100.0, 300.0, line_height), -20.0);
        assert_eq!(drag_scroll_delta(500.0, 100.0, 300.0, line_height), 20.0);
    }

    #[test]
    fn multi_click_selects_word_then_all_and_does_not_arm_drag() {
        assert_eq!(press_intent(1, false), PressIntent::PlaceCaret);
        assert_eq!(press_intent(1, true), PressIntent::ExtendSelection);
        assert_eq!(press_intent(2, false), PressIntent::SelectWord);
        assert_eq!(press_intent(2, true), PressIntent::SelectWord);
        assert_eq!(press_intent(3, false), PressIntent::SelectAll);
        assert!(press_intent(1, false).arms_drag());
        assert!(press_intent(1, true).arms_drag());
        assert!(!press_intent(2, false).arms_drag());
        assert!(!press_intent(3, false).arms_drag());
    }
}
