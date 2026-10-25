//! `color-mix(in <space> [<hue-method> hue], <color> [p%], <color> [p%])`.

use super::color_space::{Space, Vec3};
use super::syntax::{split_commas, split_whitespace};
use crate::color::{resolve_unclamped, ColorContext, ComputedColor, Srgba};

#[derive(Clone, Copy, PartialEq, Debug)]
enum HueMethod {
    Shorter,
    Longer,
    Increasing,
    Decreasing,
}

/// `args` is the text between the parentheses. Both operands are resolved with
/// `context`, so `currentColor`, `light-dark()` and a nested `color-mix()` work.
pub(crate) fn parse(args: &str, context: &ColorContext) -> Option<Srgba> {
    let (space, mixed, alpha) = parse_in_space(args, context)?;
    Some(Srgba { rgb: space.to_srgb(mixed), alpha })
}

/// The mix as the colour CSS computes it to: in the space it was mixed in, which `hsl` and `hwb`
/// leave for `srgb`.
pub(crate) fn parse_computed(args: &str, context: &ColorContext) -> Option<ComputedColor> {
    let (space, mixed, alpha) = parse_in_space(args, context)?;
    Some(match space {
        Space::Hsl | Space::Hwb => ComputedColor::Space { space: Space::Srgb, components: space.to_srgb(mixed), alpha },
        space => ComputedColor::Space { space, components: mixed, alpha },
    })
}

/// The mixed components, in the space they were mixed in, and the alpha.
fn parse_in_space(args: &str, context: &ColorContext) -> Option<(Space, Vec3, f64)> {
    let pieces = split_commas(args);
    let [header, first, second] = pieces.as_slice() else {
        return None;
    };
    let (space, hue_method) = parse_header(header)?;
    let (first, first_percent) = parse_operand(first, context)?;
    let (second, second_percent) = parse_operand(second, context)?;

    let (first_percent, second_percent) = match (first_percent, second_percent) {
        (None, None) => (50.0, 50.0),
        (Some(percent), None) => (percent, 100.0 - percent),
        (None, Some(percent)) => (100.0 - percent, percent),
        (Some(first), Some(second)) => (first, second),
    };
    let total = first_percent + second_percent;
    if total <= 0.0 {
        return None;
    }
    // A total under 100% leaves the result partly transparent; over 100% only
    // rescales the weights.
    let alpha_scale = (total / 100.0).min(1.0);
    let (mixed, alpha) = mix(
        space,
        hue_method,
        &first,
        first_percent / total,
        &second,
        second_percent / total,
        alpha_scale,
    );
    Some((space, mixed, alpha))
}

fn parse_header(header: &str) -> Option<(Space, HueMethod)> {
    let tokens = split_whitespace(header);
    let [keyword, space, rest @ ..] = tokens.as_slice() else {
        return None;
    };
    if !keyword.eq_ignore_ascii_case("in") {
        return None;
    }
    let space = Space::from_interpolation(space)?;
    let method = match rest {
        [] => HueMethod::Shorter,
        [method, hue] if hue.eq_ignore_ascii_case("hue") && space.hue_index().is_some() => {
            match method.to_ascii_lowercase().as_str() {
                "shorter" => HueMethod::Shorter,
                "longer" => HueMethod::Longer,
                "increasing" => HueMethod::Increasing,
                "decreasing" => HueMethod::Decreasing,
                _ => return None,
            }
        }
        _ => return None,
    };
    Some((space, method))
}

/// A colour with the percentage written before or after it.
fn parse_operand(operand: &str, context: &ColorContext) -> Option<(Srgba, Option<f64>)> {
    let (color, percent) = match split_whitespace(operand).as_slice() {
        [color] => (*color, None),
        [color, percent] if percent.ends_with('%') => (*color, Some(*percent)),
        [percent, color] if percent.ends_with('%') => (*color, Some(*percent)),
        _ => return None,
    };
    let percent = match percent {
        Some(text) => {
            let value = text.strip_suffix('%')?.parse::<f64>().ok()?;
            if !(0.0..=100.0).contains(&value) {
                return None;
            }
            Some(value)
        }
        None => None,
    };
    Some((resolve_unclamped(color, context)?, percent))
}

fn mix(
    space: Space,
    method: HueMethod,
    first: &Srgba,
    first_weight: f64,
    second: &Srgba,
    second_weight: f64,
    alpha_scale: f64,
) -> (Vec3, f64) {
    let (first_alpha, second_alpha) = (first.alpha.clamp(0.0, 1.0), second.alpha.clamp(0.0, 1.0));
    let mut a = space.from_srgb(first.rgb);
    let mut b = space.from_srgb(second.rgb);

    // A missing component (a grey's hue) takes the other colour's value.
    for index in 0..3 {
        match (a[index].is_nan(), b[index].is_nan()) {
            (true, false) => a[index] = b[index],
            (false, true) => b[index] = a[index],
            (true, true) => {
                a[index] = 0.0;
                b[index] = 0.0;
            }
            _ => {}
        }
    }
    if let Some(index) = space.hue_index() {
        let (hue_a, hue_b) = fix_hues(a[index], b[index], method);
        a[index] = hue_a;
        b[index] = hue_b;
    }

    let alpha = first_weight * first_alpha + second_weight * second_alpha;
    let mut mixed: Vec3 = [0.0; 3];
    for index in 0..3 {
        mixed[index] = if Some(index) == space.hue_index() {
            first_weight * a[index] + second_weight * b[index]
        } else if alpha == 0.0 {
            0.0
        } else {
            // Premultiplied, so a transparent operand does not tint the result.
            (first_weight * first_alpha * a[index] + second_weight * second_alpha * b[index]) / alpha
        };
    }
    (mixed, alpha * alpha_scale)
}

