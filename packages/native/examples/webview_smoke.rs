//! A real `WKWebView` through a load, a title, history and scripts. macOS only.
#[cfg(target_os = "macos")]
fn main() {
    gpuix_native::webview_smoke();
    eprintln!("webview smoke ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("webview is macOS only");
}
