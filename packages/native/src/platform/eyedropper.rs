//! `EyeDropper` (macOS only): a screen colour picker.
//!
//! `NSColorSampler` has no cancel call, so a session ends only when the user
//! clicks or presses Escape. The JS `EyeDropper` rejects an aborted `open()`
//! right away and ignores the late result.
use napi::{Env, Error, JsDeferred, Result};

pub(crate) type EyeDropperResolver = Box<dyn FnOnce(Env) -> Result<Option<String>> + Send>;

/// Resolves `#rrggbb` once the user picks, or `null` if they cancel.
pub(crate) type EyeDropperDeferred = JsDeferred<Option<String>, EyeDropperResolver>;

pub(crate) fn is_supported() -> bool {
    cfg!(target_os = "macos")
}

/// `sRGBHex` for a colour that is already in sRGB. Wide-gamut screens sample
/// colours outside 0..1 once converted; they clamp, as CSS does.
pub(crate) fn srgb_hex(red: f64, green: f64, blue: f64) -> String {
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", channel(red), channel(green), channel(blue))
}

#[cfg(target_os = "macos")]
pub(crate) fn open(deferred: EyeDropperDeferred) {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSColor, NSColorSampler, NSColorSpace};
    use std::cell::RefCell;

    if MainThreadMarker::new().is_none() {
        deferred.reject(Error::from_reason(
            "EyeDropper must be opened on the main thread",
        ));
        return;
    }

    let deferred = RefCell::new(Some(deferred));
    let handler = RcBlock::new(move |color: *mut NSColor| {
        let Some(deferred) = deferred.borrow_mut().take() else {
            return;
        };
        // SAFETY: AppKit passes nil or a live NSColor for the duration of the call.
        let Some(color) = (unsafe { color.as_ref() }) else {
            deferred.resolve(Box::new(|_env| Ok(None)));
            return;
        };
        let Some(srgb) = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else {
            deferred.reject(Error::from_reason(
                "The sampled colour cannot be converted to sRGB",
            ));
            return;
        };
        let hex = srgb_hex(srgb.redComponent(), srgb.greenComponent(), srgb.blueComponent());
        deferred.resolve(Box::new(move |_env| Ok(Some(hex))));
    });
    let sampler: Retained<NSColorSampler> = NSColorSampler::new();
    // SAFETY: called on the main thread (checked above), and AppKit retains
    // the sampler and copies the block until the session ends.
    unsafe { sampler.showSamplerWithSelectionHandler(&handler) };
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn open(deferred: EyeDropperDeferred) {
    deferred.reject(Error::from_reason("EyeDropper is only available on macOS"));
}

#[cfg(test)]
mod tests {
    use super::srgb_hex;

    #[test]
    fn formats_lowercase_rrggbb() {
        assert_eq!(srgb_hex(1.0, 0.5, 0.0), "#ff8000");
        assert_eq!(srgb_hex(0.0, 0.0, 0.0), "#000000");
    }

    #[test]
    fn clamps_out_of_gamut_components() {
        assert_eq!(srgb_hex(1.2, -0.1, 0.999), "#ff00ff");
    }
}
