//! The engine of a `<webview>` on macOS: a `WKWebView` in the content view of a native surface.
//!
//! WebKit calls the navigation delegate and the title observer on the main thread, outside any
//! frame of the window, and they report through the element's `EventEmitter`. Every call here
//! is made on the main thread, which is where the window's frames are drawn.
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::{c_void, CStr},
    ptr,
    rc::Rc,
};

use block2::RcBlock;
use objc2::{
    class, msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
    sel, Encode, Encoding,
};

use super::webview_url::{classify, LoadTarget};
use crate::custom_elements::webview::backend::{
    EventEmitter, ScriptDone, ScriptOutcome, WebviewBackend, WebviewEvent, WebviewState,
};

#[link(name = "WebKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFGetTypeID(object: *const c_void) -> usize;
    fn CFBooleanGetTypeID() -> usize;
}

/// `NSViewWidthSizable | NSViewHeightSizable`: the page follows the size of its content view.
const FILL_PARENT: usize = 2 | 16;
/// `NSKeyValueObservingOptionNew`
const OBSERVE_NEW: usize = 1;
/// `NSUTF8StringEncoding`
const UTF8: usize = 4;
const FAILING_URL_STRING_KEY: &str = "NSErrorFailingURLStringKey";
const FAILING_URL_KEY: &str = "NSErrorFailingURLKey";
const SCRIPT_EXCEPTION_KEY: &str = "WKJavaScriptExceptionMessage";

#[derive(Clone, Copy)]
#[repr(C)]
struct CgPoint {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct CgSize {
    width: f64,
    height: f64,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct CgRect {
    origin: CgPoint,
    size: CgSize,
}

unsafe impl Encode for CgPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]);
}

unsafe impl Encode for CgSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[Encoding::Double, Encoding::Double]);
}

unsafe impl Encode for CgRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[CgPoint::ENCODING, CgSize::ENCODING]);
}

fn ns_string(text: &str) -> Retained<AnyObject> {
    unsafe {
        let allocated: Allocated<AnyObject> = msg_send![class!(NSString), alloc];
        let string: Option<Retained<AnyObject>> = msg_send![
            allocated,
            initWithBytes: text.as_ptr().cast::<c_void>(),
            length: text.len(),
            encoding: UTF8
        ];
        string.expect("NSString takes any UTF-8")
    }
}

/// The text of an `NSString`, `None` for nil.
unsafe fn string_of(string: *mut AnyObject) -> Option<String> {
    if string.is_null() {
        return None;
    }
    let bytes: *const std::ffi::c_char = unsafe { msg_send![string, UTF8String] };
    Some(unsafe { CStr::from_ptr(bytes) }.to_string_lossy().into_owned())
}

unsafe fn url_string(web_view: *mut AnyObject) -> Option<String> {
    let url: *mut AnyObject = unsafe { msg_send![web_view, URL] };
    if url.is_null() {
        return None;
    }
    unsafe { string_of(msg_send![url, absoluteString]) }
}

// The emitter of each delegate, by the address of the delegate: WebKit calls the delegate with
// nothing else of ours.
thread_local! {
    static EMITTERS: RefCell<HashMap<usize, Rc<EventEmitter>>> = RefCell::new(HashMap::new());
}

fn emit(delegate: &AnyObject, event: impl FnOnce() -> WebviewEvent) {
    let key = delegate as *const AnyObject as usize;
    if let Some(emitter) = EMITTERS.with(|emitters| emitters.borrow().get(&key).cloned()) {
        emitter.emit(event());
    }
}

