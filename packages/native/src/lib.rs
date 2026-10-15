#![deny(clippy::all)]

#[cfg(all(
    not(all(target_arch = "wasm32", target_os = "unknown")),
    not(all(
        feature = "test-support",
        any(target_os = "macos", target_os = "windows")
    ))
))]
use napi::bindgen_prelude::*;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use napi_derive::napi;

mod accessibility;
#[cfg(target_os = "macos")]
mod app_menu;
mod automation;
mod color;
mod css;
mod custom_elements;
mod diff;
mod element_tree;
pub mod events;
mod focus;
mod forms;
mod layout;
mod markdown;
mod media;
mod motion;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod platform;
mod observers;
mod renderer;
mod scroll;
mod scrollbar;
// The data model is public so `examples/bench_serde.rs` measures the real
// types instead of a copy that silently drifts from them.
pub mod retained_tree;
pub mod style;
mod syntax;
mod text;
mod theme;
// Desktop only. HTTP goes through reqwest_client, not crates.io reqwest.
#[cfg(not(target_family = "wasm"))]
mod updater;

#[cfg(all(
    feature = "test-support",
    any(target_os = "macos", target_os = "windows")
))]
mod test_renderer;

pub use element_tree::*;
pub use observers::Observation;
pub use media::MediaFeatures;
pub use renderer::*;
pub use style::*;

#[cfg(any(
    test,
    not(all(
        feature = "test-support",
        any(target_os = "macos", target_os = "windows")
    ))
))]
const TEST_GPUIX_RENDERER_UNAVAILABLE: &str = concat!(
    "TestGpuixRenderer is macOS and Windows only. ",
    "Linux builds have no test-support because wgpu cannot read a rendered image back yet. ",
    "GpuixRenderer still works on Linux."
);

/// True only when this binary compiled the real GPU test renderer.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[napi]
pub fn has_test_gpuix_renderer() -> bool {
    cfg!(all(
        feature = "test-support",
        any(target_os = "macos", target_os = "windows")
    ))
}

/// Checks a keyframes definition the way `GpuixRenderer.defineKeyframes` does,
/// without a renderer. `defineKeyframes` from `@gpuix/native/host` runs it
/// first so a bad definition throws where it is written.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[napi]
pub fn validate_keyframes(name: String, frames_json: String) -> napi::Result<()> {
    motion::validate_keyframes(&name, &frames_json).map_err(napi::Error::from_reason)
}

#[cfg(all(
    not(all(target_arch = "wasm32", target_os = "unknown")),
    not(all(
        feature = "test-support",
        any(target_os = "macos", target_os = "windows")
    ))
))]
#[napi]
pub struct TestGpuixRenderer;

#[cfg(all(
    not(all(target_arch = "wasm32", target_os = "unknown")),
    not(all(
        feature = "test-support",
        any(target_os = "macos", target_os = "windows")
    ))
))]
#[napi]
impl TestGpuixRenderer {
    #[napi(constructor)]
    pub fn new(_width: Option<f64>, _height: Option<f64>) -> Result<Self> {
        Err(Error::from_reason(TEST_GPUIX_RENDERER_UNAVAILABLE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_message_explains_linux() {
        assert!(TEST_GPUIX_RENDERER_UNAVAILABLE.contains("macOS and Windows only"));
        assert!(TEST_GPUIX_RENDERER_UNAVAILABLE.contains(
            "Linux builds have no test-support because wgpu cannot read a rendered image back yet"
        ));
        assert!(TEST_GPUIX_RENDERER_UNAVAILABLE.contains("GpuixRenderer still works on Linux"));
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    #[test]
    fn has_test_gpuix_renderer_matches_real_impl() {
        assert_eq!(
            has_test_gpuix_renderer(),
            cfg!(all(
                feature = "test-support",
                any(target_os = "macos", target_os = "windows")
            ))
        );
    }

    #[cfg(all(
        not(all(target_arch = "wasm32", target_os = "unknown")),
        not(all(
            feature = "test-support",
            any(target_os = "macos", target_os = "windows")
        ))
    ))]
    #[test]
    fn stub_constructor_explains_why() {
        match TestGpuixRenderer::new(None, None) {
            Ok(_) => panic!("stub constructor must fail"),
            Err(err) => assert!(
                err.to_string().contains(TEST_GPUIX_RENDERER_UNAVAILABLE),
                "{err}"
            ),
        }
    }
}
