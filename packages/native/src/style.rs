use serde::{Deserialize, Deserializer, Serialize};

use crate::css::dimension::parse_linear_length;
use crate::css::length::{LengthContext, LinearLength};
use crate::css::shorthand::{self, Gap, Sides};
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
    #[serde(deserialize_with = "deserialize_shadow_length")]
    pub offset_x: LinearLength,
    #[serde(deserialize_with = "deserialize_shadow_length")]
    pub offset_y: LinearLength,
    #[serde(deserialize_with = "deserialize_shadow_length")]
    pub blur_radius: LinearLength,
    #[serde(deserialize_with = "deserialize_shadow_length")]
    pub spread_radius: LinearLength,
    pub color: String,
    /// Draws inside the border box instead of around it, like CSS `inset`.
    #[serde(default)]
    pub inset: bool,
}

/// `boxShadow` is one shadow or a list of them, painted in the order given.
/// JS may also send the CSS string, which is parsed into the same layers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, try_from = "BoxShadowInput")]
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

impl BoxShadowInput {
    /// `None` for a CSS string that is not a valid `box-shadow`.
    fn into_values(self) -> Option<BoxShadowValues> {
        match self {
            Self::Css(css) => crate::css::box_shadow::parse(&css).map(BoxShadowValues::Many),
            Self::Many(layers) => Some(BoxShadowValues::Many(layers)),
            Self::One(layer) => Some(BoxShadowValues::One(layer)),
        }
    }
}

impl TryFrom<BoxShadowInput> for BoxShadowValues {
    type Error = &'static str;

    fn try_from(input: BoxShadowInput) -> Result<Self, Self::Error> {
        input.into_values().ok_or("not a valid box-shadow")
    }
}

/// A style's `boxShadow`. An invalid CSS string leaves it unset, as CSS drops
/// an invalid declaration, so the base style's shadow still applies under a
/// state style that gets it wrong.
fn deserialize_box_shadow<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<BoxShadowValues>, D::Error> {
    Ok(Option::<BoxShadowInput>::deserialize(deserializer)?.and_then(BoxShadowInput::into_values))
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NumberOrText {
    Number(f64),
    Text(String),
}

/// A CSS string, or a number that `unit` turns into one.
fn text_or_number<'de, D: Deserializer<'de>>(deserializer: D, unit: &str) -> Result<Option<String>, D::Error> {
    Ok(Option::<NumberOrText>::deserialize(deserializer)?.map(|value| match value {
        NumberOrText::Number(number) => format!("{number}{unit}"),
        NumberOrText::Text(text) => text,
    }))
}

fn deserialize_degrees<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    text_or_number(deserializer, "deg")
}

fn deserialize_factor<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    text_or_number(deserializer, "")
}

fn deserialize_angle<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    Option::<NumberOrText>::deserialize(deserializer)?
        .map(|value| match value {
            NumberOrText::Number(degrees) => Ok(degrees),
            NumberOrText::Text(text) => crate::css::transform::parse_degrees(&text)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid angle \"{text}\": expected a number of degrees or a CSS angle"))),
        })
        .transpose()
}

/// A number, or a string of one number or percentage, as the factor of `scale`.
fn deserialize_scale_factor<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    Option::<NumberOrText>::deserialize(deserializer)?
        .map(|value| match value {
            NumberOrText::Number(factor) => Ok(factor),
            NumberOrText::Text(text) => crate::css::transform::parse_factor(&text)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid scale \"{text}\": expected a number or a percentage"))),
        })
        .transpose()
}

fn deserialize_origin<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<LinearLength>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Origin {
        Fraction(f64),
        Length(LinearLength),
    }

    Ok(Option::<Origin>::deserialize(deserializer)?.map(|origin| match origin {
        Origin::Fraction(fraction) => LinearLength { percent: fraction, ..LinearLength::default() },
        Origin::Length(length) => length,
    }))
}

fn deserialize_shadow_length<'de, D: Deserializer<'de>>(deserializer: D) -> Result<LinearLength, D::Error> {
    crate::css::length::deserialize_with(
        deserializer,
        "box shadow length",
        "a CSS length without %",
        crate::css::dimension::parse_definite_length,
    )
}

/// `Option<LinearLength>` with a narrower grammar than a plain length.
macro_rules! length_field {
    ($name:ident, $what:literal, $rule:literal, $parse:path) => {
        fn $name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<LinearLength>, D::Error> {
            struct Field(LinearLength);

            impl<'de> Deserialize<'de> for Field {
                fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                    crate::css::length::deserialize_with(deserializer, $what, $rule, $parse).map(Field)
                }
            }

            Ok(Option::<Field>::deserialize(deserializer)?.map(|field| field.0))
        }
    };
}

/// A `DimensionValue` field that is not a size: an intrinsic size keyword is not a valid
/// margin or inset.
macro_rules! dimension_field {
    ($name:ident, $property:literal, $reason:expr) => {
        fn $name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<DimensionValue>, D::Error> {
            match Option::<DimensionValue>::deserialize(deserializer)? {
                Some(DimensionValue::Intrinsic(keyword)) => {
                    Err(serde::de::Error::custom(format!("invalid {} \"{keyword}\": {}", $property, $reason)))
                }
                other => Ok(other),
            }
        }
    };
}

pub(crate) const NOT_A_SIZE: &str = "only width, height and their min and max take min-content, max-content and fit-content";

dimension_field!(deserialize_margin, "margin", NOT_A_SIZE);
dimension_field!(deserialize_inset, "inset", NOT_A_SIZE);

/// `margin` and `inset`: one to four dimensions, or a number of px.
fn dimension_sides<'de, D: Deserializer<'de>>(
    deserializer: D,
    property: &'static str,
) -> Result<Option<Sides<DimensionValue>>, D::Error> {
    let token = |token: &str| match DimensionValue::parse(token)? {
        DimensionValue::Intrinsic(_) => Err(format!("invalid {property} \"{token}\": {NOT_A_SIZE}")),
        value => Ok(value),
    };
    shorthand::deserialize_optional(
        deserializer,
        "a number of px or a string of one to four CSS lengths or auto",
        |value| Ok(Sides::all(DimensionValue::Pixels(value))),
        |text| shorthand::parse_sides(text, token),
    )
}

fn deserialize_margin_sides<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Sides<DimensionValue>>, D::Error> {
    dimension_sides(deserializer, "margin")
}

fn deserialize_inset_sides<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Sides<DimensionValue>>, D::Error> {
    dimension_sides(deserializer, "inset")
}

fn deserialize_padding<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Sides<LinearLength>>, D::Error> {
    shorthand::deserialize_length_sides(
        deserializer,
        "padding",
        "a CSS length with a unit, or calc()",
        parse_linear_length,
    )
}

fn deserialize_border_widths<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Sides<LinearLength>>, D::Error> {
    shorthand::deserialize_length_sides(
        deserializer,
        "border width",
        "a CSS length without %, or thin, medium or thick",
        crate::css::border::parse_width,
    )
}

length_field!(
    deserialize_border_width,
    "border width",
    "a CSS length without %, or thin, medium or thick",
    crate::css::border::parse_width
);
length_field!(
    deserialize_outline_offset,
    "outline offset",
    "a CSS length without %",
    crate::css::border::parse_offset
);

impl BoxShadowValues {
    pub fn as_slice(&self) -> &[BoxShadowValue] {
        match self {
            Self::Many(shadows) => shadows,
            Self::One(shadow) => std::slice::from_ref(shadow),
        }
    }
}

/// `borderRadius`: a number of px or a CSS string, parsed when the style arrives
/// so an invalid one is an error at the commit and not a missing corner. The lengths
/// resolve when the style is applied ([`CornerRadiusValue::resolve`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderRadiusValue(pub gpui::Corners<CornerRadiusValue>);

/// A corner longhand such as `borderTopLeftRadius`: the horizontal radius and, for an
/// elliptical corner, the vertical one. A percentage is of the border box on its own axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CornerRadiusValue {
    pub x: LinearLength,
    /// `None` is the same as `x`.
    pub y: Option<LinearLength>,
}

impl StyleDesc {
    /// Whether the style, or a focus variant that may be laid over it, declares a `z-index`, which
    /// is the only property of a style that an item of a flex or grid container reads differently.
    pub(crate) fn declares_z_index(&self) -> bool {
        self.z_index.is_some()
            || [&self.focus_within, &self.has_focus_visible, &self.focus_visible]
                .into_iter()
                .any(|variant| variant.as_deref().is_some_and(|variant| variant.z_index.is_some()))
    }

    /// Whether gpui's layout rewrites a box with this style after it is requested, whatever is
    /// around it: a width is a keyword of its content. (An absolutely positioned box is rewritten
    /// against its containing block, see `Retain::containing_block`.)
    pub(crate) fn lays_out_unstably(&self) -> bool {
        let keyword = |value: &Option<DimensionValue>| matches!(value, Some(DimensionValue::Intrinsic(_)));
        self.measure_natural_width
            || keyword(&self.width)
            || keyword(&self.min_width)
            || keyword(&self.max_width)
            || keyword(&self.flex_basis)
    }

    /// The four corners once the corner longhands are laid over `borderRadius`;
    /// a corner neither sets is square.
    pub(crate) fn corner_radii(&self) -> gpui::Corners<CornerRadiusValue> {
        let mut corners = self.border_radius.map(|radii| radii.0).unwrap_or_default();
        if let Some(radius) = self.border_top_left_radius {
            corners.top_left = radius;
        }
        if let Some(radius) = self.border_top_right_radius {
            corners.top_right = radius;
        }
        if let Some(radius) = self.border_bottom_right_radius {
            corners.bottom_right = radius;
        }
        if let Some(radius) = self.border_bottom_left_radius {
            corners.bottom_left = radius;
        }
        corners
    }

    /// [`Self::corner_radii`] for an element with `lengths`.
    pub(crate) fn resolved_corner_radii(
        &self,
        lengths: &crate::css::length::LengthContext,
    ) -> gpui::Corners<gpui::CornerRadius> {
        let corners = self.corner_radii();
        gpui::Corners {
            top_left: corners.top_left.resolve(lengths),
            top_right: corners.top_right.resolve(lengths),
            bottom_right: corners.bottom_right.resolve(lengths),
            bottom_left: corners.bottom_left.resolve(lengths),
        }
    }
}

impl BorderRadiusValue {
    /// A circular radius of `value` px on every corner.
    pub fn px(value: f64) -> Self {
        Self(CornerRadiusValue::px(value).on_all_corners())
    }
}

impl CornerRadiusValue {
    /// A circular radius of `value` px.
    pub fn px(value: f64) -> Self {
        Self::circular(LinearLength::px(value))
    }

    pub(crate) fn circular(radius: LinearLength) -> Self {
        Self { x: radius, y: None }
    }

    fn on_all_corners(self) -> gpui::Corners<Self> {
        gpui::Corners { top_left: self, top_right: self, bottom_right: self, bottom_left: self }
    }
}

/// A number or a string, handed to the parser that knows the property.
struct RadiusVisitor<T> {
    from_number: fn(f64) -> Result<T, String>,
    from_css: fn(&str) -> Result<T, String>,
}

impl<'de, T> serde::de::Visitor<'de> for RadiusVisitor<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a number or a CSS border-radius string")
    }

    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<T, E> {
        (self.from_number)(value).map_err(E::custom)
    }

    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<T, E> {
        self.visit_f64(value as f64)
    }

    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<T, E> {
        self.visit_f64(value as f64)
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<T, E> {
        (self.from_css)(value).map_err(E::custom)
    }
}

impl<'de> Deserialize<'de> for BorderRadiusValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(RadiusVisitor {
            from_number: |value| {
                crate::css::border_radius::corner_from_number(value).map(|corner| Self(corner.on_all_corners()))
            },
            from_css: |value| crate::css::border_radius::parse_border_radius(value).map(Self),
        })
    }
}

impl<'de> Deserialize<'de> for CornerRadiusValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(RadiusVisitor {
            from_number: crate::css::border_radius::corner_from_number,
            from_css: crate::css::border_radius::parse_corner,
        })
    }
}

impl Serialize for BorderRadiusValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        crate::css::border_radius::border_radius_to_json(&self.0).serialize(serializer)
    }
}

impl Serialize for CornerRadiusValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
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

/// A dimension value that can be a number (pixels) or a string (any CSS length
/// unit, a percentage, `calc()`, auto, etc.)
#[derive(Debug, Clone, PartialEq)]
pub enum DimensionValue {
    Pixels(f64),
    Percentage(f64), // 0.0 to 1.0
    /// A length that is neither plain px nor a plain percentage (`2em`, `50vw`,
    /// `calc(100% - 20px)`). It resolves against the element when the tree is
    /// applied, so motion cannot run from or to it.
    Calc(LinearLength),
    Auto,
    /// `min-content`, `max-content`, `fit-content` and `fit-content(<length>)`, and `content`.
    /// Only the six size properties and `flexBasis` take one (`content` only `flexBasis`); the
    /// others reject it when they are deserialized.
    Intrinsic(IntrinsicKeyword),
    /// `anchor-size(width)` / `anchor-size(height)`: the trigger of the enclosing
    /// `<anchored>`. A direct child's sizes, margins and insets resolve it (`layout/anchor_size.rs`);
    /// no other property does.
    AnchorSize(AnchorAxis),
}