fn delegate_class() -> &'static AnyClass {
    const NAME: &CStr = c"GpuixWebviewDelegate";
    if let Some(class) = AnyClass::get(NAME) {
        return class;
    }
    extern "C" fn did_start(delegate: &AnyObject, _: Sel, _: *mut AnyObject, _: *mut AnyObject) {
        emit(delegate, || WebviewEvent::DidStartLoading);
    }
    extern "C" fn did_commit(delegate: &AnyObject, _: Sel, web_view: *mut AnyObject, _: *mut AnyObject) {
        // A commit without a URL (WebKit gives none to a page it made from nothing) is no
        // navigation to tell about.
        if let Some(url) = unsafe { url_string(web_view) } {
            emit(delegate, || WebviewEvent::DidNavigate { url });
        }
    }
    extern "C" fn did_finish(delegate: &AnyObject, _: Sel, _: *mut AnyObject, _: *mut AnyObject) {
        emit(delegate, || WebviewEvent::DidStopLoading);
    }
    extern "C" fn did_fail(delegate: &AnyObject, _: Sel, _: *mut AnyObject, _: *mut AnyObject, error: *mut AnyObject) {
        let (error_code, error_description, validated_url) = unsafe { describe_error(error) };
        emit(delegate, || WebviewEvent::DidFailLoad { error_code, error_description, validated_url });
        emit(delegate, || WebviewEvent::DidStopLoading);
    }
    extern "C" fn title_changed(
        delegate: &AnyObject,
        _: Sel,
        _: *mut AnyObject,
        web_view: *mut AnyObject,
        _: *mut AnyObject,
        _: *mut c_void,
    ) {
        let title = unsafe { title_of(web_view) };
        emit(delegate, || WebviewEvent::PageTitleUpdated { explicit_set: !title.is_empty(), title });
    }
    let mut builder = ClassBuilder::new(NAME, class!(NSObject)).expect("the delegate class name is free");
    unsafe {
        builder.add_method(sel!(webView:didStartProvisionalNavigation:), did_start as extern "C" fn(_, _, _, _));
        builder.add_method(sel!(webView:didCommitNavigation:), did_commit as extern "C" fn(_, _, _, _));
        builder.add_method(sel!(webView:didFinishNavigation:), did_finish as extern "C" fn(_, _, _, _));
        builder.add_method(sel!(webView:didFailProvisionalNavigation:withError:), did_fail as extern "C" fn(_, _, _, _, _));
        builder.add_method(sel!(webView:didFailNavigation:withError:), did_fail as extern "C" fn(_, _, _, _, _));
        builder.add_method(sel!(observeValueForKeyPath:ofObject:change:context:), title_changed as extern "C" fn(_, _, _, _, _, _));
    }
    builder.register()
}

unsafe fn title_of(web_view: *mut AnyObject) -> String {
    // A page with no title has a nil one: the DOM's `document.title` is "" then.
    unsafe { string_of(msg_send![web_view, title]) }.unwrap_or_default()
}

/// The `NSURLError` code, the description and the failing URL of an `NSError`.
unsafe fn describe_error(error: *mut AnyObject) -> (i32, String, Option<String>) {
    unsafe {
        let code: isize = msg_send![error, code];
        let description = string_of(msg_send![error, localizedDescription]).unwrap_or_default();
        let info: *mut AnyObject = msg_send![error, userInfo];
        let key = ns_string(FAILING_URL_STRING_KEY);
        let url: *mut AnyObject = msg_send![info, objectForKey: &*key];
        let failing = string_of(url).or_else(|| {
            // WebKit's own errors (a restricted port) carry the URL only as an NSURL.
            let key = ns_string(FAILING_URL_KEY);
            let url: *mut AnyObject = msg_send![info, objectForKey: &*key];
            if url.is_null() {
                None
            } else {
                string_of(msg_send![url, absoluteString])
            }
        });
        (code as i32, description, failing)
    }
}

pub(crate) struct WkBackend {
    web_view: Retained<AnyObject>,
    delegate: Retained<AnyObject>,
    /// Kept so the delegate's entry in `EMITTERS` has an owner for as long as the page lives.
    _emitter: Rc<EventEmitter>,
}

