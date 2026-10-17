//! The animatable properties: one `AnimProp` per `StyleDesc` longhand whose
//! value is a number, a length, a colour or a shadow list.
//!
//! Discrete properties (`display`, `visibility`, ...) are `Kind::Keyword`. Only
//! a CSS transition with `transition-behavior: allow-discrete` runs them;
//! [`AnimProp::from_name`] leaves them out, so naming one in `animate` is an
//! error instead of a silent no-op.

use serde_json::Value;

use super::value::{AnimValue, Color, Colors, Shadow};
use crate::color::ColorContext;
use crate::css::length::{LengthContext, LinearLength};
use crate::css::flex::Flex;
use crate::css::outline::Outline;
use crate::css::line_height::LineHeight;
use crate::css::border_radius::parse_radii;
use crate::css::shorthand::{parse_pair, parse_sides, Gap, Sides};
use crate::css::syntax::split_slash_tokens;
use crate::layout::aspect_ratio::AspectRatio;
use crate::style::{
    BorderRadiusValue, BoxShadowValue, BoxShadowValues, CornerRadiusValue, DimensionValue, FontWeightValue,
    StyleDesc,
};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Number,
    /// A `LinearLength` (or a corner radius of one): px, `em`, `rem` and the viewport
    /// units resolve to px when the value is read, and a percentage stays a share of
    /// the property's basis, so a value is its px and its percentage.
    Length,
    /// A corner radius: a length, or two when the corner is elliptical.
    Corner,
    /// A `font-size`, where `em` and `%` are of the parent's size, so it is plain px.
    FontSize,
    Dimension,
    Color,
    FontWeight,
    Shadow,
    Keyword,
}

/// The range the applied value is held to. Physics keeps the unclamped value.
#[derive(Clone, Copy)]
enum Clamp {
    None,
    NonNegative,
    /// `0..=1`.
    Unit,
    /// CSS `font-weight`, `1..=1000`.
    Weight,
}

/// What a property is before anything sets it, when its computed value is
/// something an animation can start from.
#[derive(Clone, Copy)]
enum Initial {
    /// `auto`, `none`, `normal`: CSS does not interpolate these with a length,
    /// number or colour, so nothing animates from them.
    Unresolved,
    Number(f64),
    /// The element's computed `font-size`, inherited when it declares none.
    FontSize,
    /// The element's computed `font-weight`, inherited when it declares none.
    FontWeight,
    Transparent,
    /// The element's own `color`.
    CurrentColor,
    /// What the element's `color` computes to when unset: inherited, or the
    /// `CanvasText` of its scheme at the root.
    ComputedColor,
    NoShadows,
    /// A keyword that is the same as leaving the property unset.
    Keyword(&'static str),
    /// A keyword whose unset value is the element's default, which the style
    /// cannot name.
    Unset,
}

/// What a field is read in: the colours its value resolves in and the lengths.
struct Reading<'a> {
    colors: &'a ColorContext,
    lengths: &'a LengthContext,
}

/// How a `StyleDesc` field converts to and from an animated value.
trait Field {
    fn read(&self, cx: &Reading) -> Option<AnimValue>;
    fn write(&mut self, value: AnimValue);
}

/// `length` in px and a share of its percentage basis.
fn resolved(length: &LinearLength, lengths: &LengthContext) -> AnimValue {
    let (px, percent) = lengths.split(length);
    AnimValue::length(px, percent)
}

fn linear_length(value: AnimValue) -> LinearLength {
    let (px, percent) = value
        .as_length()
        .unwrap_or_else(|| unreachable!("a length field was given {value:?}"));
    length_of((px, percent))
}

fn length_of((px, percent): (f64, f64)) -> LinearLength {
    LinearLength { px, percent, ..LinearLength::default() }
}

impl Field for Option<f64> {
    fn read(&self, _cx: &Reading) -> Option<AnimValue> {
        self.map(AnimValue::Number)
    }

    fn write(&mut self, value: AnimValue) {
        match value {
            AnimValue::Number(value) => *self = Some(value),
            other => unreachable!("a number field was given {other:?}"),
        }
    }
}

/// A circular corner is a length, a percentage of the border box on each axis
/// included. An elliptical one is a length per axis.
impl Field for Option<CornerRadiusValue> {
    fn read(&self, cx: &Reading) -> Option<AnimValue> {
        let corner = self.as_ref()?;
        let x = cx.lengths.split(&corner.x);
        Some(match &corner.y {
            Some(y) => AnimValue::ellipse(x, cx.lengths.split(y)),
            None => AnimValue::length(x.0, x.1),
        })
    }

    fn write(&mut self, value: AnimValue) {
        *self = Some(match value.as_ellipse() {
            Some((x, y)) if x != y => CornerRadiusValue { x: length_of(x), y: Some(length_of(y)) },
            _ => CornerRadiusValue::circular(linear_length(value)),
        });
    }
}

/// The shorthand seen from one corner, which is what a longhand reads when it
/// is unset.
trait ShorthandCorners {
    fn top_left(&self) -> Option<CornerRadiusValue>;
    fn top_right(&self) -> Option<CornerRadiusValue>;
    fn bottom_left(&self) -> Option<CornerRadiusValue>;
    fn bottom_right(&self) -> Option<CornerRadiusValue>;
}

impl ShorthandCorners for Option<BorderRadiusValue> {
    fn top_left(&self) -> Option<CornerRadiusValue> {
        self.map(|radii| radii.0.top_left)
    }

    fn top_right(&self) -> Option<CornerRadiusValue> {
        self.map(|radii| radii.0.top_right)
    }

    fn bottom_left(&self) -> Option<CornerRadiusValue> {
        self.map(|radii| radii.0.bottom_left)
    }

    fn bottom_right(&self) -> Option<CornerRadiusValue> {
        self.map(|radii| radii.0.bottom_right)
    }
}

/// A box shorthand seen from one side, or `gap` from one axis, which is what a
/// longhand reads when it is unset.
trait ShorthandSides<T> {
    fn top(&self) -> Option<T>;
    fn right(&self) -> Option<T>;
    fn bottom(&self) -> Option<T>;
    fn left(&self) -> Option<T>;
}

impl<T: Clone> ShorthandSides<T> for Option<Sides<T>> {
    fn top(&self) -> Option<T> {
        self.as_ref().map(|sides| sides.top.clone())
    }

    fn right(&self) -> Option<T> {
        self.as_ref().map(|sides| sides.right.clone())
    }

    fn bottom(&self) -> Option<T> {
        self.as_ref().map(|sides| sides.bottom.clone())
    }

    fn left(&self) -> Option<T> {
        self.as_ref().map(|sides| sides.left.clone())
    }
}

/// `flex` seen as the longhand a transition reads when it is unset.
trait ShorthandFlex {
    fn grow(&self) -> Option<f64>;
    fn shrink(&self) -> Option<f64>;
    fn basis(&self) -> Option<DimensionValue>;
}

impl ShorthandFlex for Option<Flex> {
    fn grow(&self) -> Option<f64> {
        self.as_ref().map(|flex| flex.grow)
    }

    fn shrink(&self) -> Option<f64> {
        self.as_ref().map(|flex| flex.shrink)
    }

    fn basis(&self) -> Option<DimensionValue> {
        self.as_ref().map(|flex| flex.basis.clone())
    }
}

/// `outline` seen as the longhand a transition reads when it is unset.
trait ShorthandOutline {
    fn width(&self) -> Option<LinearLength>;
    fn color(&self) -> Option<String>;
}

impl ShorthandOutline for Option<Outline> {
    fn width(&self) -> Option<LinearLength> {
        self.as_ref().map(|outline| outline.width)
    }

    fn color(&self) -> Option<String> {
        self.as_ref().map(|outline| outline.color.clone())
    }
}

trait ShorthandAxes {
    fn row(&self) -> Option<LinearLength>;
    fn column(&self) -> Option<LinearLength>;
}

impl ShorthandAxes for Option<Gap> {
    fn row(&self) -> Option<LinearLength> {
        self.map(|gap| gap.row)
    }

    fn column(&self) -> Option<LinearLength> {
        self.map(|gap| gap.column)
    }
}

impl Field for Option<DimensionValue> {
    fn read(&self, cx: &Reading) -> Option<AnimValue> {
        match self.as_ref()? {
            DimensionValue::Pixels(value) => Some(AnimValue::Number(*value)),
            DimensionValue::Percentage(value) => Some(AnimValue::Percent(*value)),
            DimensionValue::Calc(length) => Some(resolved(length, cx.lengths)),
            DimensionValue::Auto | DimensionValue::Intrinsic(_) | DimensionValue::AnchorSize(_) => None,
        }
    }

    fn write(&mut self, value: AnimValue) {
        *self = Some(match value {
            AnimValue::Number(value) => DimensionValue::Pixels(value),
            AnimValue::Percent(value) => DimensionValue::Percentage(value),
            AnimValue::Calc { .. } => DimensionValue::Calc(linear_length(value)),
            AnimValue::Auto => DimensionValue::Auto,
            other => unreachable!("a dimension field was given {other:?}"),
        });
    }
}

impl Field for Option<LinearLength> {
    fn read(&self, cx: &Reading) -> Option<AnimValue> {
        Some(resolved(self.as_ref()?, cx.lengths))
    }

    fn write(&mut self, value: AnimValue) {
        *self = Some(linear_length(value));
    }
}

impl Field for Option<LineHeight> {
    /// Only a multiplier is a number a track can run from: `normal` has no value
    /// and a length needs the element's font size.
    fn read(&self, _cx: &Reading) -> Option<AnimValue> {
        match self.as_ref()? {
            LineHeight::Multiplier(multiplier) => Some(AnimValue::Number(*multiplier)),
            LineHeight::Normal | LineHeight::Length(_) => None,
        }
    }