/// An intrinsic size keyword of CSS Sizing 3, or `content`, which only `flex-basis` has.
#[derive(Debug, Clone, PartialEq)]
pub enum IntrinsicKeyword {
    /// `flex-basis: content`: the size of the content, ignoring `width` and `height`.
    /// [`IntrinsicKeyword::parse`] never reads it; only [`DimensionValue::parse_flex_basis`] does.
    Content,
    MinContent,
    MaxContent,
    FitContent,
    /// `fit-content(<length-percentage>)`: never negative.
    FitContentLimit(LinearLength),
}

impl IntrinsicKeyword {
    /// `None` when `text` is not one of the keywords; an `Err` when it is
    /// `fit-content(...)` with an argument that is not a non-negative length.
    fn parse(text: &str) -> Option<Result<Self, String>> {
        let text = text.trim();
        let lower = text.to_ascii_lowercase();
        Some(match lower.as_str() {
            "min-content" => Ok(Self::MinContent),
            "max-content" => Ok(Self::MaxContent),
            "fit-content" => Ok(Self::FitContent),
            _ => {
                let argument = lower.strip_prefix("fit-content(")?.strip_suffix(')')?;
                match parse_linear_length(argument.trim()) {
                    Some(limit) if !limit.is_negative() => Ok(Self::FitContentLimit(limit)),
                    _ => Err(format!(
                        "invalid fit-content: {text}: the limit is a non-negative length or percentage"
                    )),
                }
            }
        })
    }
}

impl std::fmt::Display for IntrinsicKeyword {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Content => formatter.write_str("content"),
            Self::MinContent => formatter.write_str("min-content"),
            Self::MaxContent => formatter.write_str("max-content"),
            Self::FitContent => formatter.write_str("fit-content"),
            Self::FitContentLimit(limit) => write!(formatter, "fit-content({limit})"),
        }
    }
}

impl Serialize for IntrinsicKeyword {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum AnchorAxis {
    Width,
    Height,
}

impl DimensionValue {
    /// `text` is what `length` was parsed from: a zero length has forgotten
    /// whether it was `0px` or `0%`, and only the percentage resolves against a
    /// containing block.
    fn from_length(length: LinearLength, text: &str) -> Self {
        if length.is_zero() {
            return if text.trim_end().ends_with('%') {
                DimensionValue::Percentage(0.0)
            } else {
                DimensionValue::Pixels(0.0)
            };
        }
        if let Some(px) = length.as_px() {
            DimensionValue::Pixels(px)
        } else if let Some(fraction) = length.as_percent() {
            DimensionValue::Percentage(fraction)
        } else {
            DimensionValue::Calc(length)
        }
    }
}

impl DimensionValue {
    /// One dimension written as a string: a length, `auto`, an intrinsic size
    /// keyword or `anchor-size()`.
    pub(crate) fn parse(v: &str) -> Result<Self, String> {
        if v == "auto" {
            Ok(DimensionValue::Auto)
        } else if let Some(keyword) = IntrinsicKeyword::parse(v) {
            keyword.map(DimensionValue::Intrinsic)
        } else if let Some(axis) = v.strip_prefix("anchor-size(").and_then(|rest| rest.strip_suffix(')')) {
            match axis.trim() {
                "width" => Ok(DimensionValue::AnchorSize(AnchorAxis::Width)),
                "height" => Ok(DimensionValue::AnchorSize(AnchorAxis::Height)),
                _ => Err(format!("invalid anchor-size: {v}")),
            }
        } else if let Ok(number) = v.parse::<f64>() {
            if number.is_finite() {
                Ok(DimensionValue::Pixels(number))
            } else {
                Err(format!("invalid dimension: {v}"))
            }
        } else {
            parse_linear_length(v)
                .map(|length| DimensionValue::from_length(length, v))
                .ok_or_else(|| format!("invalid dimension: {v}"))
        }
    }
}

impl DimensionValue {
    /// A `<'flex-basis'>`: [`DimensionValue::parse`], and `content`.
    pub(crate) fn parse_flex_basis(v: &str) -> Result<Self, String> {
        if v.trim().eq_ignore_ascii_case("content") {
            Ok(DimensionValue::Intrinsic(IntrinsicKeyword::Content))
        } else {
            Self::parse(v)
        }
    }
}

/// The CSS the dimension parses back from.
impl std::fmt::Display for DimensionValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pixels(pixels) => LinearLength::px(*pixels).fmt(formatter),
            // A zero length has forgotten whether it was `0px` or `0%`, and only `0%` resolves
            // against a containing block.
            Self::Percentage(fraction) if *fraction == 0.0 => formatter.write_str("0%"),
            Self::Percentage(fraction) => LinearLength::percent(*fraction).fmt(formatter),
            Self::Calc(length) => length.fmt(formatter),
            Self::Auto => formatter.write_str("auto"),
            Self::Intrinsic(keyword) => keyword.fmt(formatter),
            Self::AnchorSize(AnchorAxis::Width) => formatter.write_str("anchor-size(width)"),
            Self::AnchorSize(AnchorAxis::Height) => formatter.write_str("anchor-size(height)"),
        }
    }
}

/// A number of px stays a number; everything else is its CSS, which is what
/// [`DimensionValue::parse`] reads back (`auto` and a percentage included).
impl Serialize for DimensionValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Pixels(pixels) => serializer.serialize_f64(*pixels),
            other => serializer.collect_str(other),
        }
    }
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
        deserializer.deserialize_any(DimensionVisitor { flex_basis: false })
    }
}

/// A `flexBasis`, which is a [`DimensionValue`] that also reads `content`.
fn deserialize_flex_basis<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<DimensionValue>, D::Error> {
    struct FlexBasis(DimensionValue);

    impl<'de> Deserialize<'de> for FlexBasis {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            deserializer.deserialize_any(DimensionVisitor { flex_basis: true }).map(FlexBasis)
        }
    }

    Ok(Option::<FlexBasis>::deserialize(deserializer)?.map(|basis| basis.0))
}

struct DimensionVisitor {
    flex_basis: bool,
}

impl<'de> serde::de::Visitor<'de> for DimensionVisitor {
    type Value = DimensionValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a number of px or a CSS length string like '100%', '2em' or 'calc(100% - 20px)', or 'auto', \
             'min-content', 'max-content' or 'fit-content'")
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
        Ok(DimensionValue::Pixels(v))
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(DimensionValue::Pixels(v as f64))
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(DimensionValue::Pixels(v as f64))
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        if self.flex_basis { DimensionValue::parse_flex_basis(v) } else { DimensionValue::parse(v) }
            .map_err(serde::de::Error::custom)
    }
}

/// Style description that can be serialized from JS
/// Note: This is only used for JSON deserialization, not direct napi binding
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", remote = "Self")]
pub struct StyleDesc {
    // Display
    pub display: Option<String>,
    pub visibility: Option<String>,

    // Flexbox
    pub flex_direction: Option<String>,
    pub flex_wrap: Option<String>,
    pub flex_grow: Option<f64>,
    pub flex_shrink: Option<f64>,
    /// A length (px, `%` of the container's main size, `em`, `calc()`, ...), `auto`, `content`, or
    /// an intrinsic size keyword. Along a column container's main axis the keywords are `content`.
    #[serde(default, deserialize_with = "deserialize_flex_basis")]
    pub flex_basis: Option<DimensionValue>,
    /// CSS `flex`: a number (`flex: 1` is `1 1 0%`), `none`, `auto`, `initial`, or
    /// `<grow> <shrink>? || <basis>`. `flexGrow`, `flexShrink` and `flexBasis` win over it.
    #[serde(default, deserialize_with = "crate::css::flex::deserialize_flex")]
    pub flex: Option<crate::css::flex::Flex>,
    /// CSS `flex-flow`: a direction and a wrap, in either order. `flexDirection` and
    /// `flexWrap` win over it.
    #[serde(default, deserialize_with = "crate::css::flex::deserialize_flex_flow")]
    pub flex_flow: Option<crate::css::flex::FlexFlow>,
    pub align_items: Option<String>,
    pub align_self: Option<String>,
    pub align_content: Option<String>,
    pub justify_content: Option<String>,
    /// A number is px; a string is `<row-gap> <column-gap>?`, each a CSS length. `%` is of the
    /// container's content box. `rowGap` and `columnGap` win over it.
    #[serde(default, deserialize_with = "shorthand::deserialize_gap")]
    pub gap: Option<Gap>,
    pub row_gap: Option<LinearLength>,
    pub column_gap: Option<LinearLength>,
    /// Stable-sorts the children of a flex or grid container. Default 0.
    pub order: Option<f64>,
    pub justify_items: Option<String>,
    pub justify_self: Option<String>,
    /// CSS `place-items`, `place-content` and `place-self`: `<align> <justify>?`, where one
    /// value is both. The `align-*` and `justify-*` longhands win over them.
    #[serde(default, deserialize_with = "crate::css::place::deserialize_place_items")]
    pub place_items: Option<crate::css::place::Place>,
    #[serde(default, deserialize_with = "crate::css::place::deserialize_place_content")]
    pub place_content: Option<crate::css::place::Place>,
    #[serde(default, deserialize_with = "crate::css::place::deserialize_place_self")]
    pub place_self: Option<crate::css::place::Place>,

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

    // Sizing: a number or a CSS length, `auto`, or an intrinsic size keyword. The keywords
    // apply to the width axis only: `height`, `minHeight` and `maxHeight` take them as `auto`.
    pub width: Option<DimensionValue>,
    pub height: Option<DimensionValue>,
    pub min_width: Option<DimensionValue>,
    pub min_height: Option<DimensionValue>,
    pub max_width: Option<DimensionValue>,
    pub max_height: Option<DimensionValue>,

    // Spacing (padding): px or a CSS length; `%` is of the containing block's width. The
    // shorthand takes one to four values, for the sides in CSS order.
    #[serde(default, deserialize_with = "deserialize_padding")]
    pub padding: Option<Sides<LinearLength>>,
    /// `padding-block` (top and bottom) and `padding-inline` (left and right): one or two
    /// values, start then end. They win over `padding`, and the side longhands over them.
    #[serde(default, deserialize_with = "crate::css::logical::deserialize_padding_block")]
    pub padding_block: Option<crate::css::logical::Pair<LinearLength>>,
    #[serde(default, deserialize_with = "crate::css::logical::deserialize_padding_inline")]
    pub padding_inline: Option<crate::css::logical::Pair<LinearLength>>,
    pub padding_top: Option<LinearLength>,
    pub padding_right: Option<LinearLength>,
    pub padding_bottom: Option<LinearLength>,
    pub padding_left: Option<LinearLength>,

    // Spacing (margin): px, a percentage of the containing block width, or "auto".
    #[serde(default, deserialize_with = "deserialize_margin_sides")]
    pub margin: Option<Sides<DimensionValue>>,
    /// `margin-block` (top and bottom) and `margin-inline` (left and right), as for `padding`.
    #[serde(default, deserialize_with = "crate::css::logical::deserialize_margin_block")]
    pub margin_block: Option<crate::css::logical::Pair<DimensionValue>>,
    #[serde(default, deserialize_with = "crate::css::logical::deserialize_margin_inline")]
    pub margin_inline: Option<crate::css::logical::Pair<DimensionValue>>,
    #[serde(default, deserialize_with = "deserialize_margin")]
    pub margin_top: Option<DimensionValue>,
    #[serde(default, deserialize_with = "deserialize_margin")]
    pub margin_right: Option<DimensionValue>,
    #[serde(default, deserialize_with = "deserialize_margin")]
    pub margin_bottom: Option<DimensionValue>,
    #[serde(default, deserialize_with = "deserialize_margin")]
    pub margin_left: Option<DimensionValue>,

