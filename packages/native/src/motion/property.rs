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
use crate::layout::aspect_ratio::AspectRatio;
use crate::style::{BoxShadowValue, BoxShadowValues, DimensionValue, FontWeightValue, StyleDesc};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Number,
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

/// What a property is before anything sets it, when its CSS initial value is
/// something an animation can start from.
#[derive(Clone, Copy)]
enum Initial {
    /// `auto`, `none`, `normal`, or inherited: not a value a track can start
    /// from until layout can resolve it.
    Unresolved,
    Number(f64),
    Transparent,
    /// The element's own `color`.
    CurrentColor,
    NoShadows,
    /// A keyword that is the same as leaving the property unset.
    Keyword(&'static str),
    /// A keyword whose unset value is the element's default, which the style
    /// cannot name.
    Unset,
}

/// How a `StyleDesc` field converts to and from an animated value.
trait Field {
    fn read(&self, colors: &ColorContext) -> Option<AnimValue>;
    fn write(&mut self, value: AnimValue);
}

impl Field for Option<f64> {
    fn read(&self, _colors: &ColorContext) -> Option<AnimValue> {
        self.map(AnimValue::Number)
    }

    fn write(&mut self, value: AnimValue) {
        match value {
            AnimValue::Number(value) => *self = Some(value),
            other => unreachable!("a number field was given {other:?}"),
        }
    }
}

impl Field for Option<DimensionValue> {
    fn read(&self, _colors: &ColorContext) -> Option<AnimValue> {
        match self.as_ref()? {
            DimensionValue::Pixels(value) => Some(AnimValue::Number(*value)),
            DimensionValue::Percentage(value) => Some(AnimValue::Percent(*value)),
            DimensionValue::Auto | DimensionValue::AnchorSize(_) => None,
        }
    }