    fn write(&mut self, value: AnimValue) {
        match value {
            AnimValue::Number(value) => *self = Some(LineHeight::Multiplier(value)),
            other => unreachable!("a line height field was given {other:?}"),
        }
    }
}

impl Field for Option<AspectRatio> {
    fn read(&self, _cx: &Reading) -> Option<AnimValue> {
        self.and_then(AspectRatio::ratio).map(AnimValue::Number)
    }

    fn write(&mut self, value: AnimValue) {
        match value {
            AnimValue::Number(ratio) if ratio > 0.0 => *self = Some(AspectRatio::Ratio(ratio)),
            AnimValue::Number(_) => *self = Some(AspectRatio::Auto),
            other => unreachable!("an aspect ratio field was given {other:?}"),
        }
    }
}

impl Field for Option<String> {
    /// A colour GPUI would drop as unparsable is no value at all.
    fn read(&self, cx: &Reading) -> Option<AnimValue> {
        Color::parse(self.as_deref()?, cx.colors).map(AnimValue::Color)
    }

    fn write(&mut self, value: AnimValue) {
        match value {
            AnimValue::Color(color) => *self = Some(color.to_css()),
            other => unreachable!("a colour field was given {other:?}"),
        }
    }
}

impl Field for Option<FontWeightValue> {
    fn read(&self, _cx: &Reading) -> Option<AnimValue> {
        Some(AnimValue::Number(font_weight(self.as_ref()?)))
    }

    fn write(&mut self, value: AnimValue) {
        match value {
            AnimValue::Number(value) => *self = Some(FontWeightValue::Num(value)),
            other => unreachable!("a font weight field was given {other:?}"),
        }
    }
}

impl Field for Option<BoxShadowValues> {
    fn read(&self, cx: &Reading) -> Option<AnimValue> {
        shadows_from("boxShadow", self.as_ref()?.as_slice(), cx.colors)
            .ok()
            .map(AnimValue::Shadows)
    }

    fn write(&mut self, value: AnimValue) {
        match value {
            AnimValue::Shadows(shadows) => *self = Some(shadow_layers(shadows)),
            other => unreachable!("a shadow field was given {other:?}"),
        }
    }
}

fn keyword_read(slot: &Option<String>) -> Option<AnimValue> {
    slot.clone().map(|keyword| AnimValue::Keyword(Some(keyword)))
}

fn keyword_write(slot: &mut Option<String>, value: AnimValue) {
    match value {
        AnimValue::Keyword(keyword) => *slot = keyword,
        other => unreachable!("a keyword field was given {other:?}"),
    }
}

/// `Option<String>` holds both colours and keywords, so the kind of the row
/// picks the conversion.
fn font_size_read(slot: &Option<LinearLength>, cx: &Reading) -> Option<AnimValue> {
    let px = cx.lengths.declared_font_size(slot.as_ref()?).max(0.0);
    Some(AnimValue::Number(px))
}

macro_rules! read_field {
    (Keyword, $slot:expr, $cx:expr) => {
        keyword_read(&$slot)
    };
    (FontSize, $slot:expr, $cx:expr) => {
        font_size_read(&$slot, $cx)
    };
    ($kind:ident, $slot:expr, $cx:expr) => {
        Field::read(&$slot, $cx)
    };
}

macro_rules! write_field {
    (Keyword, $slot:expr, $value:expr) => {
        keyword_write(&mut $slot, $value)
    };
    ($kind:ident, $slot:expr, $value:expr) => {
        Field::write(&mut $slot, $value)
    };
}

pub(super) fn font_weight(value: &FontWeightValue) -> f64 {
    crate::renderer::parse_font_weight(value).0 as f64
}

/// Reads the parsed per-layer structs, so every spelling `boxShadow` accepts
/// (single, list, and whatever string forms are added) animates the same way.
fn shadows_from(
    name: &str,
    layers: &[BoxShadowValue],
    colors: &ColorContext,
) -> Result<Vec<Shadow>, String> {
    let px = |length: LinearLength| {
        length.as_px().ok_or_else(|| {
            format!(
                "motion cannot animate {name} to or from a CSS length: only a number of px, \
                 because calc(), em, rem and the viewport units are resolved against the \
                 element and the window when the tree is applied"
            )
        })
    };
    layers
        .iter()
        .map(|layer| {
            let color = Color::parse(&layer.color, colors)
                .ok_or_else(|| format!("motion {name} has an invalid colour: {}", layer.color))?;
            Ok(Shadow {
                offset_x: px(layer.offset_x)?,
                offset_y: px(layer.offset_y)?,
                blur: px(layer.blur_radius)?,
                spread: px(layer.spread_radius)?,
                color,
                inset: layer.inset,
            })
        })
        .collect()
}

macro_rules! properties {
    ($(
        $variant:ident $name:literal $kind:ident $clamp:ident $initial:expr,
        $field:ident $(| $fallback:ident $(. $corner:ident ())?)?;
    )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(super) enum AnimProp {
            $($variant),*
        }

        impl AnimProp {
            pub(super) const ALL: &'static [AnimProp] = &[$(Self::$variant),*];

            fn lookup(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)*
                    _ => None,
                }
            }

            pub(super) fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),*
                }
            }

            fn kind(self) -> Kind {
                match self {
                    $(Self::$variant => Kind::$kind),*
                }
            }

            fn clamp_rule(self) -> Clamp {
                match self {
                    $(Self::$variant => Clamp::$clamp),*
                }
            }

            fn initial_rule(self) -> Initial {
                match self {
                    $(Self::$variant => $initial),*
                }
            }

            /// The value the style declares, resolving a longhand through the
            /// shorthand the renderer would apply beneath it. A longhand that is
            /// set but has no number (a percentage radius) does not fall through.
            pub(super) fn read(self, style: &StyleDesc, colors: &Colors) -> Option<AnimValue> {
                let cx = &Reading { colors: self.colors(colors), lengths: &colors.lengths };
                let value = match self {
                    $(Self::$variant => {
                        let value = read_field!($kind, style.$field, cx);
                        $(let value = if style.$field.is_none() {
                            read_field!($kind, style.$fallback$(.$corner())?, cx)
                        } else {
                            value
                        };)?
                        value
                    }),*
                };
                // A `calc()` clamps to the range of its property, which only a
                // resolved length can be held to.
                value.map(|value| match self.kind() {
                    Kind::Length | Kind::Corner | Kind::Dimension => self.clamp_value(value),
                    _ => value,
                })
            }

            pub(super) fn write(self, style: &mut StyleDesc, value: AnimValue) {
                match self {
                    $(Self::$variant => write_field!($kind, style.$field, value)),*
                }
            }
        }
    };
}

use Initial::{ComputedColor, CurrentColor, Keyword as InitialKeyword, NoShadows, Number, Transparent, Unresolved, Unset};

properties! {
    FlexGrow "flexGrow" Number NonNegative Number(0.0), flex_grow | flex.grow();
    FlexShrink "flexShrink" Number NonNegative Number(1.0), flex_shrink | flex.shrink();
    FlexBasis "flexBasis" Dimension NonNegative Unresolved, flex_basis | flex.basis();
    RowGap "rowGap" Length NonNegative Number(0.0), row_gap | gap.row();
    ColumnGap "columnGap" Length NonNegative Number(0.0), column_gap | gap.column();

    Width "width" Dimension NonNegative Unresolved, width;
    Height "height" Dimension NonNegative Unresolved, height;
    MinWidth "minWidth" Dimension NonNegative Unresolved, min_width;
    MinHeight "minHeight" Dimension NonNegative Unresolved, min_height;
    MaxWidth "maxWidth" Dimension NonNegative Unresolved, max_width;
    MaxHeight "maxHeight" Dimension NonNegative Unresolved, max_height;

    PaddingTop "paddingTop" Length NonNegative Number(0.0), padding_top | padding.top();
    PaddingRight "paddingRight" Length NonNegative Number(0.0), padding_right | padding.right();
    PaddingBottom "paddingBottom" Length NonNegative Number(0.0), padding_bottom | padding.bottom();
    PaddingLeft "paddingLeft" Length NonNegative Number(0.0), padding_left | padding.left();

    MarginTop "marginTop" Dimension None Number(0.0), margin_top | margin.top();
    MarginRight "marginRight" Dimension None Number(0.0), margin_right | margin.right();
    MarginBottom "marginBottom" Dimension None Number(0.0), margin_bottom | margin.bottom();
    MarginLeft "marginLeft" Dimension None Number(0.0), margin_left | margin.left();

    Top "top" Dimension None Unresolved, top | inset.top();
    Right "right" Dimension None Unresolved, right | inset.right();
    Bottom "bottom" Dimension None Unresolved, bottom | inset.bottom();
    Left "left" Dimension None Unresolved, left | inset.left();

    BackgroundColor "backgroundColor" Color None Transparent, background_color;
    Color "color" Color None ComputedColor, color;
    Opacity "opacity" Number Unit Number(1.0), opacity;

    BorderTopWidth "borderTopWidth" Length NonNegative Number(0.0), border_top_width | border_width.top();
    BorderRightWidth "borderRightWidth" Length NonNegative Number(0.0), border_right_width | border_width.right();
    BorderBottomWidth "borderBottomWidth" Length NonNegative Number(0.0), border_bottom_width | border_width.bottom();
    BorderLeftWidth "borderLeftWidth" Length NonNegative Number(0.0), border_left_width | border_width.left();
    BorderTopColor "borderTopColor" Color None CurrentColor, border_top_color | border_color;
    BorderRightColor "borderRightColor" Color None CurrentColor, border_right_color | border_color;
    BorderBottomColor "borderBottomColor" Color None CurrentColor, border_bottom_color | border_color;
    BorderLeftColor "borderLeftColor" Color None CurrentColor, border_left_color | border_color;
    BorderTopLeftRadius "borderTopLeftRadius" Corner NonNegative Number(0.0), border_top_left_radius | border_radius.top_left();
    BorderTopRightRadius "borderTopRightRadius" Corner NonNegative Number(0.0), border_top_right_radius | border_radius.top_right();
    BorderBottomLeftRadius "borderBottomLeftRadius" Corner NonNegative Number(0.0), border_bottom_left_radius | border_radius.bottom_left();
    BorderBottomRightRadius "borderBottomRightRadius" Corner NonNegative Number(0.0), border_bottom_right_radius | border_radius.bottom_right();
    BoxShadow "boxShadow" Shadow None NoShadows, box_shadow;
    AspectRatio "aspectRatio" Number NonNegative Unresolved, aspect_ratio;

    OutlineWidth "outlineWidth" Length NonNegative Number(0.0), outline_width | outline.width();
    OutlineColor "outlineColor" Color None CurrentColor, outline_color | outline.color();
    TextDecorationColor "textDecorationColor" Color None CurrentColor, text_decoration_color;
    OutlineOffset "outlineOffset" Length None Number(0.0), outline_offset;

    FontSize "fontSize" FontSize NonNegative Initial::FontSize, font_size;
    FontWeight "fontWeight" FontWeight Weight Initial::FontWeight, font_weight;
    LineHeight "lineHeight" Number NonNegative Unresolved, line_height;

    ScrollbarColor "scrollbarColor" Color None Unresolved, scrollbar_color;
    ScrollbarWidth "scrollbarWidth" Number NonNegative Unresolved, scrollbar_width;
    SelectionColor "selectionColor" Color None Unresolved, selection_color;

    Display "display" Keyword None Unset, display;
    Visibility "visibility" Keyword None InitialKeyword("visible"), visibility;
    FlexDirection "flexDirection" Keyword None Unset, flex_direction;
    FlexWrap "flexWrap" Keyword None Unset, flex_wrap;
    AlignItems "alignItems" Keyword None Unset, align_items;
    AlignSelf "alignSelf" Keyword None Unset, align_self;
    AlignContent "alignContent" Keyword None Unset, align_content;
    JustifyContent "justifyContent" Keyword None Unset, justify_content;
    JustifyItems "justifyItems" Keyword None Unset, justify_items;
    JustifySelf "justifySelf" Keyword None Unset, justify_self;
}