    // Position
    pub position: Option<String>,
    /// Paint order among positioned siblings. Only applies when `position` is set.
    pub z_index: Option<f64>,
    /// One to four lengths or `auto` for `top`, `right`, `bottom` and `left` in CSS order. The
    /// four longhands win over it.
    #[serde(default, deserialize_with = "deserialize_inset_sides")]
    pub inset: Option<Sides<DimensionValue>>,
    /// `inset-block` (`top` and `bottom`) and `inset-inline` (`left` and `right`), as for `padding`.
    #[serde(default, deserialize_with = "crate::css::logical::deserialize_inset_block")]
    pub inset_block: Option<crate::css::logical::Pair<DimensionValue>>,
    #[serde(default, deserialize_with = "crate::css::logical::deserialize_inset_inline")]
    pub inset_inline: Option<crate::css::logical::Pair<DimensionValue>>,
    #[serde(default, deserialize_with = "deserialize_inset")]
    pub top: Option<DimensionValue>,
    #[serde(default, deserialize_with = "deserialize_inset")]
    pub right: Option<DimensionValue>,
    #[serde(default, deserialize_with = "deserialize_inset")]
    pub bottom: Option<DimensionValue>,
    #[serde(default, deserialize_with = "deserialize_inset")]
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
    /// A number is px; a string is one to four CSS lengths with no `%`, or `thin`, `medium`
    /// or `thick`, for the sides in CSS order. A negative width is zero.
    #[serde(default, deserialize_with = "deserialize_border_widths")]
    pub border_width: Option<Sides<LinearLength>>,
    #[serde(default, deserialize_with = "deserialize_border_width")]
    pub border_top_width: Option<LinearLength>,
    #[serde(default, deserialize_with = "deserialize_border_width")]
    pub border_right_width: Option<LinearLength>,
    #[serde(default, deserialize_with = "deserialize_border_width")]
    pub border_bottom_width: Option<LinearLength>,
    #[serde(default, deserialize_with = "deserialize_border_width")]
    pub border_left_width: Option<LinearLength>,
    /// One to four colours, like CSS `border-color`: `"red"`, `"red blue"`, ...
    pub border_color: Option<String>,
    pub border_top_color: Option<String>,
    pub border_right_color: Option<String>,
    pub border_bottom_color: Option<String>,
    pub border_left_color: Option<String>,
    /// CSS shorthands, `"1px solid red"`. The longhands above win over them.
    pub border: Option<String>,
    /// `border-block` (the top and bottom sides) and `border-inline` (left and right), written
    /// like `border`. They win over `border` and the side shorthands over them.
    pub border_block: Option<String>,
    pub border_inline: Option<String>,
    pub border_top: Option<String>,
    pub border_right: Option<String>,
    pub border_bottom: Option<String>,
    pub border_left: Option<String>,
    /// CSS `border-radius`: a number (px) or a string of one to four lengths or
    /// percentages, optionally followed by `/` and one to four vertical radii. The
    /// corner longhands win over it.
    pub border_radius: Option<BorderRadiusValue>,
    /// A number (px) or `"<horizontal> [<vertical>]"`.
    pub border_top_left_radius: Option<CornerRadiusValue>,
    pub border_top_right_radius: Option<CornerRadiusValue>,
    pub border_bottom_left_radius: Option<CornerRadiusValue>,
    pub border_bottom_right_radius: Option<CornerRadiusValue>,
    #[serde(default, deserialize_with = "deserialize_box_shadow")]
    pub box_shadow: Option<BoxShadowValues>,
    /// CSS `filter`, e.g. `"blur(4px) brightness(1.2)"`. Applied to the element and
    /// everything in it. An invalid string is ignored.
    pub filter: Option<String>,
    /// CSS `backdrop-filter`: the same functions, applied to what is behind the element.
    pub backdrop_filter: Option<String>,
    /// CSS `mask-image`: a comma list of gradients (and `repeating-` forms), `none`
    /// or `url()`. First is on top.
    pub mask_image: Option<String>,
    /// CSS `mask-size`, `-position`, `-repeat`, `-origin` and `-clip`. Each is a
    /// comma list that repeats to cover the layers.
    pub mask_size: Option<String>,
    pub mask_position: Option<String>,
    pub mask_repeat: Option<String>,
    pub mask_origin: Option<String>,
    pub mask_clip: Option<String>,
    /// CSS `mask-mode`: a comma list of `alpha`, `luminance` or `match-source`, and
    /// `mask-composite`: of `add`, `subtract`, `intersect` or `exclude`. Both repeat
    /// to cover the layers, and the bottom layer's operator is ignored.
    pub mask_mode: Option<String>,
    pub mask_composite: Option<String>,
    /// CSS `mask` shorthand. The longhands above replace its parts.
    pub mask: Option<String>,
    /// CSS `mask-border-source`: one gradient or `url()` image, or `none`. The border is a
    /// nine-slice of it around the border box that adds to the `mask-image` layers.
    pub mask_border_source: Option<String>,
    /// CSS `mask-border-slice`: one to four numbers or percentages and `fill`,
    /// `mask-border-width`: one to four lengths, percentages, numbers or `auto`,
    /// `mask-border-outset`: one to four lengths or numbers, `mask-border-repeat`: one or two of
    /// `stretch`, `repeat`, `round` and `space`, and `mask-border-mode`: `alpha` or `luminance`.
    pub mask_border_slice: Option<String>,
    pub mask_border_width: Option<String>,
    pub mask_border_outset: Option<String>,
    pub mask_border_repeat: Option<String>,
    pub mask_border_mode: Option<String>,
    /// CSS `mask-border` shorthand. The longhands above replace its parts.
    pub mask_border: Option<String>,
    /// CSS `clip-path`: `inset()`, `rect()`, `xywh()`, `circle()`, `ellipse()`, `polygon()` or `path()` and an
    /// optional reference box, or `none`. An invalid string is ignored.
    pub clip_path: Option<String>,
    /// CSS `transform`: a list of `translate()`, `scale()`, `rotate()`, `skew()` and `matrix()`
    /// functions (and their `X`/`Y`/`Z`/`3d` forms, whose z does nothing), or `none`. An invalid
    /// string is ignored.
    pub transform: Option<String>,
    /// CSS `translate`: `none` or one or two lengths or percentages.
    pub translate: Option<String>,
    /// CSS `rotate`: `none` or an angle. A number is degrees.
    #[serde(default, deserialize_with = "deserialize_degrees")]
    pub rotate: Option<String>,
    /// CSS `scale`: `none` or one or two numbers or percentages. A number is the factor.
    #[serde(default, deserialize_with = "deserialize_factor")]
    pub scale: Option<String>,
    /// CSS `transform-origin`: one or two keywords, lengths or percentages. `50% 50%` by default.
    pub transform_origin: Option<String>,
    /// Motion's transform keys, sent by `motion.*` components only (see `css/transform.rs`): a
    /// number is px, a string a CSS length (`%` is of the border box).
    pub x: Option<LinearLength>,
    pub y: Option<LinearLength>,
    pub translate_x: Option<LinearLength>,
    pub translate_y: Option<LinearLength>,
    /// Motion's `scale` and `rotate`, under other names so that they do not meet the CSS `scale`
    /// and `rotate` above. The motion components rename the keys of their style.
    #[serde(default, deserialize_with = "deserialize_scale_factor")]
    pub motion_scale: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_scale_factor")]
    pub scale_x: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_scale_factor")]
    pub scale_y: Option<f64>,
    /// An angle: a number is degrees, a string a CSS angle.
    #[serde(default, deserialize_with = "deserialize_angle")]
    pub motion_rotate: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_angle")]
    pub rotate_z: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_angle")]
    pub skew: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_angle")]
    pub skew_x: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_angle")]
    pub skew_y: Option<f64>,
    /// Motion's `originX` and `originY`: a number is a fraction of the border box, a string a CSS
    /// length or percentage. They replace the axis of `transformOrigin`.
    #[serde(default, deserialize_with = "deserialize_origin")]
    pub origin_x: Option<LinearLength>,
    #[serde(default, deserialize_with = "deserialize_origin")]
    pub origin_y: Option<LinearLength>,
    /// CSS `border-style`: one to four of `none`, `hidden`, `dotted`, `dashed`, `solid`,
    /// `double`, `groove`, `ridge`, `inset` and `outset`, for the sides like `border-color`.
    /// `none` and `hidden` leave the side without a width. An invalid string is ignored.
    pub border_style: Option<String>,
    pub border_top_style: Option<String>,
    pub border_right_style: Option<String>,
    pub border_bottom_style: Option<String>,
    pub border_left_style: Option<String>,
    /// Width divided by height, applied to whichever axis is not definite.
    /// A number or a CSS ratio string such as `"16 / 9"`.
    pub aspect_ratio: Option<crate::layout::aspect_ratio::AspectRatio>,
    /// CSS `outline`: a line outside the border box that takes no layout
    /// space, so it can appear on focus without moving anything.
    ///
    /// `outlineWidth` takes what a border width does and `outlineOffset` a length
    /// with no `%`.
    #[serde(default, deserialize_with = "deserialize_border_width")]
    pub outline_width: Option<LinearLength>,
    /// CSS `outline-style`: `none` (the default, which paints no outline), `auto` (painted as
    /// `solid`), `dotted`, `dashed`, `solid`, `double`, `groove`, `ridge`, `inset` or `outset`.
    /// An invalid string is ignored.
    pub outline_style: Option<String>,
    pub outline_color: Option<String>,
    /// CSS `outline`: `<width> || <style> || <color>`. `outlineWidth`, `outlineStyle` and
    /// `outlineColor` win over it.
    #[serde(default, deserialize_with = "crate::css::outline::deserialize_outline")]
    pub outline: Option<crate::css::outline::Outline>,
    #[serde(default, deserialize_with = "deserialize_outline_offset")]
    pub outline_offset: Option<LinearLength>,

