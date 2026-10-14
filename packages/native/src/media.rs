// objc 0.2's msg_send! expands to cfg(feature = "cargo-clippy"), which rustc flags as unexpected.
#![allow(unexpected_cfgs)]

//! OS display preferences behind `matchMedia`.
//!
//! JS owns the media query language (`js/media-query.ts`). Native only answers
//! what the OS says, and says when it changed. Only macOS has an OS read; every
//! other platform reports no preference, which is what browsers report when
//! the OS has none.

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use napi_derive::napi;

#[cfg(feature = "test-support")]
use crate::element_tree::EventPayload;
use crate::renderer::{emit_event_full, EventCallback};

/// Event type of the change notification. JS re-reads everything on it, so the
/// payload `value` only says what triggered it, for debugging.
const MEDIA_CHANGE_EVENT: &str = "mediaChange";

/// The OS accessibility preferences that media queries expose.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct MediaFeatures {
    /// `prefers-reduced-motion: reduce`.
    pub reduced_motion: bool,
    /// `prefers-contrast: more`.
    pub increased_contrast: bool,
    /// `forced-colors: active`. macOS has no forced colors mode.
    pub forced_colors: bool,
}

pub(crate) fn emit_media_change(callback: &Option<EventCallback>, reason: &str) {
    emit_event_full(callback, 0, MEDIA_CHANGE_EVENT, |payload| {
        payload.value = Some(reason.to_string());
    });
}

#[cfg(target_os = "macos")]
pub(crate) fn read_system_features() -> MediaFeatures {
    use objc::runtime::{Object, BOOL, YES};
    use objc::{class, msg_send, sel, sel_impl};

    // SAFETY: both properties are plain BOOL getters on the shared workspace.
    unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let reduce_motion: BOOL = msg_send![workspace, accessibilityDisplayShouldReduceMotion];
        let increase_contrast: BOOL =
            msg_send![workspace, accessibilityDisplayShouldIncreaseContrast];
        MediaFeatures {
            reduced_motion: reduce_motion == YES,
            increased_contrast: increase_contrast == YES,
            forced_colors: false,
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn read_system_features() -> MediaFeatures {
    MediaFeatures::default()
}

#[cfg(target_os = "macos")]
mod observer {
    use block::ConcreteBlock;
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {
        static NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification: *mut Object;
    }

    /// Observes the accessibility display options (reduce motion, increase
    /// contrast, ...) and removes itself on drop.
    pub(crate) struct MediaChangeObserver {
        token: *mut Object,
    }

    impl MediaChangeObserver {
        /// `on_change` runs on the main queue, which is the thread GPUI and the
        /// napi calls share.
        pub(crate) fn new(on_change: impl Fn() + 'static) -> Self {
            // SAFETY: the block is copied to the heap, so the center keeps it
            // alive until `removeObserver:`. `token` is retained until drop.
            unsafe {
                let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
                let center: *mut Object = msg_send![workspace, notificationCenter];
                let main_queue: *mut Object = msg_send![class!(NSOperationQueue), mainQueue];
                let block = ConcreteBlock::new(move |_notification: *mut Object| on_change());
                let block = block.copy();
                let token: *mut Object = msg_send![center,
                    addObserverForName: NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification
                    object: std::ptr::null_mut::<Object>()
                    queue: main_queue
                    usingBlock: block
                ];
                let token: *mut Object = msg_send![token, retain];
                Self { token }
            }
        }
    }

    impl Drop for MediaChangeObserver {
        fn drop(&mut self) {
            // SAFETY: `token` came from `addObserverForName:` and was retained in `new`.
            unsafe {
                let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
                let center: *mut Object = msg_send![workspace, notificationCenter];
                let _: () = msg_send![center, removeObserver: self.token];
                let _: () = msg_send![self.token, release];
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) use observer::MediaChangeObserver;

/// What the test renderer reports instead of the OS, so tests are not tied to
/// the machine's settings and can flip a preference.
#[cfg(feature = "test-support")]
#[derive(Debug, Default)]
pub(crate) struct TestMedia {
    pub(crate) features: MediaFeatures,
    pub(crate) listening: bool,
}

#[cfg(feature = "test-support")]
impl TestMedia {
    /// Applies the given preferences and returns the change event to queue, if
    /// anything changed and the window is listening.
    pub(crate) fn set(
        &mut self,
        reduced_motion: Option<bool>,
        increased_contrast: Option<bool>,
        forced_colors: Option<bool>,
    ) -> Option<EventPayload> {
        let before = self.features;
        self.features = MediaFeatures {
            reduced_motion: reduced_motion.unwrap_or(before.reduced_motion),
            increased_contrast: increased_contrast.unwrap_or(before.increased_contrast),
            forced_colors: forced_colors.unwrap_or(before.forced_colors),
        };
        if self.features == before || !self.listening {
            return None;
        }
        Some(EventPayload {
            element_id: 0.0,
            event_type: MEDIA_CHANGE_EVENT.to_string(),
            value: Some("test".to_string()),
            ..Default::default()
        })
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;

    #[test]
    fn test_media_reports_a_change_only_when_listening_and_different() {
        let mut media = TestMedia::default();
        assert!(media.set(Some(true), None, None).is_none(), "nobody listens yet");
        media.listening = true;
        assert!(media.set(Some(true), None, None).is_none(), "unchanged");
        let event = media.set(Some(false), Some(true), None).expect("changed");
        assert_eq!(event.event_type, "mediaChange");
        assert!(media.features.increased_contrast);
        assert!(!media.features.reduced_motion);
    }
}