/// What a style is the style of, which decides the properties its transitions and
/// animations may move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    Element,
    /// A `<span>`, or a `<text>` inside text: it has no layout node of its own,
    /// so only what its paragraph paints around and in its text can move: colours,
    /// and the opacity those are painted with.
    InlineBox,
}

impl Scope {
    /// What an error names as the properties of [`Scope::InlineBox`].
    pub(super) const INLINE_BOX_PROPERTIES: &'static str = "colours (color, backgroundColor, textDecorationColor, \
         outlineColor, borderColor and the four border side colours) and opacity";

    pub(super) fn animates(self, prop: AnimProp) -> bool {
        match self {
            Self::Element => true,
            Self::InlineBox => matches!(
                prop,
                AnimProp::Opacity
                    | AnimProp::Color
                    | AnimProp::BackgroundColor
                    | AnimProp::TextDecorationColor
                    | AnimProp::BorderTopColor
                    | AnimProp::BorderRightColor
                    | AnimProp::BorderBottomColor
                    | AnimProp::BorderLeftColor
                    | AnimProp::OutlineColor
            ),
        }
    }
}

/// CSS shorthands, expanded to longhands when a description is parsed so
/// animating `padding` beats a side set in the base style.
pub(super) fn expand_shorthand(name: &str) -> Option<&'static [AnimProp]> {
    use AnimProp::*;
    Some(match name {
        "padding" => &[PaddingTop, PaddingRight, PaddingBottom, PaddingLeft],
        "margin" => &[MarginTop, MarginRight, MarginBottom, MarginLeft],
        "borderWidth" => &[
            BorderTopWidth,
            BorderRightWidth,
            BorderBottomWidth,
            BorderLeftWidth,
        ],
        "borderRadius" => &[
            BorderTopLeftRadius,
            BorderTopRightRadius,
            BorderBottomLeftRadius,
            BorderBottomRightRadius,
        ],
        "borderColor" => &[
            BorderTopColor,
            BorderRightColor,
            BorderBottomColor,
            BorderLeftColor,
        ],
        "gap" => &[RowGap, ColumnGap],
        "inset" => &[Top, Right, Bottom, Left],
        _ => return None,
    })
}

/// A shorthand's value as the value of each longhand it covers.
pub(super) struct Split {
    shorthand: String,
    pub(super) channels: Vec<(AnimProp, Value)>,
}

impl Split {
    /// `error` from parsing the value of `prop`. When every longhand got the value
    /// the shorthand was written with, it names the shorthand, which is what the
    /// author wrote; otherwise it names the longhand that failed.
    pub(super) fn error(&self, prop: AnimProp, error: String) -> String {
        let uniform = self.channels.iter().all(|(_, value)| *value == self.channels[0].1);
        if uniform {
            error.replacen(prop.name(), &self.shorthand, 1)
        } else {
            error
        }
    }
}

/// Splits what `animate`, `initial`, a keyframe or a keyframes array gives a shorthand into
/// one value per longhand, so each longhand animates its own side, axis or corner:
/// `padding: "0.5em 0.75em"` is `0.5em` above and below and `0.75em` left and right.
/// Keyframes are split frame by frame. `None` when `name` is no shorthand.
pub(super) fn split_shorthand(name: &str, value: &Value) -> Option<Result<Split, String>> {
    let props = expand_shorthand(name)?;
    Some(
        split_value(name, props.len(), value)
            .map_err(|error| format!("motion {name}: {error}"))
            .map(|values| Split { shorthand: name.to_string(), channels: props.iter().copied().zip(values).collect() }),
    )
}

fn split_value(name: &str, channels: usize, value: &Value) -> Result<Vec<Value>, String> {
    match value {
        Value::String(text) => {
            Ok(split_text(name, text)?.into_iter().map(Value::String).collect())
        }
        Value::Array(frames) => {
            let mut split = vec![Vec::with_capacity(frames.len()); channels];
            for frame in frames {
                let values = match frame {
                    Value::Null => vec![Value::Null; channels],
                    frame => split_value(name, channels, frame)?,
                };
                for (channel, value) in split.iter_mut().zip(values) {
                    channel.push(value);
                }
            }
            Ok(split.into_iter().map(Value::Array).collect())
        }
        other => Ok(vec![other.clone(); channels]),
    }
}

/// One string per longhand of `name`, in the order [`expand_shorthand`] lists them.
fn split_text(name: &str, text: &str) -> Result<Vec<String>, String> {
    let token = |token: &str| Ok(token.to_string());
    match name {
        "gap" => parse_pair(text, token).map(|(row, column)| vec![row, column]),
        "borderRadius" => {
            let tokens = split_slash_tokens(text);
            let radii = parse_radii(&tokens, token).map_err(|reason| format!("invalid \"{text}\": {reason}"))?;
            // CSS lists the corners clockwise from the top left; the longhands
            // here go top left, top right, bottom left, bottom right.
            Ok([0, 1, 3, 2]
                .into_iter()
                .map(|corner| match &radii.vertical {
                    Some(vertical) => format!("{} {}", radii.horizontal[corner], vertical[corner]),
                    None => radii.horizontal[corner].clone(),
                })
                .collect())
        }
        _ => parse_sides(text, token).map(|sides| vec![sides.top, sides.right, sides.bottom, sides.left]),
    }
}

/// The axis of a box: the side of the border box a size property measures, or
/// the side of the containing block a percentage resolves against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    Width,
    Height,
}

impl AnimProp {
    /// The property `animate` and keyframes name: not a discrete one.
    pub(super) fn from_name(name: &str) -> Option<Self> {
        Self::lookup(name).filter(|prop| !prop.is_discrete())
    }

    /// The property a CSS `transition-property` name stands for, discrete ones
    /// included.
    pub(super) fn from_css_name(name: &str) -> Option<Self> {
        Self::lookup(name)
    }

    /// Whether the value flips instead of blending, so only a transition with
    /// `allow-discrete` runs it.
    pub(super) fn is_discrete(self) -> bool {
        self.kind() == Kind::Keyword
    }

    /// The axis whose laid-out size an unset value of this property is, so an
    /// animation can start from what layout produced.
    pub(super) fn size_axis(self) -> Option<Axis> {
        match self {
            Self::Width | Self::MinWidth | Self::MaxWidth => Some(Axis::Width),
            Self::Height | Self::MinHeight | Self::MaxHeight => Some(Axis::Height),
            _ => None,
        }
    }

    /// The axis of the containing block a percentage of this property is a
    /// share of. Margins and padding take the width on every side, as CSS does;
    /// insets take the height for `top` and `bottom`; `flexBasis` the main size
    /// of its flex container, `flex_main`. `None` for a property that takes no
    /// percentage here.
    pub(super) fn percent_axis(self, flex_main: Axis) -> Option<Axis> {
        match self {
            Self::MarginTop
            | Self::MarginRight
            | Self::MarginBottom
            | Self::MarginLeft
            | Self::PaddingTop
            | Self::PaddingRight
            | Self::PaddingBottom
            | Self::PaddingLeft
            | Self::Left
            | Self::Right => Some(Axis::Width),
            Self::Top | Self::Bottom => Some(Axis::Height),
            Self::FlexBasis => Some(flex_main),
            _ => self.size_axis(),
        }
    }

