//! Animated values as vectors of `f64` channels.
//!
//! A track interpolates channels, never typed values. [`pair`] turns two
//! values of the same kind into a [`Layout`] plus a start and end channel
//! vector, and [`Layout::decode`] turns sampled channels back into a value.

use super::list::ListValue;
use gpui::FontVariations;

use crate::custom_elements::svg_path::PathData;

use crate::color::ColorContext;
use crate::css::color_space::Space;
use crate::css::length::{LengthContext, LinearLength};
use crate::style::StyleDesc;

/// The colour contexts an element's animated colours resolve in, so
/// `currentColor`, `light-dark()` and `color-mix()` mean what they mean in CSS
/// at that element. Resolved colours do not keep the context: a holder
/// compares `Colors` to know when to resolve again.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Colors {
    /// What every colour property but `color` resolves in: the element's own.
    pub(super) element: ColorContext,
    /// What `color` resolves in: `currentColor` is the parent's colour.
    pub(super) text: ColorContext,
    /// The element's computed font, which an unset `fontSize` or `fontWeight` is.
    pub(super) font: Font,
    /// What `em`, `rem` and the viewport units of the element's lengths mean. A length
    /// resolves to px against it when a value is parsed or read, as in a CSS computed
    /// value, so a track never holds a relative unit.
    pub(super) lengths: LengthContext,
}

/// A computed `font-size` in px, `font-weight` and `font-variation-settings`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Font {
    pub(super) size: f64,
    pub(super) weight: f64,
    pub(super) variations: FontVariations,
}

/// The font an element inherits, with what its own `font-size` needs to resolve:
/// `em` and `%` are of the parent's size, `rem` of the root's, `vw` of the window.
#[derive(Clone, Debug)]
pub(crate) struct ParentFont {
    /// The parent's context, as `LengthContext::descend` takes it.
    pub(crate) lengths: LengthContext,
    /// The element is the root, whose `rem` is its own computed size.
    pub(crate) above_root: bool,
    pub(crate) weight: f64,
    pub(crate) variations: FontVariations,
}

impl ParentFont {
    /// The font above everything: the initial size and weight, in a window of unknown size.
    pub(crate) fn initial() -> Self {
        Self {
            lengths: LengthContext::initial(0.0, 0.0),
            above_root: true,
            weight: INITIAL_FONT_WEIGHT,
            variations: FontVariations::default(),
        }
    }

    /// What children of an element styled by `style` inherit.
    pub(crate) fn descend(&self, style: Option<&StyleDesc>) -> Self {
        let font = self.of(style);
        Self {
            lengths: self.lengths.descend(style.and_then(|style| style.font_size.as_ref()), self.above_root),
            above_root: false,
            weight: font.weight,
            variations: font.variations,
        }
    }

    /// The context of an element styled by `style` whose parent has this font: what its
    /// own `em`, `rem` and viewport units mean.
    pub(crate) fn lengths_of(&self, style: Option<&StyleDesc>) -> LengthContext {
        self.lengths.descend(style.and_then(|style| style.font_size.as_ref()), self.above_root)
    }

    fn of(&self, style: Option<&StyleDesc>) -> Font {
        Font {
            size: self.lengths_of(style).font_size,
            weight: style
                .and_then(|style| style.font_weight.as_ref())
                .map_or(self.weight, super::property::font_weight),
            variations: style
                .and_then(|style| style.font_variation_settings.as_ref())
                .map_or_else(|| self.variations.clone(), |settings| settings.variations()),
        }
    }
}

/// CSS `font-weight: normal`.
pub(crate) const INITIAL_FONT_WEIGHT: f64 = 400.0;

static NO_STYLE: std::sync::LazyLock<StyleDesc> = std::sync::LazyLock::new(StyleDesc::default);

impl Colors {
    /// Stands for any element where a value is checked without one: a keyframe
    /// definition is shared, so it is checked here and resolved at each element.
    pub(super) fn root() -> Self {
        Self::of(&ColorContext::root(crate::color::Scheme::Light), None)
    }

    /// `parent` is the context the element inherits, `style` what it declares. The font
    /// is the initial one: an element whose font may differ goes through [`Self::inheriting`].
    pub(crate) fn of(parent: &ColorContext, style: Option<&StyleDesc>) -> Self {
        Self::inheriting(parent, &ParentFont::initial(), style)
    }

    pub(crate) fn inheriting(parent: &ColorContext, font: &ParentFont, style: Option<&StyleDesc>) -> Self {
        let declared = style.unwrap_or(&NO_STYLE);
        Self {
            element: parent.descend(declared),
            text: parent.for_color_property(declared),
            font: font.of(style),
            lengths: font.lengths_of(style),
        }
    }

    /// These contexts with `currentColor` pinned to one colour. Two values
    /// resolved in them differ only where their CSS computed values differ:
    /// computed, `currentColor` stays the keyword, so a new `color` is no change,
    /// while a `light-dark()` that picks another branch is.
    pub(super) fn with_current_color_pinned(&self) -> Self {
        const PINNED: gpui::Rgba = gpui::Rgba { r: 0.2, g: 0.4, b: 0.6, a: 1.0 };
        let mut pinned = self.clone();
        pinned.element.current = PINNED;
        pinned.text.current = PINNED;
        pinned
    }
}

/// A colour in Oklab with straight (not premultiplied) alpha. Channels are
/// premultiplied while interpolating, as CSS Color 4 requires, so fading to
/// `transparent` does not drag the hue through grey.
#[derive(Clone, Copy, Debug)]
pub(super) struct Color {
    lab: [f64; 3],
    alpha: f64,
    /// Written in a legacy syntax (hex, a name, `rgb()`, `hsl()`), which CSS
    /// interpolates in sRGB against another legacy colour. See [`Interpolation`].
    legacy: bool,
}

// The syntax is not part of the value: `red` and `#ff0000` are no change, and
// a style that still computes to the same colour must not restart a transition.
impl PartialEq for Color {
    fn eq(&self, other: &Self) -> bool {
        self.lab == other.lab && self.alpha == other.alpha
    }
}

impl Color {
    pub(super) const TRANSPARENT: Self = Self {
        lab: [0.0; 3],
        alpha: 0.0,
        legacy: true,
    };

    pub(super) fn parse(value: &str, colors: &ColorContext) -> Option<Self> {
        colors
            .resolve(value)
            .map(|rgba| Self::from_rgba(rgba, crate::color::is_legacy_syntax(value, colors)))
    }

    pub(super) fn from_rgba(rgba: gpui::Rgba, legacy: bool) -> Self {
        let [l, a, b, alpha] =
            csscolorparser::Color::new(rgba.r, rgba.g, rgba.b, rgba.a).to_oklaba();
        Self {
            lab: [l as f64, a as f64, b as f64],
            alpha: alpha as f64,
            legacy,
        }
    }

    fn channels(self) -> [f64; 4] {
        let [l, a, b] = self.lab;
        [
            l * self.alpha,
            a * self.alpha,
            b * self.alpha,
            self.alpha,
        ]
    }

    fn from_channels(channels: &[f64], legacy: bool) -> Self {
        let alpha = channels[3];
        if alpha <= 0.0 {
            return Self {
                legacy,
                ..Self::TRANSPARENT
            };
        }
        Self {
            lab: [
                channels[0] / alpha,
                channels[1] / alpha,
                channels[2] / alpha,
            ],
            alpha: alpha.min(1.0),
            legacy,
        }
    }

