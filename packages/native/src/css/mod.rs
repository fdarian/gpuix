//! CSS value parsers, one file per concern. Each takes the CSS text of a value
//! and returns a typed result, so `renderer.rs` and `style.rs` only wire fields.

pub(crate) mod border;
pub(crate) mod box_shadow;
pub(crate) mod color_function;
pub(crate) mod color_mix;
pub(crate) mod color_space;
pub(crate) mod font_features;
pub(crate) mod syntax;