impl WkBackend {
    /// A page that fills `content_view`, the content `NSView` of a native surface.
    pub(crate) fn new(content_view: usize, emitter: Rc<EventEmitter>) -> Result<Self, String> {
        let content = content_view as *mut AnyObject;
        if content.is_null() {
            return Err("the surface has no content view".to_string());
        }
        unsafe {
            let configuration: Retained<AnyObject> = msg_send![class!(WKWebViewConfiguration), new];
            let frame: CgRect = msg_send![content, bounds];
            let allocated: Allocated<AnyObject> = msg_send![class!(WKWebView), alloc];
            let web_view: Retained<AnyObject> =
                msg_send![allocated, initWithFrame: frame, configuration: &*configuration];
            let _: () = msg_send![&*web_view, setAutoresizingMask: FILL_PARENT];
            let _: () = msg_send![content, addSubview: &*web_view];

            let delegate: Retained<AnyObject> = msg_send![delegate_class(), new];
            EMITTERS.with(|emitters| {
                emitters.borrow_mut().insert(&*delegate as *const AnyObject as usize, emitter.clone())
            });
            let _: () = msg_send![&*web_view, setNavigationDelegate: &*delegate];
            let title = ns_string("title");
            let _: () = msg_send![
                &*web_view,
                addObserver: &*delegate,
                forKeyPath: &*title,
                options: OBSERVE_NEW,
                context: ptr::null_mut::<c_void>()
            ];
            Ok(Self { web_view, delegate, _emitter: emitter })
        }
    }

    fn web_view(&self) -> *mut AnyObject {
        &*self.web_view as *const AnyObject as *mut AnyObject
    }
}

impl Drop for WkBackend {
    fn drop(&mut self) {
        unsafe {
            let title = ns_string("title");
            let _: () = msg_send![&*self.web_view, removeObserver: &*self.delegate, forKeyPath: &*title];
            let nil: *mut AnyObject = ptr::null_mut();
            let _: () = msg_send![&*self.web_view, setNavigationDelegate: nil];
            let _: () = msg_send![&*self.web_view, stopLoading];
            let _: () = msg_send![&*self.web_view, removeFromSuperview];
        }
        EMITTERS.with(|emitters| emitters.borrow_mut().remove(&(&*self.delegate as *const AnyObject as usize)));
    }
}

impl WebviewBackend for WkBackend {
    fn load(&mut self, url: &str) -> Result<(), String> {
        unsafe {
            match classify(url)? {
                LoadTarget::File { url, read_access } => {
                    let page = ns_url(&url)?;
                    let directory: Retained<AnyObject> = msg_send![
                        class!(NSURL),
                        fileURLWithPath: &*ns_string(&read_access.to_string_lossy()),
                        isDirectory: true
                    ];
                    let _: *mut AnyObject =
                        msg_send![&*self.web_view, loadFileURL: &*page, allowingReadAccessToURL: &*directory];
                }
                LoadTarget::Request { url } => {
                    let request: Retained<AnyObject> = msg_send![class!(NSURLRequest), requestWithURL: &*ns_url(&url)?];
                    let _: *mut AnyObject = msg_send![&*self.web_view, loadRequest: &*request];
                }
            }
        }
        Ok(())
    }

    fn reload(&mut self) {
        let _: *mut AnyObject = unsafe { msg_send![&*self.web_view, reload] };
    }

    fn stop(&mut self) {
        let _: () = unsafe { msg_send![&*self.web_view, stopLoading] };
    }

    fn go_back(&mut self) {
        let _: *mut AnyObject = unsafe { msg_send![&*self.web_view, goBack] };
    }

    fn go_forward(&mut self) {
        let _: *mut AnyObject = unsafe { msg_send![&*self.web_view, goForward] };
    }

    fn state(&self) -> WebviewState {
        unsafe {
            WebviewState {
                // No URL before the first load: the page has none.
                url: url_string(self.web_view()).unwrap_or_default(),
                title: title_of(self.web_view()),
                can_go_back: msg_send![&*self.web_view, canGoBack],
                can_go_forward: msg_send![&*self.web_view, canGoForward],
            }
        }
    }

