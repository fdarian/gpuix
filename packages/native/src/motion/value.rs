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
    /// interpolated, so one channel runs 0 to 1 and the value flips at half.
    DiscreteShadows { from: Vec<Shadow>, to: Vec<Shadow> },
    Numbers,
    /// Paths with the same commands: the channels are their numbers, in order.
    Path(PathData),
    /// Paths with different commands cannot blend, so one channel runs 0 to 1
    /// and the value flips at half.
    DiscretePath { from: PathData, to: PathData },
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
            Self::DiscreteShadows { from, to } => {
                AnimValue::Shadows(if channels[0] < 0.5 { from } else { to }.clone())
            }
            Self::Numbers => AnimValue::Numbers(channels.to_vec()),
            Self::Path(template) => AnimValue::Path(template.with_numbers(channels)),
            Self::DiscretePath { from, to } => {
                AnimValue::Path(if channels[0] < 0.5 { from } else { to }.clone())
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
            Layout::DiscreteShadows { .. } | Layout::DiscretePath { .. }
        )
    }
}

pub(super) fn pair(name: &str, from: &AnimValue, to: &AnimValue) -> Result<Pair, String> {
    match (from, to) {
        (AnimValue::Number(from), AnimValue::Number(to)) => Ok(Pair {
            layout: Layout::Number,
            from: vec![*from],
            to: vec![*to],
        }),
        (AnimValue::Percent(from), AnimValue::Percent(to)) => Ok(Pair {
            layout: Layout::Percent,
            from: vec![*from],
            to: vec![*to],
        }),
        (AnimValue::Color(from), AnimValue::Color(to)) => Ok(Pair {
            layout: Layout::Color,
            from: from.channels().to_vec(),
            to: to.channels().to_vec(),
        }),
        (AnimValue::Shadows(from), AnimValue::Shadows(to)) => Ok(pair_shadows(from, to)),
        (AnimValue::Numbers(from), AnimValue::Numbers(to)) => pair_numbers(name, from, to),
        (AnimValue::Path(from), AnimValue::Path(to)) => Ok(pair_paths(from, to)),
        _ => Err(format!(
            "motion {name} cannot animate from {} to {}: mixed units are not supported",
            from.describe(),
            to.describe()
        )),
    }
}

/// Lists of different lengths are repeated to their least common multiple, as
/// CSS interpolates `stroke-dasharray`.
fn pair_numbers(name: &str, from: &[f64], to: &[f64]) -> Result<Pair, String> {
    if from.is_empty() || to.is_empty() {
        return Err(format!("motion {name} cannot animate from or to an empty list"));
    }
    let divisor = (1..=from.len().min(to.len()))
        .rev()
        .find(|candidate| from.len() % candidate == 0 && to.len() % candidate == 0)
        .unwrap_or(1);
    let length = from.len() / divisor * to.len();
    let repeat = |list: &[f64]| list.iter().copied().cycle().take(length).collect();
    Ok(Pair {
        layout: Layout::Numbers,
        from: repeat(from),
        to: repeat(to),
    })
}

fn pair_paths(from: &PathData, to: &PathData) -> Pair {
    if from.same_structure(to) {
        return Pair {
            layout: Layout::Path(from.clone()),
            from: from.numbers(),
            to: to.numbers(),
        };
    }
    Pair {
        layout: Layout::DiscretePath {
            from: from.clone(),
            to: to.clone(),
        },
        from: vec![0.0],
        to: vec![1.0],
    }
}

fn pair_shadows(from: &[Shadow], to: &[Shadow]) -> Pair {
    let layers = from.len().max(to.len());
    let padded = |list: &[Shadow], other: &[Shadow]| -> Vec<Shadow> {
        (0..layers)
            .map(|index| {
                list.get(index)
                    .cloned()
                    .unwrap_or_else(|| Shadow::none(other[index].inset))
            })
            .collect()
    };
    let from_layers = padded(from, to);
    let to_layers = padded(to, from);

    if from_layers
        .iter()
        .zip(&to_layers)
        .any(|(from, to)| from.inset != to.inset)
    {
        return Pair {
            layout: Layout::DiscreteShadows {
                from: from.to_vec(),
                to: to.to_vec(),
            },
            from: vec![0.0],
            to: vec![1.0],
        };
    }

    let channels = |layers: &[Shadow]| {
        let mut channels = Vec::with_capacity(layers.len() * SHADOW_CHANNELS);
        for layer in layers {
            layer.push_channels(&mut channels);
        }
        channels
    };
    Pair {
        layout: Layout::Shadows(from_layers.iter().map(|layer| layer.inset).collect()),
        from: channels(&from_layers),
        to: channels(&to_layers),
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
}
