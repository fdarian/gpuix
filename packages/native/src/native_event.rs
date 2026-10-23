//! NSEvents for a renderer's window, posted from NSEventThread the way WindowServer input
//! reaches AppKit, so a test can check that input wakes JavaScript. Nothing here touches the OS
//! input stream: the events are made in-process and handed to `NSApplication`'s own queue.

use std::ffi::c_void;

use napi::{Error, Result};
use objc::rc::autoreleasepool;
use objc::runtime::{Object, NO};
use objc::{class, msg_send, sel, sel_impl, Encode, Encoding};

type Id = *mut Object;

const KEY_DOWN: u64 = 10;
const LEFT_MOUSE_DOWN: u64 = 1;
const LEFT_MOUSE_UP: u64 = 2;
const MOUSE_MOVED: u64 = 5;
const CG_MOUSE_MOVED: u32 = 5;
const CG_SCROLL_WHEEL: u32 = 22;
const CG_SCROLL_WHEEL_DELTA_AXIS_1: u32 = 11;
const CG_SCROLL_WHEEL_POINT_DELTA_AXIS_1: u32 = 96;
/// Negative is down, like the DOM's positive `deltaY`.
const SCROLL_LINES_DOWN: i64 = -3;
/// The CGEvent field that `+[NSEvent eventWithCGEvent:]` reads the window number from. Found by
/// setting every field in turn: no public name has it (91 and 92 do not).
const CG_EVENT_WINDOW_NUMBER: u32 = 51;

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct Point {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Size {
    width: f64,
    height: f64,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Rect {
    origin: Point,
    size: Size,
}

// SAFETY: the layouts and encodings of CGPoint and CGRect.
unsafe impl Encode for Point {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{CGPoint=dd}") }
    }
}

unsafe impl Encode for Rect {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateMouseEvent(
        source: *const c_void,
        kind: u32,
        at: Point,
        button: u32,
    ) -> *mut c_void;
    fn CGEventSetType(event: *mut c_void, kind: u32);
    fn CGEventSetIntegerValueField(event: *mut c_void, field: u32, value: i64);
    /// Private: the one place AppKit reads an event's location in its window from.
    fn CGEventSetWindowLocation(event: *mut c_void, location: Point);
    fn CFRelease(object: *const c_void);
}

pub(crate) enum Kind {
    KeyDown,
    MouseMove(Point),
    Click(Point),
    Scroll(Point),
}

impl Kind {
    pub(crate) fn parse(kind: &str, x: Option<f64>, y: Option<f64>) -> Result<Self> {
        let point = || match (x, y) {
            (Some(x), Some(y)) => Ok(Point { x, y }),
            _ => Err(Error::from_reason(format!(
                "postNativeEvent \"{kind}\" needs x and y"
            ))),
        };
        match kind {
            "keyDown" => Ok(Self::KeyDown),
            "mouseMove" => point().map(Self::MouseMove),
            "click" => point().map(Self::Click),
            "scroll" => point().map(Self::Scroll),
            other => Err(Error::from_reason(format!(
                "postNativeEvent kind must be keyDown, mouseMove, click or scroll, got \"{other}\""
            ))),
        }
    }
}

/// Posts the events of `kind` for the window that holds `ns_view`. `x` and `y` are in the
/// window's logical pixels, from its top left, like the `simulate*` methods.
///
/// # Safety
///
/// `ns_view` must be the live content view of an AppKit window, and this must run on the main
/// thread.
pub(crate) unsafe fn post(ns_view: Id, kind: Kind) {
    autoreleasepool(|| unsafe {
        let window: Id = msg_send![ns_view, window];
        let window_number: i64 = msg_send![window, windowNumber];
        let content: Id = msg_send![window, contentView];
        let frame: Rect = msg_send![content, frame];
        // AppKit's window coordinates start at the bottom left; gpui flips them by the content height.
        let in_window = |at: Point| Point {
            x: at.x,
            y: frame.size.height - at.y,
        };
        let process_info: Id = msg_send![class!(NSProcessInfo), processInfo];
        let timestamp: f64 = msg_send![process_info, systemUptime];

        match kind {
            Kind::KeyDown => post_event(key_down(window_number, timestamp)),
            Kind::MouseMove(at) => post_event(mouse(
                MOUSE_MOVED,
                in_window(at),
                window_number,
                timestamp,
                0,
            )),
            Kind::Click(at) => {
                let at = in_window(at);
                post_event(mouse(LEFT_MOUSE_DOWN, at, window_number, timestamp, 1));
                post_event(mouse(LEFT_MOUSE_UP, at, window_number, timestamp, 1));
            }
            Kind::Scroll(at) => post_event(scroll(window, window_number, in_window(at))),
        }
    })
}

unsafe fn post_event(event: Id) {
    unsafe { gpui_macos::post_event_on_event_thread(event.cast()) }
}

unsafe fn key_down(window_number: i64, timestamp: f64) -> Id {
    unsafe {
        let characters: Id = msg_send![class!(NSString), stringWithUTF8String: c"a".as_ptr()];
        msg_send![class!(NSEvent),
            keyEventWithType: KEY_DOWN
            location: Point { x: 0.0, y: 0.0 }
            modifierFlags: 0u64
            timestamp: timestamp
            windowNumber: window_number
            context: std::ptr::null_mut::<Object>()
            characters: characters
            charactersIgnoringModifiers: characters
            isARepeat: NO
            keyCode: 0u16]
    }
}

unsafe fn mouse(
    event_type: u64,
    location: Point,
    window_number: i64,
    timestamp: f64,
    click_count: i64,
) -> Id {
    unsafe {
        msg_send![class!(NSEvent),
            mouseEventWithType: event_type
            location: location
            modifierFlags: 0u64
            timestamp: timestamp
            windowNumber: window_number
            context: std::ptr::null_mut::<Object>()
            eventNumber: 0i64
            clickCount: click_count
            pressure: if event_type == LEFT_MOUSE_DOWN { 1.0f32 } else { 0.0f32 }]
    }
}

/// A scroll event has no NSEvent factory: it is a CGEvent wrapped by AppKit, which reads the
/// window and the location in it back from the CGEvent. Neither has a public setter for an event
/// that is not a mouse event's, so the event is made as a mouse event and turned into a scroll.
unsafe fn scroll(window: Id, window_number: i64, in_window: Point) -> Id {
    unsafe {
        let frame: Rect = msg_send![window, frame];
        let cg_event = CGEventCreateMouseEvent(
            std::ptr::null(),
            CG_MOUSE_MOVED,
            Point { x: 0.0, y: 0.0 },
            0,
        );
        assert!(!cg_event.is_null(), "could not create the scroll CGEvent");
        CGEventSetType(cg_event, CG_SCROLL_WHEEL);
        CGEventSetIntegerValueField(cg_event, CG_SCROLL_WHEEL_DELTA_AXIS_1, SCROLL_LINES_DOWN);
        CGEventSetIntegerValueField(
            cg_event,
            CG_SCROLL_WHEEL_POINT_DELTA_AXIS_1,
            SCROLL_LINES_DOWN,
        );
        CGEventSetIntegerValueField(cg_event, CG_EVENT_WINDOW_NUMBER, window_number);
        // From the top left of the window's frame, where `in_window` is from the bottom left.
        CGEventSetWindowLocation(
            cg_event,
            Point {
                x: in_window.x,
                y: frame.size.height - in_window.y,
            },
        );
        let event: Id = msg_send![class!(NSEvent), eventWithCGEvent: cg_event];
        CFRelease(cg_event);
        assert!(!event.is_null(), "AppKit rejected the scroll CGEvent");
        event
    }
}