    /// Springs overshoot, so the result is clamped to the sRGB gamut here and
    /// only here. Fractional channels keep slow fades from stepping in 8-bit.
    pub(super) fn to_css(self) -> String {
        let [l, a, b] = self.lab;
        let rgb = csscolorparser::Color::from_oklaba(l as f32, a as f32, b as f32, 1.0);
        format!(
            "rgba({:.3}, {:.3}, {:.3}, {:.5})",
            rgb.r.clamp(0.0, 1.0) * 255.0,
            rgb.g.clamp(0.0, 1.0) * 255.0,
            rgb.b.clamp(0.0, 1.0) * 255.0,
            self.alpha.clamp(0.0, 1.0),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Shadow {
    pub(super) offset_x: f64,
    pub(super) offset_y: f64,
    pub(super) blur: f64,
    pub(super) spread: f64,
    pub(super) color: Color,
    pub(super) inset: bool,
}

impl Shadow {
    /// The layer a shorter list is padded with: `0 0 0 0 transparent`, with the
    /// other list's `inset` so the two stay interpolable.
    fn none(inset: bool) -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 0.0,
            spread: 0.0,
            color: Color::TRANSPARENT,
            inset,
        }
    }

    fn push_channels(&self, channels: &mut Vec<f64>) {
        channels.extend([self.offset_x, self.offset_y, self.blur, self.spread]);
        channels.extend(self.color.channels());
    }

    fn from_channels(channels: &[f64], inset: bool, legacy: bool) -> Self {
        Self {
            offset_x: channels[0],
            offset_y: channels[1],
            blur: channels[2],
            spread: channels[3],
            color: Color::from_channels(&channels[SHADOW_COLOUR..SHADOW_CHANNELS], legacy),
            inset,
        }
    }
}

const SHADOW_CHANNELS: usize = 8;
/// Where a layer's colour channels start: after its offsets, blur and spread.
const SHADOW_COLOUR: usize = 4;

/// The space two colours blend in. Channels are stored in premultiplied Oklab
/// whatever the blend, so a retarget or a spring reads them the same way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Blend {
    Oklab,
    Srgb,
}

/// How a colour pair picks its [`Blend`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Interpolation {
    /// Oklab for every pair. `motion` springs carry a velocity per channel and
    /// Framer has no notion of colour syntax, so they never change space.
    Oklab,
    /// CSS Color 4: sRGB when both colours are written in a legacy syntax,
    /// Oklab otherwise. For CSS transitions and CSS animations.
    Css,
}

impl Interpolation {
    fn blend(self, from: &Color, to: &Color) -> Blend {
        if self == Self::Css && from.legacy && to.legacy {
            Blend::Srgb
        } else {
            Blend::Oklab
        }
    }
}

/// `to` sits `progress` of the way from `from` (both premultiplied Oklab) when
/// the pair blends in premultiplied sRGB. The sRGB channels are not clamped, so
/// an overshooting easing extrapolates past the gamut and `Color::to_css`
/// clips once, at the end.
fn blend_in_srgb(out: &mut [f64], from: &[f64], to: &[f64], progress: f64) {
    let from = premultiplied_srgb(from);
    let to = premultiplied_srgb(to);
    let mixed: [f64; 4] = std::array::from_fn(|index| from[index] + (to[index] - from[index]) * progress);
    out.copy_from_slice(&premultiplied_oklab(mixed));
}

fn premultiplied_srgb(oklab: &[f64]) -> [f64; 4] {
    let alpha = oklab[3];
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    let [r, g, b] = Space::Oklab.to_srgb([oklab[0] / alpha, oklab[1] / alpha, oklab[2] / alpha]);
    [r * alpha, g * alpha, b * alpha, alpha]
}

fn premultiplied_oklab(srgb: [f64; 4]) -> [f64; 4] {
    let alpha = srgb[3];
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    let [l, a, b] = Space::Oklab.from_srgb([srgb[0] / alpha, srgb[1] / alpha, srgb[2] / alpha]);
    [l * alpha, a * alpha, b * alpha, alpha]
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum AnimValue {
    /// A number or a length in px.
    Number(f64),
    /// A percentage as a fraction, matching `DimensionValue::Percentage`.
    Percent(f64),
    /// A length with both a px part and a share of its percentage basis,
    /// `calc(<percent> * 100% + <px>px)`. `em`, `rem` and the viewport units are
    /// already px. Built through [`AnimValue::length`], which keeps a length that
    /// has only one of the two as a [`Self::Number`] or [`Self::Percent`].
    Calc { px: f64, percent: f64 },
    /// A length that keeps `em` and `rem` as multiples of the font size, because the
    /// element it lands on is not known yet: only a motion value holds one, and it
    /// resolves against each element it is bound to. Built through
    /// [`AnimValue::relative`], which keeps a length with neither as a plain one.
    Relative { px: f64, percent: f64, em: f64, rem: f64 },
    /// A corner radius whose horizontal and vertical radii differ, each a length as
    /// in [`Self::Calc`]. Built through [`AnimValue::ellipse`], which keeps a circular
    /// one as a length.
    Ellipse { x: (f64, f64), y: (f64, f64) },
    Color(Color),
    Shadows(Vec<Shadow>),
    /// A list of numbers, such as `stroke-dasharray`.
    Numbers(Vec<f64>),
    /// `font-variation-settings`: axis tags and values, sorted by tag. Empty is `normal`.
    Variations(Vec<(u32, f64)>),
    /// SVG path data, normalised to absolute commands.
    Path(PathData),
    /// `height: auto`. Never interpolated: a track swaps it for the measured
    /// height while it runs and applies it again once it settles.
    Auto,
    /// An enum-like property (`display`, `visibility`, ...). `None` is the
    /// style leaving it unset.
    Keyword(Option<String>),
    /// A multi-value CSS string such as `"0.5em 0.75em"` or `"red blue"`. Only a
    /// motion value holds one: a style property takes the components one by one.
    List(ListValue),
}

impl AnimValue {
    /// `auto` for a margin, an inset or `flex-basis`: a keyword that is no length, so it
    /// blends with none and a pair of it with a length flips at the halfway point.
    pub(super) fn auto_keyword() -> Self {
        Self::Keyword(Some("auto".to_string()))
    }

    pub(super) fn is_auto_keyword(&self) -> bool {
        matches!(self, Self::Keyword(Some(keyword)) if keyword == "auto")
    }

    /// `auto`, or `from-font` for a decoration thickness: a keyword no length blends with.
    fn is_length_keyword(&self) -> bool {
        self.is_auto_keyword() || matches!(self, Self::Keyword(Some(keyword)) if keyword == "from-font")
    }

    /// The simplest value for `px` plus `percent` of the basis.
    pub(super) fn length(px: f64, percent: f64) -> Self {
        if percent == 0.0 {
            Self::Number(px)
        } else if px == 0.0 {
            Self::Percent(percent)
        } else {
            Self::Calc { px, percent }
        }
    }

    pub(super) fn relative(px: f64, percent: f64, em: f64, rem: f64) -> Self {
        if em == 0.0 && rem == 0.0 {
            Self::length(px, percent)
        } else {
            Self::Relative { px, percent, em, rem }
        }
    }

    /// The four parts of a length, whichever of the four it is held as.
    pub(super) fn as_terms(&self) -> Option<[f64; 4]> {
        match self {
            Self::Relative { px, percent, em, rem } => Some([*px, *percent, *em, *rem]),
            other => other.as_length().map(|(px, percent)| [px, percent, 0.0, 0.0]),
        }
    }

    /// This value with `em` and `rem` resolved against `lengths`: a length that has neither,
    /// and every value that is no length, is returned as it is.
    pub(super) fn resolved_in(&self, lengths: &LengthContext) -> Self {
        let Self::Relative { px, percent, em, rem } = self else {
            return self.clone();
        };
        let (px, percent) = lengths.split(&LinearLength { px: *px, percent: *percent, em: *em, rem: *rem, ..Default::default() });
        Self::length(px, percent)
    }

    /// A corner of radii `x` and `y`, each its px and its share of the border box's side.
    pub(super) fn ellipse(x: (f64, f64), y: (f64, f64)) -> Self {
        if x == y {
            Self::length(x.0, x.1)
        } else {
            Self::Ellipse { x, y }
        }
    }

    /// The horizontal and vertical radii of a corner, a circular one having the same on both.
    pub(super) fn as_ellipse(&self) -> Option<((f64, f64), (f64, f64))> {
        match self {
            Self::Ellipse { x, y } => Some((*x, *y)),
            other => other.as_length().map(|length| (length, length)),
        }
    }

    /// The px and percentage parts of a length, whichever of the three it is held as.
    pub(super) fn as_length(&self) -> Option<(f64, f64)> {
        match self {
            Self::Number(px) => Some((*px, 0.0)),
            Self::Percent(percent) => Some((0.0, *percent)),
            Self::Calc { px, percent } => Some((*px, *percent)),
            _ => None,
        }
    }

    fn describe(&self) -> &'static str {
        match self {
            Self::Number(_) => "a number or px length",
            Self::Percent(_) => "a percentage",
            Self::Calc { .. } => "a length mixing px and a percentage",
            Self::Relative { .. } => "a length in em or rem",
            Self::Ellipse { .. } => "an elliptical corner",
            Self::Color(_) => "a colour",
            Self::Shadows(_) => "a shadow list",
            Self::Numbers(_) => "a list of numbers",
            Self::Variations(_) => "variation settings",
            Self::Path(_) => "path data",
            Self::Auto => "auto",
            Self::Keyword(_) => "a keyword",
            Self::List(_) => "a list of values",
        }
    }
}