    // Text
    /// A number is px; a string is a CSS length. `em` and `%` are relative to the
    /// parent's font size, `rem` to the root's. Inherited.
    pub font_size: Option<LinearLength>,
    pub font_family: Option<String>,
    pub font_weight: Option<FontWeightValue>,
    /// CSS `font-style`: `normal`, `italic` or `oblique`. Inherited.
    pub font_style: Option<String>,
    pub text_align: Option<String>,
    /// CSS `direction`, inherited: mirrors flex rows, grids and tables, and decides which side
    /// `text-align: start` and `end` are. Not animatable.
    pub direction: Option<crate::css::direction::Direction>,
    /// CSS `line-height`: a number multiplies the element's font size, `"normal"`,
    /// or a length where `%` is of the font size. Inherited.
    pub line_height: Option<crate::css::line_height::LineHeight>,
    pub white_space: Option<String>,
    /// CSS `tab-size`: a number of spaces, or a length. Inherited, 8 by default.
    /// Tab stops are measured from the start of each line of text.
    pub tab_size: Option<crate::css::tab_size::TabSize>,
    /// CSS `text-box`: `normal`, or a trim and an edge in either order, a part
    /// left out being `trim-both` and `auto`. `textBoxTrim` and `textBoxEdge` are
    /// laid over it.
    pub text_box: Option<String>,
    /// CSS `text-box-trim`. Not inherited: it cuts the space above the first
    /// line and below the last line of this block container, and of the block
    /// containers inside it that have no padding or border on that side.
    pub text_box_trim: Option<crate::css::text_box::Trim>,
    /// CSS `text-box-edge`. Inherited, `auto` (the same as `text`) by default.
    pub text_box_edge: Option<crate::css::text_box::Edge>,
    /// CSS `word-break`. Inherited, `normal` by default. Only a container sets
    /// it: a paragraph is laid out as one string with one value.
    pub word_break: Option<crate::css::word_break::WordBreak>,
    /// CSS `overflow-wrap`. Inherited, `normal` by default: a word longer than
    /// its line overflows it. Only a container sets it.
    pub overflow_wrap: Option<crate::css::word_break::OverflowWrap>,
    /// CSS `text-wrap`, the shorthand of `text_wrap_mode` and `text_wrap_style`.
    /// Inherited. A longhand declared on the same element wins.
    pub text_wrap: Option<crate::css::text_wrap::TextWrap>,
    /// CSS `text-wrap-mode`: `nowrap` keeps the text on one line. Inherited, and wins
    /// over the wrap component of `white_space` on the same element. Only a
    /// container sets it.
    pub text_wrap_mode: Option<crate::css::text_wrap::TextWrapMode>,
    /// CSS `text-wrap-style`: `balance` and `pretty` choose among the breaks that
    /// make no more lines than greedy wrapping. Inherited. Only a container sets it.
    pub text_wrap_style: Option<crate::css::text_wrap::TextWrapStyle>,
    pub text_overflow: Option<String>,
    /// CSS `vertical-align`. Not inherited. A table cell reads it.
    pub vertical_align: Option<crate::css::vertical_align::VerticalAlign>,
    /// CSS `table-layout`. Not inherited.
    pub table_layout: Option<crate::css::table::TableLayout>,
    /// CSS `border-spacing`, inherited. Only a `display: table` element reads it.
    pub border_spacing: Option<crate::css::table::BorderSpacing>,
    /// CSS `border-collapse`, inherited. Parsed and handed to layout, but collapsed borders are
    /// not resolved or painted yet.
    pub border_collapse: Option<crate::css::table::BorderCollapse>,
    /// CSS `caption-side`, inherited: a caption reads the one of its table.
    pub caption_side: Option<crate::css::table::CaptionSide>,
    /// CSS `empty-cells`, inherited.
    pub empty_cells: Option<crate::css::table::EmptyCells>,
    pub line_clamp: Option<f64>,
    /// CSS `text-decoration` and its longhands. `css::text_decoration` says what
    /// is drawn.
    pub text_decoration: Option<String>,
    pub text_decoration_line: Option<String>,
    pub text_decoration_style: Option<String>,
    pub text_decoration_color: Option<String>,
    /// `auto`, `from-font` or a length.
    pub text_decoration_thickness: Option<String>,
    /// CSS `text-underline-offset`: `auto`, or a length or percentage (of `1em`).
    /// Inherited; the box that declares an underline decides where it sits.
    pub text_underline_offset: Option<crate::css::text_underline_offset::TextUnderlineOffset>,
    /// CSS `font-variant-numeric` keywords, e.g. `"tabular-nums slashed-zero"`.
    /// Inherited. Not supported inside `hover`/`active`/`focusVisible`.
    #[serde(default, deserialize_with = "crate::css::font_features::deserialize_variant_numeric")]
    pub font_variant_numeric: Option<String>,
    /// CSS `font-feature-settings`, e.g. `'"tnum" 1, "liga" 0'`. Inherited, and it
    /// wins over `fontVariantNumeric` on a feature both name.
    #[serde(default, deserialize_with = "crate::css::font_features::deserialize_feature_settings")]
    pub font_feature_settings: Option<String>,
    /// CSS `font-variation-settings`, e.g. `'"wght" 650, "wdth" 90'`. Inherited. An axis
    /// the font does not have is ignored, and one named here beats `fontWeight` on `wght`.
    /// A value CSS does not have is ignored, as `fontFeatureSettings` is.
    #[serde(default, deserialize_with = "crate::css::font_variations::deserialize_ignoring_invalid")]
    pub font_variation_settings: Option<crate::css::font_variations::VariationSettings>,
    /// CSS `-webkit-font-smoothing`: `auto`, `antialiased` or `subpixel-antialiased` (which is
    /// `auto`, as on current macOS). Inherited. A value CSS does not have is ignored.
    #[serde(
        rename = "WebkitFontSmoothing",
        default,
        deserialize_with = "crate::css::font_smoothing::deserialize_ignoring_invalid"
    )]
    pub webkit_font_smoothing: Option<crate::css::font_smoothing::FontSmoothing>,

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
    /// `"auto"` (default) or `"none"`: whether the box can be the anchor that keeps a scroller
    /// from moving what it shows when the layout above it changes. Not inherited, not animatable.
    /// A value CSS does not have is ignored.
    #[serde(default, deserialize_with = "crate::css::overflow_anchor::deserialize_ignoring_invalid")]
    pub overflow_anchor: Option<crate::css::overflow_anchor::OverflowAnchor>,

    // Cursor
    pub cursor: Option<String>,
    /// `"none"` makes the element never a pointer target; it inherits and a
    /// descendant's `"auto"` restores it, as in CSS.
    pub pointer_events: Option<String>,
    /// `"drag"` or `"no-drag"`: a window drag region of a frameless window. Inherited.
    pub app_region: Option<String>,

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
    pub transition_behavior: Option<String>,
    /// `numeric-only` or `allow-keywords`. Inherited, like the CSS property.
    pub interpolate_size: Option<String>,
    /// Set by the renderer on the copy of the style it builds an element from, never by JS: layout
    /// also measures the width this element has with `width: auto`.
    #[serde(skip)]
    pub measure_natural_width: bool,
    /// Set by the renderer on the copy of the style it builds an element from, never by JS: the
    /// element is an item of a flex or grid container, so its `z-index` applies without a
    /// `position`. Only set when it has a `z-index`.
    #[serde(skip)]
    pub flex_or_grid_item: bool,
    /// Set by the renderer on the copy of the style it builds an element from, never by JS: the
    /// element is an atomic inline, which the text decoration of the boxes around it does not reach.
    #[serde(skip)]
    pub atomic_inline: bool,
    /// Set by the renderer on the copy of the style it builds an element from, never by JS: an
    /// animation or a transition of `opacity` is current on the element (a CSS animation or
    /// transition, `motion`, a motion value that is moving), whatever opacity it samples. Chrome
    /// treats the element as `will-change: opacity` for the whole run (Web Animations, "side effects
    /// of animation"): it is an effect group, lifted at 0, at opacity 1 and 0 as well, and a group
    /// at opacity 0 still paints. A run that ends puts the element back to what its static opacity says.
    #[serde(skip)]
    pub animating_opacity: bool,
    /// Set by the renderer like `animating_opacity`, for an animation of the transform: the element
    /// has a transform, a stacking context and is a containing block, at every value it samples
    /// (`none` and the identity included), so a tick never flips them.
    #[serde(skip)]
    pub animating_transform: bool,

    // CSS animations: CSS syntax, as in a DOM inline style. The longhands win
    // over the `animation` shorthand. Names refer to `defineKeyframes`.
    pub animation: Option<String>,
    pub animation_name: Option<String>,
    pub animation_duration: Option<String>,
    pub animation_timing_function: Option<String>,
    pub animation_delay: Option<String>,
    pub animation_iteration_count: Option<String>,
    pub animation_direction: Option<String>,
    pub animation_fill_mode: Option<String>,
    pub animation_play_state: Option<String>,

    // Pseudo-selector styles — applied by GPUI natively (no JS round-trip).
    // Uses Box to avoid infinite-size struct (StyleDesc contains StyleDesc).
    pub hover: Option<Box<StyleDesc>>,
    pub active: Option<Box<StyleDesc>>,
    /// While focused after keyboard input, like CSS `:focus-visible`.
    pub focus_visible: Option<Box<StyleDesc>>,
    /// While this element or a descendant is focused, like CSS `:focus-within`.
    pub focus_within: Option<Box<StyleDesc>>,
    /// Makes this element a group that `groupHover`/`groupActive`/`groupFocusWithin`
    /// styles on its descendants react to. `true` is the default group, a string
    /// names one. Inside those styles it names the group to follow instead.
    pub group: Option<GroupName>,
    /// Applied while the nearest ancestor `group` of that name is hovered.
    pub group_hover: Option<Box<StyleDesc>>,
    /// Applied while the mouse is down inside the nearest ancestor `group` of that name.
    pub group_active: Option<Box<StyleDesc>>,
    /// Applied while the nearest ancestor `group` of that name, or anything inside
    /// it, has focus.
    pub group_focus_within: Option<Box<StyleDesc>>,
    /// While a strict descendant is hovered, like CSS `:has(:hover)`. The element
    /// being hovered itself does not count.
    pub has_hover: Option<Box<StyleDesc>>,
    /// While the mouse is down on a strict descendant, like CSS `:has(:active)`.
    pub has_active: Option<Box<StyleDesc>>,
    /// While a strict descendant is focused, like CSS `:has(:focus)`.
    pub has_focus: Option<Box<StyleDesc>>,
    /// While a strict descendant matches `:focus-visible`, like CSS `:has(:focus-visible)`.
    pub has_focus_visible: Option<Box<StyleDesc>>,
    /// While the element is a disabled form control, like CSS `:disabled`.
    pub disabled: Option<Box<StyleDesc>>,
    /// While the element is an enabled form control, like CSS `:enabled`.
    pub enabled: Option<Box<StyleDesc>>,
    /// While a strict descendant is a disabled form control, like CSS `:has(:disabled)`.
    pub has_disabled: Option<Box<StyleDesc>>,
    /// While a strict descendant is an enabled form control, like CSS `:has(:enabled)`.
    pub has_enabled: Option<Box<StyleDesc>>,
    /// `false` keeps this element at full opacity while another control has
    /// keyboard focus. Not inherited.
    pub keyboard_focus_dim: Option<bool>,

    // Containment. See `layout::container`.
    /// Makes the element a query container of `@container` blocks and `cq` units below it.
    /// `inline-size` and `size` also make the size on that axis independent of the content.
    pub container_type: Option<crate::layout::container::ContainerType>,
    /// The names a query can pick this container by, space separated.
    pub container_name: Option<crate::layout::container::ContainerNames>,
    /// `<name> [ / <type> ]`, the two longhands in one. The longhands win.
    pub container: Option<crate::layout::container::ContainerShorthand>,
    /// `visible` (default), `auto` or `hidden`. Anything but `visible` contains the element's style,
    /// layout and paint, and `hidden` (and `auto` while it is off screen) skips its content. See
    /// `layout::content_visibility`.
    pub content_visibility: Option<crate::layout::content_visibility::ContentVisibility>,
    /// `<axis> [<axis>]`, the width and then the height of a box with size containment that has no
    /// content to size it: a skipped one. An axis is `none`, a length or `auto <length>`.
    pub contain_intrinsic_size: Option<crate::css::contain_intrinsic::IntrinsicSize>,
    pub contain_intrinsic_width: Option<crate::css::contain_intrinsic::IntrinsicAxis>,
    pub contain_intrinsic_height: Option<crate::css::contain_intrinsic::IntrinsicAxis>,
    /// The logical longhands are the width and the height: the writing mode is horizontal.
    pub contain_intrinsic_inline_size: Option<crate::css::contain_intrinsic::IntrinsicAxis>,
    pub contain_intrinsic_block_size: Option<crate::css::contain_intrinsic::IntrinsicAxis>,
    /// Style blocks that apply while a container query is true, in written order, the later
    /// one winning. A block may carry state keys (`hover`, ...).
    #[serde(rename = "@container")]
    pub container_rules: Option<crate::layout::container::ContainerRules>,
    /// The axes of a query container that the container units in this style (blocks and state
    /// styles included) read. Set when the style is interned, never by JS.
    #[serde(skip)]
    pub container_unit_axes: crate::css::container_query::Axes,
    /// Counts the clones of this struct for the test renderer. Carries no data.
    #[serde(skip)]
    pub clone_probe: crate::perf_counters::CloneProbe<{ crate::perf_counters::STYLE_DESC }>,
    /// The `--name` keys of the style object. They are not fields of the struct: the parser in
    /// `css/covers.rs` collects them, and the derived serializer writes them back as keys.
    #[serde(flatten, skip_deserializing)]
    pub custom_properties: Option<crate::css::custom_properties::CustomProperties>,
    /// On the overlay a spliced child takes from its `display: contents` ancestors: what the
    /// ancestors pass down that no field of this style carries. Never from JS.
    #[serde(skip)]
    pub(crate) contents_levels: Option<std::sync::Arc<[crate::layout::contents::ContentsLevel]>>,
}

/// The derived deserialiser is wrapped so that a shorthand drops the longhands written before it:
/// see [`crate::css::covers`].
impl<'de> Deserialize<'de> for StyleDesc {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        crate::css::covers::deserialize_ordered(deserializer)
    }
}

impl Serialize for StyleDesc {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        StyleDesc::serialize(self, serializer)
    }
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
    /// Whether `other` differs from this style in nothing but keys that change what a box paints,
    /// so a window whose layout was current stays current after the change (see
    /// `RetainedTree::take_dirty`). The keys are those of `AnimProp::paints_only`, which is the
    /// list of what no layout, text measure, scroll overflow or box position reads, and the
    /// shorthand `borderColor`, which is four colours and nothing else. The `border` and `outline`
    /// shorthands carry widths, so they are not here. The transform keys (CSS and Motion's) are paint
    /// while the element keeps or lacks a transform, because having one makes it a containing block.
    ///
    /// Every other field, a state style (`hover`, ...) and a custom property included, is compared
    /// whole: a key that is added to the struct is a layout key until it is listed here.
    pub(crate) fn differs_only_in_paint(&self, other: &Self) -> bool {
        let mut painted = self.clone();
        painted.opacity = other.opacity;
        painted.box_shadow = other.box_shadow.clone();
        painted.color = other.color.clone();
        painted.background_color = other.background_color.clone();
        painted.border_color = other.border_color.clone();
        painted.border_top_color = other.border_top_color.clone();
        painted.border_right_color = other.border_right_color.clone();
        painted.border_bottom_color = other.border_bottom_color.clone();
        painted.border_left_color = other.border_left_color.clone();
        painted.outline_color = other.outline_color.clone();
        painted.text_decoration_color = other.text_decoration_color.clone();
        painted.scrollbar_color = other.scrollbar_color.clone();
        painted.selection_color = other.selection_color.clone();
        // Having a transform makes the element a containing block, which layout reads: only the
        // values of one that stays on or off are paint.
        let has_transform = |style: &Self| crate::css::transform::is_active(style, &LengthContext::initial(0.0, 0.0));
        if has_transform(self) == has_transform(other) {
            painted.transform = other.transform.clone();
            painted.translate = other.translate.clone();
            painted.rotate = other.rotate.clone();
            painted.scale = other.scale.clone();
            painted.transform_origin = other.transform_origin.clone();
            painted.x = other.x;
            painted.y = other.y;
            painted.translate_x = other.translate_x;
            painted.translate_y = other.translate_y;
            painted.motion_scale = other.motion_scale;
            painted.scale_x = other.scale_x;
            painted.scale_y = other.scale_y;
            painted.motion_rotate = other.motion_rotate;
            painted.rotate_z = other.rotate_z;
            painted.skew = other.skew;
            painted.skew_x = other.skew_x;
            painted.skew_y = other.skew_y;
            painted.origin_x = other.origin_x;
            painted.origin_y = other.origin_y;
        }
        painted == *other
    }

    /// Whether `other` has another transform, an origin or one of Motion's transform keys included: the
    /// keys `differs_only_in_paint` lets through for an element that keeps a transform. A change that
    /// is only of paint moves no box, but it moves what the boxes of the element and of what is
    /// inside it are seen through.
    pub(crate) fn touches_transform(&self, other: &Self) -> bool {
        (&self.transform, &self.translate, &self.rotate, &self.scale, &self.transform_origin)
            != (&other.transform, &other.translate, &other.rotate, &other.scale, &other.transform_origin)
            || (&self.x, &self.y, &self.translate_x, &self.translate_y, &self.origin_x, &self.origin_y)
                != (&other.x, &other.y, &other.translate_x, &other.translate_y, &other.origin_x, &other.origin_y)
            || (&self.motion_scale, &self.scale_x, &self.scale_y, &self.motion_rotate, &self.rotate_z, &self.skew, &self.skew_x, &self.skew_y)
                != (&other.motion_scale, &other.scale_x, &other.scale_y, &other.motion_rotate, &other.rotate_z, &other.skew, &other.skew_x, &other.skew_y)
    }

    /// Whether the style depends on the size of a query container: it has `@container` blocks
    /// or names a container unit.
    pub(crate) fn reads_containers(&self) -> bool {
        self.container_rules.is_some()
            || self.container_unit_axes != crate::css::container_query::Axes::NONE
    }

    /// The group this element declares, if any.
    pub(crate) fn declared_group(&self) -> Option<gpui::SharedString> {
        match self.group.as_ref()? {
            GroupName::Flag(true) => Some(DEFAULT_GROUP.into()),
            GroupName::Name(name) if !name.is_empty() => Some(name.clone().into()),
            _ => None,
        }
    }

    /// The group a `groupHover`/`groupActive`/`groupFocusWithin` style follows: its own `group`
    /// field, or the default group.
    pub(crate) fn followed_group(&self) -> gpui::SharedString {
        self.declared_group().unwrap_or_else(|| DEFAULT_GROUP.into())
    }
}
use crate::color::ColorContext;

