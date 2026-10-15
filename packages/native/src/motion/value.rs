//! Animated values as vectors of `f64` channels.
//!
//! A track interpolates channels, never typed values. [`pair`] turns two
//! values of the same kind into a [`Layout`] plus a start and end channel
//! vector, and [`Layout::decode`] turns sampled channels back into a value.

use crate::custom_elements::svg_path::PathData;

/// The one place a colour string is parsed, so the context-aware resolver
/// (`currentColor`, `light-dark()`, `color-mix()`) can replace it in one edit.
fn resolve_color(value: &str) -> Option<gpui::Rgba> {
    crate::color::parse_color_rgba(value)
}

/// A colour in Oklab with straight (not premultiplied) alpha. Channels are
/// premultiplied while interpolating, as CSS Color 4 requires, so fading to
/// `transparent` does not drag the hue through grey.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Color {
    lab: [f64; 3],
    alpha: f64,
}

impl Color {
    pub(super) const TRANSPARENT: Self = Self {
        lab: [0.0; 3],
        alpha: 0.0,
    };

    pub(super) fn parse(value: &str) -> Option<Self> {
        let rgba = resolve_color(value)?;
        let [l, a, b, alpha] =
            csscolorparser::Color::new(rgba.r, rgba.g, rgba.b, rgba.a).to_oklaba();
        Some(Self {
            lab: [l as f64, a as f64, b as f64],
            alpha: alpha as f64,
        })
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

    fn from_channels(channels: &[f64]) -> Self {
        let alpha = channels[3];
        if alpha <= 0.0 {
            return Self::TRANSPARENT;
        }
        Self {
            lab: [
                channels[0] / alpha,
                channels[1] / alpha,
                channels[2] / alpha,
            ],
            alpha: alpha.min(1.0),
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

    fn from_channels(channels: &[f64], inset: bool) -> Self {
        Self {
            offset_x: channels[0],
            offset_y: channels[1],
            blur: channels[2],
            spread: channels[3],
            color: Color::from_channels(&channels[4..8]),
            inset,
        }
    }
}

const SHADOW_CHANNELS: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum AnimValue {
    /// A number or a length in px.
    Number(f64),
    /// A percentage as a fraction, matching `DimensionValue::Percentage`.
    Percent(f64),
    Color(Color),
    Shadows(Vec<Shadow>),
    /// A list of numbers, such as `stroke-dasharray`.
    Numbers(Vec<f64>),
    /// SVG path data, normalised to absolute commands.
    Path(PathData),
    /// `height: auto`. Never interpolated: a track swaps it for the measured
    /// height while it runs and applies it again once it settles.
    Auto,
}

impl AnimValue {
    fn describe(&self) -> &'static str {
        match self {
            Self::Number(_) => "a number or px length",
            Self::Percent(_) => "a percentage",
            Self::Color(_) => "a colour",
            Self::Shadows(_) => "a shadow list",
            Self::Numbers(_) => "a list of numbers",
            Self::Path(_) => "path data",
            Self::Auto => "auto",
        }
    }
}

/// How a channel vector maps back to a value.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Layout {
    Number,
    Percent,
    Color,
    /// One entry per layer, giving its `inset`.
    Shadows(Vec<bool>),
    /// Shadow lists whose `inset` flags differ at some index cannot be
    /// interpolated: one channel runs from option to option and the value
    /// flips at the halfway point between them.
    DiscreteShadows(Vec<Vec<Shadow>>),
    Numbers,
    /// Paths with the same commands: the channels are their numbers, in order.
    Path(PathData),
    /// Paths with different commands cannot blend: one channel runs from
    /// option to option and the value flips at the halfway point between them.
    DiscretePath(Vec<PathData>),
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
            Self::Color => AnimValue::Color(Color::from_channels(channels)),
            Self::Shadows(insets) => AnimValue::Shadows(
                insets
                    .iter()
                    .zip(channels.chunks(SHADOW_CHANNELS))
                    .map(|(inset, layer)| Shadow::from_channels(layer, *inset))
                    .collect(),
            ),
            Self::DiscreteShadows(options) => {
                AnimValue::Shadows(options[option_index(channels[0], options.len())].clone())
            }
            Self::Numbers => AnimValue::Numbers(channels.to_vec()),
            Self::Path(template) => AnimValue::Path(template.with_numbers(channels)),
            Self::DiscretePath(options) => {
                AnimValue::Path(options[option_index(channels[0], options.len())].clone())
            }
        }
    }

    /// Whether a velocity measured in `self`'s channels still means the same
    /// thing in `other`'s, so a retarget can carry it over.
    pub(super) fn carries_velocity_to(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number, Self::Number)
            | (Self::Percent, Self::Percent)
            | (Self::Color, Self::Color) => true,
            (Self::Shadows(a), Self::Shadows(b)) => a.len() == b.len(),
            (Self::Numbers, Self::Numbers) => true,
            (Self::Path(a), Self::Path(b)) => a.same_structure(b),
            _ => false,
        }
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
        let channels: Vec<f64> = self
            .from
            .iter()
            .zip(&self.to)
            .map(|(from, to)| from + (to - from) * progress)
            .collect();
        self.layout.decode(&channels)
    }

    /// Shadow lists that differ in `inset` flip at half instead of blending,
    /// which a CSS transition does not run at all.
    pub(super) fn is_discrete(&self) -> bool {
        matches!(
            self.layout,
            Layout::DiscreteShadows(_) | Layout::DiscretePath(_)
        )
    }
}

