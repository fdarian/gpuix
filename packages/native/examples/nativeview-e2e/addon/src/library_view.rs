//! `E2ELibraryView`: a plain `NSView` subclass declared at run time (no AppKit crate is linked: the
//! classes are looked up in the host process) that counts the pointer calls AppKit makes on it.
use std::os::raw::c_void;

use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel, BOOL, NO, YES};
use objc::{class, msg_send, sel, sel_impl, Encode, Encoding};

pub type Id = *mut Object;

const NIL: Id = std::ptr::null_mut();

/// How far the outside view reaches past the content view on every side, in points.
const OUTSIDE_REACH: f64 = 20.0;

const AUTORESIZE_WIDTH_AND_HEIGHT: usize = 2 | 16;

const HIT_SELF: u64 = 0;
const HIT_NIL: u64 = 1;
const HIT_LEFT_HALF: u64 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Size {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    origin: Point,
    size: Size,
}

unsafe impl Encode for Point {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{CGPoint=dd}") }
    }
}

unsafe impl Encode for Size {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{CGSize=dd}") }
    }
}

unsafe impl Encode for Rect {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
    }
}

extern "C" fn mouse_down(this: &mut Object, _: Sel, _event: Id) {
    unsafe {
        let count = *this.get_ivar::<u64>("mouse_downs");
        this.set_ivar("mouse_downs", count + 1);
    }
}

extern "C" fn hit_test(this: &mut Object, _: Sel, point: Point) -> Id {
    unsafe {
        let count = *this.get_ivar::<u64>("hit_tests");
        this.set_ivar("hit_tests", count + 1);
        match *this.get_ivar::<u64>("hit_mode") {
            HIT_NIL => NIL,
            HIT_LEFT_HALF => {
                let frame: Rect = msg_send![&*this, frame];
                if point.x >= frame.origin.x + frame.size.width / 2.0 {
                    NIL
                } else {
                    msg_send![super(&*this, class!(NSView)), hitTest: point]
                }
            }
            _ => msg_send![super(&*this, class!(NSView)), hitTest: point],
        }
    }
}

extern "C" fn accepts_first_responder(this: &Object, _: Sel) -> BOOL {
    unsafe { *this.get_ivar::<BOOL>("accepts_first_responder") }
}

fn view_class() -> &'static Class {
    if let Some(class) = Class::get("E2ELibraryView") {
        return class;
    }
    let mut declaration = ClassDecl::new("E2ELibraryView", class!(NSView)).expect("E2ELibraryView is declared once");
    declaration.add_ivar::<u64>("mouse_downs");
    declaration.add_ivar::<u64>("hit_tests");
    declaration.add_ivar::<u64>("hit_mode");
    declaration.add_ivar::<BOOL>("accepts_first_responder");
    unsafe {
        declaration.add_method(sel!(mouseDown:), mouse_down as extern "C" fn(&mut Object, Sel, Id));
        declaration.add_method(sel!(hitTest:), hit_test as extern "C" fn(&mut Object, Sel, Point) -> Id);
        declaration.add_method(sel!(acceptsFirstResponder), accepts_first_responder as extern "C" fn(&Object, Sel) -> BOOL);
    }
    declaration.register()
}

fn parse_color(text: &str) -> Result<[f64; 3], String> {
    let digits = text.strip_prefix('#').filter(|digits| digits.len() == 6).ok_or_else(|| format!("{text:?} is not #rrggbb"))?;
    let mut channels = [0.0; 3];
    for (index, channel) in channels.iter_mut().enumerate() {
        let byte = u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16).map_err(|_| format!("{text:?} is not #rrggbb"))?;
        *channel = f64::from(byte) / 255.0;
    }
    Ok(channels)
}

/// A layer-backed view with a flat background that fills a rectangle of its parent.
unsafe fn plain_view(class: &Class, frame: Rect, color: &str, alpha: f64) -> Result<Id, String> {
    let channels = parse_color(color)?;
    let view: Id = msg_send![class, alloc];
    let view: Id = msg_send![view, initWithFrame: frame];
    let _: () = msg_send![view, setWantsLayer: YES];
    let _: () = msg_send![view, setAutoresizingMask: AUTORESIZE_WIDTH_AND_HEIGHT];
    let ns_color: Id = msg_send![class!(NSColor), colorWithSRGBRed: channels[0] green: channels[1] blue: channels[2] alpha: alpha];
    let cg_color: *mut c_void = msg_send![ns_color, CGColor];
    let layer: Id = msg_send![view, layer];
    let _: () = msg_send![layer, setBackgroundColor: cg_color];
    Ok(view)
}

/// What the library holds of one attach: its view, which it must remove and release itself.
pub struct LibraryView {
    view: Id,
    outside: Option<Id>,
}

// The addon is only called on the JS thread, which is the main thread the views belong to.
unsafe impl Send for LibraryView {}

impl LibraryView {
    pub fn add_to(content: Id, color: &str, alpha: f64, hit: &str, outside: Option<&str>, accepts_first_responder: bool) -> Result<Self, String> {
        let hit_mode = match hit {
            "self" => HIT_SELF,
            "nil" => HIT_NIL,
            "left-half" => HIT_LEFT_HALF,
            other => return Err(format!("unknown hit mode {other:?}")),
        };
        unsafe {
            let bounds: Rect = msg_send![content, bounds];
            let outside_view = match outside {
                Some(outside_color) => {
                    let reach = Rect {
                        origin: Point { x: -OUTSIDE_REACH, y: -OUTSIDE_REACH },
                        size: Size { width: bounds.size.width + 2.0 * OUTSIDE_REACH, height: bounds.size.height + 2.0 * OUTSIDE_REACH },
                    };
                    let view = plain_view(class!(NSView), reach, outside_color, 1.0)?;
                    let _: () = msg_send![content, addSubview: view];
                    Some(view)
                }
                None => None,
            };
            let view = plain_view(view_class(), bounds, color, alpha)?;
            (*view).set_ivar("hit_mode", hit_mode);
            (*view).set_ivar("accepts_first_responder", if accepts_first_responder { YES } else { NO });
            let _: () = msg_send![content, addSubview: view];
            Ok(Self { view, outside: outside_view })
        }
    }

    pub fn mouse_downs(&self) -> u64 {
        unsafe { *(*self.view).get_ivar::<u64>("mouse_downs") }
    }

    pub fn hit_tests(&self) -> u64 {
        unsafe { *(*self.view).get_ivar::<u64>("hit_tests") }
    }

    pub fn has_superview(&self) -> bool {
        unsafe {
            let superview: Id = msg_send![self.view, superview];
            !superview.is_null()
        }
    }

    pub fn has_window(&self) -> bool {
        unsafe {
            let window: Id = msg_send![self.view, window];
            !window.is_null()
        }
    }

    pub fn make_first_responder(&self) -> bool {
        unsafe {
            let window: Id = msg_send![self.view, window];
            if window.is_null() {
                return false;
            }
            let accepted: BOOL = msg_send![window, makeFirstResponder: self.view];
            accepted == YES
        }
    }

    pub fn remove_and_release(self) {
        unsafe {
            for view in [Some(self.view), self.outside].into_iter().flatten() {
                let _: () = msg_send![view, removeFromSuperview];
                let _: () = msg_send![view, release];
            }
        }
    }
}