    fn execute_javascript(&mut self, code: &str, done: ScriptDone) {
        let done = RefCell::new(Some(done));
        let handler = RcBlock::new(move |result: *mut AnyObject, error: *mut AnyObject| {
            if let Some(done) = done.borrow_mut().take() {
                done(unsafe { script_outcome(result, error) });
            }
        });
        let code = ns_string(code);
        let _: () = unsafe {
            msg_send![&*self.web_view, evaluateJavaScript: &*code, completionHandler: &*handler]
        };
    }

    /// The hook for the focus handoff: the element has focus in the window, and the page should
    /// have the keyboard. Not done yet.
    fn set_first_responder(&mut self, _focused: bool) {}
}

fn ns_url(url: &str) -> Result<Retained<AnyObject>, String> {
    let url_string = ns_string(url);
    let url: Option<Retained<AnyObject>> = unsafe { msg_send![class!(NSURL), URLWithString: &*url_string] };
    url.ok_or_else(|| "Invalid URL".to_string())
}

unsafe fn script_outcome(result: *mut AnyObject, error: *mut AnyObject) -> ScriptOutcome {
    if !error.is_null() {
        return Err(unsafe { script_error(error) });
    }
    if result.is_null() {
        return Ok(None);
    }
    unsafe { json_of(result) }.map(Some)
}

/// What a script that threw said: WebKit's message for it, else the error's own description.
unsafe fn script_error(error: *mut AnyObject) -> String {
    unsafe {
        let info: *mut AnyObject = msg_send![error, userInfo];
        let key = ns_string(SCRIPT_EXCEPTION_KEY);
        let message: *mut AnyObject = msg_send![info, objectForKey: &*key];
        string_of(message)
            .or_else(|| string_of(msg_send![error, localizedDescription]))
            .unwrap_or_else(|| "The script failed".to_string())
    }
}

unsafe fn is_kind(object: *mut AnyObject, class: &AnyClass) -> bool {
    unsafe { msg_send![object, isKindOfClass: class] }
}

/// The value a script returned, as JSON. WebKit hands back strings, numbers, booleans, null,
/// arrays, dictionaries and dates; a date is its ISO string, as `JSON.stringify` writes it.
pub(crate) unsafe fn json_of(object: *mut AnyObject) -> Result<serde_json::Value, String> {
    use serde_json::Value;
    unsafe {
        if object.is_null() {
            return Ok(Value::Null);
        }
        if is_kind(object, class!(NSNull)) {
            return Ok(Value::Null);
        }
        if is_kind(object, class!(NSString)) {
            return Ok(Value::String(string_of(object).expect("a string")));
        }
        if is_kind(object, class!(NSNumber)) {
            if CFGetTypeID(object as *const c_void) == CFBooleanGetTypeID() {
                let value: bool = msg_send![object, boolValue];
                return Ok(Value::Bool(value));
            }
            let kind: *const std::ffi::c_char = msg_send![object, objCType];
            return Ok(match *kind as u8 {
                b'f' | b'd' => {
                    let value: f64 = msg_send![object, doubleValue];
                    // JavaScript numbers all arrive as doubles; a whole one is an integer in JSON.
                    // (`JSON.stringify` writes NaN and the infinities as null.)
                    if value.fract() == 0. && value.abs() < 9007199254740992. {
                        Value::Number((value as i64).into())
                    } else {
                        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
                    }
                }
                b'Q' | b'L' | b'I' | b'S' => {
                    let value: u64 = msg_send![object, unsignedLongLongValue];
                    Value::Number(value.into())
                }
                _ => {
                    let value: i64 = msg_send![object, longLongValue];
                    Value::Number(value.into())
                }
            });
        }
        if is_kind(object, class!(NSDate)) {
            let seconds: f64 = msg_send![object, timeIntervalSince1970];
            return iso_date(seconds).map(Value::String);
        }
        if is_kind(object, class!(NSArray)) {
            let count: usize = msg_send![object, count];
            let mut items = Vec::with_capacity(count);
            for index in 0..count {
                let item: *mut AnyObject = msg_send![object, objectAtIndex: index];
                items.push(json_of(item)?);
            }
            return Ok(Value::Array(items));
        }
        if is_kind(object, class!(NSDictionary)) {
            let keys: *mut AnyObject = msg_send![object, allKeys];
            let count: usize = msg_send![keys, count];
            let mut entries = serde_json::Map::new();
            for index in 0..count {
                let key: *mut AnyObject = msg_send![keys, objectAtIndex: index];
                let value: *mut AnyObject = msg_send![object, objectForKey: key];
                let name = string_of(key).ok_or_else(|| "An object key of the result is not a string".to_string())?;
                entries.insert(name, json_of(value)?);
            }
            return Ok(Value::Object(entries));
        }
        let class: *const AnyClass = msg_send![object, class];
        Err(format!(
            "The result of the script cannot be turned into JSON: a {}",
            (*class).name().to_string_lossy()
        ))
    }
}