/// How a channel vector maps back to a value.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Layout {
    Number,
    Percent,
    /// `[px, percent]`: lengths that are not all px or all percentages blend as
    /// the two parts of a `calc()`, each on its own channel.
    Calc,
    /// `[px, percent, em, rem]`: lengths of which one is in `em` or `rem`, kept apart
    /// from px until they are applied to an element.
    Relative,
    /// `[x px, x percent, y px, y percent]`: a corner radius with two radii, which
    /// blend part by part like the two parts of a [`Self::Calc`].
    Ellipse,
    /// The [`Blend`] of each segment between two keyframes.
    Color(Vec<Blend>),
    Shadows {
        /// One entry per layer.
        insets: Vec<bool>,
        /// The blend of each layer's colour, per segment.
        blends: Vec<Vec<Blend>>,
    },
    /// Shadow lists whose `inset` flags differ at some index cannot be
    /// interpolated: one channel runs from option to option and the value
    /// flips at the halfway point between them.
    DiscreteShadows(Vec<Vec<Shadow>>),
    Numbers,
    /// Settings that name the same axes: the channels are their values, in tag order.
    Variations(Vec<u32>),
    /// Settings that name other axes, or `normal` against some: one channel runs from
    /// option to option and the value flips at the halfway point between them.
    DiscreteVariations(Vec<Vec<(u32, f64)>>),
    /// Paths with the same commands: the channels are their numbers, in order.
    Path(PathData),
    /// Paths with different commands cannot blend: one channel runs from
    /// option to option and the value flips at the halfway point between them.
    DiscretePath(Vec<PathData>),
    /// Two keywords: one channel, the eased progress, with the value flipping
    /// at the halfway point.
    Keyword {
        from: Option<String>,
        to: Option<String>,
        rule: KeywordRule,
    },
    /// The components of a [`AnimValue::List`], each in its own layout with the
    /// number of channels it takes, side by side. `slash` is where the `/` stands.
    List {
        parts: Vec<(Layout, usize)>,
        slash: Option<usize>,
    },
    /// Values that cannot blend part by part (lists of different shapes): one
    /// channel runs from option to option and the value flips at the halfway point.
    DiscreteList(Vec<AnimValue>),
}

/// How a keyword property interpolates between its two values, CSS Transitions
/// Level 2 "discrete" with the exceptions the specs give a few properties.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum KeywordRule {
    Discrete,
    /// `none` is not visible, so the other value shows for the whole duration.
    Display,
    /// `visible` shows for the whole duration when either end has it.
    Visibility,
}

impl KeywordRule {
    fn of(name: &str) -> Self {
        match name {
            "display" => Self::Display,
            "visibility" => Self::Visibility,
            _ => Self::Discrete,
        }
    }

    fn at<'a>(self, from: &'a Option<String>, to: &'a Option<String>, progress: f64) -> &'a Option<String> {
        if progress > 0.0 && progress < 1.0 {
            let shown = |keyword: &str, value: &'a Option<String>| value.as_deref() == Some(keyword);
            match self {
                Self::Display if shown("none", from) => return to,
                Self::Display if shown("none", to) => return from,
                Self::Visibility if shown("visible", from) => return from,
                Self::Visibility if shown("visible", to) => return to,
                _ => {}
            }
        }
        if progress < 0.5 {
            from
        } else {
            to
        }
    }
}

/// The option a discrete channel stands on: the nearest whole number, halves
/// rounding up, kept inside the list because a spring can overshoot.
fn option_index(channel: f64, options: usize) -> usize {
    (channel + 0.5).floor().clamp(0.0, (options - 1) as f64) as usize
}

impl Layout {
    pub(super) fn decode(&self, channels: &[f64]) -> AnimValue {
        match self {
            Self::Number => AnimValue::Number(channels[0]),
            Self::Percent => AnimValue::Percent(channels[0]),
            Self::Calc => AnimValue::length(channels[0], channels[1]),
            Self::Relative => AnimValue::relative(channels[0], channels[1], channels[2], channels[3]),
            Self::Ellipse => AnimValue::ellipse((channels[0], channels[1]), (channels[2], channels[3])),
            Self::Color(blends) => AnimValue::Color(Color::from_channels(channels, all_srgb(blends))),
            Self::Shadows { insets, blends } => AnimValue::Shadows(
                insets
                    .iter()
                    .zip(channels.chunks(SHADOW_CHANNELS))
                    .enumerate()
                    .map(|(index, (inset, layer))| {
                        let legacy = blends.iter().all(|segment| segment[index] == Blend::Srgb);
                        Shadow::from_channels(layer, *inset, legacy)
                    })
                    .collect(),
            ),
            Self::DiscreteShadows(options) => {
                AnimValue::Shadows(options[option_index(channels[0], options.len())].clone())
            }
            Self::Numbers => AnimValue::Numbers(channels.to_vec()),
            Self::Variations(tags) => {
                AnimValue::Variations(tags.iter().copied().zip(channels.iter().copied()).collect())
            }
            Self::DiscreteVariations(options) => {
                AnimValue::Variations(options[option_index(channels[0], options.len())].clone())
            }
            Self::Path(template) => AnimValue::Path(template.with_numbers(channels)),
            Self::DiscretePath(options) => {
                AnimValue::Path(options[option_index(channels[0], options.len())].clone())
            }
            Self::Keyword { from, to, rule } => {
                AnimValue::Keyword(rule.at(from, to, channels[0]).clone())
            }
            Self::List { parts, slash } => {
                let mut rest = channels;
                let mut items = Vec::with_capacity(parts.len());
                for (layout, count) in parts {
                    let (own, after) = rest.split_at(*count);
                    items.push(layout.decode(own));
                    rest = after;
                }
                AnimValue::List(ListValue { items, slash: *slash })
            }
            Self::DiscreteList(options) => options[option_index(channels[0], options.len())].clone(),
        }
    }

    /// Whether `channel` is a fraction of the percentage basis rather than px.
    pub(super) fn is_percent_channel(&self, channel: usize) -> bool {
        match self {
            Self::Percent => true,
            Self::Calc | Self::Relative => channel == 1,
            Self::Ellipse => channel % 2 == 1,
            Self::List { parts, .. } => {
                let mut start = 0;
                for (layout, count) in parts {
                    if channel < start + count {
                        return layout.is_percent_channel(channel - start);
                    }
                    start += count;
                }
                false
            }
            _ => false,
        }
    }

    /// A velocity measured in `self`'s channels as `other`'s, so a retarget can
    /// carry it over; `None` where it means nothing there. A length moves between
    /// px, percentage and `calc` channels, keeping the part it has.
    pub(super) fn velocity_in(&self, other: &Self, rate: &[f64]) -> Option<Vec<f64>> {
        if let (Self::List { parts, slash }, Self::List { parts: other_parts, slash: other_slash }) = (self, other) {
            if slash != other_slash || parts.len() != other_parts.len() {
                return None;
            }
            let mut rest = rate;
            let mut carried = Vec::new();
            for ((layout, count), (other_layout, _)) in parts.iter().zip(other_parts) {
                let (own, after) = rest.split_at(*count);
                carried.extend(layout.velocity_in(other_layout, own)?);
                rest = after;
            }
            return Some(carried);
        }
        let same = match (self, other) {
            (Self::Number, Self::Number)
            | (Self::Percent, Self::Percent)
            | (Self::Calc, Self::Calc)
            | (Self::Relative, Self::Relative)
            | (Self::Ellipse, Self::Ellipse)
            | (Self::Color(_), Self::Color(_))
            | (Self::Numbers, Self::Numbers) => true,
            (Self::Shadows { insets: a, .. }, Self::Shadows { insets: b, .. }) => a.len() == b.len(),
            (Self::Variations(a), Self::Variations(b)) => a == b,
            (Self::Path(a), Self::Path(b)) => a.same_structure(b),
            _ => false,
        };
        if same {
            return Some(rate.to_vec());
        }
        match (self, other) {
            (Self::Number, Self::Calc) => Some(vec![rate[0], 0.0]),
            (Self::Percent, Self::Calc) => Some(vec![0.0, rate[0]]),
            (Self::Number, Self::Relative) => Some(vec![rate[0], 0.0, 0.0, 0.0]),
            (Self::Percent, Self::Relative) => Some(vec![0.0, rate[0], 0.0, 0.0]),
            (Self::Calc, Self::Relative) => Some(vec![rate[0], rate[1], 0.0, 0.0]),
            (Self::Relative, Self::Number) => Some(vec![rate[0]]),
            (Self::Relative, Self::Percent) => Some(vec![rate[1]]),
            (Self::Relative, Self::Calc) => Some(vec![rate[0], rate[1]]),
            (Self::Calc, Self::Number) => Some(vec![rate[0]]),
            (Self::Calc, Self::Percent) => Some(vec![rate[1]]),
            // A circular corner moves both radii at the one rate.
            (Self::Number, Self::Ellipse) => Some(vec![rate[0], 0.0, rate[0], 0.0]),
            (Self::Percent, Self::Ellipse) => Some(vec![0.0, rate[0], 0.0, rate[0]]),
            (Self::Calc, Self::Ellipse) => Some(vec![rate[0], rate[1], rate[0], rate[1]]),
            _ => None,
        }
    }
}