    /// Whether reduced motion replaces the property's animation with a jump.
    /// Framer's `positionalKeys` less the transforms GPUI does not have; padding,
    /// margin, gap, min/max sizes and `flexBasis` keep animating there.
    pub(super) fn is_positional(self) -> bool {
        matches!(
            self,
            Self::Width | Self::Height | Self::Top | Self::Right | Self::Bottom | Self::Left
        )
    }

    /// Whether the property's grammar takes a percentage. Border widths and the
    /// outline do not; `font-size` does, but resolves it against the parent's
    /// font size, so it is never a share of anything at run time.
    fn takes_percent(self) -> bool {
        !matches!(
            self,
            Self::BorderTopWidth
                | Self::BorderRightWidth
                | Self::BorderBottomWidth
                | Self::BorderLeftWidth
                | Self::OutlineWidth
                | Self::OutlineOffset
        )
    }

    /// The context a value of this property resolves its colours in. `color`
    /// is the one that differs: `currentColor` there is the parent's colour.
    fn colors(self, colors: &Colors) -> &ColorContext {
        match self {
            Self::Color => &colors.text,
            _ => &colors.element,
        }
    }

    /// The value before the style sets it: the CSS computed value of an unset
    /// property. `None` where that is `auto`, `normal` or a size layout decides,
    /// which a track cannot blend from, or an inherited value the renderer does
    /// not carry yet.
    pub(super) fn initial(self, colors: &Colors) -> Option<AnimValue> {
        match self.initial_rule() {
            Initial::Unresolved => None,
            Initial::Number(value) => Some(AnimValue::Number(value)),
            Initial::FontSize => Some(AnimValue::Number(colors.font.size)),
            Initial::FontWeight => Some(AnimValue::Number(colors.font.weight)),
            Initial::Transparent => Some(AnimValue::Color(Color::TRANSPARENT)),
            Initial::CurrentColor => Some(AnimValue::Color(Color::from_rgba(colors.element.current, false))),
            // The context keeps the resolved colour, not the syntax it was
            // written in. Hex, names and `rgb()` are what a colour is nearly
            // always written in, so it counts as legacy and blends in sRGB.
            Initial::ComputedColor => Some(AnimValue::Color(Color::from_rgba(colors.element.current, true))),
            Initial::NoShadows => Some(AnimValue::Shadows(Vec::new())),
            Initial::Keyword(keyword) => Some(AnimValue::Keyword(Some(keyword.to_string()))),
            Initial::Unset => Some(AnimValue::Keyword(None)),
        }
    }

    /// Whether `items` is one value of a list-valued property (a shadow's
    /// layers) and not keyframes. Keyframes of such a property wrap each list
    /// in an array of its own.
    pub(super) fn is_list_value(self, items: &[Value]) -> bool {
        self.kind() == Kind::Shadow && items.iter().all(Value::is_object)
    }

    /// Parses and validates one value from a description. A length with `em`,
    /// `rem`, the viewport units or `calc()` resolves to px against the element
    /// in `colors`, as a CSS computed value does; a percentage stays a share of
    /// the property's basis.
    pub(super) fn parse(self, value: &Value, colors: &Colors) -> Result<AnimValue, String> {
        let name = self.name();
        let lengths = &colors.lengths;
        let colors = self.colors(colors);
        let parsed = match self.kind() {
            Kind::Number if self == Self::LineHeight && value.is_string() => {
                return Err(format!(
                    "motion cannot animate {name} to or from normal or a CSS length: only a number, a multiple of \
                     the font size, because normal and lengths are resolved against the element when \
                     the tree is applied"
                ));
            }
            Kind::Number => AnimValue::Number(parse_number(name, value)?),
            Kind::Length => {
                let length: LinearLength = serde_json::from_value(value.clone())
                    .map_err(|error| format!("motion {name} must be a number or a CSS length: {error}"))?;
                if length.has_percent() && !self.takes_percent() {
                    return Err(format!("motion {name} cannot be a percentage: it takes a length"));
                }
                self.resolve_length(&length, lengths)?
            }
            Kind::Corner => {
                let corner: CornerRadiusValue = serde_json::from_value(value.clone())
                    .map_err(|error| format!("motion {name} must be a number or a CSS length: {error}"))?;
                let x = self.resolve_length(&corner.x, lengths)?;
                match corner.y {
                    None => x,
                    Some(y) => {
                        let y = self.resolve_length(&y, lengths)?;
                        let radius = |value: &AnimValue| value.as_length().expect("a radius is a length");
                        AnimValue::ellipse(radius(&x), radius(&y))
                    }
                }
            }
            Kind::FontSize => {
                let length: LinearLength = serde_json::from_value(value.clone())
                    .map_err(|error| format!("motion {name} must be a number or a CSS length: {error}"))?;
                if length.is_negative() {
                    return Err(format!("motion {name} must be non-negative"));
                }
                AnimValue::Number(check_finite(name, lengths.declared_font_size(&length).max(0.0))?)
            }
            Kind::FontWeight => {
                let weight: FontWeightValue = serde_json::from_value(value.clone())
                    .map_err(|_| format!("motion {name} must be a number or a font weight"))?;
                AnimValue::Number(font_weight(&weight))
            }
            Kind::Dimension => {
                let dimension: DimensionValue = serde_json::from_value(value.clone())
                    .map_err(|error| format!("motion {name} must be a number or a CSS length: {error}"))?;
                match dimension {
                    DimensionValue::Pixels(value) => {
                        AnimValue::Number(check_finite(name, value)?)
                    }
                    DimensionValue::Percentage(value) => {
                        AnimValue::Percent(check_finite(name, value)?)
                    }
                    DimensionValue::Calc(length) => self.resolve_length(&length, lengths)?,
                    DimensionValue::AnchorSize(_) => {
                        return Err(format!("motion cannot animate {name} to or from anchor-size()"));
                    }
                    DimensionValue::Intrinsic(keyword) => {
                        return Err(format!(
                            "motion cannot animate {name} to or from {keyword}: only a length or a \
                             percentage, because the size of the content is measured by layout"
                        ));
                    }
                    DimensionValue::Auto if matches!(self, Self::Width | Self::Height) => AnimValue::Auto,
                    DimensionValue::Auto => {
                        return Err(format!(
                            "motion cannot animate {name} to or from auto: only width and height can, \
                             because the min and max sizes have no auto to measure"
                        ));
                    }
                }
            }
            Kind::Color => {
                let text = value
                    .as_str()
                    .ok_or_else(|| format!("motion {name} must be a colour string"))?;
                AnimValue::Color(
                    Color::parse(text, colors)
                        .ok_or_else(|| format!("motion {name} is not a valid colour: {text}"))?,
                )
            }
            Kind::Shadow => AnimValue::Shadows(parse_shadow_list(name, value, colors)?),
            Kind::Keyword => {
                return Err(format!("motion cannot animate {name}: it is discrete"));
            }
        };
        self.validate(&parsed)?;
        Ok(parsed)
    }

    /// A parsed length as a value. One that is negative in any context is an error;
    /// one that only resolves to a negative (`calc(1em - 20px)`) is clamped, as CSS
    /// clamps a `calc()` to its property's range. This depends on the element only
    /// through the clamp, because keyframes are checked once against a stand-in.
    fn resolve_length(self, length: &LinearLength, lengths: &LengthContext) -> Result<AnimValue, String> {
        let name = self.name();
        if !length.is_finite() {
            return Err(format!("motion {name} must fit a finite 32-bit float"));
        }
        if matches!(self.clamp_rule(), Clamp::NonNegative) && length.is_negative() {
            return Err(format!("motion {name} must be non-negative"));
        }
        let (px, percent) = lengths.split(length);
        let value = AnimValue::length(check_finite(name, px)?, check_finite(name, percent)?);
        Ok(self.clamp_value(value))
    }

    fn validate(self, value: &AnimValue) -> Result<(), String> {
        let name = self.name();
        let (AnimValue::Number(number) | AnimValue::Percent(number)) = value else {
            return Ok(());
        };
        // A mixed length is checked as a length (`resolve_length`).
        match self.clamp_rule() {
            Clamp::NonNegative if *number < 0.0 => {
                Err(format!("motion {name} must be non-negative"))
            }
            Clamp::Unit if !(0.0..=1.0).contains(number) => {
                Err(format!("motion {name} must be between 0 and 1"))
            }
            Clamp::Weight if !(1.0..=1000.0).contains(number) => {
                Err(format!("motion {name} must be between 1 and 1000"))
            }
            _ => Ok(()),
        }
    }

    /// Whether `write` can put `value` in this property.
    pub(super) fn accepts(self, value: &AnimValue) -> bool {
        matches!(
            (self.kind(), value),
            (Kind::Number | Kind::FontWeight | Kind::FontSize, AnimValue::Number(_))
                | (
                    Kind::Dimension | Kind::Length | Kind::Corner,
                    AnimValue::Number(_) | AnimValue::Percent(_) | AnimValue::Calc { .. }
                )
                | (Kind::Corner, AnimValue::Ellipse { .. })
                | (Kind::Color, AnimValue::Color(_))
                | (Kind::Shadow, AnimValue::Shadows(_))
        )
    }

