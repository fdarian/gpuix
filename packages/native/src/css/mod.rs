//! CSS value parsers, one file per concern. Each takes the CSS text of a value
//! and returns a typed result, so `renderer.rs` and `style.rs` only wire fields.

pub(crate) mod background;
pub(crate) mod border;
pub(crate) mod border_radius;
pub(crate) mod box_shadow;
pub(crate) mod cache;
pub(crate) mod clip_path;
pub(crate) mod color_function;
pub(crate) mod color_mix;
pub(crate) mod color_space;
pub(crate) mod container_query;
pub(crate) mod covers;
pub(crate) mod dimension;
pub(crate) mod filter;
pub(crate) mod flex;
pub(crate) mod font_features;
pub(crate) mod font_variations;
pub(crate) mod gradient;
pub(crate) mod length;
pub(crate) mod line_height;
pub(crate) mod logical;
pub(crate) mod mask;
#[cfg(test)]
mod parse_cost;
pub(crate) mod outline;
pub(crate) mod place;
pub(crate) mod position;
pub(crate) mod shorthand;
pub(crate) mod syntax;
pub(crate) mod tab_size;
pub(crate) mod table;
pub(crate) mod text_box;
pub(crate) mod text_decoration;
pub(crate) mod text_underline_offset;
pub(crate) mod text_wrap;
pub(crate) mod url;
pub(crate) mod vertical_align;
pub(crate) mod word_break;