fn iso_date(seconds: f64) -> Result<String, String> {
    let nanos = (seconds * 1e3).round() as i128 * 1_000_000;
    let date = time::OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .map_err(|error| format!("The date of the result is out of range: {error}"))?;
    let format = time::format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z")
        .expect("a fixed format");
    date.format(&format).map_err(|error| error.to_string())
}

#[cfg(feature = "test-support")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: *const c_void;
    fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source_handled: u8) -> i32;
}

/// Runs the main run loop, where WebKit reports, until `done` or a timeout.
fn pump(done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "WebKit did not answer in 30 s");
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, 0) };
    }
}

fn run(backend: &mut WkBackend, code: &str) -> ScriptOutcome {
    let outcome = Rc::new(RefCell::new(None));
    let slot = outcome.clone();
    backend.execute_javascript(code, Box::new(move |result| *slot.borrow_mut() = Some(result)));
    pump(|| outcome.borrow().is_some());
    let result = outcome.borrow_mut().take().expect("settled");
    result
}

/// A real `WKWebView` in a real view, run by `cargo run --example webview_smoke`: WebKit wants the
/// main thread, which a test of the test harness is not on. Panics when the page does not behave.
pub(crate) fn smoke() {
    use std::sync::{Arc, Mutex};

    assert!(objc2::MainThreadMarker::new().is_some(), "WebKit needs the main thread");
    unsafe {
        let _: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let frame = CgRect { origin: CgPoint { x: 0., y: 0. }, size: CgSize { width: 300., height: 150. } };
        let allocated: Allocated<AnyObject> = msg_send![class!(NSView), alloc];
        let content: Retained<AnyObject> = msg_send![allocated, initWithFrame: frame];

        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = heard.clone();
        let emitter = EventEmitter::new(1);
        emitter.set_callback(Some(Arc::new(move |payload| sink.lock().unwrap().push(payload))));
        let mut backend = WkBackend::new(&*content as *const AnyObject as usize, emitter).unwrap();
        let names = || heard.lock().unwrap().iter().map(|event| event.event_type.clone()).collect::<Vec<_>>();

        backend.load("data:text/html,%3Ctitle%3EHi%3C%2Ftitle%3E%3Cp%3Ex%3C%2Fp%3E").unwrap();
        pump(|| names().contains(&"did-stop-loading".to_string()));
        let names = names();
        assert_eq!(names.first().map(String::as_str), Some("did-start-loading"), "{names:?}");
        assert!(names.contains(&"did-navigate".to_string()), "{names:?}");
        assert!(names.contains(&"page-title-updated".to_string()), "{names:?}");
        let state = backend.state();
        assert_eq!(state.title, "Hi");
        assert!(state.url.starts_with("data:text/html"));
        assert!(!state.can_go_back && !state.can_go_forward);

        assert_eq!(run(&mut backend, "1 + 1"), Ok(Some(serde_json::json!(2))));
        assert_eq!(run(&mut backend, "document.title"), Ok(Some(serde_json::json!("Hi"))));
        assert_eq!(run(&mut backend, "({a: [1, 'b', null, true]})"), Ok(Some(serde_json::json!({"a": [1, "b", null, true]}))));
        assert_eq!(run(&mut backend, "void 0"), Ok(None));
        assert_eq!(run(&mut backend, "null"), Ok(Some(serde_json::Value::Null)));
        assert_eq!(run(&mut backend, "true"), Ok(Some(serde_json::json!(true))));
        let error = run(&mut backend, "nope()").unwrap_err();
        assert!(error.contains("nope"), "{error}");
        assert!(run(&mut backend, "(function () {})").is_err());

        heard.lock().unwrap().clear();
        backend.load("file:///nonexistent-dir/none.html").unwrap();
        pump(|| heard.lock().unwrap().iter().any(|event| event.event_type == "did-fail-load"));
        let failed = heard.lock().unwrap().iter().find(|event| event.event_type == "did-fail-load").cloned().unwrap();
        assert!(failed.error_code.is_some_and(|code| code < 0), "{failed:?}");
        assert_eq!(failed.validated_url.as_deref(), Some("file:///nonexistent-dir/none.html"));
        drop(backend);
    }
}