    /// Springs overshoot their target, so what is applied has to stay inside
    /// the range GPUI accepts. The unclamped value keeps driving the physics.
    pub(super) fn clamp_value(self, value: AnimValue) -> AnimValue {
        match (self.clamp_rule(), value) {
            (Clamp::NonNegative, AnimValue::Number(value)) => AnimValue::Number(value.max(0.0)),
            (Clamp::NonNegative, AnimValue::Percent(value)) => AnimValue::Percent(value.max(0.0)),
            // Negative whatever the basis is, and only then can it be held.
            (Clamp::NonNegative, AnimValue::Calc { px, percent }) if px <= 0.0 && percent <= 0.0 => {
                AnimValue::Number(0.0)
            }
            (Clamp::NonNegative, AnimValue::Ellipse { x, y }) => {
                let radius = |(px, percent): (f64, f64)| {
                    self.clamp_value(AnimValue::length(px, percent)).as_length().expect("a radius is a length")
                };
                AnimValue::ellipse(radius(x), radius(y))
            }
            (Clamp::Unit, AnimValue::Number(value)) => AnimValue::Number(value.clamp(0.0, 1.0)),
            (Clamp::Weight, AnimValue::Number(value)) => {
                AnimValue::Number(value.clamp(1.0, 1000.0))
            }
            (_, AnimValue::Shadows(mut shadows)) => {
                for shadow in &mut shadows {
                    shadow.blur = shadow.blur.max(0.0);
                }
                AnimValue::Shadows(shadows)
            }
            (_, value) => value,
        }
    }
}

/// A `boxShadow` value: one layer or a list of them.
pub(super) fn parse_shadow_list(
    name: &str,
    value: &Value,
    colors: &ColorContext,
) -> Result<Vec<Shadow>, String> {
    let shadows: BoxShadowValues = serde_json::from_value(value.clone())
        .map_err(|error| format!("motion {name}: {error}"))?;
    let shadows = shadows_from(name, shadows.as_slice(), colors)?;
    if shadows.iter().any(|shadow| shadow.blur < 0.0) {
        return Err(format!("motion {name} blur must be non-negative"));
    }
    Ok(shadows)
}

pub(super) fn shadow_layers(shadows: Vec<Shadow>) -> BoxShadowValues {
    BoxShadowValues::Many(
        shadows
            .into_iter()
            .map(|shadow| BoxShadowValue {
                offset_x: LinearLength::px(shadow.offset_x),
                offset_y: LinearLength::px(shadow.offset_y),
                blur_radius: LinearLength::px(shadow.blur),
                spread_radius: LinearLength::px(shadow.spread),
                color: shadow.color.to_css(),
                inset: shadow.inset,
            })
            .collect(),
    )
}

pub(super) fn check_finite(name: &str, value: f64) -> Result<f64, String> {
    if !value.is_finite() || value.abs() > f32::MAX as f64 {
        return Err(format!("motion {name} must fit a finite 32-bit float"));
    }
    Ok(value)
}

