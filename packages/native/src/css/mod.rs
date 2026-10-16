//! CSS value parsers, one file per concern. Each takes the CSS text of a value
//! and returns a typed result, so `renderer.rs` and `style.rs` only wire fields.

pub(crate) mod background;
pub(crate) mod border;
pub(crate) mod box_shadow;
pub(crate) mod cache;
pub(crate) mod clip_path;
pub(crate) mod color_function;
pub(crate) mod color_mix;
pub(crate) mod color_space;
pub(crate) mod dimension;
pub(crate) mod filter;
pub(crate) mod font_features;
pub(crate) mod gradient;
pub(crate) mod length;
pub(crate) mod line_height;
pub(crate) mod mask;
#[cfg(test)]
mod parse_cost;
pub(crate) mod position;
pub(crate) mod syntax;
pub(crate) mod text_decoration;
