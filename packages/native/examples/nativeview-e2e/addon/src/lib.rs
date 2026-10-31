//! The library side of the `<nativeview>` e2e. It is given the content `NSView` of a native surface
//! as a pointer, adds a plain coloured view to it and counts what AppKit does with that view.
//! It links no GPUI: everything it knows of the surface is the pointer, as for a real library.
mod library_view;

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

use napi::bindgen_prelude::BigInt;
use napi::{Error, Result};
use napi_derive::napi;

use library_view::{Id, LibraryView};

#[napi(object)]
pub struct AttachOptions {
    /// `#rrggbb` background of the library's view.
    pub color: String,
    /// Opacity of that background, 0 to 1 (default 1).
    pub alpha: Option<f64>,
    /// `"self"` (default), `"nil"` or `"left-half"`: what `hitTest:` of the view answers.
    pub hit: Option<String>,
    /// Adds a second view that reaches 20 points past the content view on every side.
    pub outside: Option<bool>,
    /// `#rrggbb` of that view.
    pub outside_color: Option<String>,
    /// Whether the view accepts the first responder.
    pub accepts_first_responder: Option<bool>,
}

#[napi(object)]
pub struct Stats {
    pub mouse_downs: u32,
    pub hit_tests: u32,
    pub attached: bool,
}

#[derive(Clone)]
#[napi(object)]
pub struct LogEntry {
    /// Counts every attach and detach call across the addon, from 1.
    pub seq: u32,
    /// `"attach"` or `"detach"`.
    pub kind: String,
    /// The content view pointer as a decimal string.
    pub handle: String,
    /// For a detach: the library's view still had a superview when the call came.
    pub had_superview: Option<bool>,
    /// For a detach: the library's view was still in a window when the call came.
    pub had_window: Option<bool>,
}

struct Entry {
    handle: usize,
    view: Option<LibraryView>,
    mouse_downs: u64,
    hit_tests: u64,
}

struct State {
    entries: Vec<Entry>,
    log: Vec<LogEntry>,
}

static STATE: Mutex<State> = Mutex::new(State { entries: Vec::new(), log: Vec::new() });

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().expect("the addon state is only used on the JS thread")
}

/// Records the call in `state.log`, and in the file named by `NATIVEVIEW_E2E_ADDON_LOG` when it is set: a
/// call made while the app is going away has to be readable after the process ended.
fn record(state: &mut State, kind: &str, pointer: usize, had_superview: Option<bool>, had_window: Option<bool>) -> Result<()> {
    let entry = LogEntry {
        seq: state.log.len() as u32 + 1,
        kind: kind.to_string(),
        handle: pointer.to_string(),
        had_superview,
        had_window,
    };
    if let Some(path) = std::env::var_os("NATIVEVIEW_E2E_ADDON_LOG") {
        let mut line = format!(r#"{{"seq":{},"kind":"{}","handle":"{}""#, entry.seq, entry.kind, entry.handle);
        if let Some(value) = entry.had_superview {
            line.push_str(&format!(r#","hadSuperview":{value}"#));
        }
        if let Some(value) = entry.had_window {
            line.push_str(&format!(r#","hadWindow":{value}"#));
        }
        line.push_str("}\n");
        let mut file = OpenOptions::new().create(true).append(true).open(&path).map_err(|error| Error::from_reason(error.to_string()))?;
        file.write_all(line.as_bytes()).map_err(|error| Error::from_reason(error.to_string()))?;
    }
    state.log.push(entry);
    Ok(())
}

fn pointer_of(handle: &BigInt) -> Result<usize> {
    let (signed, value, lossless) = handle.get_u64();
    if signed || !lossless || value == 0 {
        return Err(Error::from_reason(format!("{value} is not a view pointer")));
    }
    usize::try_from(value).map_err(|error| Error::from_reason(error.to_string()))
}

fn latest(entries: &mut [Entry], handle: usize) -> Result<&mut Entry> {
    entries
        .iter_mut()
        .rev()
        .find(|entry| entry.handle == handle)
        .ok_or_else(|| Error::from_reason(format!("no view was attached to {handle}")))
}

/// Adds the library's view to the content view, filling it.
#[napi]
pub fn attach(handle: BigInt, options: AttachOptions) -> Result<()> {
    let pointer = pointer_of(&handle)?;
    let mut state = state();
    if state.entries.iter().any(|entry| entry.handle == pointer && entry.view.is_some()) {
        return Err(Error::from_reason(format!("{pointer} already has a library view")));
    }
    let view = LibraryView::add_to(
        pointer as Id,
        &options.color,
        options.alpha.unwrap_or(1.0),
        options.hit.as_deref().unwrap_or("self"),
        options.outside.unwrap_or(false).then(|| options.outside_color.as_deref().unwrap_or("#808080")),
        options.accepts_first_responder.unwrap_or(false),
    )
    .map_err(Error::from_reason)?;
    state.entries.push(Entry { handle: pointer, view: Some(view), mouse_downs: 0, hit_tests: 0 });
    record(&mut state, "attach", pointer, None, None)
}

/// Removes the library's view and releases it: the duty of a library when `onDetach` runs.
#[napi]
pub fn detach(handle: BigInt) -> Result<()> {
    let pointer = pointer_of(&handle)?;
    let mut state = state();
    let entry = latest(&mut state.entries, pointer)?;
    let view = entry
        .view
        .take()
        .ok_or_else(|| Error::from_reason(format!("the view of {pointer} was already detached")))?;
    entry.mouse_downs = view.mouse_downs();
    entry.hit_tests = view.hit_tests();
    let had_superview = view.has_superview();
    let had_window = view.has_window();
    view.remove_and_release();
    record(&mut state, "detach", pointer, Some(had_superview), Some(had_window))
}

/// The counts of the latest view attached to the content view.
#[napi]
pub fn stats(handle: BigInt) -> Result<Stats> {
    let pointer = pointer_of(&handle)?;
    let mut state = state();
    let entry = latest(&mut state.entries, pointer)?;
    let (mouse_downs, hit_tests) = match entry.view.as_ref() {
        Some(view) => (view.mouse_downs(), view.hit_tests()),
        None => (entry.mouse_downs, entry.hit_tests),
    };
    Ok(Stats { mouse_downs: mouse_downs as u32, hit_tests: hit_tests as u32, attached: entry.view.is_some() })
}

/// Makes the library's view the window's first responder. Whether AppKit agreed.
#[napi]
pub fn make_first_responder(handle: BigInt) -> Result<bool> {
    let pointer = pointer_of(&handle)?;
    let mut state = state();
    let entry = latest(&mut state.entries, pointer)?;
    let view = entry.view.as_ref().ok_or_else(|| Error::from_reason(format!("the view of {pointer} was detached")))?;
    Ok(view.make_first_responder())
}

/// Every attach and detach call so far, in order.
#[napi]
pub fn log() -> Vec<LogEntry> {
    state().log.clone()
}