/// For a test: the views the window made for a webview, read back from AppKit.
#[cfg(feature = "test-support")]
pub(crate) mod inspect {
    use super::*;

    /// Where AppKit has a native container, in window points from the top left, and how it looks.
    #[derive(Debug)]
    pub(crate) struct AppliedView {
        pub(crate) hidden: bool,
        pub(crate) x: f64,
        pub(crate) y: f64,
        pub(crate) width: f64,
        pub(crate) height: f64,
        /// The content view, relative to the container.
        pub(crate) content_x: f64,
        pub(crate) content_y: f64,
        pub(crate) content_width: f64,
        pub(crate) content_height: f64,
        pub(crate) opacity: f64,
        pub(crate) masked: bool,
    }

    pub(crate) fn applied(content_view: usize) -> AppliedView {
        unsafe {
            let content = content_view as *mut AnyObject;
            let container: *mut AnyObject = msg_send![content, superview];
            let root: *mut AnyObject = msg_send![container, superview];
            let root_bounds: CgRect = msg_send![root, bounds];
            let frame: CgRect = msg_send![container, frame];
            let content_frame: CgRect = msg_send![content, frame];
            let hidden: bool = msg_send![container, isHidden];
            let layer: *mut AnyObject = msg_send![container, layer];
            let opacity: f32 = msg_send![layer, opacity];
            let mask: *mut AnyObject = msg_send![layer, mask];
            AppliedView {
                hidden,
                x: frame.origin.x,
                y: root_bounds.size.height - (frame.origin.y + frame.size.height),
                width: frame.size.width,
                height: frame.size.height,
                content_x: content_frame.origin.x,
                // AppKit's content frame is from the bottom of the container.
                content_y: frame.size.height - (content_frame.origin.y + content_frame.size.height),
                content_width: content_frame.size.width,
                content_height: content_frame.size.height,
                opacity: f64::from(opacity),
                masked: !mask.is_null(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_json(text: &str) -> Result<serde_json::Value, String> {
        unsafe {
            let data: *mut AnyObject = msg_send![
                class!(NSData),
                dataWithBytes: text.as_ptr().cast::<c_void>(),
                length: text.len()
            ];
            let object: *mut AnyObject = msg_send![
                class!(NSJSONSerialization),
                JSONObjectWithData: data,
                options: 4usize,
                error: ptr::null_mut::<*mut AnyObject>()
            ];
            assert!(!object.is_null(), "{text} is JSON");
            json_of(object)
        }
    }

    #[test]
    fn json_comes_back_as_it_went_in() {
        for text in [
            r#"{"a":1,"b":[true,false,null,"x"],"c":{"d":-2.5}}"#,
            r#"[1,2,3]"#,
            r#""text""#,
            r#"12"#,
            r#"true"#,
            r#"null"#,
            r#"1.5"#,
        ] {
            let expected: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(from_json(text).unwrap(), expected, "{text}");
        }
    }

    #[test]
    fn a_date_is_its_iso_string() {
        assert_eq!(iso_date(0.0).unwrap(), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_date(1_700_000_000.123).unwrap(), "2023-11-14T22:13:20.123Z");
    }
}