fn all_srgb(blends: &[Blend]) -> bool {
    blends.iter().all(|blend| *blend == Blend::Srgb)
}

impl Layout {
    /// The channels `progress` of the way from `from` to `to`, the keyframes of
    /// segment number `segment`. Linear except where a colour blends in sRGB,
    /// which is not linear in the stored Oklab channels. Progress outside 0..=1
    /// extrapolates, which is how an overshooting easing is applied.
    pub(super) fn blend(&self, segment: usize, from: &[f64], to: &[f64], progress: f64) -> Vec<f64> {
        if let Self::List { parts, .. } = self {
            let mut start = 0;
            let mut channels = Vec::with_capacity(from.len());
            for (layout, count) in parts {
                let range = start..start + count;
                channels.extend(layout.blend(segment, &from[range.clone()], &to[range], progress));
                start += count;
            }
            return channels;
        }
        let mut channels: Vec<f64> = from
            .iter()
            .zip(to)
            .map(|(from, to)| from + (to - from) * progress)
            .collect();
        match self {
            Self::Color(blends) if blends[segment] == Blend::Srgb => {
                blend_in_srgb(&mut channels, from, to, progress);
            }
            Self::Shadows { blends, .. } => {
                for (layer, blend) in blends[segment].iter().enumerate() {
                    if *blend == Blend::Srgb {
                        let colour = layer * SHADOW_CHANNELS + SHADOW_COLOUR..(layer + 1) * SHADOW_CHANNELS;
                        blend_in_srgb(&mut channels[colour.clone()], &from[colour.clone()], &to[colour], progress);
                    }
                }
            }
            _ => {}
        }
        channels
    }
}

#[derive(Clone, Debug)]
pub(super) struct Pair {
    pub(super) layout: Layout,
    pub(super) from: Vec<f64>,
    pub(super) to: Vec<f64>,
}

impl Pair {
    /// The value `progress` of the way from `from` to `to`. Progress outside
    /// 0..=1 extrapolates, which is how an overshooting easing is applied.
    pub(super) fn at(&self, progress: f64) -> AnimValue {
        self.layout.decode(&self.layout.blend(0, &self.from, &self.to, progress))
    }

    /// Values that flip at half instead of blending (shadow lists that differ
    /// in `inset`, path data with other commands, keywords), which a CSS
    /// transition runs only under `transition-behavior: allow-discrete`.
    pub(super) fn is_discrete(&self) -> bool {
        matches!(
            self.layout,
            Layout::DiscreteShadows(_)
                | Layout::DiscretePath(_)
                | Layout::DiscreteList(_)
                | Layout::DiscreteVariations(_)
        ) || matches!(
            self.layout,
            Layout::Keyword { rule, .. } if rule != KeywordRule::Visibility
        )
    }
}

/// [`pair_in`] with colours always in Oklab.
pub(super) fn pair(name: &str, from: &AnimValue, to: &AnimValue) -> Result<Pair, String> {
    pair_in(name, from, to, Interpolation::Oklab)
}

pub(super) fn pair_in(
    name: &str,
    from: &AnimValue,
    to: &AnimValue,
    interpolation: Interpolation,
) -> Result<Pair, String> {
    let mut aligned = align_in(name, &[from, to], interpolation)?;
    let to = aligned.frames.pop().expect("two values were aligned");
    let from = aligned.frames.pop().expect("two values were aligned");
    Ok(Pair {
        layout: aligned.layout,
        from,
        to,
    })
}

/// Several values of one kind as channel vectors in a shared layout.
#[derive(Clone, Debug)]
pub(super) struct Aligned {
    pub(super) layout: Layout,
    pub(super) frames: Vec<Vec<f64>>,
}

/// [`align_in`] with colours always in Oklab.
pub(super) fn align(name: &str, values: &[&AnimValue]) -> Result<Aligned, String> {
    align_in(name, values, Interpolation::Oklab)
}

/// [`pair_in`] for any number of values, such as the keyframes of one track.
/// Each pair of neighbouring colours picks its own blend.
pub(super) fn align_in(
    name: &str,
    values: &[&AnimValue],
    interpolation: Interpolation,
) -> Result<Aligned, String> {
    if values.iter().any(|value| matches!(value, AnimValue::List(_))) {
        return super::list::align(name, values, interpolation);
    }
    // A keyword that is no length (`auto`, `from-font`, the `vertical-align` keywords) against a length: one
    // channel runs from option to option, and the value flips at the halfway point.
    let keyword = |value: &AnimValue| matches!(value, AnimValue::Keyword(Some(_)));
    if (values.iter().any(|value| value.is_length_keyword())
        && values.iter().all(|value| value.is_length_keyword() || value.as_length().is_some()))
        || (values.iter().any(|value| keyword(value))
            && values.iter().any(|value| value.as_length().is_some())
            && values.iter().all(|value| keyword(value) || value.as_length().is_some()))
    {
        return Ok(Aligned {
            layout: Layout::DiscreteList(values.iter().map(|value| (*value).clone()).collect()),
            frames: (0..values.len()).map(|index| vec![index as f64]).collect(),
        });
    }
    if values.iter().any(|value| matches!(value, AnimValue::Relative { .. }))
        && values.iter().all(|value| value.as_terms().is_some())
    {
        return Ok(simple(Layout::Relative, values, |value| {
            value.as_terms().expect("kinds were checked").to_vec()
        }));
    }
    for window in values.windows(2) {
        let lengths = window.iter().all(|value| value.as_ellipse().is_some());
        if !lengths && std::mem::discriminant(window[0]) != std::mem::discriminant(window[1]) {
            return Err(format!(
                "motion {name} cannot animate from {} to {}: mixed units are not supported",
                window[0].describe(),
                window[1].describe()
            ));
        }
    }
    if values.iter().any(|value| matches!(value, AnimValue::Ellipse { .. })) {
        return Ok(simple(Layout::Ellipse, values, |value| {
            let (x, y) = value.as_ellipse().expect("kinds were checked");
            vec![x.0, x.1, y.0, y.1]
        }));
    }
    match values[0] {
        AnimValue::Number(_) | AnimValue::Percent(_) | AnimValue::Calc { .. } => {
            Ok(align_lengths(values))
        }
        AnimValue::Relative { .. } => unreachable!("a relative length was aligned above"),
        AnimValue::Ellipse { .. } => unreachable!("an ellipse was aligned above"),
        AnimValue::List(_) => unreachable!("a list was aligned above"),
        AnimValue::Color(_) => {
            let colors = unwrap_each(values, |value| match value {
                AnimValue::Color(color) => color,
                _ => unreachable!("kinds were checked"),
            });
            let blends = colors
                .windows(2)
                .map(|window| interpolation.blend(window[0], window[1]))
                .collect();
            Ok(simple(Layout::Color(blends), values, |value| match value {
                AnimValue::Color(color) => color.channels().to_vec(),
                _ => unreachable!("kinds were checked"),
            }))
        }
        AnimValue::Shadows(_) => Ok(align_shadows(
            &unwrap_each(values, |value| match value {
                AnimValue::Shadows(shadows) => shadows.as_slice(),
                _ => unreachable!("kinds were checked"),
            }),
            interpolation,
        )),
        AnimValue::Numbers(_) => align_numbers(
            name,
            &unwrap_each(values, |value| match value {
                AnimValue::Numbers(numbers) => numbers.as_slice(),
                _ => unreachable!("kinds were checked"),
            }),
        ),
        AnimValue::Variations(_) => Ok(align_variations(&unwrap_each(values, |value| match value {
            AnimValue::Variations(axes) => axes.as_slice(),
            _ => unreachable!("kinds were checked"),
        }))),
        AnimValue::Path(_) => Ok(align_paths(&unwrap_each(values, |value| match value {
            AnimValue::Path(path) => path,
            _ => unreachable!("kinds were checked"),
        }))),
        AnimValue::Keyword(_) => {
            let keyword = |value: &AnimValue| match value {
                AnimValue::Keyword(keyword) => keyword.clone(),
                _ => unreachable!("kinds were checked"),
            };
            let [from, to] = values else {
                return Err(format!("motion {name} cannot animate more than two keywords"));
            };
            Ok(Aligned {
                layout: Layout::Keyword {
                    from: keyword(from),
                    to: keyword(to),
                    rule: KeywordRule::of(name),
                },
                frames: vec![vec![0.0], vec![1.0]],
            })
        }
        AnimValue::Auto => Err(format!(
            "motion {name} cannot animate from {} to {}: mixed units are not supported",
            values[0].describe(),
            values[values.len() - 1].describe()
        )),
    }
}

