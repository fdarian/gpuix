use serde::{Deserialize, Deserializer, Serialize};

use crate::layout::grid_values::{
    GridAreaValue, GridAutoTracks, GridLinePairValue, GridLineValue, GridTemplateAreasValue,
    GridTemplateValue,
};

/// Font weight value — accepts both CSS strings ("bold", "700") and numbers (700).
/// JS style objects commonly use both `fontWeight: "bold"` and `fontWeight: 700`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FontWeightValue {
    Num(f64),
    Str(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxShadowValue {
    pub offset_x: f64,
    pub offset_y: f64,
    pub blur_radius: f64,
    pub spread_radius: f64,
    pub color: String,
    /// Draws inside the border box instead of around it, like CSS `inset`.
    #[serde(default)]
    pub inset: bool,
}

/// `boxShadow` is one shadow or a list of them, painted in the order given.
/// JS may also send the CSS string, which is parsed into the same layers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, from = "BoxShadowInput")]
pub enum BoxShadowValues {
    Many(Vec<BoxShadowValue>),
    One(BoxShadowValue),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BoxShadowInput {
    Css(String),
    Many(Vec<BoxShadowValue>),
    One(BoxShadowValue),
}

impl From<BoxShadowInput> for BoxShadowValues {
    fn from(input: BoxShadowInput) -> Self {
        match input {
            // An invalid declaration is ignored like in CSS: no layers, no shadow.
            BoxShadowInput::Css(css) => {
                Self::Many(crate::css::box_shadow::parse(&css).unwrap_or_default())
            }
            BoxShadowInput::Many(layers) => Self::Many(layers),
            BoxShadowInput::One(layer) => Self::One(layer),
        }
    }
}

impl BoxShadowValues {
    pub fn as_slice(&self) -> &[BoxShadowValue] {
        match self {
            Self::Many(shadows) => shadows,
            Self::One(shadow) => std::slice::from_ref(shadow),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearGradientStopValue {
    pub color: String,
    pub position: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum GradientValue {
    #[serde(rename = "linear-gradient")]
    LinearGradient {
        angle: f64,
        stops: [LinearGradientStopValue; 2],
        #[serde(rename = "colorSpace")]
        color_space: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BackgroundValue {
    Color(String),
    Gradient(Box<GradientValue>),
}

/// A dimension value that can be a number (pixels) or a string (percentage, auto, etc.)
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum DimensionValue {
    Pixels(f64),
    Percentage(f64), // 0.0 to 1.0
    Auto,
    /// `anchor-size(width)` / `anchor-size(height)`: the trigger of the enclosing
    /// `<anchored>`. Only the six size properties of its direct children resolve it.
    AnchorSize(AnchorAxis),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum AnchorAxis {
    Width,
    Height,
}

impl Default for DimensionValue {
    fn default() -> Self {
        DimensionValue::Auto
    }
}

impl<'de> Deserialize<'de> for DimensionValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{self, Visitor};

        struct DimensionVisitor;

        impl<'de> Visitor<'de> for DimensionVisitor {
            type Value = DimensionValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a number or a string like '100%' or 'auto'")
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(DimensionValue::Pixels(v))
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(DimensionValue::Pixels(v as f64))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(DimensionValue::Pixels(v as f64))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if v == "auto" {
                    Ok(DimensionValue::Auto)
                } else if let Some(axis) = v
                    .strip_prefix("anchor-size(")
                    .and_then(|rest| rest.strip_suffix(')'))
                {
                    match axis.trim() {
                        "width" => Ok(DimensionValue::AnchorSize(AnchorAxis::Width)),
                        "height" => Ok(DimensionValue::AnchorSize(AnchorAxis::Height)),
                        _ => Err(de::Error::custom(format!("invalid anchor-size: {}", v))),
                    }
                } else if v.ends_with('%') {
                    let num_str = v.trim_end_matches('%');
                    match num_str.parse::<f64>() {
                        Ok(n) => Ok(DimensionValue::Percentage(n / 100.0)),
                        Err(_) => Err(de::Error::custom(format!("invalid percentage: {}", v))),
                    }
                } else {
                    // Try to parse as a number
                    match v.parse::<f64>() {
                        Ok(n) => Ok(DimensionValue::Pixels(n)),
                        Err(_) => Err(de::Error::custom(format!("invalid dimension: {}", v))),
                    }
                }
            }
        }

        deserializer.deserialize_any(DimensionVisitor)
    }
}

/// Style description that can be serialized from JS
/// Note: This is only used for JSON deserialization, not direct napi binding
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleDesc {
    // Display
    pub display: Option<String>,
    pub visibility: Option<String>,

    // Flexbox
    pub flex_direction: Option<String>,
    pub flex_wrap: Option<String>,
    pub flex_grow: Option<f64>,
    pub flex_shrink: Option<f64>,
    pub flex_basis: Option<f64>,
    pub align_items: Option<String>,
    pub align_self: Option<String>,
    pub align_content: Option<String>,
    pub justify_content: Option<String>,
    pub gap: Option<f64>,
    pub row_gap: Option<f64>,
    pub column_gap: Option<f64>,
    /// Stable-sorts the children of a flex or grid container. Default 0.
    pub order: Option<f64>,
    pub justify_items: Option<String>,
    pub justify_self: Option<String>,

    // Grid. A template is a column count `n` or a CSS track list.
    pub grid_template_columns: Option<GridTemplateValue>,
    pub grid_template_rows: Option<GridTemplateValue>,
    pub grid_column_min: Option<String>,
    pub grid_row_min: Option<String>,
    pub grid_auto_columns: Option<GridAutoTracks>,
    pub grid_auto_rows: Option<GridAutoTracks>,
    pub grid_auto_flow: Option<String>,
    pub grid_template_areas: Option<GridTemplateAreasValue>,
    pub grid_area: Option<GridAreaValue>,
    pub grid_column: Option<GridLinePairValue>,
    pub grid_row: Option<GridLinePairValue>,
    pub grid_column_start: Option<GridLineValue>,
    pub grid_column_end: Option<GridLineValue>,
    pub grid_row_start: Option<GridLineValue>,
    pub grid_row_end: Option<GridLineValue>,

    // Sizing - now supports both numbers and strings like "100%" or "auto"
    pub width: Option<DimensionValue>,
    pub height: Option<DimensionValue>,
    pub min_width: Option<DimensionValue>,
    pub min_height: Option<DimensionValue>,
    pub max_width: Option<DimensionValue>,
    pub max_height: Option<DimensionValue>,

    // Spacing (padding)
    pub padding: Option<f64>,
    pub padding_top: Option<f64>,
    pub padding_right: Option<f64>,
    pub padding_bottom: Option<f64>,
    pub padding_left: Option<f64>,

    // Spacing (margin): px, a percentage of the containing block width, or "auto".
    pub margin: Option<DimensionValue>,
    pub margin_top: Option<DimensionValue>,
    pub margin_right: Option<DimensionValue>,
    pub margin_bottom: Option<DimensionValue>,
    pub margin_left: Option<DimensionValue>,

    // Position
    pub position: Option<String>,
    /// Paint order among positioned siblings. Only applies when `position` is set.
    pub z_index: Option<f64>,
    pub top: Option<DimensionValue>,
    pub right: Option<DimensionValue>,
    pub bottom: Option<DimensionValue>,
    pub left: Option<DimensionValue>,

    // Background & Colors
    pub background: Option<BackgroundValue>,
    pub background_color: Option<String>,
    /// CSS `background-image`: a comma list of `linear-`, `radial-` and
    /// `conic-gradient()` (and `repeating-` forms), `none` or `url()`. First is on top.
    pub background_image: Option<String>,
    /// CSS `background-size`, `-position`, `-repeat`, `-origin` and `-clip`. Each
    /// is a comma list that repeats to cover the layers.
    pub background_size: Option<String>,
    pub background_position: Option<String>,
    pub background_repeat: Option<String>,
    pub background_origin: Option<String>,
    pub background_clip: Option<String>,
    pub color: Option<String>,
    /// CSS `color-scheme`: `"normal"`, `"light"`, `"dark"`, `"light dark"`,
    /// `"only light"`, `"only dark"`. Inherited. Picks the side of `light-dark()`
    /// that applies; `"light dark"` follows the window appearance.
    pub color_scheme: Option<String>,
    pub opacity: Option<f64>,

    // Border
    pub border_width: Option<f64>,
    pub border_top_width: Option<f64>,
    pub border_right_width: Option<f64>,
    pub border_bottom_width: Option<f64>,
    pub border_left_width: Option<f64>,
    /// One to four colours, like CSS `border-color`: `"red"`, `"red blue"`, ...
    pub border_color: Option<String>,
    pub border_top_color: Option<String>,
    pub border_right_color: Option<String>,
    pub border_bottom_color: Option<String>,
    pub border_left_color: Option<String>,
    /// CSS shorthands, `"1px solid red"`. The longhands above win over them.
    pub border: Option<String>,
    pub border_top: Option<String>,
    pub border_right: Option<String>,
    pub border_bottom: Option<String>,
    pub border_left: Option<String>,
    pub border_radius: Option<f64>,
    pub border_top_left_radius: Option<f64>,
    pub border_top_right_radius: Option<f64>,
    pub border_bottom_left_radius: Option<f64>,
    pub border_bottom_right_radius: Option<f64>,
    pub box_shadow: Option<BoxShadowValues>,
    /// CSS `filter`, e.g. `"blur(4px) brightness(1.2)"`. Applied to the element and
    /// everything in it. An invalid string is ignored.
    pub filter: Option<String>,
    /// CSS `backdrop-filter`: the same functions, applied to what is behind the element.
    pub backdrop_filter: Option<String>,
    /// CSS `mask-image`: a comma list of gradients (and `repeating-` forms), `none`
    /// or `url()`. Only the alpha of each layer is used. First is on top.
    pub mask_image: Option<String>,
    /// CSS `mask-size`, `-position`, `-repeat`, `-origin` and `-clip`. Each is a
    /// comma list that repeats to cover the layers.
    pub mask_size: Option<String>,
    pub mask_position: Option<String>,
    pub mask_repeat: Option<String>,
    pub mask_origin: Option<String>,
    pub mask_clip: Option<String>,
    /// CSS `mask` shorthand. The longhands above replace its parts.
    pub mask: Option<String>,
    /// CSS `clip-path`: `inset()`, `circle()`, `ellipse()` or `polygon()` and an
    /// optional reference box, or `none`. An invalid string is ignored.
    pub clip_path: Option<String>,
    /// `"solid"` or `"dashed"`, for every side.
    pub border_style: Option<String>,
    /// Width divided by height, applied to whichever axis is not definite.
    /// A number or a CSS ratio string such as `"16 / 9"`.
    pub aspect_ratio: Option<crate::layout::aspect_ratio::AspectRatio>,
    /// CSS `outline`: a line outside the border box that takes no layout
    /// space, so it can appear on focus without moving anything.
    pub outline_width: Option<f64>,
    pub outline_color: Option<String>,
    pub outline_offset: Option<f64>,

    // Text
    pub font_size: Option<f64>,
    pub font_family: Option<String>,
    pub font_weight: Option<FontWeightValue>,
    /// CSS `font-style`: `normal`, `italic` or `oblique`. Inherited.
    pub font_style: Option<String>,
    pub text_align: Option<String>,
    pub line_height: Option<f64>,
    pub white_space: Option<String>,
    pub text_overflow: Option<String>,
    pub line_clamp: Option<f64>,
    pub text_decoration: Option<String>,
    /// CSS `font-variant-numeric` keywords, e.g. `"tabular-nums slashed-zero"`.
    /// Inherited. Not supported inside `hover`/`active`/`focusVisible`.
    pub font_variant_numeric: Option<String>,
    /// CSS `font-feature-settings`, e.g. `'"tnum" 1, "liga" 0'`. Inherited, and it
    /// wins over `fontVariantNumeric` on a feature both name.
    pub font_feature_settings: Option<String>,

    // Overflow
    pub overflow: Option<String>,
    pub overflow_x: Option<String>,
    pub overflow_y: Option<String>,
    /// `"auto"` or `"<thumb> <track>"`. Without it the colours follow the system appearance.
    pub scrollbar_color: Option<String>,
    /// `"auto"`, `"thin"` or `"none"`, which hides the bar but leaves the container scrollable.
    pub scrollbar_width: Option<String>,
    /// `"auto"`, `"contain"` or `"none"`: whether a scroller at its limit hands the wheel to
    /// the scroller around it. `overscrollBehaviorX` / `Y` override it per axis.
    pub overscroll_behavior: Option<String>,
    pub overscroll_behavior_x: Option<String>,
    pub overscroll_behavior_y: Option<String>,
    /// `"smooth"` animates `scrollTo` and `scrollIntoView` calls that name no behavior.
    pub scroll_behavior: Option<String>,

    // Cursor
    pub cursor: Option<String>,
    /// `"none"` makes the element never a pointer target; it inherits and a
    /// descendant's `"auto"` restores it, as in CSS.
    pub pointer_events: Option<String>,

    // Text selection. "none" opts an element and its subtree out of the
    // selection registry, so buttons and toolbars never start a drag.
    // Inherited down the tree like the CSS property of the same name.
    pub user_select: Option<String>,
    /// Selection wash colour for this subtree. Defaults to the theme accent at
    /// 35% opacity, the same tone Comet uses.
    pub selection_color: Option<String>,

    // CSS transitions: CSS syntax, kebab-case property names, as in a DOM inline
    // style. The longhands win over the `transition` shorthand.
    pub transition: Option<String>,
    pub transition_property: Option<String>,
    pub transition_duration: Option<String>,
    pub transition_timing_function: Option<String>,
    pub transition_delay: Option<String>,

    // Pseudo-selector styles — applied by GPUI natively (no JS round-trip).
    // Uses Box to avoid infinite-size struct (StyleDesc contains StyleDesc).
    pub hover: Option<Box<StyleDesc>>,
    pub active: Option<Box<StyleDesc>>,
    /// While focused after keyboard input, like CSS `:focus-visible`.
    pub focus_visible: Option<Box<StyleDesc>>,
    /// While this element or a descendant is focused, like CSS `:focus-within`.
    pub focus_within: Option<Box<StyleDesc>>,
    /// Makes this element a group that `groupHover`/`groupActive` styles on its
    /// descendants react to. `true` is the default group, a string names one.
    /// Inside `groupHover`/`groupActive` it names the group to follow instead.
    pub group: Option<GroupName>,
    /// Applied while the nearest ancestor `group` of that name is hovered.
    pub group_hover: Option<Box<StyleDesc>>,
    /// Applied while the mouse is down inside the nearest ancestor `group` of that name.
    pub group_active: Option<Box<StyleDesc>>,
    /// `false` keeps this element at full opacity while another control has
    /// keyboard focus. Not inherited.
    pub keyboard_focus_dim: Option<bool>,
}

pub use crate::color::{parse_color, parse_color_hex};

/// `group: true` or `group: "card"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GroupName {
    Flag(bool),
    Name(String),
}

/// Name of the group `group: true` and an unnamed `groupHover` refer to.
const DEFAULT_GROUP: &str = "__gpuix_group";

impl StyleDesc {
    /// The group this element declares, if any.
    pub(crate) fn declared_group(&self) -> Option<gpui::SharedString> {
        match self.group.as_ref()? {
            GroupName::Flag(true) => Some(DEFAULT_GROUP.into()),
            GroupName::Name(name) if !name.is_empty() => Some(name.clone().into()),
            _ => None,
        }
    }

    /// The group a `groupHover`/`groupActive` style follows: its own `group`
    /// field, or the default group.
    pub(crate) fn followed_group(&self) -> gpui::SharedString {
        self.declared_group().unwrap_or_else(|| DEFAULT_GROUP.into())
    }
}
use crate::color::ColorContext;

impl StyleDesc {
    /// The style with everything that needs a mouse hitbox removed: `hover`,
    /// `active` and `cursor`. What an `inert` subtree paints.
    pub fn without_pointer_states(&self) -> Self {
        Self {
            hover: None,
            active: None,
            group_hover: None,
            group_active: None,
            cursor: None,
            ..self.clone()
        }
    }
}

/// What a style paints behind its content.
pub(crate) struct Fill {
    pub color: Option<gpui::Background>,
    /// First is on top.
    pub layers: Vec<gpui::BackgroundLayer>,
}

impl BackgroundValue {
    fn resolve(&self, colors: &ColorContext) -> Option<gpui::Background> {
        match self {
            Self::Color(value) => colors.resolve(value).map(Into::into),
            Self::Gradient(gradient) => {
                let GradientValue::LinearGradient {
                    angle,
                    stops,
                    color_space,
                } = gradient.as_ref();
                let angle = *angle as f32;
                let [from, to] = stops;
                let from_position = from.position as f32;
                let to_position = to.position as f32;
                if !angle.is_finite()
                    || !(0.0..=1.0).contains(&from_position)
                    || !(0.0..=1.0).contains(&to_position)
                {
                    return None;
                }

                let color_space = match color_space.as_deref() {
                    None | Some("srgb") => gpui::ColorSpace::Srgb,
                    Some("oklab") => gpui::ColorSpace::Oklab,
                    _ => return None,
                };
                Some(
                    gpui::linear_gradient(
                        angle,
                        gpui::linear_color_stop(colors.resolve(&from.color)?, from_position),
                        gpui::linear_color_stop(colors.resolve(&to.color)?, to_position),
                    )
                    .color_space(color_space),
                )
            }
        }
    }
}

impl StyleDesc {
    pub(crate) fn has_background(&self) -> bool {
        self.background_color.is_some()
            || self.background.is_some()
            || self.background_image.is_some()
    }

    /// Whether this style sets the background layers, even to none. The
    /// `background` shorthand resets every layer in CSS, so a state style that
    /// declares it must replace the base layers rather than leave them.
    pub(crate) fn declares_background_layers(&self) -> bool {
        self.background.is_some() || self.background_image.is_some()
    }

    /// The colour declarations of this style that mention `currentColor` and
    /// that `state` does not set itself. A state that changes `color` repaints
    /// them, because in CSS the keyword follows the element's colour.
    pub(crate) fn colors_following_current(&self, state: &StyleDesc) -> Option<StyleDesc> {
        fn follows(value: &Option<String>) -> Option<String> {
            value
                .as_deref()
                .filter(|value| value.to_ascii_lowercase().contains("currentcolor"))
                .map(str::to_owned)
        }
        state.color.as_ref()?;
        let mut followers = StyleDesc::default();
        if state.background_color.is_none() && state.background.is_none() {
            followers.background_color = follows(&self.background_color);
        }
        if state.border_color.is_none() {
            followers.border_color = follows(&self.border_color);
        }
        // A border shorthand without a colour is `currentColor`, so it follows too.
        let shorthand_follows = |value: &Option<String>| {
            value
                .as_deref()
                .filter(|value| {
                    crate::css::border::parse_border(value).is_some_and(|parsed| {
                        parsed.width > 0.0 && parsed.color.to_ascii_lowercase().contains("currentcolor")
                    })
                })
                .map(str::to_owned)
        };
        macro_rules! follow {
            ($field:ident, $rule:expr) => {
                if state.$field.is_none() {
                    followers.$field = $rule(&self.$field);
                }
            };
        }
        follow!(background_image, follows);
        follow!(mask_image, follows);
        follow!(mask, follows);
        if state.background.is_none() {
            if let Some(BackgroundValue::Color(value)) = &self.background {
                if value.to_ascii_lowercase().contains("currentcolor") {
                    followers.background = self.background.clone();
                }
            }
        }
        follow!(border_top_color, follows);
        follow!(border_right_color, follows);
        follow!(border_bottom_color, follows);
        follow!(border_left_color, follows);
        follow!(border, shorthand_follows);
        follow!(border_top, shorthand_follows);
        follow!(border_right, shorthand_follows);
        follow!(border_bottom, shorthand_follows);
        follow!(border_left, shorthand_follows);
        if state.outline_color.is_none() {
            followers.outline_color = follows(&self.outline_color);
            if followers.outline_color.is_some() {
                followers.outline_width = self.outline_width;
                followers.outline_offset = self.outline_offset;
            }
        }
        if state.box_shadow.is_none()
            && self.box_shadow.as_ref().is_some_and(|shadows| {
                shadows
                    .as_slice()
                    .iter()
                    .any(|shadow| follows(&Some(shadow.color.clone())).is_some())
            })
        {
            followers.box_shadow = self.box_shadow.clone();
        }
        if state.filter.is_none()
            && self
                .filter
                .as_deref()
                .is_some_and(crate::css::filter::follows_current_color)
        {
            followers.filter = self.filter.clone();
        }
        if state.backdrop_filter.is_none()
            && self
                .backdrop_filter
                .as_deref()
                .is_some_and(crate::css::filter::follows_current_color)
        {
            followers.backdrop_filter = self.backdrop_filter.clone();
        }
        (followers != StyleDesc::default()).then_some(followers)
    }

    /// The mask layers, or `None` when the style sets no valid mask. `Some(empty)`
    /// is `mask: none`.
    pub(crate) fn resolved_mask(&self, colors: &ColorContext) -> Option<Vec<gpui::BackgroundLayer>> {
        crate::css::mask::resolve(
            &crate::css::mask::Inputs {
                shorthand: self.mask.as_deref(),
                image: self.mask_image.as_deref(),
                size: self.mask_size.as_deref(),
                position: self.mask_position.as_deref(),
                repeat: self.mask_repeat.as_deref(),
                origin: self.mask_origin.as_deref(),
                clip: self.mask_clip.as_deref(),
            },
            colors,
        )
    }

    /// The background colour (or object gradient) and the gradient layers.
    ///
    /// `background` takes a plain colour as it always did, or the CSS shorthand
    /// with layers and a final colour. `backgroundColor` wins over the
    /// shorthand's colour, and the `background-*` longhands replace its parts.
    pub(crate) fn resolved_fill(&self, colors: &ColorContext) -> Fill {
        let shorthand = match self.background.as_ref() {
            Some(BackgroundValue::Color(value)) if colors.resolve(value).is_none() => {
                Some(value.as_str())
            }
            _ => None,
        };
        let inputs = crate::css::background::Inputs {
            shorthand,
            image: self.background_image.as_deref(),
            size: self.background_size.as_deref(),
            position: self.background_position.as_deref(),
            repeat: self.background_repeat.as_deref(),
            origin: self.background_origin.as_deref(),
            clip: self.background_clip.as_deref(),
        };
        let parsed = crate::css::background::resolve(&inputs, colors);
        let color = if let Some(value) = self.background_color.as_deref() {
            colors.resolve(value).map(Into::into)
        } else if let Some(background) = self.background.as_ref() {
            match background {
                BackgroundValue::Color(value) => colors
                    .resolve(value)
                    .or(parsed.color)
                    .map(Into::into),
                gradient => gradient.resolve(colors),
            }
        } else {
            None
        };
        let mut layers = parsed.layers;
        // GPUI clips the colour with the last layer's clip, so a colour that
        // `background-clip: text` confines becomes the bottom layer.
        let color = match color.as_ref().and_then(gpui::Background::as_solid) {
            Some(solid) if parsed.color_clip == gpui::BackgroundBox::Text => {
                layers.push(crate::css::background::solid_layer(solid, parsed.color_clip));
                None
            }
            _ => color,
        };
        Fill { color, layers }
    }
}

/// Map a CSS `cursor` keyword onto a GPUI cursor. Unknown keywords return
/// `None` so the property is ignored, like every other invalid style value.
///
/// `ResizeUpLeftDownRight` is the NorthWest/SouthEast cursor on every backend,
/// so it is `nwse-resize`. GPUI's doc comments and its browser backend named
/// the opposite CSS values until the pinned fork corrected them, so do not
/// "fix" this pair back by reading an older GPUI.
pub fn parse_cursor(name: &str) -> Option<gpui::CursorStyle> {
    use gpui::CursorStyle;
    Some(match name {
        "default" | "auto" => CursorStyle::Arrow,
        "pointer" => CursorStyle::PointingHand,
        "text" => CursorStyle::IBeam,
        "vertical-text" => CursorStyle::IBeamCursorForVerticalLayout,
        "crosshair" => CursorStyle::Crosshair,
        "grab" => CursorStyle::OpenHand,
        "grabbing" | "move" | "all-scroll" => CursorStyle::ClosedHand,
        "col-resize" => CursorStyle::ResizeColumn,
        "row-resize" => CursorStyle::ResizeRow,
        "ew-resize" => CursorStyle::ResizeLeftRight,
        "ns-resize" => CursorStyle::ResizeUpDown,
        "nwse-resize" | "nw-resize" | "se-resize" => CursorStyle::ResizeUpLeftDownRight,
        "nesw-resize" | "ne-resize" | "sw-resize" => CursorStyle::ResizeUpRightDownLeft,
        "w-resize" => CursorStyle::ResizeLeft,
        "e-resize" => CursorStyle::ResizeRight,
        "n-resize" => CursorStyle::ResizeUp,
        "s-resize" => CursorStyle::ResizeDown,
        "not-allowed" | "no-drop" => CursorStyle::OperationNotAllowed,
        "alias" => CursorStyle::DragLink,
        "copy" => CursorStyle::DragCopy,
        "context-menu" => CursorStyle::ContextualMenu,
        "none" => CursorStyle::None,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light() -> ColorContext {
        ColorContext::root(crate::color::Scheme::Light)
    }

    #[test]
    fn box_shadow_takes_a_css_string() {
        let style: StyleDesc = serde_json::from_str(
            r##"{"boxShadow":"0 1px 2px rgba(0,0,0,.2), inset 0 0 0 1px red"}"##,
        )
        .unwrap();
        let shadows = style.box_shadow.as_ref().unwrap().as_slice();
        assert_eq!(shadows.len(), 2);
        assert_eq!(shadows[0].offset_y, 1.0);
        assert_eq!(shadows[0].color, "rgba(0,0,0,.2)");
        assert!(shadows[1].inset);
        assert_eq!(shadows[1].spread_radius, 1.0);
    }

    #[test]
    fn an_invalid_box_shadow_string_paints_nothing_and_keeps_the_rest_of_the_style() {
        let style: StyleDesc =
            serde_json::from_str(r##"{"boxShadow":"4 4 red","color":"red"}"##).unwrap();
        assert!(style.box_shadow.as_ref().unwrap().as_slice().is_empty());
        assert_eq!(style.color.as_deref(), Some("red"));
    }

    #[test]
    fn group_names_and_the_default_group() {
        let style: StyleDesc = serde_json::from_str(
            r##"{"group":true,"groupHover":{"backgroundColor":"red"},"groupActive":{"group":"card","color":"blue"}}"##,
        )
        .unwrap();
        let default = style.declared_group().unwrap();
        // An unnamed groupHover follows the default group, which `true` declares.
        assert_eq!(style.group_hover.as_ref().unwrap().followed_group(), default);
        assert_eq!(style.group_active.as_ref().unwrap().followed_group().as_ref(), "card");

        let named: StyleDesc = serde_json::from_str(r##"{"group":"card"}"##).unwrap();
        assert_eq!(named.declared_group().unwrap().as_ref(), "card");
        for none in [r##"{"group":false}"##, r##"{"group":""}"##, r##"{}"##] {
            let style: StyleDesc = serde_json::from_str(none).unwrap();
            assert_eq!(style.declared_group(), None, "{none}");
        }
    }

    #[test]
    fn a_state_that_sets_color_repaints_the_current_color_followers() {
        let base: StyleDesc = serde_json::from_str(
            r##"{"borderColor":"currentColor","backgroundColor":"#fff","outlineColor":"color-mix(in srgb, currentcolor, red)","outlineWidth":2,"boxShadow":{"offsetX":0,"offsetY":0,"blurRadius":0,"spreadRadius":1,"color":"currentColor"}}"##,
        )
        .unwrap();
        let hover = StyleDesc {
            color: Some("red".to_owned()),
            ..Default::default()
        };
        let followers = base.colors_following_current(&hover).unwrap();
        assert_eq!(followers.border_color.as_deref(), Some("currentColor"));
        assert_eq!(followers.background_color, None);
        assert_eq!(followers.outline_width, Some(2.0));
        assert!(followers.outline_color.is_some());

        let overriding = StyleDesc {
            border_color: Some("blue".to_owned()),
            ..hover.clone()
        };
        assert_eq!(
            base.colors_following_current(&overriding).unwrap().border_color,
            None
        );
        // Without a new colour nothing changes, so nothing is repainted.
        assert_eq!(base.colors_following_current(&StyleDesc::default()), None);
    }

    #[test]
    fn resolves_a_two_stop_linear_gradient() {
        let style: StyleDesc = serde_json::from_str(
            r##"{"background":{"type":"linear-gradient","angle":90,"stops":[{"color":"#ff0000","position":0},{"color":"#0000ff","position":1}],"colorSpace":"oklab"}}"##,
        )
        .unwrap();
        let expected = gpui::linear_gradient(
            90.0,
            gpui::linear_color_stop(crate::color::parse_color_rgba("#ff0000").unwrap(), 0.0),
            gpui::linear_color_stop(crate::color::parse_color_rgba("#0000ff").unwrap(), 1.0),
        )
        .color_space(gpui::ColorSpace::Oklab);

        assert_eq!(style.resolved_fill(&light()).color, Some(expected));
    }

    #[test]
    fn box_shadow_takes_one_shadow_or_a_list() {
        let one: StyleDesc = serde_json::from_str(
            r##"{"boxShadow":{"offsetX":0,"offsetY":4,"blurRadius":12,"spreadRadius":0,"color":"#00000033"}}"##,
        )
        .unwrap();
        let shadows = one.box_shadow.as_ref().unwrap().as_slice();
        assert_eq!(shadows.len(), 1);
        assert!(
            !shadows[0].inset,
            "a shadow is outside unless it says otherwise"
        );

        let many: StyleDesc = serde_json::from_str(
            r##"{"boxShadow":[
                {"offsetX":0,"offsetY":1,"blurRadius":2,"spreadRadius":0,"color":"#0003"},
                {"offsetX":0,"offsetY":0,"blurRadius":0,"spreadRadius":1,"color":"#fff1","inset":true}
            ]}"##,
        )
        .unwrap();
        let shadows = many.box_shadow.as_ref().unwrap().as_slice();
        assert_eq!(shadows.len(), 2);
        assert!(shadows[1].inset);
        assert_eq!(shadows[1].spread_radius, 1.0);
    }

    #[test]
    fn maps_the_timeline_cursors() {
        assert_eq!(
            parse_cursor("col-resize"),
            Some(gpui::CursorStyle::ResizeColumn)
        );
        assert_eq!(parse_cursor("grab"), Some(gpui::CursorStyle::OpenHand));
        assert_eq!(
            parse_cursor("grabbing"),
            Some(gpui::CursorStyle::ClosedHand)
        );
        assert_eq!(
            parse_cursor("pointer"),
            Some(gpui::CursorStyle::PointingHand)
        );
        assert_eq!(parse_cursor("default"), Some(gpui::CursorStyle::Arrow));
    }

    #[test]
    fn cursor_none_hides_the_cursor() {
        assert_eq!(parse_cursor("none"), Some(gpui::CursorStyle::None));
    }

    #[test]
    fn ignores_an_unknown_cursor() {
        assert_eq!(parse_cursor("zoom-in"), None);
        assert_eq!(parse_cursor("POINTER"), None);
    }

    #[test]
    fn deserializes_text_decoration() {
        let style: StyleDesc = serde_json::from_str(r#"{"textDecoration":"underline"}"#).unwrap();
        assert_eq!(style.text_decoration.as_deref(), Some("underline"));

        let style: StyleDesc =
            serde_json::from_str(r#"{"textDecoration":"line-through"}"#).unwrap();
        assert_eq!(style.text_decoration.as_deref(), Some("line-through"));

        let style: StyleDesc = serde_json::from_str(r#"{"textDecoration":"none"}"#).unwrap();
        assert_eq!(style.text_decoration.as_deref(), Some("none"));
    }
}
