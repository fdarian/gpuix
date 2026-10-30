//! Platform services that have no GPUI API of their own.
pub(crate) mod eyedropper;
pub(crate) mod file_filter;
pub(crate) mod webview_url;
#[cfg(target_os = "macos")]
pub(crate) mod webview_macos;