pub(super) fn pair(name: &str, from: &AnimValue, to: &AnimValue) -> Result<Pair, String> {
    let mut aligned = align(name, &[from, to])?;
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

/// [`pair`] for any number of values, such as the keyframes of one track.
pub(super) fn align(name: &str, values: &[&AnimValue]) -> Result<Aligned, String> {
    for window in values.windows(2) {
        if std::mem::discriminant(window[0]) != std::mem::discriminant(window[1]) {
            return Err(format!(
                "motion {name} cannot animate from {} to {}: mixed units are not supported",
                window[0].describe(),
                window[1].describe()
            ));
        }
    }
    match values[0] {
        AnimValue::Number(_) => Ok(simple(Layout::Number, values, |value| match value {
            AnimValue::Number(number) => vec![*number],
            _ => unreachable!("kinds were checked"),
        })),
        AnimValue::Percent(_) => Ok(simple(Layout::Percent, values, |value| match value {
            AnimValue::Percent(fraction) => vec![*fraction],
            _ => unreachable!("kinds were checked"),
        })),
        AnimValue::Color(_) => Ok(simple(Layout::Color, values, |value| match value {
            AnimValue::Color(color) => color.channels().to_vec(),
            _ => unreachable!("kinds were checked"),
        })),
        AnimValue::Shadows(_) => Ok(align_shadows(&unwrap_each(values, |value| match value {
            AnimValue::Shadows(shadows) => shadows.as_slice(),
            _ => unreachable!("kinds were checked"),
        }))),
        AnimValue::Numbers(_) => align_numbers(
            name,
            &unwrap_each(values, |value| match value {
                AnimValue::Numbers(numbers) => numbers.as_slice(),
                _ => unreachable!("kinds were checked"),
            }),
        ),
        AnimValue::Path(_) => Ok(align_paths(&unwrap_each(values, |value| match value {
            AnimValue::Path(path) => path,
            _ => unreachable!("kinds were checked"),
        }))),
        AnimValue::Auto => Err(format!(
            "motion {name} cannot animate from {} to {}: mixed units are not supported",
            values[0].describe(),
            values[values.len() - 1].describe()
        )),
    }
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

fn align_shadows(lists: &[&[Shadow]]) -> Aligned {
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

    Aligned {
        layout: Layout::Shadows(padded[0].iter().map(|layer| layer.inset).collect()),
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
        Color::parse(value).unwrap()
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
        let overshoot = Color::from_channels(&[1.4, 0.4, 0.3, 1.0]);
        let [r, g, b, a] = rgba(overshoot);
        for channel in [r, g, b, a] {
            assert!((0.0..=1.0).contains(&channel));
        }
        let beyond = Color::from_channels(&[0.5, 0.0, 0.0, 1.3]);
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
    fn mixed_units_are_rejected() {
        let error = pair("width", &AnimValue::Number(10.0), &AnimValue::Percent(0.5)).unwrap_err();
        assert!(error.contains("width") && error.contains("mixed units"), "{error}");
        assert!(pair("color", &AnimValue::Number(1.0), &AnimValue::Color(color("#fff"))).is_err());
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
        let values = [AnimValue::Number(1.0), AnimValue::Number(2.0), AnimValue::Percent(0.5)];
        let refs: Vec<&AnimValue> = values.iter().collect();
        let error = align("width", &refs).unwrap_err();
        assert!(error.contains("from a number or px length to a percentage"), "{error}");
    }
}