    fn write(&mut self, value: AnimValue) {
        *self = Some(match value {
            AnimValue::Number(value) => DimensionValue::Pixels(value),
            AnimValue::Percent(value) => DimensionValue::Percentage(value),
            AnimValue::Auto => DimensionValue::Auto,
            other => unreachable!("a dimension field was given {other:?}"),
        });
    }
}

impl Field for Option<AspectRatio> {
    fn read(&self, _colors: &ColorContext) -> Option<AnimValue> {
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
    fn read(&self, colors: &ColorContext) -> Option<AnimValue> {
        Color::parse(self.as_deref()?, colors).map(AnimValue::Color)
    }

    fn write(&mut self, value: AnimValue) {
        match value {
            AnimValue::Color(color) => *self = Some(color.to_css()),
            other => unreachable!("a colour field was given {other:?}"),
        }
    }
}

impl Field for Option<FontWeightValue> {
    fn read(&self, _colors: &ColorContext) -> Option<AnimValue> {
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
    fn read(&self, colors: &ColorContext) -> Option<AnimValue> {
        shadows_from(self.as_ref()?.as_slice(), colors)
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
macro_rules! read_field {
    (Keyword, $slot:expr, $colors:expr) => {
        keyword_read(&$slot)
    };
    ($kind:ident, $slot:expr, $colors:expr) => {
        Field::read(&$slot, $colors)
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

fn font_weight(value: &FontWeightValue) -> f64 {
    crate::renderer::parse_font_weight(value).0 as f64
}

/// Reads the parsed per-layer structs, so every spelling `boxShadow` accepts
/// (single, list, and whatever string forms are added) animates the same way.
fn shadows_from(layers: &[BoxShadowValue], colors: &ColorContext) -> Result<Vec<Shadow>, String> {
    layers
        .iter()
        .map(|layer| {
            let color = Color::parse(&layer.color, colors)
                .ok_or_else(|| format!("motion boxShadow has an invalid colour: {}", layer.color))?;
            Ok(Shadow {
                offset_x: layer.offset_x,
                offset_y: layer.offset_y,
                blur: layer.blur_radius,
                spread: layer.spread_radius,
                color,
                inset: layer.inset,
            })
        })
        .collect()
}

macro_rules! properties {
    ($(
        $variant:ident $name:literal $kind:ident $clamp:ident $initial:expr,
        $field:ident $(| $fallback:ident)?;
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
            /// shorthand the renderer would apply beneath it.
            pub(super) fn read(self, style: &StyleDesc, colors: &Colors) -> Option<AnimValue> {
                let colors = self.colors(colors);
                match self {
                    $(Self::$variant => {
                        let value = read_field!($kind, style.$field, colors);
                        $(let value = value.or_else(|| read_field!($kind, style.$fallback, colors));)?
                        value
                    }),*
                }
            }

            pub(super) fn write(self, style: &mut StyleDesc, value: AnimValue) {
                match self {
                    $(Self::$variant => write_field!($kind, style.$field, value)),*
                }
            }
        }
    };
}

use Initial::{CurrentColor, Keyword as InitialKeyword, NoShadows, Number, Transparent, Unresolved, Unset};

properties! {
    FlexGrow "flexGrow" Number NonNegative Number(0.0), flex_grow;
    FlexShrink "flexShrink" Number NonNegative Number(1.0), flex_shrink;
    FlexBasis "flexBasis" Number NonNegative Unresolved, flex_basis;
    RowGap "rowGap" Number NonNegative Number(0.0), row_gap | gap;
    ColumnGap "columnGap" Number NonNegative Number(0.0), column_gap | gap;

    Width "width" Dimension NonNegative Unresolved, width;
    Height "height" Dimension NonNegative Unresolved, height;
    MinWidth "minWidth" Dimension NonNegative Unresolved, min_width;
    MinHeight "minHeight" Dimension NonNegative Unresolved, min_height;
    MaxWidth "maxWidth" Dimension NonNegative Unresolved, max_width;
    MaxHeight "maxHeight" Dimension NonNegative Unresolved, max_height;

    PaddingTop "paddingTop" Number NonNegative Number(0.0), padding_top | padding;
    PaddingRight "paddingRight" Number NonNegative Number(0.0), padding_right | padding;
    PaddingBottom "paddingBottom" Number NonNegative Number(0.0), padding_bottom | padding;
    PaddingLeft "paddingLeft" Number NonNegative Number(0.0), padding_left | padding;

    MarginTop "marginTop" Dimension None Number(0.0), margin_top | margin;
    MarginRight "marginRight" Dimension None Number(0.0), margin_right | margin;
    MarginBottom "marginBottom" Dimension None Number(0.0), margin_bottom | margin;
    MarginLeft "marginLeft" Dimension None Number(0.0), margin_left | margin;

    Top "top" Dimension None Unresolved, top;
    Right "right" Dimension None Unresolved, right;
    Bottom "bottom" Dimension None Unresolved, bottom;
    Left "left" Dimension None Unresolved, left;

    BackgroundColor "backgroundColor" Color None Transparent, background_color;
    Color "color" Color None Unresolved, color;
    Opacity "opacity" Number Unit Number(1.0), opacity;

    BorderTopWidth "borderTopWidth" Number NonNegative Number(0.0), border_top_width | border_width;
    BorderRightWidth "borderRightWidth" Number NonNegative Number(0.0), border_right_width | border_width;
    BorderBottomWidth "borderBottomWidth" Number NonNegative Number(0.0), border_bottom_width | border_width;
    BorderLeftWidth "borderLeftWidth" Number NonNegative Number(0.0), border_left_width | border_width;
    BorderColor "borderColor" Color None CurrentColor, border_color;
    BorderTopLeftRadius "borderTopLeftRadius" Number NonNegative Number(0.0), border_top_left_radius | border_radius;
    BorderTopRightRadius "borderTopRightRadius" Number NonNegative Number(0.0), border_top_right_radius | border_radius;
    BorderBottomLeftRadius "borderBottomLeftRadius" Number NonNegative Number(0.0), border_bottom_left_radius | border_radius;
    BorderBottomRightRadius "borderBottomRightRadius" Number NonNegative Number(0.0), border_bottom_right_radius | border_radius;
    BoxShadow "boxShadow" Shadow None NoShadows, box_shadow;
    AspectRatio "aspectRatio" Number NonNegative Unresolved, aspect_ratio;

    OutlineWidth "outlineWidth" Number NonNegative Number(0.0), outline_width;
    OutlineColor "outlineColor" Color None CurrentColor, outline_color;
    OutlineOffset "outlineOffset" Number None Number(0.0), outline_offset;

    FontSize "fontSize" Number NonNegative Unresolved, font_size;
    FontWeight "fontWeight" FontWeight Weight Number(400.0), font_weight;
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
        "gap" => &[RowGap, ColumnGap],
        _ => return None,
    })
}

/// The axis of a box: the side of the border box a size property measures, or
/// the side of the containing block a percentage resolves against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Axis {
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
    /// share of. Margins take the width on every side, as CSS does; insets take
    /// the height for `top` and `bottom`. `None` for a property that takes no
    /// percentage here.
    pub(super) fn percent_axis(self) -> Option<Axis> {
        match self {
            Self::MarginTop
            | Self::MarginRight
            | Self::MarginBottom
            | Self::MarginLeft
            | Self::Left
            | Self::Right => Some(Axis::Width),
            Self::Top | Self::Bottom => Some(Axis::Height),
            _ => self.size_axis(),
        }
    }

    /// Whether the property moves or resizes boxes, which reduced motion
    /// replaces with a jump (Framer disables its layout animations). Colours,
    /// opacity, shadows and radii are not among them.
    pub(super) fn is_layout(self) -> bool {
        matches!(
            self,
            Self::FlexBasis
                | Self::RowGap
                | Self::ColumnGap
                | Self::Width
                | Self::Height
                | Self::MinWidth
                | Self::MinHeight
                | Self::MaxWidth
                | Self::MaxHeight
                | Self::PaddingTop
                | Self::PaddingRight
                | Self::PaddingBottom
                | Self::PaddingLeft
                | Self::MarginTop
                | Self::MarginRight
                | Self::MarginBottom
                | Self::MarginLeft
                | Self::Top
                | Self::Right
                | Self::Bottom
                | Self::Left
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

    /// The value before the style sets it. `None` means CSS leaves it to layout
    /// or inheritance, which a track cannot start from yet.
    pub(super) fn initial(self, colors: &Colors) -> Option<AnimValue> {
        match self.initial_rule() {
            Initial::Unresolved => None,
            Initial::Number(value) => Some(AnimValue::Number(value)),
            Initial::Transparent => Some(AnimValue::Color(Color::TRANSPARENT)),
            Initial::CurrentColor => Some(AnimValue::Color(Color::from_rgba(colors.element.current, false))),
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

    /// Parses and validates one value from a description.
    pub(super) fn parse(self, value: &Value, colors: &Colors) -> Result<AnimValue, String> {
        let name = self.name();
        let colors = self.colors(colors);
        let parsed = match self.kind() {
            Kind::Number => AnimValue::Number(parse_number(name, value)?),
            Kind::FontWeight => {
                let weight: FontWeightValue = serde_json::from_value(value.clone())
                    .map_err(|_| format!("motion {name} must be a number or a font weight"))?;
                AnimValue::Number(font_weight(&weight))
            }
            Kind::Dimension => {
                let dimension: DimensionValue = serde_json::from_value(value.clone())
                    .map_err(|_| format!("motion {name} must be a number or a percentage"))?;
                match dimension {
                    DimensionValue::Pixels(value) => {
                        AnimValue::Number(check_finite(name, value)?)
                    }
                    DimensionValue::Percentage(value) => {
                        AnimValue::Percent(check_finite(name, value)?)
                    }
                    DimensionValue::AnchorSize(_) => {
                        return Err(format!("motion cannot animate {name} to or from anchor-size()"));
                    }
                    DimensionValue::Auto if self == Self::Height => AnimValue::Auto,
                    DimensionValue::Auto => {
                        return Err(format!(
                            "motion cannot animate {name} to or from auto: only height can, because auto \
                             width is the width of the content and measuring that needs a second layout pass"
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

    fn validate(self, value: &AnimValue) -> Result<(), String> {
        let name = self.name();
        let (AnimValue::Number(number) | AnimValue::Percent(number)) = value else {
            return Ok(());
        };
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
            (Kind::Number | Kind::FontWeight, AnimValue::Number(_))
                | (Kind::Dimension, AnimValue::Number(_) | AnimValue::Percent(_))
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
    for layer in shadows.as_slice() {
        for number in [
            layer.offset_x,
            layer.offset_y,
            layer.blur_radius,
            layer.spread_radius,
        ] {
            check_finite(name, number)?;
        }
        if layer.blur_radius < 0.0 {
            return Err(format!("motion {name} blur must be non-negative"));
        }
    }
    shadows_from(shadows.as_slice(), colors)
}

pub(super) fn shadow_layers(shadows: Vec<Shadow>) -> BoxShadowValues {
    BoxShadowValues::Many(
        shadows
            .into_iter()
            .map(|shadow| BoxShadowValue {
                offset_x: shadow.offset_x,
                offset_y: shadow.offset_y,
                blur_radius: shadow.blur,
                spread_radius: shadow.spread,
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

    #[test]
    fn a_longhand_reads_through_its_shorthand() {
        let colors = Colors::root();
        let style = StyleDesc {
            padding: Some(8.0),
            padding_left: Some(20.0),
            border_radius: Some(6.0),
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
            AnimProp::BorderColor.initial(&colors),
            AnimProp::Color.read(&style, &colors)
        );
        // With no `color` of its own it is the inherited one.
        let inherited = Colors::of(&ColorContext::root(Scheme::Light), None);
        assert_eq!(
            AnimProp::BorderColor.initial(&inherited),
            Some(AnimValue::Color(Color::from_rgba(gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, false)))
        );
    }

    #[test]
    fn rejects_out_of_range_and_wrong_typed_values() {
        for (prop, value) in [
            (AnimProp::Opacity, serde_json::json!(2.0)),
            (AnimProp::Width, serde_json::json!(-1.0)),
            (AnimProp::Width, serde_json::json!("auto")),
            (AnimProp::Width, serde_json::json!(1e300)),
            (AnimProp::PaddingTop, serde_json::json!("12px")),
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