/// Adjusts two hue angles so linear interpolation takes the requested arc.
fn fix_hues(first: f64, second: f64, method: HueMethod) -> (f64, f64) {
    let (mut first, mut second) = (first.rem_euclid(360.0), second.rem_euclid(360.0));
    let delta = second - first;
    match method {
        HueMethod::Shorter => {
            if delta > 180.0 {
                first += 360.0;
            } else if delta < -180.0 {
                second += 360.0;
            }
        }
        HueMethod::Longer => {
            if 0.0 < delta && delta < 180.0 {
                first += 360.0;
            } else if -180.0 < delta && delta <= 0.0 {
                second += 360.0;
            }
        }
        HueMethod::Increasing => {
            if second < first {
                second += 360.0;
            }
        }
        HueMethod::Decreasing => {
            if first < second {
                first += 360.0;
            }
        }
    }
    (first, second)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Scheme;

    fn mixed(args: &str) -> Srgba {
        parse(args, &ColorContext::root(Scheme::Light)).unwrap_or_else(|| panic!("did not parse {args}"))
    }

    fn close(actual: [f64; 3], expected: [f64; 3]) {
        for index in 0..3 {
            assert!((actual[index] - expected[index]).abs() < 2e-3, "{actual:?} != {expected:?}");
        }
    }

    #[test]
    fn mixes_in_srgb_half_and_half() {
        let result = mixed("in srgb, red, blue");
        close(result.rgb, [0.5, 0.0, 0.5]);
        assert_eq!(result.alpha, 1.0);
    }

    #[test]
    fn percentages_weight_and_default_to_the_remainder() {
        close(mixed("in srgb, red 25%, blue").rgb, [0.25, 0.0, 0.75]);
        close(mixed("in srgb, red, blue 25%").rgb, [0.75, 0.0, 0.25]);
        close(mixed("in srgb, 25% red, 75% blue").rgb, [0.25, 0.0, 0.75]);
        // Over 100% rescales the weights.
        close(mixed("in srgb, red 60%, blue 60%").rgb, [0.5, 0.0, 0.5]);
    }

    #[test]
    fn a_total_under_100_percent_is_partly_transparent() {
        let result = mixed("in srgb, red 20%, blue 20%");
        close(result.rgb, [0.5, 0.0, 0.5]);
        assert!((result.alpha - 0.4).abs() < 1e-9);
    }

    #[test]
    fn oklab_matches_the_css_color_4_example() {
        // Browsers render color-mix(in oklab, white, black) as rgb(99 99 99).
        let result = mixed("in oklab, white, black");
        let grey = (result.rgb[0] * 255.0).round();
        assert!((grey - 99.0).abs() <= 1.0, "{grey}");
        close(result.rgb, [result.rgb[0]; 3]);
    }

    #[test]
    fn linear_srgb_mix_of_black_and_white_is_lighter_than_srgb() {
        let linear = mixed("in srgb-linear, white, black").rgb[0];
        let gamma = mixed("in srgb, white, black").rgb[0];
        assert!((gamma - 0.5).abs() < 1e-9);
        assert!((linear * 255.0).round() == 188.0, "{linear}");
    }

    #[test]
    fn hue_takes_the_shorter_arc_by_default() {
        // hsl hue 350 and 10 average to 0 (red), not 180 (cyan).
        close(mixed("in hsl, hsl(350 100% 50%), hsl(10 100% 50%)").rgb, [1.0, 0.0, 0.0]);
        // Longer takes the far side: hue 180, cyan.
        close(
            mixed("in hsl longer hue, hsl(350 100% 50%), hsl(10 100% 50%)").rgb,
            [0.0, 1.0, 1.0],
        );
    }

    #[test]
    fn increasing_and_decreasing_pick_a_direction() {
        // 10 -> 350 increasing passes through 180.
        close(
            mixed("in hsl increasing hue, hsl(10 100% 50%), hsl(350 100% 50%)").rgb,
            [0.0, 1.0, 1.0],
        );
        // 10 -> 350 decreasing passes through 0.
        close(
            mixed("in hsl decreasing hue, hsl(10 100% 50%), hsl(350 100% 50%)").rgb,
            [1.0, 0.0, 0.0],
        );
    }

    #[test]
    fn a_grey_borrows_the_other_colours_hue() {
        // White has no hue in oklch, so the mix keeps blue's hue and lightens it.
        let result = mixed("in oklch, white, blue");
        assert!(result.rgb[2] > result.rgb[0], "{:?}", result.rgb);
    }

    #[test]
    fn transparent_does_not_tint_the_mix() {
        let result = mixed("in srgb, red, transparent");
        close(result.rgb, [1.0, 0.0, 0.0]);
        assert!((result.alpha - 0.5).abs() < 1e-9);
    }

    #[test]
    fn nests_and_resolves_current_color() {
        let mut context = ColorContext::root(Scheme::Light);
        context = context.with_current(gpui::Rgba { r: 0.0, g: 0.0, b: 1.0, a: 1.0 });
        let result = parse("in srgb, currentColor, color-mix(in srgb, red, red)", &context).unwrap();
        close(result.rgb, [0.5, 0.0, 0.5]);
    }

    #[test]
    fn rejects_malformed_input() {
        for input in [
            "",
            "in srgb, red",
            "in srgb, red, blue, green",
            "srgb, red, blue",
            "in nope, red, blue",
            "in srgb, red 150%, blue",
            "in srgb, red 0%, blue 0%",
            "in srgb longer hue, red, blue",
            "in hsl sideways hue, red, blue",
            "in srgb, notacolor, blue",
            "in srgb, red 10% 20%, blue",
        ] {
            assert!(parse(input, &ColorContext::root(Scheme::Light)).is_none(), "{input}");
        }
    }
}