fn parse_number(name: &str, value: &Value) -> Result<f64, String> {
    let number = value
        .as_f64()
        .ok_or_else(|| format!("motion {name} must be a number"))?;
    check_finite(name, number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Scheme;

    #[test]
    fn every_property_round_trips_its_name() {
        for name in ["opacity", "boxShadow", "borderBottomRightRadius", "scrollbarWidth"] {
            assert_eq!(AnimProp::from_name(name).unwrap().name(), name);
        }
        for discrete in ["display", "position", "flexDirection", "padding", "bogus"] {
            assert!(AnimProp::from_name(discrete).is_none(), "{discrete}");
        }
    }

    #[test]
    fn discrete_properties_are_css_names_but_not_animatable() {
        for name in ["display", "visibility", "flexDirection", "justifySelf"] {
            let prop = AnimProp::from_css_name(name).unwrap();
            assert!(prop.is_discrete(), "{name}");
            assert!(AnimProp::from_name(name).is_none(), "{name}");
        }
        assert!(!AnimProp::from_css_name("opacity").unwrap().is_discrete());
        assert!(AnimProp::from_css_name("position").is_none());
    }

    #[test]
    fn an_unset_discrete_property_starts_from_its_initial_keyword() {
        let colors = Colors::root();
        assert_eq!(AnimProp::Display.initial(&colors), Some(AnimValue::Keyword(None)));
        assert_eq!(
            AnimProp::Visibility.initial(&colors),
            Some(AnimValue::Keyword(Some("visible".to_string())))
        );
        let style = StyleDesc { display: Some("grid".to_string()), ..Default::default() };
        assert_eq!(
            AnimProp::Display.read(&style, &colors),
            Some(AnimValue::Keyword(Some("grid".to_string())))
        );
        assert_eq!(AnimProp::Visibility.read(&style, &colors), None);
    }

    /// Colours for an element with `1em` = 20px, `1rem` = 10px and an 800x600 window; its
    /// parent's font size is 10px, which is what its own `fontSize` resolves against.
    fn sized_colors() -> Colors {
        let mut colors = Colors::root();
        colors.lengths = LengthContext::for_tests();
        colors.font.size = 20.0;
        colors
    }

    fn json(value: &str) -> serde_json::Value {
        serde_json::from_str(value).unwrap()
    }

    #[test]
    fn a_font_size_reads_and_parses_lengths_against_the_parent_font() {
        let colors = sized_colors();
        let read = |json: &str| {
            let style: StyleDesc = serde_json::from_str(json).unwrap();
            AnimProp::FontSize.read(&style, &colors)
        };
        assert_eq!(read(r#"{ "fontSize": 18 }"#), Some(AnimValue::Number(18.0)));
        assert_eq!(read(r#"{ "fontSize": "18px" }"#), Some(AnimValue::Number(18.0)));
        assert_eq!(read(r#"{ "fontSize": "1.5em" }"#), Some(AnimValue::Number(15.0)));
        assert_eq!(read(r#"{ "fontSize": "50%" }"#), Some(AnimValue::Number(5.0)));
        assert_eq!(read(r#"{ "fontSize": "calc(1rem + 2vw)" }"#), Some(AnimValue::Number(26.0)));
        assert_eq!(read(r#"{ "fontSize": "calc(1em - 40px)" }"#), Some(AnimValue::Number(0.0)));
        assert_eq!(read("{}"), None);

        let mut style = StyleDesc::default();
        AnimProp::FontSize.write(&mut style, AnimValue::Number(24.0));
        assert_eq!(style.font_size, Some(LinearLength::px(24.0)));
    }
    #[test]
    fn a_font_size_target_is_px_by_the_time_it_is_parsed() {
        let colors = sized_colors();
        let parse = |value: serde_json::Value| AnimProp::FontSize.parse(&value, &colors);
        assert_eq!(parse(serde_json::json!(18)).unwrap(), AnimValue::Number(18.0));
        assert_eq!(parse(serde_json::json!("2em")).unwrap(), AnimValue::Number(20.0));
        assert_eq!(parse(serde_json::json!("200%")).unwrap(), AnimValue::Number(20.0));
        assert_eq!(parse(serde_json::json!("calc(1em + 2px)")).unwrap(), AnimValue::Number(12.0));
        assert_eq!(parse(serde_json::json!("calc(1em - 20px)")).unwrap(), AnimValue::Number(0.0));
        assert!(parse(serde_json::json!(-1)).is_err());
        assert!(parse(serde_json::json!("-1em")).is_err());
        let error = parse(serde_json::json!("big")).unwrap_err();
        assert!(error.starts_with("motion fontSize must be a number or a CSS length"), "{error}");
    }
    #[test]
    fn a_line_height_animates_as_a_multiplier_and_a_string_says_why_it_cannot() {
        let colors = Colors::root();
        let read = |json: &str| {
            let style: StyleDesc = serde_json::from_str(json).unwrap();
            AnimProp::LineHeight.read(&style, &colors)
        };
        assert_eq!(read(r#"{ "lineHeight": 1.5 }"#), Some(AnimValue::Number(1.5)));
        assert_eq!(read(r#"{ "lineHeight": "1.5" }"#), Some(AnimValue::Number(1.5)));
        assert_eq!(read(r#"{ "lineHeight": "24px" }"#), None);
        assert_eq!(read(r#"{ "lineHeight": "normal" }"#), None);

        let mut style = StyleDesc::default();
        AnimProp::LineHeight.write(&mut style, AnimValue::Number(2.0));
        assert_eq!(style.line_height, Some(LineHeight::Multiplier(2.0)));

        assert_eq!(
            AnimProp::LineHeight.parse(&serde_json::json!(1.4), &colors).unwrap(),
            AnimValue::Number(1.4)
        );
        for target in ["24px", "1.5em", "150%", "normal", "1.5"] {
            let error = AnimProp::LineHeight.parse(&serde_json::json!(target), &colors).unwrap_err();
            assert!(
                error.starts_with("motion cannot animate lineHeight to or from normal or a CSS length"),
                "{target}: {error}"
            );
        }
        assert!(AnimProp::LineHeight.parse(&serde_json::json!(-1), &colors).is_err());
    }

    #[test]
    fn a_box_shadow_animates_px_and_says_why_a_css_length_does_not() {
        let colors = Colors::root();
        let target = |value: serde_json::Value| AnimProp::BoxShadow.parse(&value, &colors);
        assert!(matches!(target(serde_json::json!("0 2px 4px red")), Ok(AnimValue::Shadows(_))));
        assert!(matches!(
            target(serde_json::json!({"offsetX": 0, "offsetY": 2, "blurRadius": "4px", "spreadRadius": 0, "color": "red"})),
            Ok(AnimValue::Shadows(_))
        ));
        for shadow in ["0 0 1em red", "calc(1px + 1rem) 0 red", "0 0 0 5vw red"] {
            let error = target(serde_json::json!(shadow)).unwrap_err();
            assert!(error.starts_with("motion cannot animate boxShadow to or from a CSS length"), "{error}");
        }
        assert!(target(serde_json::json!("10% 0 red")).is_err());

        let read = |json: &str| {
            let style: StyleDesc = serde_json::from_str(json).unwrap();
            AnimProp::BoxShadow.read(&style, &colors)
        };
        assert!(matches!(read(r#"{ "boxShadow": "0 2px 4px red" }"#), Some(AnimValue::Shadows(_))));
        assert_eq!(read(r#"{ "boxShadow": "0 0 1em red" }"#), None);
    }

    #[test]
    fn a_calc_dimension_resolves_its_relative_units_and_keeps_its_percentage() {
        let colors = sized_colors();
        let read = |json: &str| {
            let style: StyleDesc = serde_json::from_str(json).unwrap();
            AnimProp::Width.read(&style, &colors)
        };
        assert_eq!(read(r#"{ "width": "calc(100% - 20px)" }"#), Some(AnimValue::Calc { px: -20.0, percent: 1.0 }));
        assert_eq!(read(r#"{ "width": "10em" }"#), Some(AnimValue::Number(200.0)));
        assert_eq!(read(r#"{ "width": "calc(50% + 1rem + 5vw)" }"#), Some(AnimValue::Calc { px: 50.0, percent: 0.5 }));
        assert_eq!(read(r#"{ "width": "calc(0% + 2em)" }"#), Some(AnimValue::Number(40.0)));
        assert_eq!(read(r#"{ "width": "2in" }"#), Some(AnimValue::Number(192.0)));
        assert_eq!(read(r#"{ "width": "50%" }"#), Some(AnimValue::Percent(0.5)));
        assert_eq!(read(r#"{ "width": "auto" }"#), None);

        for prop in [AnimProp::Width, AnimProp::MinHeight, AnimProp::MarginLeft, AnimProp::Top] {
            let parse = |target: &str| prop.parse(&serde_json::json!(target), &colors).unwrap();
            assert_eq!(parse("calc(100% - 20px)"), AnimValue::Calc { px: -20.0, percent: 1.0 }, "{prop:?}");
            assert_eq!(parse("50vw"), AnimValue::Number(400.0), "{prop:?}");
            assert_eq!(parse("2em"), AnimValue::Number(40.0), "{prop:?}");
            assert_eq!(parse("1.5rem"), AnimValue::Number(15.0), "{prop:?}");
            assert_eq!(parse("calc(2 * (10% + 1em))"), AnimValue::Calc { px: 40.0, percent: 0.2 }, "{prop:?}");
        }
        assert_eq!(
            AnimProp::Width.parse(&serde_json::json!("2in"), &colors).unwrap(),
            AnimValue::Number(192.0)
        );
        let error = AnimProp::Width.parse(&serde_json::json!("12foo"), &colors).unwrap_err();
        assert!(error.contains("motion width must be a number or a CSS length") && error.contains("12foo"), "{error}");
    }

    #[test]
    fn a_calc_is_held_to_its_range_and_a_clamped_one_does_not_depend_on_the_element() {
        let colors = sized_colors();
        // Negative in any context is an error; negative here only is clamped, as CSS
        // clamps a `calc()`. A keyframe checked against the root stays valid on another element.
        assert!(AnimProp::Width.parse(&serde_json::json!("calc(-10% - 1px)"), &colors).is_err());
        assert_eq!(
            AnimProp::Width.parse(&serde_json::json!("calc(1em - 30px)"), &colors).unwrap(),
            AnimValue::Number(0.0)
        );
        assert_eq!(
            AnimProp::Width.parse(&serde_json::json!("calc(1em - 30px)"), &Colors::root()).unwrap(),
            AnimValue::Number(0.0)
        );
        assert_eq!(
            AnimProp::MarginLeft.parse(&serde_json::json!("calc(1em - 30px)"), &colors).unwrap(),
            AnimValue::Number(-10.0)
        );
        assert_eq!(
            AnimProp::Width.clamp_value(AnimValue::Calc { px: -5.0, percent: -0.1 }),
            AnimValue::Number(0.0)
        );
        assert_eq!(
            AnimProp::Width.clamp_value(AnimValue::Calc { px: -5.0, percent: 0.1 }),
            AnimValue::Calc { px: -5.0, percent: 0.1 }
        );
    }
    #[test]
    fn a_gap_takes_a_length_or_a_percentage_of_the_content_box() {
        let colors = sized_colors();
        let props = [
            (AnimProp::RowGap, "rowGap"),
            (AnimProp::ColumnGap, "columnGap"),
        ];
        for (prop, name) in props {
            let read = |json: &str| prop.read(&serde_json::from_str::<StyleDesc>(json).unwrap(), &colors);
            assert_eq!(read(&format!(r#"{{ "{name}": 6 }}"#)), Some(AnimValue::Number(6.0)));
            assert_eq!(read(&format!(r#"{{ "{name}": "6px" }}"#)), Some(AnimValue::Number(6.0)));
            assert_eq!(read(&format!(r#"{{ "{name}": "1in" }}"#)), Some(AnimValue::Number(96.0)));
            assert_eq!(read(&format!(r#"{{ "{name}": "2em" }}"#)), Some(AnimValue::Number(40.0)));
            assert_eq!(
                read(&format!(r#"{{ "{name}": "calc(10% + 4px)" }}"#)),
                Some(AnimValue::Calc { px: 4.0, percent: 0.1 })
            );
            assert_eq!(read(&format!(r#"{{ "gap": "10%" }}"#)), Some(AnimValue::Percent(0.1)));

            assert_eq!(prop.parse(&serde_json::json!(12), &colors).unwrap(), AnimValue::Number(12.0));
            assert!(prop.parse(&serde_json::json!(-1), &colors).is_err());
            assert_eq!(prop.parse(&serde_json::json!("12px"), &colors).unwrap(), AnimValue::Number(12.0));
            assert_eq!(prop.parse(&serde_json::json!("2em"), &colors).unwrap(), AnimValue::Number(40.0));
            assert_eq!(prop.parse(&serde_json::json!("10%"), &colors).unwrap(), AnimValue::Percent(0.1));
            assert_eq!(
                prop.parse(&serde_json::json!("calc(10% + 4px)"), &colors).unwrap(),
                AnimValue::Calc { px: 4.0, percent: 0.1 }
            );
            let error = prop.parse(&serde_json::json!("wide"), &colors).unwrap_err();
            assert!(error.starts_with(&format!("motion {name} must be a number or a CSS length")), "{error}");
        }

        let mut style = StyleDesc::default();
        AnimProp::RowGap.write(&mut style, AnimValue::Number(3.0));
        AnimProp::ColumnGap.write(&mut style, AnimValue::Calc { px: 3.0, percent: 0.5 });
        assert_eq!(style.row_gap, Some(LinearLength::px(3.0)));
        assert_eq!(style.column_gap, Some(LinearLength { px: 3.0, percent: 0.5, ..LinearLength::default() }));
    }
    #[test]
    fn padding_is_a_length_a_percentage_or_a_mix() {
        let colors = sized_colors();
        for (prop, name) in [(AnimProp::PaddingTop, "paddingTop"), (AnimProp::PaddingLeft, "paddingLeft")] {
            let read = |json: &str| prop.read(&serde_json::from_str::<StyleDesc>(json).unwrap(), &colors);
            assert_eq!(read(&format!(r#"{{ "{name}": 6 }}"#)), Some(AnimValue::Number(6.0)));
            assert_eq!(read(&format!(r#"{{ "{name}": "6px" }}"#)), Some(AnimValue::Number(6.0)));
            assert_eq!(read(&format!(r#"{{ "{name}": "25%" }}"#)), Some(AnimValue::Percent(0.25)));
            assert_eq!(read(&format!(r#"{{ "{name}": "2em" }}"#)), Some(AnimValue::Number(40.0)));
            assert_eq!(
                read(&format!(r#"{{ "{name}": "calc(10% + 4px)" }}"#)),
                Some(AnimValue::Calc { px: 4.0, percent: 0.1 })
            );

            assert_eq!(prop.parse(&serde_json::json!(12), &colors).unwrap(), AnimValue::Number(12.0));
            assert_eq!(prop.parse(&serde_json::json!("12px"), &colors).unwrap(), AnimValue::Number(12.0));
            assert_eq!(prop.parse(&serde_json::json!("10%"), &colors).unwrap(), AnimValue::Percent(0.1));
            assert_eq!(prop.parse(&serde_json::json!("2em"), &colors).unwrap(), AnimValue::Number(40.0));
            assert_eq!(prop.parse(&serde_json::json!("1vw"), &colors).unwrap(), AnimValue::Number(8.0));
            assert_eq!(
                prop.parse(&serde_json::json!("calc(10% + 1em)"), &colors).unwrap(),
                AnimValue::Calc { px: 20.0, percent: 0.1 }
            );
            assert!(prop.parse(&serde_json::json!(-1), &colors).is_err());
            assert!(prop.parse(&serde_json::json!("-10%"), &colors).is_err());
            assert!(prop.parse(&serde_json::json!("-1em"), &colors).is_err());
        }

        let mut style = StyleDesc::default();
        AnimProp::PaddingTop.write(&mut style, AnimValue::Number(9.0));
        AnimProp::PaddingLeft.write(&mut style, AnimValue::Percent(0.5));
        AnimProp::PaddingRight.write(&mut style, AnimValue::Calc { px: 4.0, percent: 0.5 });
        assert_eq!(style.padding_top, Some(LinearLength::px(9.0)));
        assert_eq!(style.padding_left, Some(LinearLength::percent(0.5)));
        assert_eq!(style.padding_right, Some(LinearLength { px: 4.0, percent: 0.5, ..LinearLength::default() }));
    }
    #[test]
    fn the_percentage_axis_of_padding_is_the_width_and_of_flex_basis_the_main_axis() {
        for side in [AnimProp::PaddingTop, AnimProp::PaddingRight, AnimProp::PaddingBottom, AnimProp::PaddingLeft] {
            assert_eq!(side.percent_axis(Axis::Height), Some(Axis::Width), "{side:?}");
        }
        assert_eq!(AnimProp::FlexBasis.percent_axis(Axis::Width), Some(Axis::Width));
        assert_eq!(AnimProp::FlexBasis.percent_axis(Axis::Height), Some(Axis::Height));
        assert_eq!(AnimProp::Opacity.percent_axis(Axis::Width), None);
    }

    #[test]
    fn an_unset_font_size_and_weight_are_the_computed_ones() {
        let font = crate::motion::value::Font { size: 18.0, weight: 700.0 };
        let mut colors = Colors::root();
        colors.font = font;
        assert_eq!(AnimProp::FontSize.initial(&colors), Some(AnimValue::Number(18.0)));
        assert_eq!(AnimProp::FontWeight.initial(&colors), Some(AnimValue::Number(700.0)));
    }

    #[test]
    fn a_border_width_or_outline_takes_a_length_but_not_a_percentage() {
        let colors = sized_colors();
        let props = [
            (AnimProp::BorderTopWidth, "borderTopWidth"),
            (AnimProp::BorderRightWidth, "borderRightWidth"),
            (AnimProp::BorderBottomWidth, "borderBottomWidth"),
            (AnimProp::BorderLeftWidth, "borderLeftWidth"),
            (AnimProp::OutlineWidth, "outlineWidth"),
            (AnimProp::OutlineOffset, "outlineOffset"),
        ];
        for (prop, name) in props {
            let read = |json: &str| prop.read(&serde_json::from_str::<StyleDesc>(json).unwrap(), &colors);
            assert_eq!(read(&format!(r#"{{ "{name}": 2 }}"#)), Some(AnimValue::Number(2.0)));
            assert_eq!(read(&format!(r#"{{ "{name}": "1in" }}"#)), Some(AnimValue::Number(96.0)));
            assert_eq!(read(&format!(r#"{{ "{name}": "0.5em" }}"#)), Some(AnimValue::Number(10.0)));
            assert_eq!(prop.parse(&serde_json::json!(4), &colors).unwrap(), AnimValue::Number(4.0));
            assert_eq!(prop.parse(&serde_json::json!("3px"), &colors).unwrap(), AnimValue::Number(3.0));
            assert_eq!(prop.parse(&serde_json::json!("0.5em"), &colors).unwrap(), AnimValue::Number(10.0));
            assert_eq!(prop.parse(&serde_json::json!("calc(1px + 1rem)"), &colors).unwrap(), AnimValue::Number(11.0));
            for target in ["50%", "calc(10% + 1px)"] {
                let error = prop.parse(&serde_json::json!(target), &colors).unwrap_err();
                assert_eq!(error, format!("motion {name} cannot be a percentage: it takes a length"), "{target}");
            }
            let error = prop.parse(&serde_json::json!("thick"), &colors).unwrap_err();
            assert!(error.starts_with(&format!("motion {name} must be a number or a CSS length")), "{error}");
        }
        let mut style = StyleDesc::default();
        AnimProp::BorderLeftWidth.write(&mut style, AnimValue::Number(2.0));
        AnimProp::OutlineOffset.write(&mut style, AnimValue::Number(-1.0));
        assert_eq!(style.border_left_width, Some(LinearLength::px(2.0)));
        assert_eq!(style.outline_offset, Some(LinearLength::px(-1.0)));
        assert!(AnimProp::BorderTopWidth.parse(&serde_json::json!(-1), &colors).is_err());
        assert_eq!(AnimProp::OutlineOffset.parse(&serde_json::json!(-1), &colors).unwrap(), AnimValue::Number(-1.0));
    }
    #[test]
    fn flex_basis_animates_between_lengths_and_percentages_but_not_auto() {
        let colors = sized_colors();
        let read = |json: &str| {
            AnimProp::FlexBasis.read(&serde_json::from_str::<StyleDesc>(json).unwrap(), &colors)
        };
        assert_eq!(read(r#"{ "flexBasis": 40 }"#), Some(AnimValue::Number(40.0)));
        assert_eq!(read(r#"{ "flexBasis": "50%" }"#), Some(AnimValue::Percent(0.5)));
        assert_eq!(read(r#"{ "flexBasis": "auto" }"#), None);
        assert_eq!(
            read(r#"{ "flexBasis": "calc(100% - 40px)" }"#),
            Some(AnimValue::Calc { px: -40.0, percent: 1.0 })
        );

        assert_eq!(
            AnimProp::FlexBasis.parse(&serde_json::json!("25%"), &colors).unwrap(),
            AnimValue::Percent(0.25)
        );
        assert_eq!(
            AnimProp::FlexBasis.parse(&serde_json::json!("calc(100% - 2em)"), &colors).unwrap(),
            AnimValue::Calc { px: -40.0, percent: 1.0 }
        );
        assert!(AnimProp::FlexBasis.parse(&serde_json::json!("auto"), &colors).is_err());

        let mut style = StyleDesc::default();
        AnimProp::FlexBasis.write(&mut style, AnimValue::Percent(0.5));
        assert_eq!(style.flex_basis, Some(DimensionValue::Percentage(0.5)));
        AnimProp::FlexBasis.write(&mut style, AnimValue::Calc { px: 3.0, percent: 0.5 });
        assert_eq!(
            style.flex_basis,
            Some(DimensionValue::Calc(LinearLength { px: 3.0, percent: 0.5, ..LinearLength::default() }))
        );
    }
    #[test]
    fn an_intrinsic_size_keyword_is_not_a_start_value_or_a_target() {
        let colors = Colors::root();
        for (prop, field) in [
            (AnimProp::Width, "width"),
            (AnimProp::Height, "height"),
            (AnimProp::MinWidth, "minWidth"),
            (AnimProp::MaxHeight, "maxHeight"),
            (AnimProp::FlexBasis, "flexBasis"),
        ] {
            let style = serde_json::from_str::<StyleDesc>(&format!(r#"{{ "{field}": "fit-content(10px)" }}"#)).unwrap();
            assert_eq!(prop.read(&style, &colors), None, "{field}");
            for target in ["min-content", "max-content", "fit-content", "fit-content(50%)"] {
                let error = prop.parse(&serde_json::json!(target), &colors).unwrap_err();
                assert!(
                    error.starts_with(&format!("motion cannot animate {field} to or from {target}")),
                    "{error}"
                );
            }
        }
    }

    #[test]
    fn a_longhand_reads_through_its_shorthand() {
        let colors = Colors::root();
        let style = StyleDesc {
            padding: Some(Sides::all(LinearLength::px(8.0))),
            padding_left: Some(LinearLength::px(20.0)),
            border_radius: Some(BorderRadiusValue::px(6.0)),
            ..Default::default()
        };
        assert_eq!(
            AnimProp::PaddingTop.read(&style, &colors),
            Some(AnimValue::Number(8.0))
        );
        assert_eq!(
            AnimProp::PaddingLeft.read(&style, &colors),
            Some(AnimValue::Number(20.0))
        );
        assert_eq!(
            AnimProp::BorderTopLeftRadius.read(&style, &colors),
            Some(AnimValue::Number(6.0))
        );
        assert_eq!(AnimProp::MarginTop.read(&style, &colors), None);
    }

    #[test]
    fn the_flex_and_outline_longhands_read_through_their_shorthands() {
        let colors = Colors::root();
        let style: StyleDesc = serde_json::from_str(
            r#"{"flex": "2 3 40px", "flexShrink": 0, "outline": "5px solid red", "outlineColor": "blue"}"#,
        )
        .unwrap();
        assert_eq!(AnimProp::FlexGrow.read(&style, &colors), Some(AnimValue::Number(2.0)));
        assert_eq!(AnimProp::FlexShrink.read(&style, &colors), Some(AnimValue::Number(0.0)));
        assert_eq!(AnimProp::FlexBasis.read(&style, &colors), AnimProp::FlexBasis.read(
            &serde_json::from_str(r#"{"flexBasis": 40}"#).unwrap(),
            &colors
        ));
        assert_eq!(AnimProp::OutlineWidth.read(&style, &colors), Some(AnimValue::Number(5.0)));
        let blue = AnimProp::OutlineColor.read(&serde_json::from_str(r#"{"outlineColor": "blue"}"#).unwrap(), &colors);
        assert_eq!(AnimProp::OutlineColor.read(&style, &colors), blue, "the longhand wins");
        let red = AnimProp::OutlineColor.read(&serde_json::from_str(r#"{"outlineColor": "red"}"#).unwrap(), &colors);
        let only_shorthand: StyleDesc = serde_json::from_str(r#"{"outline": "5px solid red"}"#).unwrap();
        assert_eq!(AnimProp::OutlineColor.read(&only_shorthand, &colors), red);
        assert_eq!(AnimProp::FlexGrow.read(&StyleDesc::default(), &colors), None);
    }

    #[test]
    fn a_radius_reads_as_a_length_or_as_two_when_the_corner_is_elliptical() {
        let colors = sized_colors();
        let read = |json: &str| {
            let style: StyleDesc = serde_json::from_str(json).unwrap();
            AnimProp::BorderTopLeftRadius.read(&style, &colors)
        };
        assert_eq!(read(r##"{"borderTopLeftRadius":"9px"}"##), Some(AnimValue::Number(9.0)));
        assert_eq!(read(r##"{"borderRadius":"4px 8px"}"##), Some(AnimValue::Number(4.0)));
        assert_eq!(read(r##"{"borderRadius":"50%"}"##), Some(AnimValue::Percent(0.5)));
        assert_eq!(read(r##"{"borderRadius":"1em"}"##), Some(AnimValue::Number(20.0)));
        assert_eq!(read(r##"{"borderTopLeftRadius":"calc(10% + 2px)"}"##), Some(AnimValue::Calc { px: 2.0, percent: 0.1 }));
        assert_eq!(
            read(r##"{"borderTopLeftRadius":"10px 20px"}"##),
            Some(AnimValue::Ellipse { x: (10.0, 0.0), y: (20.0, 0.0) })
        );
        assert_eq!(
            read(r##"{"borderRadius":"1em 2em / 10% 3px"}"##),
            Some(AnimValue::Ellipse { x: (20.0, 0.0), y: (0.0, 0.1) })
        );
        assert_eq!(read(r##"{"borderTopLeftRadius":"10px 10px"}"##), Some(AnimValue::Number(10.0)));
        assert_eq!(read(r##"{"borderRadius":"10px","borderTopLeftRadius":"30%"}"##), Some(AnimValue::Percent(0.3)));
    }

    #[test]
    fn a_motion_radius_is_a_length_or_two_and_writes_a_circular_or_elliptical_corner() {
        let colors = sized_colors();
        let parse = |value: &str| AnimProp::BorderTopLeftRadius.parse(&json(value), &colors);
        assert_eq!(parse(r#""50%""#).unwrap(), AnimValue::Percent(0.5));
        assert_eq!(parse(r#""2em""#).unwrap(), AnimValue::Number(40.0));
        assert_eq!(parse(r#""calc(50% - 2px)""#).unwrap(), AnimValue::Calc { px: -2.0, percent: 0.5 });
        assert!(parse("-1").is_err());
        let mut style: StyleDesc = serde_json::from_str(r##"{"borderTopLeftRadius":"50% 20px"}"##).unwrap();
        AnimProp::BorderTopLeftRadius.write(&mut style, AnimValue::Number(7.0));
        assert_eq!(style.border_top_left_radius, Some(CornerRadiusValue::px(7.0)));
        AnimProp::BorderTopLeftRadius.write(&mut style, AnimValue::Percent(0.5));
        assert_eq!(style.border_top_left_radius, Some(CornerRadiusValue::circular(LinearLength::percent(0.5))));

        let ellipse = AnimValue::Ellipse { x: (10.0, 0.0), y: (2.0, 0.5) };
        assert_eq!(parse(r#""10px calc(50% + 2px)""#).unwrap(), ellipse);
        assert!(parse(r#""10px -1px""#).is_err());
        // The same radius twice is a circle, and so it stays one value.
        assert_eq!(parse(r#""10px 10px""#).unwrap(), AnimValue::Number(10.0));
        AnimProp::BorderTopLeftRadius.write(&mut style, ellipse);
        assert_eq!(
            style.border_top_left_radius,
            Some(CornerRadiusValue { x: LinearLength::px(10.0), y: Some(LinearLength { px: 2.0, percent: 0.5, ..Default::default() }) })
        );
        assert_eq!(
            AnimProp::BorderTopLeftRadius.clamp_value(AnimValue::Ellipse { x: (-4.0, 0.0), y: (5.0, 0.0) }),
            AnimValue::Ellipse { x: (0.0, 0.0), y: (5.0, 0.0) }
        );
    }

    fn split(name: &str, value: &str) -> Result<Vec<(AnimProp, serde_json::Value)>, String> {
        split_shorthand(name, &json(value)).expect("a shorthand").map(|split| split.channels)
    }

    fn texts(name: &str, value: &str) -> Vec<String> {
        split(name, value)
            .unwrap()
            .into_iter()
            .map(|(_, value)| value.as_str().expect("a string").to_string())
            .collect()
    }

    #[test]
    fn a_box_shorthand_string_gives_each_side_its_own_value() {
        assert_eq!(texts("padding", r#""1px 2px""#), ["1px", "2px", "1px", "2px"]);
        assert_eq!(texts("padding", r#""1px 2px 3px""#), ["1px", "2px", "3px", "2px"]);
        assert_eq!(texts("borderWidth", r#""1px 2px 3px 4px""#), ["1px", "2px", "3px", "4px"]);
        assert_eq!(texts("margin", r#""0 auto""#), ["0", "auto", "0", "auto"]);
        assert_eq!(texts("inset", r#""10px 20px""#), ["10px", "20px", "10px", "20px"]);
        assert_eq!(texts("borderColor", r#""red rgb(0 0 255)""#), ["red", "rgb(0 0 255)", "red", "rgb(0 0 255)"]);
        assert_eq!(
            split("inset", r#""10px 20px""#).unwrap().iter().map(|(prop, _)| *prop).collect::<Vec<_>>(),
            [AnimProp::Top, AnimProp::Right, AnimProp::Bottom, AnimProp::Left]
        );
        assert!(split("padding", r#""1px 2px 3px 4px 5px""#).is_err());
        assert!(split("padding", r#""""#).is_err());
    }

    #[test]
    fn a_gap_string_is_the_row_then_the_column() {
        assert_eq!(texts("gap", r#""4px 8px""#), ["4px", "8px"]);
        assert_eq!(texts("gap", r#""4px""#), ["4px", "4px"]);
        assert!(split("gap", r#""1px 2px 3px""#).is_err());
    }

    #[test]
    fn a_border_radius_string_gives_each_corner_its_radii_in_gpuix_order() {
        // Top left, top right, bottom left, bottom right.
        assert_eq!(texts("borderRadius", r#""1px 2px 3px 4px""#), ["1px", "2px", "4px", "3px"]);
        assert_eq!(
            texts("borderRadius", r#""1px 2px 3px 4px / 5px 6px""#),
            ["1px 5px", "2px 6px", "4px 6px", "3px 5px"]
        );
        assert_eq!(texts("borderRadius", r#""10px/20px""#), ["10px 20px"; 4]);
    }

    #[test]
    fn a_number_or_a_keyframes_list_is_split_value_by_value() {
        let numbers = split("padding", "8").unwrap();
        assert!(numbers.iter().all(|(_, value)| *value == json("8")));
        let frames = split("padding", r#"["1px 2px", null, 3]"#).unwrap();
        assert_eq!(frames[0].1, json(r#"["1px", null, 3]"#));
        assert_eq!(frames[1].1, json(r#"["2px", null, 3]"#));
        assert_eq!(frames[3].1, json(r#"["2px", null, 3]"#));
    }

    #[test]
    fn a_shorthand_error_names_the_shorthand_unless_the_sides_differ() {
        let split = |name: &str, value: &str| split_shorthand(name, &json(value)).unwrap().unwrap();
        let uniform = split("padding", "-1");
        assert_eq!(uniform.error(AnimProp::PaddingTop, "motion paddingTop must be non-negative".into()), "motion padding must be non-negative");
        let mixed = split("padding", r#""1px -1px""#);
        assert_eq!(mixed.error(AnimProp::PaddingRight, "motion paddingRight must be non-negative".into()), "motion paddingRight must be non-negative");
    }
    #[test]
    fn clamps_hold_the_applied_value_in_range() {
        assert_eq!(
            AnimProp::Opacity.clamp_value(AnimValue::Number(1.2)),
            AnimValue::Number(1.0)
        );
        assert_eq!(
            AnimProp::Width.clamp_value(AnimValue::Number(-4.0)),
            AnimValue::Number(0.0)
        );
        assert_eq!(
            AnimProp::MarginLeft.clamp_value(AnimValue::Number(-4.0)),
            AnimValue::Number(-4.0)
        );
        assert_eq!(
            AnimProp::Top.clamp_value(AnimValue::Number(-4.0)),
            AnimValue::Number(-4.0)
        );
    }

    #[test]
    fn current_colour_comes_from_the_elements_own_colour() {
        let style = StyleDesc {
            color: Some("#ff0000".to_string()),
            ..Default::default()
        };
        let colors = Colors::of(&ColorContext::root(Scheme::Light), Some(&style));
        assert_eq!(
            AnimProp::BorderTopColor.initial(&colors),
            AnimProp::Color.read(&style, &colors)
        );
        // With no `color` of its own it is the inherited one.
        let inherited = Colors::of(&ColorContext::root(Scheme::Light), None);
        assert_eq!(
            AnimProp::BorderTopColor.initial(&inherited),
            Some(AnimValue::Color(Color::from_rgba(gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, false)))
        );
    }

    #[test]
    fn only_width_and_height_take_auto() {
        let colors = Colors::root();
        for prop in [AnimProp::Width, AnimProp::Height] {
            assert_eq!(prop.parse(&serde_json::json!("auto"), &colors), Ok(AnimValue::Auto), "{prop:?}");
        }
        for prop in [AnimProp::MinWidth, AnimProp::MaxWidth, AnimProp::MinHeight, AnimProp::MaxHeight] {
            let error = prop.parse(&serde_json::json!("auto"), &colors).unwrap_err();
            assert!(error.contains("only width and height"), "{prop:?}: {error}");
        }
    }

    #[test]
    fn an_auto_dimension_writes_back_as_auto() {
        let mut style = StyleDesc::default();
        AnimProp::Width.write(&mut style, AnimValue::Auto);
        assert_eq!(style.width, Some(DimensionValue::Auto));
    }

    #[test]
    fn rejects_out_of_range_and_wrong_typed_values() {
        for (prop, value) in [
            (AnimProp::Opacity, serde_json::json!(2.0)),
            (AnimProp::Width, serde_json::json!(-1.0)),
            (AnimProp::MinWidth, serde_json::json!("auto")),
            (AnimProp::MaxHeight, serde_json::json!("auto")),
            (AnimProp::Width, serde_json::json!(1e300)),
            (AnimProp::PaddingTop, serde_json::json!("12foo")),
            (AnimProp::BackgroundColor, serde_json::json!("not a colour")),
            (AnimProp::BackgroundColor, serde_json::json!(4)),
            (
                AnimProp::BoxShadow,
                serde_json::json!({ "offsetX": 0, "offsetY": 0, "blurRadius": -2, "spreadRadius": 0, "color": "#000" }),
            ),
        ] {
            assert!(prop.parse(&value, &Colors::root()).is_err(), "{prop:?} {value}");
        }
    }
}