/// Lengths that are all px or all percentages keep their one channel. Any other
/// mix blends as a `calc()`: CSS Values 4 interpolates `20px` to `50%` as
/// `calc(20px * (1 - p) + 50% * p)`, each part on its own.
fn align_lengths(values: &[&AnimValue]) -> Aligned {
    let length = |value: &AnimValue| value.as_length().expect("kinds were checked");
    if values.iter().all(|value| matches!(value, AnimValue::Number(_))) {
        return simple(Layout::Number, values, |value| vec![length(value).0]);
    }
    if values.iter().all(|value| matches!(value, AnimValue::Percent(_))) {
        return simple(Layout::Percent, values, |value| vec![length(value).1]);
    }
    simple(Layout::Calc, values, |value| {
        let (px, percent) = length(value);
        vec![px, percent]
    })
}

fn unwrap_each<'a, T: ?Sized>(
    values: &[&'a AnimValue],
    inner: impl Fn(&'a AnimValue) -> &'a T,
) -> Vec<&'a T> {
    values.iter().map(|value| inner(value)).collect()
}

fn simple(layout: Layout, values: &[&AnimValue], channels: impl Fn(&AnimValue) -> Vec<f64>) -> Aligned {
    Aligned {
        layout,
        frames: values.iter().map(|value| channels(value)).collect(),
    }
}

/// Lists of different lengths are repeated to their least common multiple, as
/// CSS interpolates `stroke-dasharray`.
fn align_numbers(name: &str, lists: &[&[f64]]) -> Result<Aligned, String> {
    if lists.iter().any(|list| list.is_empty()) {
        return Err(format!("motion {name} cannot animate from or to an empty list"));
    }
    let length = lists.iter().fold(1, |length, list| {
        let (mut a, mut b) = (length, list.len());
        while b != 0 {
            (a, b) = (b, a % b);
        }
        length / a * list.len()
    });
    Ok(Aligned {
        layout: Layout::Numbers,
        frames: lists
            .iter()
            .map(|list| list.iter().copied().cycle().take(length).collect())
            .collect(),
    })
}

/// Settings interpolate per axis when they name the same axes, as Chrome's
/// `CSSFontVariationSettingsInterpolationType` does. `normal` names none, so it is
/// discrete against any list. Chrome decides each pair of keyframes on its own; a track
/// flips as a whole when any two differ, as shadow lists do.
fn align_variations(lists: &[&[(u32, f64)]]) -> Aligned {
    let same_axes = !lists[0].is_empty()
        && lists.iter().all(|list| {
            list.len() == lists[0].len() && list.iter().zip(lists[0]).all(|(a, b)| a.0 == b.0)
        });
    if same_axes {
        return Aligned {
            layout: Layout::Variations(lists[0].iter().map(|(tag, _)| *tag).collect()),
            frames: lists
                .iter()
                .map(|list| list.iter().map(|(_, value)| *value).collect())
                .collect(),
        };
    }
    Aligned {
        layout: Layout::DiscreteVariations(lists.iter().map(|list| list.to_vec()).collect()),
        frames: (0..lists.len()).map(|index| vec![index as f64]).collect(),
    }
}

fn align_paths(paths: &[&PathData]) -> Aligned {
    if paths.iter().all(|path| path.same_structure(paths[0])) {
        return Aligned {
            layout: Layout::Path(paths[0].clone()),
            frames: paths.iter().map(|path| path.numbers()).collect(),
        };
    }
    Aligned {
        layout: Layout::DiscretePath(paths.iter().map(|path| (*path).clone()).collect()),
        frames: (0..paths.len()).map(|index| vec![index as f64]).collect(),
    }
}