impl StyleDesc {
    /// The style with everything that needs a mouse hitbox removed: `hover`,
    /// `active` and `cursor`. What an `inert` subtree paints.
    ///
    /// Borrowed when there is nothing to remove, which is nearly every element: an inert
    /// subtree is the whole page behind a dialog, and this runs for each of its elements.
    pub fn without_pointer_states(&self) -> std::borrow::Cow<'_, Self> {
        if self.hover.is_none()
            && self.active.is_none()
            && self.group_hover.is_none()
            && self.group_active.is_none()
            && self.group_focus_within.is_none()
            && self.has_hover.is_none()
            && self.has_active.is_none()
            && self.has_focus.is_none()
            && self.cursor.is_none()
        {
            return std::borrow::Cow::Borrowed(self);
        }
        std::borrow::Cow::Owned(Self {
            hover: None,
            active: None,
            group_hover: None,
            group_active: None,
            group_focus_within: None,
            has_hover: None,
            has_active: None,
            has_focus: None,
            cursor: None,
            ..self.clone()
        })
    }
}

/// What a style paints behind its content.
pub(crate) struct Fill {
    pub color: Option<gpui::Background>,
    /// First is on top.
    pub layers: Vec<gpui::BackgroundLayer>,
}

impl BackgroundValue {
    pub(crate) fn resolve(&self, colors: &ColorContext) -> Option<gpui::Background> {
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
    pub(crate) fn colors_following_current(
        &self,
        state: &StyleDesc,
        lengths: &LengthContext,
    ) -> Option<StyleDesc> {
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
                        parsed.style.is_visible()
                            && parsed.color.to_ascii_lowercase().contains("currentcolor")
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
        follow!(border_block, shorthand_follows);
        follow!(border_inline, shorthand_follows);
        follow!(border_top, shorthand_follows);
        follow!(border_right, shorthand_follows);
        follow!(border_bottom, shorthand_follows);
        follow!(border_left, shorthand_follows);
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
                .is_some_and(|value| crate::css::filter::follows_current_color(value, lengths))
        {
            followers.filter = self.filter.clone();
        }
        if state.backdrop_filter.is_none()
            && self
                .backdrop_filter
                .as_deref()
                .is_some_and(|value| crate::css::filter::follows_current_color(value, lengths))
        {
            followers.backdrop_filter = self.backdrop_filter.clone();
        }
        (followers != StyleDesc::default()).then_some(followers)
    }

    /// The mask layers, or `None` when the style sets no valid mask. `Some(empty)`
    /// is `mask: none`.
    pub(crate) fn resolved_mask(
        &self,
        colors: &ColorContext,
        lengths: &LengthContext,
    ) -> Option<Vec<gpui::BackgroundLayer>> {
        crate::css::mask::resolve(
            &crate::css::mask::Inputs {
                shorthand: self.mask.as_deref(),
                image: self.mask_image.as_deref(),
                size: self.mask_size.as_deref(),
                position: self.mask_position.as_deref(),
                repeat: self.mask_repeat.as_deref(),
                origin: self.mask_origin.as_deref(),
                clip: self.mask_clip.as_deref(),
                mode: self.mask_mode.as_deref(),
                composite: self.mask_composite.as_deref(),
            },
            colors,
            lengths,
        )
    }

    /// The `mask-border`, or `None` when the style has none.
    pub(crate) fn resolved_mask_border(
        &self,
        colors: &ColorContext,
        lengths: &LengthContext,
    ) -> Option<gpui::MaskBorder> {
        crate::css::mask_border::resolve(
            &crate::css::mask_border::Inputs {
                shorthand: self.mask_border.as_deref(),
                source: self.mask_border_source.as_deref(),
                slice: self.mask_border_slice.as_deref(),
                width: self.mask_border_width.as_deref(),
                outset: self.mask_border_outset.as_deref(),
                repeat: self.mask_border_repeat.as_deref(),
                mode: self.mask_border_mode.as_deref(),
            },
            colors,
            lengths,
        )
    }

    /// The background colour (or object gradient) and the gradient layers.
    ///
    /// `background` takes a plain colour as it always did, or the CSS shorthand
    /// with layers and a final colour. `backgroundColor` wins over the
    /// shorthand's colour, and the `background-*` longhands replace its parts.
    pub(crate) fn resolved_fill(&self, colors: &ColorContext, lengths: &LengthContext) -> Fill {
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
        let parsed = crate::css::background::resolve(&inputs, colors, lengths);
        // An invalid `backgroundColor` is dropped, so the shorthand's colour still shows.
        let own_color = self
            .background_color
            .as_deref()
            .and_then(|value| colors.resolve(value));
        let color = if let Some(own_color) = own_color {
            Some(own_color.into())
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

/// The CSS keyword for a GPUI cursor, the one `parse_cursor` maps back to the
/// same cursor. Several keywords name one cursor, and this is the canonical one.
pub fn cursor_name(style: gpui::CursorStyle) -> &'static str {
    use gpui::CursorStyle;
    match style {
        CursorStyle::Arrow => "default",
        CursorStyle::PointingHand => "pointer",
        CursorStyle::IBeam => "text",
        CursorStyle::IBeamCursorForVerticalLayout => "vertical-text",
        CursorStyle::Crosshair => "crosshair",
        CursorStyle::OpenHand => "grab",
        CursorStyle::ClosedHand => "grabbing",
        CursorStyle::ResizeColumn => "col-resize",
        CursorStyle::ResizeRow => "row-resize",
        CursorStyle::ResizeLeftRight => "ew-resize",
        CursorStyle::ResizeUpDown => "ns-resize",
        CursorStyle::ResizeUpLeftDownRight => "nwse-resize",
        CursorStyle::ResizeUpRightDownLeft => "nesw-resize",
        CursorStyle::ResizeLeft => "w-resize",
        CursorStyle::ResizeRight => "e-resize",
        CursorStyle::ResizeUp => "n-resize",
        CursorStyle::ResizeDown => "s-resize",
        CursorStyle::OperationNotAllowed => "not-allowed",
        CursorStyle::DragLink => "alias",
        CursorStyle::DragCopy => "copy",
        CursorStyle::ContextualMenu => "context-menu",
        CursorStyle::None => "none",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light() -> ColorContext {
        ColorContext::root(crate::color::Scheme::Light)
    }

    fn dimension(json: &str) -> Result<DimensionValue, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn calc(text: &str) -> DimensionValue {
        DimensionValue::Calc(parse_linear_length(text).unwrap())
    }

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn the_values_of_a_transform_are_paint_but_having_one_is_layout() {
        let paint_only = |from: &str, to: &str| style(from).differs_only_in_paint(&style(to));
        assert!(paint_only(r#"{"transform":"scale(2)"}"#, r#"{"transform":"rotate(1deg)"}"#));
        assert!(paint_only(r#"{"translate":"1px"}"#, r#"{"translate":"9px 2px","transformOrigin":"0 0"}"#));
        assert!(paint_only(r#"{"rotate":1}"#, r#"{"rotate":"3turn","scale":2}"#));
        assert!(paint_only(r#"{"transform":"none"}"#, "{}"));
        assert!(paint_only(r#"{"transform":"bogus"}"#, "{}"));
        // Starting or ending: the element becomes or stops being a containing block.
        assert!(!paint_only("{}", r#"{"transform":"scale(2)"}"#));
        assert!(!paint_only(r#"{"rotate":"1deg"}"#, r#"{"transform":"none"}"#));
        assert!(!paint_only(r#"{"transform":"scale(2)"}"#, r#"{"transform":"bogus"}"#));
        // And a key that is layout still is.
        assert!(!paint_only(r#"{"transform":"scale(2)","width":1}"#, r#"{"transform":"scale(3)","width":2}"#));
        // Motion's keys are the same: their values are paint while the element keeps its transform.
        assert!(paint_only(r#"{"x":1,"originX":0}"#, r#"{"x":9,"motionScale":2,"skewX":"1deg","originX":1,"originY":"4px"}"#));
        assert!(!paint_only("{}", r#"{"x":9}"#));
        assert!(!paint_only(r#"{"motionRotate":3}"#, "{}"));
        assert!(paint_only("{}", r#"{"originX":1}"#));
    }

    #[test]
    fn every_key_of_the_transform_is_a_change_of_it_and_nothing_else_is() {
        let touches = |from: &str, to: &str| style(from).touches_transform(&style(to));
        for (key, from, to) in [
            ("transform", r#""scale(2)""#, r#""scale(3)""#),
            ("translate", r#""1px""#, r#""2px""#),
            ("rotate", r#""1deg""#, r#""2deg""#),
            ("scale", "2", "3"),
            ("transformOrigin", r#""0 0""#, r#""1px 1px""#),
            ("x", "1", "2"),
            ("y", "1", "2"),
            ("translateX", "1", "2"),
            ("translateY", "1", "2"),
            ("motionScale", "1", "2"),
            ("scaleX", "1", "2"),
            ("scaleY", "1", "2"),
            ("motionRotate", "1", "2"),
            ("rotateZ", "1", "2"),
            ("skew", "1", "2"),
            ("skewX", "1", "2"),
            ("skewY", "1", "2"),
            ("originX", "1", "2"),
            ("originY", "1", "2"),
        ] {
            assert!(touches(&format!(r#"{{"{key}":{from}}}"#), &format!(r#"{{"{key}":{to}}}"#)), "{key}");
            assert!(!touches(&format!(r#"{{"{key}":{from},"opacity":1}}"#), &format!(r#"{{"{key}":{from},"opacity":0.5}}"#)), "{key}");
        }
        assert!(!touches(r#"{"opacity":1,"width":3}"#, r#"{"opacity":0.5,"width":4}"#));
    }

    #[test]
    fn rotate_and_scale_take_a_number_or_a_string() {
        let parsed = style(r#"{"rotate":45,"scale":1.5}"#);
        assert_eq!((parsed.rotate.as_deref(), parsed.scale.as_deref()), (Some("45deg"), Some("1.5")));
        let parsed = style(r#"{"rotate":"0.25turn","scale":"2 3"}"#);
        assert_eq!((parsed.rotate.as_deref(), parsed.scale.as_deref()), (Some("0.25turn"), Some("2 3")));
        assert_eq!(style(r#"{"rotate":0}"#).rotate.as_deref(), Some("0deg"));
    }

    #[test]
    fn a_number_and_a_numeric_string_are_px() {
        assert_eq!(dimension("12").unwrap(), DimensionValue::Pixels(12.0));
        assert_eq!(dimension("-3.5").unwrap(), DimensionValue::Pixels(-3.5));
        assert_eq!(dimension("\"12\"").unwrap(), DimensionValue::Pixels(12.0));
        assert_eq!(dimension("\"12px\"").unwrap(), DimensionValue::Pixels(12.0));
        assert_eq!(dimension("\"0\"").unwrap(), DimensionValue::Pixels(0.0));
    }

    #[test]
    fn an_absolute_unit_is_px() {
        for (text, px) in [("1in", 96.0), ("2.54cm", 96.0), ("25.4mm", 96.0), ("101.6q", 96.0), ("72pt", 96.0), ("6pc", 96.0)] {
            assert_eq!(dimension(&format!("\"{text}\"")).unwrap(), DimensionValue::Pixels(px), "{text}");
        }
        assert_eq!(dimension("\"calc(1in + 24px)\"").unwrap(), DimensionValue::Pixels(120.0));
    }

    #[test]
    fn a_percentage_is_a_fraction_and_a_zero_percentage_stays_one() {
        assert_eq!(dimension("\"50%\"").unwrap(), DimensionValue::Percentage(0.5));
        assert_eq!(dimension("\"-25%\"").unwrap(), DimensionValue::Percentage(-0.25));
        assert_eq!(dimension("\"calc(100% / 4)\"").unwrap(), DimensionValue::Percentage(0.25));
        assert_eq!(dimension("\"0%\"").unwrap(), DimensionValue::Percentage(0.0));
        assert_eq!(dimension("\"0px\"").unwrap(), DimensionValue::Pixels(0.0));
        assert_eq!(dimension("\"0em\"").unwrap(), DimensionValue::Pixels(0.0));
    }

    #[test]
    fn every_other_length_is_unresolved_until_it_is_applied() {
        for text in ["2em", "1.5rem", "50vw", "25vh", "10vmin", "75vmax", "calc(100% - 20px)", "calc(1em + 4px)", "calc(50vw - 100%)"] {
            assert_eq!(dimension(&format!("\"{text}\"")).unwrap(), calc(text), "{text}");
        }
        assert!(matches!(dimension("\"calc(1em + 4px)\"").unwrap(), DimensionValue::Calc(_)));
    }

    #[test]
    fn auto_and_anchor_size_keep_their_own_variants() {
        assert_eq!(dimension("\"auto\"").unwrap(), DimensionValue::Auto);
        assert_eq!(dimension("\"anchor-size(width)\"").unwrap(), DimensionValue::AnchorSize(AnchorAxis::Width));
        assert_eq!(dimension("\"anchor-size( height )\"").unwrap(), DimensionValue::AnchorSize(AnchorAxis::Height));
        assert!(dimension("\"anchor-size(depth)\"").unwrap_err().to_string().contains("invalid anchor-size"));
    }

    fn intrinsic(text: &str) -> DimensionValue {
        DimensionValue::Intrinsic(IntrinsicKeyword::parse(text).unwrap().unwrap())
    }

    #[test]
    fn the_intrinsic_size_keywords_have_their_own_variant() {
        assert_eq!(dimension("\"min-content\"").unwrap(), DimensionValue::Intrinsic(IntrinsicKeyword::MinContent));
        assert_eq!(dimension("\"max-content\"").unwrap(), DimensionValue::Intrinsic(IntrinsicKeyword::MaxContent));
        assert_eq!(dimension("\"fit-content\"").unwrap(), DimensionValue::Intrinsic(IntrinsicKeyword::FitContent));
        for (text, limit) in [
            ("fit-content(100px)", "100px"),
            ("fit-content(50%)", "50%"),
            ("fit-content(2em)", "2em"),
            ("fit-content(calc(100% - 2rem))", "calc(100% - 2rem)"),
            ("fit-content( 10vw )", "10vw"),
            ("fit-content(0)", "0"),
        ] {
            assert_eq!(
                dimension(&format!("\"{text}\"")).unwrap(),
                DimensionValue::Intrinsic(IntrinsicKeyword::FitContentLimit(parse_linear_length(limit).unwrap())),
                "{text}"
            );
        }
    }

    #[test]
    fn the_intrinsic_size_keywords_are_case_insensitive() {
        assert_eq!(dimension("\"MIN-CONTENT\"").unwrap(), intrinsic("min-content"));
        assert_eq!(dimension("\"Max-Content\"").unwrap(), intrinsic("max-content"));
        assert_eq!(dimension("\" fit-content \"").unwrap(), intrinsic("fit-content"));
        assert_eq!(dimension("\"FIT-CONTENT(10PX)\"").unwrap(), intrinsic("fit-content(10px)"));
    }

    #[test]
    fn a_bad_fit_content_limit_is_an_error() {
        for text in ["fit-content()", "fit-content(10)", "fit-content(-5px)", "fit-content(auto)", "fit-content(min-content)", "fit-content(1px 2px)", "fit-content(calc(1px * 2px))"] {
            let error = dimension(&format!("\"{text}\"")).unwrap_err().to_string();
            assert!(error.contains("invalid fit-content"), "{text}: {error}");
        }
        for text in ["fit-content(10px", "fit-content 10px", "min-content()", "content", "min content", "intrinsic"] {
            assert!(dimension(&format!("\"{text}\"")).is_err(), "{text}");
        }
    }

    #[test]
    fn the_intrinsic_size_keywords_serialize_as_css() {
        for text in ["min-content", "max-content", "fit-content", "fit-content(100px)", "fit-content(50%)", "fit-content(calc(100% - 2rem))"] {
            let value = intrinsic(text);
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(json, format!("\"{text}\""));
            assert_eq!(dimension(&json).unwrap(), value);
        }
    }

    #[test]
    fn the_six_size_properties_take_the_intrinsic_size_keywords() {
        let style: StyleDesc = serde_json::from_str(
            r#"{"width":"max-content","height":"min-content","minWidth":"fit-content","minHeight":"fit-content(10px)",
                "maxWidth":"MAX-CONTENT","maxHeight":"fit-content(50%)"}"#,
        )
        .unwrap();
        assert_eq!(style.width, Some(intrinsic("max-content")));
        assert_eq!(style.height, Some(intrinsic("min-content")));
        assert_eq!(style.min_width, Some(intrinsic("fit-content")));
        assert_eq!(style.min_height, Some(intrinsic("fit-content(10px)")));
        assert_eq!(style.max_width, Some(intrinsic("max-content")));
        assert_eq!(style.max_height, Some(intrinsic("fit-content(50%)")));
        assert_eq!(style_of(&serde_json::to_string(&style).unwrap()), style);
    }

    #[test]
    fn margins_and_insets_reject_the_intrinsic_size_keywords() {
        for field in ["margin", "marginTop", "marginRight", "marginBottom", "marginLeft"] {
            for value in ["min-content", "max-content", "fit-content", "fit-content(10px)"] {
                let error = serde_json::from_str::<StyleDesc>(&format!(r#"{{"{field}": "{value}"}}"#))
                    .unwrap_err()
                    .to_string();
                assert!(error.contains(&format!("invalid margin \"{value}\"")), "{field} {value}: {error}");
            }
        }
        for field in ["top", "right", "bottom", "left"] {
            let error = serde_json::from_str::<StyleDesc>(&format!(r#"{{"{field}": "max-content"}}"#)).unwrap_err().to_string();
            assert!(error.contains("invalid inset \"max-content\""), "{field}: {error}");
        }
        assert_eq!(style_of(r#"{"margin": null, "top": 3, "flexBasis": "auto"}"#).top, Some(DimensionValue::Pixels(3.0)));
    }

    #[test]
    fn flex_basis_takes_the_intrinsic_size_keywords() {
        for text in ["min-content", "max-content", "fit-content", "fit-content(50%)", "FIT-CONTENT(10PX)"] {
            let style = style_of(&format!(r#"{{"flexBasis": "{text}"}}"#));
            assert_eq!(style.flex_basis, Some(intrinsic(text)), "{text}");
            assert_eq!(style_of(&serde_json::to_string(&style).unwrap()), style);
        }
        let error = serde_json::from_str::<StyleDesc>(r#"{"flexBasis": "fit-content(-5px)"}"#).unwrap_err().to_string();
        assert!(error.contains("invalid fit-content"), "{error}");
    }

    #[test]
    fn padding_and_gap_are_not_dimensions_and_reject_the_keywords() {
        for json in [r#"{"padding": "min-content"}"#, r#"{"gap": "max-content"}"#, r#"{"paddingLeft": "fit-content"}"#] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
    }

    #[test]
    fn an_invalid_dimension_names_the_value() {
        for text in ["12foo", "1ch", "calc(1px + 2)", "calc(1px * 2px)", "calc()", "inf", "NaN", "12 px", "", "px", "banana"] {
            let error = dimension(&format!("\"{text}\"")).unwrap_err().to_string();
            assert!(error.contains(&format!("invalid dimension: {text}")), "{text}: {error}");
        }
        for json in ["true", "null", "[1]"] {
            assert!(dimension(json).is_err(), "{json}");
        }
    }

    #[test]
    fn dimensions_serialize_as_css() {
        let json = |value: DimensionValue| serde_json::to_string(&value).unwrap();
        assert_eq!(json(DimensionValue::Pixels(12.0)), "12.0");
        assert_eq!(json(calc("calc(100% - 20px)")), "\"calc(100% - 20px)\"");
        assert_eq!(json(calc("2em")), "\"2em\"");
        assert_eq!(json(DimensionValue::Auto), "\"auto\"");
        assert_eq!(json(DimensionValue::Percentage(0.5)), "\"50%\"");
        assert_eq!(dimension(&json(DimensionValue::Percentage(0.07))).unwrap(), DimensionValue::Percentage(0.07));
        let back = dimension(&json(calc("calc(1em + 2vw - 3px)"))).unwrap();
        assert_eq!(back, calc("calc(1em + 2vw - 3px)"));
    }

    #[test]
    fn every_dimension_field_takes_a_length() {
        let style: StyleDesc = serde_json::from_str(
            r#"{"width":"50vw","height":"calc(50% + 1rem)","minWidth":"10em","minHeight":"1in","maxWidth":"calc(100% - 20px)","maxHeight":"90%",
                "margin":"calc(5% + 4px)","marginTop":"2em","marginRight":"1cm","marginBottom":"auto","marginLeft":3,
                "top":"calc(100% - 30px)","right":"1rem","bottom":"10vh","left":"50%"}"#,
        )
        .unwrap();
        assert_eq!(style.width, Some(calc("50vw")));
        assert_eq!(style.height, Some(calc("calc(50% + 1rem)")));
        assert_eq!(style.min_width, Some(calc("10em")));
        assert_eq!(style.min_height, Some(DimensionValue::Pixels(96.0)));
        assert_eq!(style.max_width, Some(calc("calc(100% - 20px)")));
        assert_eq!(style.max_height, Some(DimensionValue::Percentage(0.9)));
        assert_eq!(style.margin, Some(Sides::all(calc("calc(5% + 4px)"))));
        assert_eq!(style.margin_top, Some(calc("2em")));
        assert_eq!(style.margin_bottom, Some(DimensionValue::Auto));
        assert_eq!(style.top, Some(calc("calc(100% - 30px)")));
        assert_eq!(style.bottom, Some(calc("10vh")));
        assert_eq!(style.left, Some(DimensionValue::Percentage(0.5)));
        assert!(serde_json::from_str::<StyleDesc>(r#"{"width":"12foo"}"#).is_err());
    }

    #[test]
    fn box_shadow_takes_a_css_string() {
        let style: StyleDesc = serde_json::from_str(
            r##"{"boxShadow":"0 1px 2px rgba(0,0,0,.2), inset 0 0 0 1px red"}"##,
        )
        .unwrap();
        let shadows = style.box_shadow.as_ref().unwrap().as_slice();
        assert_eq!(shadows.len(), 2);
        assert_eq!(shadows[0].offset_y, LinearLength::px(1.0));
        assert_eq!(shadows[0].color, "rgba(0,0,0,.2)");
        assert!(shadows[1].inset);
        assert_eq!(shadows[1].spread_radius, LinearLength::px(1.0));
    }

    #[test]
    fn an_invalid_box_shadow_string_is_left_unset_and_keeps_the_rest_of_the_style() {
        let style: StyleDesc =
            serde_json::from_str(r##"{"boxShadow":"4 4 red","color":"red"}"##).unwrap();
        assert_eq!(style.box_shadow, None);
        assert_eq!(style.color.as_deref(), Some("red"));
    }

    fn radius(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    fn circle(radius: f32) -> CornerRadiusValue {
        CornerRadiusValue::px(radius as f64)
    }

    fn ellipse(x: LinearLength, y: LinearLength) -> CornerRadiusValue {
        CornerRadiusValue { x, y: Some(y) }
    }

    #[test]
    fn a_radius_is_a_number_or_a_css_string() {
        let style = radius(
            r##"{"borderRadius":"10px 20px / 5px","borderTopLeftRadius":"50% 20px","borderTopRightRadius":8,"borderBottomLeftRadius":0}"##,
        );
        let corners = style.border_radius.unwrap().0;
        assert_eq!(corners.top_right, ellipse(LinearLength::px(20.0), LinearLength::px(5.0)));
        assert_eq!(
            style.border_top_left_radius.unwrap(),
            ellipse(LinearLength::percent(0.5), LinearLength::px(20.0))
        );
        assert_eq!(style.border_top_right_radius, Some(CornerRadiusValue::px(8.0)));
        assert_eq!(style.border_bottom_left_radius, Some(CornerRadiusValue::px(0.0)));
        assert_eq!(radius(r##"{"borderRadius":12}"##).border_radius, Some(BorderRadiusValue::px(12.0)));
    }

    #[test]
    fn an_invalid_radius_is_an_error_naming_the_value_even_in_a_state_style() {
        for json in [
            r##"{"borderRadius":"1ch"}"##,
            r##"{"borderRadius":"-4px"}"##,
            r##"{"borderRadius":"calc(-1em - 2px)"}"##,
            r##"{"borderRadius":-4}"##,
            r##"{"borderRadius":"10px / "}"##,
            r##"{"borderTopLeftRadius":"1px 2px 3px"}"##,
            r##"{"borderBottomRightRadius":"10px / 5px"}"##,
            r##"{"hover":{"borderRadius":"bogus"}}"##,
            r##"{"borderRadius":true}"##,
        ] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
        let error = serde_json::from_str::<StyleDesc>(r##"{"borderRadius":"1ch"}"##).unwrap_err().to_string();
        assert!(error.contains("\"1ch\""), "{error}");
    }

    #[test]
    fn corner_longhands_are_laid_over_the_shorthand() {
        let style = radius(r##"{"borderRadius":"10px 20px","borderBottomRightRadius":"50%"}"##);
        let corners = style.corner_radii();
        assert_eq!(corners.top_left, circle(10.0));
        assert_eq!(corners.top_right, circle(20.0));
        assert_eq!(corners.bottom_left, circle(20.0));
        assert_eq!(corners.bottom_right, CornerRadiusValue::circular(LinearLength::percent(0.5)));
        assert_eq!(StyleDesc::default().corner_radii(), gpui::Corners::default());
    }

    #[test]
    fn a_radius_resolves_against_the_lengths_of_the_element() {
        use crate::css::length::LengthContext;
        use gpui::{AbsoluteLength, CornerRadius, DefiniteLength};
        let style = radius(
            r##"{"borderRadius":"1em 2rem 10vw 5vh / 50% calc(100% - 1em) calc(1em + 25%) 4vmin","borderTopLeftRadius":"1rem"}"##,
        );
        // `1em` is 20px, `1rem` 10px and the viewport 800 by 600.
        let corners = style.resolved_corner_radii(&LengthContext::for_tests());
        let length = |pixels: f32| DefiniteLength::Absolute(AbsoluteLength::Pixels(gpui::px(pixels)));
        assert_eq!(corners.top_left, CornerRadius::from(length(10.0)));
        assert_eq!(
            corners.top_right,
            CornerRadius { x: length(20.0), y: DefiniteLength::calc(gpui::px(-20.0), 1.0) }
        );
        assert_eq!(
            corners.bottom_right,
            CornerRadius { x: length(80.0), y: DefiniteLength::calc(gpui::px(20.0), 0.25) }
        );
        assert_eq!(corners.bottom_left, CornerRadius { x: length(30.0), y: length(24.0) });
        // The units follow the element: the same style on a larger font.
        let large = LengthContext::for_tests().descend(Some(&LinearLength::px(40.0)), false);
        assert_eq!(style.resolved_corner_radii(&large).top_right.y, DefiniteLength::calc(gpui::px(-40.0), 1.0));
    }

    #[test]
    fn a_radius_serializes_for_the_inspector() {
        let style = radius(r##"{"borderRadius":"10px 20px / 5px","borderTopLeftRadius":4}"##);
        let json = serde_json::to_value(&style).unwrap();
        assert_eq!(json["borderRadius"], "10px 20px 10px 20px / 5px 5px 5px 5px");
        assert_eq!(json["borderTopLeftRadius"], 4.0);
        assert!(json["borderTopRightRadius"].is_null());
    }

    #[test]
    fn group_names_and_the_default_group() {
        let style: StyleDesc = serde_json::from_str(
            r##"{"group":true,"groupHover":{"backgroundColor":"red"},"groupActive":{"group":"card","color":"blue"},"groupFocusWithin":{"color":"green"}}"##,
        )
        .unwrap();
        let default = style.declared_group().unwrap();
        // An unnamed groupHover follows the default group, which `true` declares.
        assert_eq!(style.group_hover.as_ref().unwrap().followed_group(), default);
        assert_eq!(style.group_active.as_ref().unwrap().followed_group().as_ref(), "card");
        assert_eq!(style.group_focus_within.as_ref().unwrap().followed_group(), default);

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
        let followers = base.colors_following_current(&hover, &LengthContext::for_tests()).unwrap();
        assert_eq!(followers.border_color.as_deref(), Some("currentColor"));
        assert_eq!(followers.background_color, None);

        let overriding = StyleDesc {
            border_color: Some("blue".to_owned()),
            ..hover.clone()
        };
        assert_eq!(
            base.colors_following_current(&overriding, &LengthContext::for_tests()).unwrap().border_color,
            None
        );
        // Without a new colour nothing changes, so nothing is repainted.
        assert_eq!(base.colors_following_current(&StyleDesc::default(), &LengthContext::for_tests()), None);
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

        assert_eq!(style.resolved_fill(&light(), &LengthContext::for_tests()).color, Some(expected));
    }

    #[test]
    fn the_mask_longhands_reach_the_layers_from_json() {
        let style: StyleDesc = serde_json::from_str(
            r##"{"maskImage":"url(a.png), linear-gradient(black, transparent)",
                "maskMode":"luminance, alpha","maskComposite":"intersect"}"##,
        )
        .unwrap();
        let layers = style.resolved_mask(&light(), &LengthContext::for_tests()).unwrap();
        let parts: Vec<_> = layers.iter().map(|layer| (layer.mask_mode, layer.mask_composite)).collect();
        assert_eq!(
            parts,
            vec![
                (gpui::MaskMode::Luminance, gpui::MaskComposite::Intersect),
                (gpui::MaskMode::Alpha, gpui::MaskComposite::Intersect),
            ]
        );
        assert!(matches!(layers[0].image, gpui::BackgroundImage::Image(_)));
    }

    #[test]
    fn an_invalid_background_color_is_dropped_so_the_shorthands_colour_applies() {
        let style: StyleDesc = serde_json::from_str(
            r##"{"background":"linear-gradient(red, blue), #0000ff","backgroundColor":"notacolour"}"##,
        )
        .unwrap();
        let fill = style.resolved_fill(&light(), &LengthContext::for_tests());
        assert_eq!(fill.color, Some(crate::color::parse_color_rgba("#0000ff").unwrap().into()));
        assert_eq!(fill.layers.len(), 1);
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
        assert_eq!(shadows[1].spread_radius, LinearLength::px(1.0));
    }

    #[test]
    fn a_box_shadow_object_takes_lengths_with_units_but_no_percentage() {
        let style: StyleDesc = serde_json::from_str(
            r##"{"boxShadow":{"offsetX":"1em","offsetY":2,"blurRadius":"calc(1rem + 2px)","spreadRadius":"10vw","color":"red"}}"##,
        )
        .unwrap();
        let shadow = &style.box_shadow.as_ref().unwrap().as_slice()[0];
        assert_eq!(shadow.offset_x, parse_linear_length("1em").unwrap());
        assert_eq!(shadow.offset_y, LinearLength::px(2.0));
        assert_eq!(shadow.blur_radius, parse_linear_length("calc(1rem + 2px)").unwrap());
        assert_eq!(shadow.spread_radius, parse_linear_length("10vw").unwrap());
        let percent = r##"{"boxShadow":{"offsetX":"10%","offsetY":0,"blurRadius":0,"spreadRadius":0,"color":"red"}}"##;
        assert!(serde_json::from_str::<StyleDesc>(percent).is_err());
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
    fn every_cursor_has_a_keyword_that_parses_back_to_it() {
        use gpui::CursorStyle::*;
        let all = [
            Arrow,
            IBeam,
            Crosshair,
            ClosedHand,
            OpenHand,
            PointingHand,
            ResizeLeft,
            ResizeRight,
            ResizeLeftRight,
            ResizeUp,
            ResizeDown,
            ResizeUpDown,
            ResizeUpLeftDownRight,
            ResizeUpRightDownLeft,
            ResizeColumn,
            ResizeRow,
            IBeamCursorForVerticalLayout,
            OperationNotAllowed,
            DragLink,
            DragCopy,
            ContextualMenu,
            None,
        ];
        for style in all {
            assert_eq!(parse_cursor(cursor_name(style)), Some(style), "{style:?}");
        }
        let names: std::collections::HashSet<_> = all.into_iter().map(cursor_name).collect();
        assert_eq!(names.len(), all.len(), "one keyword per cursor");
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

    fn style_of(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn padding_gap_and_their_longhands_take_a_length_of_any_unit() {
        let style = style_of(
            r#"{
                "padding": 4, "paddingTop": "2em", "paddingRight": "1rem", "paddingBottom": "10%", "paddingLeft": "calc(10% + 4px)",
                "gap": "1in", "rowGap": "50vw", "columnGap": "calc(1em + 2px)"
            }"#,
        );
        let length = |text: &str| Some(parse_linear_length(text).unwrap());
        assert_eq!(style.padding, Some(Sides::all(LinearLength::px(4.0))));
        assert_eq!(style.padding_top, length("2em"));
        assert_eq!(style.padding_right, length("1rem"));
        assert_eq!(style.padding_bottom, Some(LinearLength::percent(0.1)));
        assert_eq!(style.padding_left, length("calc(10% + 4px)"));
        assert_eq!(style.gap, Some(Gap::all(LinearLength::px(96.0))));
        assert_eq!(style.row_gap, length("50vw"));
        assert_eq!(style.column_gap, length("calc(1em + 2px)"));
    }

    #[test]
    fn a_bad_padding_or_gap_is_an_error() {
        assert_eq!(style_of(r#"{"gap": "12px"}"#).gap, Some(Gap::all(LinearLength::px(12.0))));
        for json in [r#"{"padding": "12"}"#, r#"{"padding": "auto"}"#, r#"{"gap": "12foo"}"#, r#"{"paddingLeft": true}"#, r#"{"rowGap": "calc(1px + 2)"}"#] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
    }

    #[test]
    fn flex_basis_takes_a_dimension_including_auto() {
        assert_eq!(style_of(r#"{"flexBasis": 40}"#).flex_basis, Some(DimensionValue::Pixels(40.0)));
        assert_eq!(style_of(r#"{"flexBasis": "50%"}"#).flex_basis, Some(DimensionValue::Percentage(0.5)));
        assert_eq!(style_of(r#"{"flexBasis": "auto"}"#).flex_basis, Some(DimensionValue::Auto));
        assert_eq!(style_of(r#"{"flexBasis": "2em"}"#).flex_basis, Some(calc("2em")));
        assert_eq!(
            style_of(r#"{"flexBasis": "calc(100% - 40px)"}"#).flex_basis,
            Some(calc("calc(100% - 40px)"))
        );
    }

    #[test]
    fn only_flex_basis_takes_content() {
        let content = DimensionValue::Intrinsic(IntrinsicKeyword::Content);
        assert_eq!(style_of(r#"{"flexBasis": "content"}"#).flex_basis, Some(content.clone()));
        assert_eq!(style_of(r#"{"flexBasis": "CONTENT"}"#).flex_basis, Some(content.clone()));
        let style = style_of(r#"{"flexBasis": "content"}"#);
        assert_eq!(style_of(&serde_json::to_string(&style).unwrap()), style);
        assert_eq!(style_of(r#"{"flex": "1 1 content"}"#).effective_flex_basis(), Some(&content));
        assert_eq!(style_of(r#"{"flex": "content 2 3"}"#).flex, Some(crate::css::flex::Flex { grow: 2.0, shrink: 3.0, basis: content.clone() }));
        for json in [
            r#"{"width": "content"}"#,
            r#"{"height": "content"}"#,
            r#"{"minWidth": "content"}"#,
            r#"{"maxHeight": "content"}"#,
            r#"{"margin": "content"}"#,
            r#"{"top": "content"}"#,
            r#"{"padding": "content"}"#,
            r#"{"flex": "1 1 content content"}"#,
        ] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
    }

    #[test]
    fn a_padding_and_gap_round_trip_through_serialization() {
        let style = style_of(r#"{"padding": "calc(10% + 4px)", "paddingTop": 3, "gap": "2em", "flexBasis": "calc(1em + 1%)"}"#);
        assert_eq!(style_of(&serde_json::to_string(&style).unwrap()), style);
    }

    #[test]
    fn border_and_outline_widths_take_a_length_of_any_unit_or_a_keyword() {
        let style = style_of(
            r#"{
                "borderWidth": 2, "borderTopWidth": "0.5em", "borderRightWidth": "calc(1px + 1rem)",
                "borderBottomWidth": "thick", "borderLeftWidth": "1in",
                "outlineWidth": "thin", "outlineOffset": "calc(-1 * (2vw + 1px))"
            }"#,
        );
        let length = |text: &str| Some(parse_linear_length(text).unwrap());
        assert_eq!(style.border_width, Some(Sides::all(LinearLength::px(2.0))));
        assert_eq!(style.border_top_width, length("0.5em"));
        assert_eq!(style.border_right_width, length("calc(1px + 1rem)"));
        assert_eq!(style.border_bottom_width, Some(LinearLength::px(5.0)));
        assert_eq!(style.border_left_width, Some(LinearLength::px(96.0)));
        assert_eq!(style.outline_width, Some(LinearLength::px(1.0)));
        assert_eq!(style.outline_offset, length("calc(-1 * (2vw + 1px))"));
        assert_eq!(style_of(r#"{"borderWidth": "medium"}"#).border_width, Some(Sides::all(LinearLength::px(3.0))));
        assert_eq!(style_of(r#"{"borderWidth": -3}"#).border_width, Some(Sides::all(LinearLength::px(-3.0))));
        assert_eq!(style_of(r#"{"borderWidth": null}"#).border_width, None);
        assert_eq!(style_of("{}").outline_offset, None);
    }

    #[test]
    fn a_percentage_or_a_bad_value_is_not_a_border_or_outline_length() {
        for json in [
            r#"{"borderWidth": "10%"}"#,
            r#"{"borderTopWidth": "calc(1px + 10%)"}"#,
            r#"{"borderLeftWidth": "wide"}"#,
            r#"{"borderRightWidth": "3"}"#,
            r#"{"borderBottomWidth": true}"#,
            r#"{"outlineWidth": "50%"}"#,
            r#"{"outlineOffset": "5%"}"#,
            r#"{"outlineOffset": "thin"}"#,
        ] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
        let error = serde_json::from_str::<StyleDesc>(r#"{"borderWidth": "10%"}"#).unwrap_err().to_string();
        assert!(error.contains("invalid border width \"10%\""), "{error}");
        let error = serde_json::from_str::<StyleDesc>(r#"{"outlineOffset": "5%"}"#).unwrap_err().to_string();
        assert!(error.contains("invalid outline offset \"5%\""), "{error}");
    }

    #[test]
    fn border_and_outline_lengths_round_trip_through_serialization() {
        let style = style_of(r#"{"borderWidth": "0.5em", "borderTopWidth": 3, "outlineWidth": "thick", "outlineOffset": "calc(1em - 2px)"}"#);
        assert_eq!(style_of(&serde_json::to_string(&style).unwrap()), style);
    }

    fn length(text: &str) -> LinearLength {
        parse_linear_length(text).unwrap()
    }

    fn sides<T>(top: T, right: T, bottom: T, left: T) -> Option<Sides<T>> {
        Some(Sides { top, right, bottom, left })
    }

    fn px(value: f64) -> DimensionValue {
        DimensionValue::Pixels(value)
    }

    #[test]
    fn padding_takes_one_to_four_values_and_fills_the_missing_sides_like_css() {
        let (a, b, c, d) = (length("1px"), length("2px"), length("3px"), length("4px"));
        assert_eq!(style_of(r#"{"padding": "1px"}"#).padding, sides(a, a, a, a));
        assert_eq!(style_of(r#"{"padding": "1px 2px"}"#).padding, sides(a, b, a, b));
        assert_eq!(style_of(r#"{"padding": "1px 2px 3px"}"#).padding, sides(a, b, c, b));
        assert_eq!(style_of(r#"{"padding": "1px 2px 3px 4px"}"#).padding, sides(a, b, c, d));
        assert_eq!(style_of(r#"{"padding": 6}"#).padding, Some(Sides::all(length("6px"))));
        assert_eq!(style_of(r#"{"padding": "  0.5em   0.75em "}"#).padding, sides(length("0.5em"), length("0.75em"), length("0.5em"), length("0.75em")));
        assert_eq!(style_of(r#"{"padding": "0 1px"}"#).padding, sides(length("0"), a, length("0"), a));
        assert_eq!(style_of(r#"{"padding": null}"#).padding, None);
    }

    #[test]
    fn padding_values_may_be_em_percent_and_calc_with_spaces_inside() {
        let padding = style_of(r#"{"padding": "calc(1px + 1em) 2px 10% 1rem"}"#).padding.unwrap();
        assert_eq!(padding.top, length("calc(1px + 1em)"));
        assert_eq!(padding.right, length("2px"));
        assert_eq!(padding.bottom, LinearLength::percent(0.1));
        assert_eq!(padding.left, length("1rem"));
        let two = style_of(r#"{"padding": "calc(1px + 1em) 2px"}"#).padding.unwrap();
        assert_eq!((two.top, two.bottom), (length("calc(1px + 1em)"), length("calc(1px + 1em)")));
        assert_eq!((two.right, two.left), (length("2px"), length("2px")));
    }

    #[test]
    fn a_bad_padding_shorthand_throws() {
        for json in [
            r#"{"padding": ""}"#,
            r#"{"padding": "   "}"#,
            r#"{"padding": "1px 2px 3px 4px 5px"}"#,
            r#"{"padding": "1px auto"}"#,
            r#"{"padding": "1px 12"}"#,
            r#"{"padding": "1px, 2px"}"#,
            r#"{"padding": "1px calc(1px + 2)"}"#,
            r#"{"padding": "1px min-content"}"#,
            r#"{"padding": [1, 2]}"#,
            r#"{"padding": true}"#,
        ] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
        let error = serde_json::from_str::<StyleDesc>(r#"{"padding": "1px 12"}"#).unwrap_err().to_string();
        assert!(error.contains("invalid padding \"12\""), "{error}");
        let error = serde_json::from_str::<StyleDesc>(r#"{"padding": "1px 2px 3px 4px 5px"}"#).unwrap_err().to_string();
        assert!(error.contains("one to four values"), "{error}");
    }

    #[test]
    fn margin_takes_one_to_four_values_each_of_which_may_be_auto() {
        let style = |json: &str| style_of(json).margin;
        assert_eq!(style(r#"{"margin": "0 auto"}"#), sides(px(0.0), DimensionValue::Auto, px(0.0), DimensionValue::Auto));
        assert_eq!(style(r#"{"margin": "auto"}"#), Some(Sides::all(DimensionValue::Auto)));
        assert_eq!(
            style(r#"{"margin": "1px auto 3px"}"#),
            sides(px(1.0), DimensionValue::Auto, px(3.0), DimensionValue::Auto)
        );
        assert_eq!(
            style(r#"{"margin": "auto 2px 50% calc(1px + 1em)"}"#),
            sides(DimensionValue::Auto, px(2.0), DimensionValue::Percentage(0.5), calc("calc(1px + 1em)"))
        );
        assert_eq!(style(r#"{"margin": "0.5em 1em"}"#), sides(calc("0.5em"), calc("1em"), calc("0.5em"), calc("1em")));
        assert_eq!(style(r#"{"margin": "-4px 8px"}"#), sides(px(-4.0), px(8.0), px(-4.0), px(8.0)));
        assert_eq!(style(r#"{"margin": 5}"#), Some(Sides::all(px(5.0))));
        assert_eq!(style(r#"{"margin": null}"#), None);
        assert_eq!(
            style(r#"{"margin": "anchor-size(width) 0"}"#),
            sides(
                DimensionValue::AnchorSize(AnchorAxis::Width),
                px(0.0),
                DimensionValue::AnchorSize(AnchorAxis::Width),
                px(0.0)
            )
        );
    }

    #[test]
    fn a_bad_margin_shorthand_throws() {
        for json in [
            r#"{"margin": ""}"#,
            r#"{"margin": "1px 2px 3px 4px 5px"}"#,
            r#"{"margin": "1px 12foo"}"#,
            r#"{"margin": "auto, 1px"}"#,
            r#"{"margin": "1px calc(1px + 2)"}"#,
            r#"{"margin": "1px max-content"}"#,
            r#"{"margin": "fit-content(10px) 1px"}"#,
            r#"{"margin": [1]}"#,
        ] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
        let error = serde_json::from_str::<StyleDesc>(r#"{"margin": "1px max-content"}"#).unwrap_err().to_string();
        assert!(error.contains("invalid margin \"max-content\""), "{error}");
    }

    #[test]
    fn inset_takes_one_to_four_values_and_auto() {
        let style = |json: &str| style_of(json).inset;
        assert_eq!(style(r#"{"inset": 0}"#), Some(Sides::all(px(0.0))));
        assert_eq!(style(r#"{"inset": "10px 20px"}"#), sides(px(10.0), px(20.0), px(10.0), px(20.0)));
        assert_eq!(style(r#"{"inset": "1px 2px 3px"}"#), sides(px(1.0), px(2.0), px(3.0), px(2.0)));
        assert_eq!(
            style(r#"{"inset": "auto 0 50% 2em"}"#),
            sides(DimensionValue::Auto, px(0.0), DimensionValue::Percentage(0.5), calc("2em"))
        );
        assert_eq!(
            style(r#"{"inset": "calc(100% - 4px) auto"}"#),
            sides(calc("calc(100% - 4px)"), DimensionValue::Auto, calc("calc(100% - 4px)"), DimensionValue::Auto)
        );
        assert_eq!(style(r#"{"inset": null}"#), None);
        assert_eq!(style_of(r#"{"inset": "1px", "top": 5}"#).top, Some(px(5.0)), "the longhand is its own field");
        for json in [
            r#"{"inset": ""}"#,
            r#"{"inset": "1px 2px 3px 4px 5px"}"#,
            r#"{"inset": "1px nope"}"#,
            r#"{"inset": "max-content"}"#,
            r#"{"inset": "0 fit-content"}"#,
        ] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
        let error = serde_json::from_str::<StyleDesc>(r#"{"inset": "0 fit-content"}"#).unwrap_err().to_string();
        assert!(error.contains("invalid inset \"fit-content\""), "{error}");
    }

    #[test]
    fn border_width_takes_one_to_four_values_with_the_keywords_per_side() {
        let (a, b, c, d) = (length("1px"), length("2px"), length("3px"), length("4px"));
        let style = |json: &str| style_of(json).border_width;
        assert_eq!(style(r#"{"borderWidth": "1px 2px 3px 4px"}"#), sides(a, b, c, d));
        assert_eq!(style(r#"{"borderWidth": "1px 2px"}"#), sides(a, b, a, b));
        assert_eq!(style(r#"{"borderWidth": "1px 2px 3px"}"#), sides(a, b, c, b));
        assert_eq!(style(r#"{"borderWidth": "thin medium thick"}"#), sides(a, c, length("5px"), c));
        assert_eq!(style(r#"{"borderWidth": "thick 1px"}"#), sides(length("5px"), a, length("5px"), a));
        assert_eq!(style(r#"{"borderWidth": "0.5em calc(1px + 1rem)"}"#).unwrap().right, length("calc(1px + 1rem)"));
        assert_eq!(style(r#"{"borderWidth": 2}"#), Some(Sides::all(length("2px"))));
        assert_eq!(style(r#"{"borderWidth": "medium"}"#), Some(Sides::all(c)));
        for json in [
            r#"{"borderWidth": ""}"#,
            r#"{"borderWidth": "1px 10%"}"#,
            r#"{"borderWidth": "1px wide"}"#,
            r#"{"borderWidth": "1px 2px 3px 4px 5px"}"#,
            r#"{"borderWidth": "1px 3"}"#,
            r#"{"borderWidth": "1px auto"}"#,
        ] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
        let error = serde_json::from_str::<StyleDesc>(r#"{"borderWidth": "1px 10%"}"#).unwrap_err().to_string();
        assert!(error.contains("invalid border width \"10%\""), "{error}");
    }

    #[test]
    fn gap_takes_a_row_and_an_optional_column() {
        let style = |json: &str| style_of(json).gap;
        let gap = |row: &str, column: &str| Some(Gap { row: length(row), column: length(column) });
        assert_eq!(style(r#"{"gap": "4px 8px"}"#), gap("4px", "8px"));
        assert_eq!(style(r#"{"gap": "4px"}"#), gap("4px", "4px"));
        assert_eq!(style(r#"{"gap": 6}"#), gap("6px", "6px"));
        assert_eq!(style(r#"{"gap": "1em calc(1px + 1em)"}"#), gap("1em", "calc(1px + 1em)"));
        assert_eq!(style(r#"{"gap": "10% 0"}"#), gap("10%", "0"));
        assert_eq!(style(r#"{"gap": null}"#), None);
        for json in [
            r#"{"gap": ""}"#,
            r#"{"gap": "1px 2px 3px"}"#,
            r#"{"gap": "1px auto"}"#,
            r#"{"gap": "1px 2"}"#,
            r#"{"gap": "normal"}"#,
            r#"{"gap": [1, 2]}"#,
        ] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
        let error = serde_json::from_str::<StyleDesc>(r#"{"gap": "1px 2px 3px"}"#).unwrap_err().to_string();
        assert!(error.contains("one or two values"), "{error}");
    }

    #[test]
    fn the_box_shorthands_round_trip_through_serialization() {
        let style = style_of(
            r#"{
                "padding": "calc(1px + 1em) 2px 10% 1rem", "margin": "1px auto 50% 2em", "inset": "auto 0",
                "borderWidth": "thin 2px 3px 4px", "gap": "4px 1em"
            }"#,
        );
        assert_eq!(style_of(&serde_json::to_string(&style).unwrap()), style);
        let uniform = style_of(r#"{"padding": "2em 2em", "margin": "auto", "inset": "0", "borderWidth": "2px", "gap": "3px 3px"}"#);
        let json = serde_json::to_value(&uniform).unwrap();
        assert_eq!(json["padding"], "2em");
        assert_eq!(json["margin"], "auto");
        assert_eq!(json["gap"], 3.0);
        assert_eq!(style_of(&serde_json::to_string(&uniform).unwrap()), uniform);
    }

    #[test]
    fn border_color_style_and_radius_already_take_one_to_four_values() {
        let style = style_of(
            r##"{"borderColor": "red blue green white", "borderStyle": "solid dashed", "borderRadius": "1px 2px 3px / 4px 5px"}"##,
        );
        assert_eq!(style.border_color.as_deref(), Some("red blue green white"));
        assert_eq!(style.border_style.as_deref(), Some("solid dashed"));
        let corners = style.corner_radii();
        assert_eq!(corners.top_left, CornerRadiusValue { x: length("1px"), y: Some(length("4px")) });
        assert_eq!(corners.bottom_right, CornerRadiusValue { x: length("3px"), y: Some(length("4px")) });
        let em = style_of(r#"{"borderRadius": "0.5em 1rem"}"#).corner_radii();
        assert_eq!((em.top_left.x, em.top_right.x), (length("0.5em"), length("1rem")));
        assert!(serde_json::from_str::<StyleDesc>(r#"{"borderRadius": "1px 2px 3px 4px 5px"}"#).is_err());
    }
}
