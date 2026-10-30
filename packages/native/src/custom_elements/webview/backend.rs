//! The seam between the `<webview>` element and the web engine behind it: the engine is a
//! `WKWebView` in a window and a scripted page in the test renderer.
use std::{cell::RefCell, rc::Rc};

use crate::element_tree::EventPayload;
use crate::renderer::EventCallback;

/// What the page of a `<webview>` tells its element. The names are Electron's.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum WebviewEvent {
    DidStartLoading,
    DidStopLoading,
    DidNavigate {
        url: String,
    },
    PageTitleUpdated {
        title: String,
        /// The page set its title, and the browser did not make one from the URL.
        explicit_set: bool,
    },
    DidFailLoad {
        /// An `NSURLError` code, not a Chromium net error: `-1009` is no connection where
        /// Electron says `-106`, and `-999` a load that was cancelled where it says `-3`. An
        /// error of WebKit's own (a restricted port, `103`) keeps the code of its domain.
        error_code: i32,
        error_description: String,
        validated_url: Option<String>,
    },
}

impl WebviewEvent {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::DidStartLoading => "did-start-loading",
            Self::DidStopLoading => "did-stop-loading",
            Self::DidNavigate { .. } => "did-navigate",
            Self::PageTitleUpdated { .. } => "page-title-updated",
            Self::DidFailLoad { .. } => "did-fail-load",
        }
    }

    fn fill(self, payload: &mut EventPayload) {
        match self {
            Self::DidStartLoading | Self::DidStopLoading => {}
            Self::DidNavigate { url } => payload.url = Some(url),
            Self::PageTitleUpdated { title, explicit_set } => {
                payload.title = Some(title);
                payload.explicit_set = Some(explicit_set);
            }
            Self::DidFailLoad { error_code, error_description, validated_url } => {
                payload.error_code = Some(error_code);
                payload.error_description = Some(error_description);
                payload.validated_url = validated_url;
            }
        }
    }
}

/// Sends the events of one `<webview>` to JS. The element hands it the event callback each
/// frame it is built, and the engine calls it from wherever the page reports: the main thread,
/// outside any frame.
pub(crate) struct EventEmitter {
    id: u64,
    callback: RefCell<Option<EventCallback>>,
}

impl EventEmitter {
    pub(crate) fn new(id: u64) -> Rc<Self> {
        Rc::new(Self { id, callback: RefCell::new(None) })
    }

    pub(crate) fn set_callback(&self, callback: Option<EventCallback>) {
        *self.callback.borrow_mut() = callback;
    }

    pub(crate) fn emit(&self, event: WebviewEvent) {
        let callback = self.callback.borrow().clone();
        crate::renderer::emit_event_full(&callback, self.id, event.name(), |payload| event.fill(payload));
    }
}

/// What the page of a `<webview>` is at, read when asked.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct WebviewState {
    /// Empty before the first navigation commits, as the URL of a page that has none.
    pub(crate) url: String,
    /// Empty when the page has no title.
    pub(crate) title: String,
    pub(crate) can_go_back: bool,
    pub(crate) can_go_forward: bool,
}

/// A script's outcome: `None` is `undefined`, which JSON cannot hold. `Err` is the message of
/// the error the script threw, or of the engine failing to run it.
pub(crate) type ScriptOutcome = Result<Option<serde_json::Value>, String>;

/// Takes the outcome of a script once it has run. Called on the main thread, at some later turn
/// of the run loop.
pub(crate) type ScriptDone = Box<dyn FnOnce(ScriptOutcome)>;

/// A web engine in the content view of a native surface. Dropping it removes the page.
pub(crate) trait WebviewBackend {
    /// Starts loading `url`, a full URL. Fails when the engine cannot take it.
    fn load(&mut self, url: &str) -> Result<(), String>;
    fn reload(&mut self);
    fn stop(&mut self);
    fn go_back(&mut self);
    fn go_forward(&mut self);
    fn state(&self) -> WebviewState;
    /// Runs `code` in the page and calls `done` with the result of its last expression.
    fn execute_javascript(&mut self, code: &str, done: ScriptDone);
    /// Moves keyboard input to the page or back. The element is focused or blurred in the window
    /// first; this is the follow-up that moves the first responder to or from the page.
    fn set_first_responder(&mut self, focus: PageFocus) -> Result<(), String>;
}

/// Where the keyboard goes when the focus of a `<webview>` element changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PageFocus {
    /// To the window's own view.
    Blurred,
    /// To the page, as a script or a click moves it there: the page keeps what it had focused.
    Focused,
    /// To the page by Tab or Shift+Tab, which focus its first or last tabbable element, as the
    /// focus of an iframe does.
    FocusedByTab { backwards: bool },
}

impl PageFocus {
    pub(crate) fn from_step(focused: bool, tab: Option<bool>) -> Self {
        match (focused, tab) {
            (false, _) => Self::Blurred,
            (true, None) => Self::Focused,
            (true, Some(backwards)) => Self::FocusedByTab { backwards },
        }
    }
}

/// Asks the window to move focus past the page: `true` for Shift+Tab. The page calls it when
/// Tab leaves its last tabbable element or Shift+Tab its first, and may call it from any turn
/// of the run loop.
pub(crate) type FocusExit = Rc<dyn Fn(bool)>;

/// The engine for a webview whose surface has the content view `content_view`
/// (`NativeSurface::platform_handle`).
pub(crate) fn create_backend(
    id: u64,
    content_view: usize,
    emitter: Rc<EventEmitter>,
    focus_exit: FocusExit,
    cx: &gpui::App,
) -> Result<Box<dyn WebviewBackend>, String> {
    if cx.has_global::<super::test_backend::TestWebviews>() {
        let _ = content_view;
        return Ok(Box::new(super::test_backend::TestBackend::new(
            id,
            cx.global::<super::test_backend::TestWebviews>(),
            emitter,
            focus_exit,
        )));
    }
    #[cfg(target_os = "macos")]
    {
        let _ = id;
        return Ok(Box::new(crate::platform::webview_macos::WkBackend::new(content_view, emitter, focus_exit)?));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (id, content_view, emitter, focus_exit);
        Err("webview is not supported on this platform".to_string())
    }
}