fn align_shadows(lists: &[&[Shadow]], interpolation: Interpolation) -> Aligned {
    let layers = lists.iter().map(|list| list.len()).max().unwrap_or_default();
    // A layer a list lacks is a transparent one that takes the `inset` of the
    // first list that has it.
    let padded: Vec<Vec<Shadow>> = lists
        .iter()
        .map(|list| {
            (0..layers)
                .map(|index| {
                    list.get(index).cloned().unwrap_or_else(|| {
                        let inset = lists
                            .iter()
                            .find_map(|other| other.get(index))
                            .expect("a layer index below the longest list is in some list")
                            .inset;
                        Shadow::none(inset)
                    })
                })
                .collect()
        })
        .collect();

    let compatible = (0..layers).all(|index| {
        padded
            .iter()
            .all(|list| list[index].inset == padded[0][index].inset)
    });
    if !compatible {
        return Aligned {
            layout: Layout::DiscreteShadows(lists.iter().map(|list| list.to_vec()).collect()),
            frames: (0..lists.len()).map(|index| vec![index as f64]).collect(),
        };
    }

    let blends = padded
        .windows(2)
        .map(|window| {
            window[0]
                .iter()
                .zip(&window[1])
                .map(|(from, to)| interpolation.blend(&from.color, &to.color))
                .collect()
        })
        .collect();
    Aligned {
        layout: Layout::Shadows {
            insets: padded[0].iter().map(|layer| layer.inset).collect(),
            blends,
        },
        frames: padded
            .iter()
            .map(|layers| {
                let mut channels = Vec::with_capacity(layers.len() * SHADOW_CHANNELS);
                for layer in layers {
                    layer.push_channels(&mut channels);
                }
                channels
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color(value: &str) -> Color {
        Color::parse(value, &Colors::root().element).unwrap()
    }

    fn mix(from: &str, to: &str, progress: f64) -> Color {
        let pair = pair("color", &AnimValue::Color(color(from)), &AnimValue::Color(color(to)))
            .unwrap();
        let channels: Vec<f64> = pair
            .from
            .iter()
            .zip(&pair.to)
            .map(|(from, to)| from + (to - from) * progress)
            .collect();
        match pair.layout.decode(&channels) {
            AnimValue::Color(color) => color,
            other => panic!("expected a colour, got {other:?}"),
        }
    }

    fn rgba(color: Color) -> [f64; 4] {
        let rgba = crate::color::parse_color_rgba(&color.to_css()).unwrap();
        [rgba.r as f64, rgba.g as f64, rgba.b as f64, rgba.a as f64]
    }

    fn assert_close(actual: [f64; 4], expected: [f64; 4], tolerance: f64) {
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                (actual - expected).abs() <= tolerance,
                "{actual:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn colour_midpoint_is_taken_in_oklab() {
        // red and blue are oklab(0.62796 0.22486 0.12585) and
        // oklab(0.45201 -0.03246 -0.31153); `color-mix(in oklab, red, blue)` is
        // the oklab midpoint, not the sRGB one (which would be #800080).
        let midpoint = rgba(mix("#ff0000", "#0000ff", 0.5));
        let expected = crate::color::parse_color_rgba("oklab(0.539985 0.096200 -0.092840)").unwrap();
        assert_close(
            midpoint,
            [expected.r as f64, expected.g as f64, expected.b as f64, 1.0],
            1.5 / 255.0,
        );
        assert!((midpoint[0] - 128.0 / 255.0).abs() > 0.02, "not the sRGB midpoint");
    }

    /// `from` to `to` `progress` of the way, the way a CSS transition blends it.
    fn css_mix(from: &str, to: &str, progress: f64) -> Color {
        let pair = pair_in(
            "color",
            &AnimValue::Color(color(from)),
            &AnimValue::Color(color(to)),
            Interpolation::Css,
        )
        .unwrap();
        match pair.at(progress) {
            AnimValue::Color(color) => color,
            other => panic!("expected a colour, got {other:?}"),
        }
    }

    fn assert_srgb_midpoint(from: &str, to: &str) {
        assert_close(
            rgba(css_mix(from, to, 0.5)),
            [128.0 / 255.0, 0.0, 128.0 / 255.0, 1.0],
            1.0 / 255.0,
        );
    }

    #[test]
    fn two_legacy_colours_blend_in_srgb_under_css() {
        for (from, to) in [
            ("red", "blue"),
            ("#ff0000", "#0000ff"),
            ("#f00", "rgb(0, 0, 255)"),
            ("rgba(255 0 0 / 100%)", "hsl(240 100% 50%)"),
            ("#ff0000", "hsla(240, 100%, 50%, 1)"),
            ("red", "light-dark(blue, green)"),
        ] {
            assert_srgb_midpoint(from, to);
        }
    }

    #[test]
    fn a_modern_syntax_on_either_end_blends_in_oklab_under_css() {
        let oklch_blue = "oklch(0.45201 0.31321 264.052)";
        for (from, to) in [
            ("#ff0000", oklch_blue),
            (oklch_blue, "#ff0000"),
            ("oklch(0.62796 0.25768 29.234)", oklch_blue),
            ("oklab(0.62796 0.22486 0.12585)", "oklab(0.45201 -0.03246 -0.31153)"),
            ("lab(54.29 80.8 69.89)", "#0000ff"),
            ("color(display-p3 1 0 0)", "blue"),
            ("hwb(0deg 0% 0%)", "blue"),
            ("color-mix(in srgb, red, red)", "blue"),
            ("currentColor", "blue"),
        ] {
            let css = rgba(css_mix(from, to, 0.5));
            let oklab = rgba(mix(from, to, 0.5));
            assert_close(css, oklab, 1e-9);
        }
        let oklab_mid = rgba(css_mix("#ff0000", oklch_blue, 0.5));
        assert!((oklab_mid[1] - 0.0).abs() > 0.1, "not the sRGB midpoint: {oklab_mid:?}");
    }

    #[test]
    fn light_dark_has_the_syntax_of_the_side_it_picks() {
        let mut dark = Colors::root().element;
        dark.scheme = crate::color::Scheme::Dark;
        let parse = |value: &str| Color::parse(value, &dark).unwrap();
        assert!(parse("light-dark(oklch(0.5 0.1 20), red)").legacy);
        assert!(!parse("light-dark(red, oklch(0.5 0.1 20))").legacy);
    }

    #[test]
    fn transparent_is_a_legacy_colour_and_blends_premultiplied() {
        // Premultiplied, transparent contributes nothing, so the midpoint is
        // the full red at half alpha. Straight-alpha channels would give a
        // darker red, (127.5, 0, 0).
        let halfway = css_mix("transparent", "#ff0000", 0.5);
        assert_close(rgba(halfway), [1.0, 0.0, 0.0, 0.5], 1.0 / 255.0);
        assert_close(rgba(css_mix("#ff0000", "transparent", 0.5)), [1.0, 0.0, 0.0, 0.5], 1.0 / 255.0);
    }

    #[test]
    fn alpha_blends_premultiplied_in_srgb() {
        // (0.2, 0, 0, 0.2) and (0, 0, 0.8, 0.8) average to (0.1, 0, 0.4, 0.5).
        let halfway = css_mix("rgba(255, 0, 0, 0.2)", "rgba(0, 0, 255, 0.8)", 0.5);
        assert_close(rgba(halfway), [0.2, 0.0, 0.8, 0.5], 1.0 / 255.0);
    }

    #[test]
    fn css_pairs_end_on_their_endpoints_and_extrapolate_in_srgb() {
        assert_close(rgba(css_mix("#102030", "#c0d0e0", 0.0)), rgba(color("#102030")), 1e-4);
        assert_close(rgba(css_mix("#102030", "#c0d0e0", 1.0)), rgba(color("#c0d0e0")), 1e-4);
        // An overshooting easing carries red past blue's red channel, which
        // `to_css` clips to the gamut.
        let past = rgba(css_mix("#ff0000", "#0000ff", 1.5));
        assert_close(past, [0.0, 0.0, 1.0, 1.0], 1e-4);
    }

    #[test]
    fn the_blended_value_keeps_the_syntax_the_pair_used() {
        assert!(css_mix("red", "blue", 0.3).legacy);
        assert!(!css_mix("red", "oklch(0.5 0.1 20)", 0.3).legacy);
        assert!(!mix("red", "blue", 0.3).legacy, "an Oklab pair never claims a legacy syntax");
    }

    #[test]
    fn the_syntax_is_not_part_of_equality() {
        assert_eq!(color("red"), color("#ff0000"));
        assert_eq!(color("red"), color("rgb(255 0 0)"));
    }

    #[test]
    fn keyframes_pick_a_blend_per_neighbouring_pair() {
        let values = ["red", "blue", "oklch(0.7 0.15 150)", "green"]
            .map(|value| AnimValue::Color(color(value)));
        let refs: Vec<&AnimValue> = values.iter().collect();
        let css = align_in("color", &refs, Interpolation::Css).unwrap();
        assert_eq!(css.layout, Layout::Color(vec![Blend::Srgb, Blend::Oklab, Blend::Oklab]));
        let oklab = align_in("color", &refs, Interpolation::Oklab).unwrap();
        assert_eq!(oklab.layout, Layout::Color(vec![Blend::Oklab; 3]));
        assert_eq!(css.frames, oklab.frames, "the stored channels do not depend on the blend");

        let at = |segment: usize, progress: f64| match css.layout.decode(&css.layout.blend(
            segment,
            &css.frames[segment],
            &css.frames[segment + 1],
            progress,
        )) {
            AnimValue::Color(color) => rgba(color),
            other => panic!("expected a colour, got {other:?}"),
        };
        assert_close(at(0, 0.5), [128.0 / 255.0, 0.0, 128.0 / 255.0, 1.0], 1.0 / 255.0);
        let oklab_middle = |segment: usize| match oklab.layout.decode(&oklab.layout.blend(
            segment,
            &oklab.frames[segment],
            &oklab.frames[segment + 1],
            0.5,
        )) {
            AnimValue::Color(color) => rgba(color),
            other => panic!("expected a colour, got {other:?}"),
        };
        assert_close(at(1, 0.5), oklab_middle(1), 1e-9);
        assert_close(at(2, 0.5), oklab_middle(2), 1e-9);
    }

    #[test]
    fn shadow_colours_blend_per_layer() {
        let from = AnimValue::Shadows(vec![
            shadow(0.0, 4.0, "red", false),
            shadow(0.0, 4.0, "red", false),
        ]);
        let to = AnimValue::Shadows(vec![
            shadow(0.0, 4.0, "blue", false),
            shadow(0.0, 4.0, "oklch(0.45201 0.31321 264.052)", false),
        ]);
        let css = pair_in("boxShadow", &from, &to, Interpolation::Css).unwrap();
        let oklab = pair("boxShadow", &from, &to).unwrap();
        let (AnimValue::Shadows(css), AnimValue::Shadows(oklab)) = (css.at(0.5), oklab.at(0.5)) else {
            panic!("shadows decode to shadows");
        };
        assert_close(rgba(css[0].color), [128.0 / 255.0, 0.0, 128.0 / 255.0, 1.0], 1.0 / 255.0);
        assert_close(rgba(css[1].color), rgba(oklab[1].color), 1e-9);
        assert!(css[0].color.legacy && !css[1].color.legacy);
    }

    #[test]
    fn colour_endpoints_round_trip() {
        for value in ["#ff0000", "#1e1e2e", "rgba(10, 200, 30, 0.4)", "transparent"] {
            let expected = crate::color::parse_color_rgba(value).unwrap();
            assert_close(
                rgba(mix(value, "#ffffff", 0.0)),
                [expected.r as f64, expected.g as f64, expected.b as f64, expected.a as f64],
                1.0 / 255.0,
            );
        }
    }

    #[test]
    fn alpha_is_premultiplied_while_interpolating() {
        // Halfway from opaque red to transparent is red at half alpha. A
        // straight-alpha mix would pass through transparent black's lightness
        // and come out as a darker, muddier red.
        let halfway = rgba(mix("#ff0000", "transparent", 0.5));
        let red = crate::color::parse_color_rgba("#ff0000").unwrap();
        assert_close(
            halfway,
            [red.r as f64, red.g as f64, red.b as f64, 0.5],
            1.5 / 255.0,
        );
    }

    #[test]
    fn colour_output_is_clamped_to_srgb() {
        let overshoot = Color::from_channels(&[1.4, 0.4, 0.3, 1.0], false);
        let [r, g, b, a] = rgba(overshoot);
        for channel in [r, g, b, a] {
            assert!((0.0..=1.0).contains(&channel));
        }
        let beyond = Color::from_channels(&[0.5, 0.0, 0.0, 1.3], false);
        assert_eq!(rgba(beyond)[3], 1.0);
    }

    fn shadow(x: f64, blur: f64, color: &str, inset: bool) -> Shadow {
        Shadow {
            offset_x: x,
            offset_y: 0.0,
            blur,
            spread: 0.0,
            color: self::color(color),
            inset,
        }
    }

    fn halfway(from: Vec<Shadow>, to: Vec<Shadow>, progress: f64) -> Vec<Shadow> {
        let pair = pair("boxShadow", &AnimValue::Shadows(from), &AnimValue::Shadows(to)).unwrap();
        let channels: Vec<f64> = pair
            .from
            .iter()
            .zip(&pair.to)
            .map(|(from, to)| from + (to - from) * progress)
            .collect();
        match pair.layout.decode(&channels) {
            AnimValue::Shadows(shadows) => shadows,
            other => panic!("expected shadows, got {other:?}"),
        }
    }

    #[test]
    fn shorter_shadow_list_is_padded_with_transparent_layers() {
        let from = vec![shadow(4.0, 8.0, "#000000", false)];
        let to = vec![
            shadow(4.0, 8.0, "#000000", false),
            shadow(10.0, 20.0, "#ff0000", false),
        ];
        let middle = halfway(from, to, 0.5);
        assert_eq!(middle.len(), 2);
        assert_eq!(middle[0].offset_x, 4.0);
        assert_eq!(middle[1].offset_x, 5.0);
        assert_eq!(middle[1].blur, 10.0);
        assert!((middle[1].color.alpha - 0.5).abs() < 1e-9);
        assert!(!middle[1].inset);
    }

    #[test]
    fn padded_layers_take_the_inset_of_the_other_list() {
        let to = vec![shadow(0.0, 4.0, "#000000", true)];
        let middle = halfway(Vec::new(), to, 0.5);
        assert_eq!(middle.len(), 1);
        assert!(middle[0].inset);
        assert_eq!(middle[0].blur, 2.0);
    }

    #[test]
    fn mismatched_inset_switches_at_half() {
        let from = vec![shadow(2.0, 4.0, "#000000", false)];
        let to = vec![shadow(6.0, 4.0, "#000000", true)];
        assert_eq!(halfway(from.clone(), to.clone(), 0.49), from);
        assert_eq!(halfway(from, to.clone(), 0.5), to);
    }

    #[test]
    fn a_length_and_a_colour_are_rejected() {
        let error = pair("width", &AnimValue::Number(10.0), &AnimValue::Color(color("#fff"))).unwrap_err();
        assert!(error.contains("width") && error.contains("mixed units"), "{error}");
        assert!(pair("color", &AnimValue::Percent(1.0), &AnimValue::Color(color("#fff"))).is_err());
        assert!(pair("width", &AnimValue::Auto, &AnimValue::Percent(0.5)).is_err());
    }

    fn length_at(from: &AnimValue, to: &AnimValue, progress: f64) -> AnimValue {
        pair("width", from, to).unwrap().at(progress)
    }

    #[test]
    fn a_px_length_and_a_percentage_blend_as_the_two_parts_of_a_calc() {
        // CSS Values 4: 20px to 50% is calc(20px * (1 - p) + 50% * p).
        let pair = pair("width", &AnimValue::Number(20.0), &AnimValue::Percent(0.5)).unwrap();
        assert_eq!(pair.layout, Layout::Calc);
        assert_eq!(pair.at(0.5), AnimValue::Calc { px: 10.0, percent: 0.25 });
        assert_eq!(pair.at(0.0), AnimValue::Number(20.0));
        assert_eq!(pair.at(1.0), AnimValue::Percent(0.5));
        assert_eq!(pair.at(1.5), AnimValue::Calc { px: -10.0, percent: 0.75 });
    }

    #[test]
    fn an_elliptical_corner_blends_each_radius_on_its_own() {
        let circle = AnimValue::Number(10.0);
        let ellipse = AnimValue::Ellipse { x: (30.0, 0.0), y: (0.0, 0.5) };
        let pair = pair("borderTopLeftRadius", &circle, &ellipse).unwrap();
        assert_eq!(pair.layout, Layout::Ellipse);
        assert_eq!(pair.at(0.5), AnimValue::Ellipse { x: (20.0, 0.0), y: (5.0, 0.25) });
        assert_eq!(pair.at(0.0), circle);
        assert_eq!(pair.at(1.0), ellipse);
        assert!(Layout::Ellipse.is_percent_channel(1) && Layout::Ellipse.is_percent_channel(3));
        assert!(!Layout::Ellipse.is_percent_channel(0) && !Layout::Ellipse.is_percent_channel(2));
        assert_eq!(Layout::Number.velocity_in(&Layout::Ellipse, &[3.0]), Some(vec![3.0, 0.0, 3.0, 0.0]));
        assert_eq!(Layout::Ellipse.velocity_in(&Layout::Ellipse, &[1.0, 0.0, 2.0, 0.0]), Some(vec![1.0, 0.0, 2.0, 0.0]));
    }

    #[test]
    fn a_corner_with_the_same_radius_on_both_axes_is_a_length() {
        assert_eq!(AnimValue::ellipse((10.0, 0.0), (10.0, 0.0)), AnimValue::Number(10.0));
        assert_eq!(AnimValue::ellipse((0.0, 0.5), (0.0, 0.5)), AnimValue::Percent(0.5));
        assert_eq!(AnimValue::ellipse((10.0, 0.0), (20.0, 0.0)), AnimValue::Ellipse { x: (10.0, 0.0), y: (20.0, 0.0) });
    }

    #[test]
    fn two_calcs_mix_each_part_on_its_own() {
        // calc(100% - 20px) to calc(50% + 10px) is calc(75% - 5px) halfway: 295px of 400.
        let from = AnimValue::Calc { px: -20.0, percent: 1.0 };
        let to = AnimValue::Calc { px: 10.0, percent: 0.5 };
        let AnimValue::Calc { px, percent } = length_at(&from, &to, 0.5) else {
            panic!("a calc stays a calc");
        };
        assert_eq!((px, percent), (-5.0, 0.75));
        assert_eq!(px + percent * 400.0, 295.0);
        assert_eq!(length_at(&from, &AnimValue::Number(40.0), 0.5), AnimValue::Calc { px: 10.0, percent: 0.5 });
    }

    #[test]
    fn lengths_with_one_part_keep_the_simplest_channels() {
        let numbers = pair("width", &AnimValue::Number(1.0), &AnimValue::Number(3.0)).unwrap();
        assert_eq!((numbers.layout, numbers.from.len()), (Layout::Number, 1));
        let percents = pair("width", &AnimValue::Percent(0.1), &AnimValue::Percent(0.5)).unwrap();
        assert_eq!((percents.layout, percents.from.len()), (Layout::Percent, 1));
        // A calc that lost a part reads as the simple value again.
        assert_eq!(AnimValue::length(5.0, 0.0), AnimValue::Number(5.0));
        assert_eq!(AnimValue::length(0.0, 0.5), AnimValue::Percent(0.5));
        assert_eq!(AnimValue::length(0.0, 0.0), AnimValue::Number(0.0));
    }

    #[test]
    fn keyframes_of_mixed_lengths_share_one_calc_layout() {
        let values = [AnimValue::Number(10.0), AnimValue::Percent(0.5), AnimValue::Calc { px: -4.0, percent: 1.0 }];
        let refs: Vec<&AnimValue> = values.iter().collect();
        let aligned = align("width", &refs).unwrap();
        assert_eq!(aligned.layout, Layout::Calc);
        assert_eq!(aligned.frames, vec![vec![10.0, 0.0], vec![0.0, 0.5], vec![-4.0, 1.0]]);
    }

    #[test]
    fn a_velocity_moves_between_length_layouts_keeping_the_part_it_has() {
        assert_eq!(Layout::Number.velocity_in(&Layout::Calc, &[3.0]), Some(vec![3.0, 0.0]));
        assert_eq!(Layout::Percent.velocity_in(&Layout::Calc, &[0.2]), Some(vec![0.0, 0.2]));
        assert_eq!(Layout::Calc.velocity_in(&Layout::Calc, &[3.0, 0.2]), Some(vec![3.0, 0.2]));
        assert_eq!(Layout::Calc.velocity_in(&Layout::Percent, &[3.0, 0.2]), Some(vec![0.2]));
        assert_eq!(Layout::Number.velocity_in(&Layout::Percent, &[3.0]), None);
    }

    fn path(data: &str) -> PathData {
        PathData::parse(data).unwrap()
    }

    fn at(from: &AnimValue, to: &AnimValue, progress: f64) -> AnimValue {
        let pair = pair("d", from, to).unwrap();
        let channels: Vec<f64> = pair
            .from
            .iter()
            .zip(&pair.to)
            .map(|(from, to)| from + (to - from) * progress)
            .collect();
        pair.layout.decode(&channels)
    }

    #[test]
    fn paths_with_the_same_commands_blend_every_number() {
        let from = AnimValue::Path(path("M0 0 L10 0 C0 0 5 5 10 10"));
        let to = AnimValue::Path(path("m10 10 l10 10 c0 0 5 5 10 10"));
        match at(&from, &to, 0.5) {
            AnimValue::Path(middle) => {
                assert_eq!(middle.to_data(), "M 5 5 L 15 10 C 10 10 15 15 20 20");
            }
            other => panic!("expected a path, got {other:?}"),
        }
    }

    #[test]
    fn paths_with_other_commands_switch_at_half() {
        let from = AnimValue::Path(path("M0 0 L10 10"));
        let to = AnimValue::Path(path("M0 0 H10"));
        assert!(pair("d", &from, &to).unwrap().is_discrete());
        assert_eq!(at(&from, &to, 0.49), from);
        assert_eq!(at(&from, &to, 0.5), to);
    }

    #[test]
    fn dash_lists_repeat_to_a_common_length() {
        let from = AnimValue::Numbers(vec![4.0, 2.0]);
        let to = AnimValue::Numbers(vec![6.0, 0.0, 6.0]);
        assert_eq!(
            at(&from, &to, 0.5),
            AnimValue::Numbers(vec![5.0, 1.0, 5.0, 4.0, 2.0, 4.0])
        );
        assert!(pair("strokeDasharray", &from, &AnimValue::Numbers(Vec::new())).is_err());
    }
    fn decoded(values: &[AnimValue], channel: impl Fn(&[Vec<f64>]) -> Vec<f64>) -> AnimValue {
        let refs: Vec<&AnimValue> = values.iter().collect();
        let aligned = align("test", &refs).unwrap();
        aligned.layout.decode(&channel(&aligned.frames))
    }

    #[test]
    fn many_values_share_one_layout() {
        let lists = [
            AnimValue::Numbers(vec![1.0]),
            AnimValue::Numbers(vec![2.0, 4.0]),
            AnimValue::Numbers(vec![3.0, 6.0, 9.0]),
        ];
        let refs: Vec<&AnimValue> = lists.iter().collect();
        let aligned = align("strokeDasharray", &refs).unwrap();
        assert!(aligned.frames.iter().all(|frame| frame.len() == 6));
        assert_eq!(aligned.frames[1], vec![2.0, 4.0, 2.0, 4.0, 2.0, 4.0]);

        let shadows = [
            AnimValue::Shadows(vec![shadow(1.0, 2.0, "#000000", false)]),
            AnimValue::Shadows(Vec::new()),
            AnimValue::Shadows(vec![
                shadow(1.0, 2.0, "#000000", false),
                shadow(3.0, 4.0, "#000000", true),
            ]),
        ];
        let AnimValue::Shadows(layers) = decoded(&shadows, |frames| frames[1].clone()) else {
            panic!("shadows decode to shadows");
        };
        assert_eq!(layers.len(), 2);
        assert!(layers[1].inset, "a padded layer takes the inset of the list that has it");
        assert_eq!(layers[0].blur, 0.0);
    }

    #[test]
    fn incompatible_values_are_discrete_across_every_option() {
        let paths = [path("M0 0 L1 1"), path("M0 0 H5"), path("M2 2 L3 3")]
            .map(AnimValue::Path);
        let refs: Vec<&AnimValue> = paths.iter().collect();
        let aligned = align("d", &refs).unwrap();
        assert!(matches!(aligned.layout, Layout::DiscretePath(_)));
        for (channel, expected) in [(0.0, 0), (0.49, 0), (0.5, 1), (1.4, 1), (1.6, 2), (3.0, 2), (-1.0, 0)] {
            assert_eq!(aligned.layout.decode(&[channel]), paths[expected], "{channel}");
        }
    }

    #[test]
    fn the_first_mismatched_neighbours_are_named() {
        let values = [AnimValue::Number(1.0), AnimValue::Number(2.0), AnimValue::Color(color("red"))];
        let refs: Vec<&AnimValue> = values.iter().collect();
        let error = align("width", &refs).unwrap_err();
        assert!(error.contains("from a number or px length to a colour"), "{error}");
    }

    fn tag(name: &str) -> u32 {
        u32::from_be_bytes(name.as_bytes().try_into().unwrap())
    }

    fn axes(list: &[(&str, f64)]) -> AnimValue {
        AnimValue::Variations(list.iter().map(|(name, value)| (tag(name), *value)).collect())
    }

    #[test]
    fn variations_naming_the_same_axes_blend_per_axis() {
        let blended = pair("fontVariationSettings", &axes(&[("wght", 400.0)]), &axes(&[("wght", 700.0)])).unwrap();
        assert!(!blended.is_discrete());
        assert_eq!(blended.at(0.5), axes(&[("wght", 550.0)]));

        let from = axes(&[("wdth", 100.0), ("wght", 400.0)]);
        let to = axes(&[("wdth", 50.0), ("wght", 800.0)]);
        let blended = pair("fontVariationSettings", &from, &to).unwrap();
        assert_eq!(blended.at(0.25), axes(&[("wdth", 87.5), ("wght", 500.0)]));
        assert_eq!(blended.at(1.5), axes(&[("wdth", 25.0), ("wght", 1000.0)]), "an overshoot extrapolates");
    }

    #[test]
    fn variations_naming_other_axes_flip_at_half() {
        let wght = axes(&[("wght", 400.0)]);
        for to in [
            axes(&[("wdth", 80.0)]),
            axes(&[("wght", 700.0), ("wdth", 80.0)]),
            axes(&[]),
        ] {
            let flipped = pair("fontVariationSettings", &wght, &to).unwrap();
            assert!(flipped.is_discrete(), "{to:?}");
            assert_eq!(flipped.at(0.0), wght);
            assert_eq!(flipped.at(0.49), wght);
            assert_eq!(flipped.at(0.5), to);
            assert_eq!(flipped.at(1.0), to);

            let back = pair("fontVariationSettings", &to, &wght).unwrap();
            assert!(back.is_discrete());
            assert_eq!(back.at(0.49), to);
            assert_eq!(back.at(0.5), wght);
        }
    }

    #[test]
    fn normal_is_discrete_even_against_normal() {
        let normal = axes(&[]);
        assert!(pair("fontVariationSettings", &normal, &normal).unwrap().is_discrete());
    }

    #[test]
    fn a_track_of_variations_that_differ_anywhere_flips_as_a_whole() {
        let values = [axes(&[("wght", 100.0)]), axes(&[("wght", 500.0)]), axes(&[("wdth", 60.0)])];
        let refs: Vec<&AnimValue> = values.iter().collect();
        let aligned = align("fontVariationSettings", &refs).unwrap();
        assert!(matches!(aligned.layout, Layout::DiscreteVariations(_)));
        for (channel, expected) in [(0.0, 0), (0.6, 1), (1.4, 1), (1.5, 2)] {
            assert_eq!(aligned.layout.decode(&[channel]), values[expected], "{channel}");
        }

        let same = [axes(&[("wght", 100.0)]), axes(&[("wght", 500.0)]), axes(&[("wght", 300.0)])];
        let refs: Vec<&AnimValue> = same.iter().collect();
        assert!(matches!(align("fontVariationSettings", &refs).unwrap().layout, Layout::Variations(_)));
    }
}
