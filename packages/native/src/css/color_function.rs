//! `color(<space> c1 c2 c3 [/ alpha])`, for the spaces CSS Color 4 defines.

use super::color_space::Space;
use super::syntax::split_whitespace;
use crate::color::{ComputedColor, Srgba};

/// `args` is the text between the parentheses. Out-of-gamut results are left
/// for the caller to clip.
pub(crate) fn parse(args: &str) -> Option<Srgba> {
    let (space, components, alpha) = parse_in_space(args)?;
    Some(Srgba { rgb: space.to_srgb(components), alpha })
}

/// The colour as CSS computes it: in the space it names, components as written.
pub(crate) fn parse_computed(args: &str) -> Option<ComputedColor> {
    let (space, components, alpha) = parse_in_space(args)?;
    Some(ComputedColor::Space { space, components, alpha })
}

fn parse_in_space(args: &str) -> Option<(Space, [f64; 3], f64)> {
    let spaced = args.replace('/', " / ");
    let tokens = split_whitespace(&spaced);
    let (space, rest) = tokens.split_first()?;
    let space = Space::from_color_function(space)?;
    let alpha = match rest {
        [_, _, _] => 1.0,
        [_, _, _, "/", alpha] => parse_component(alpha)?.clamp(0.0, 1.0),
        _ => return None,
    };
    let mut components = [0.0; 3];
    for (slot, token) in components.iter_mut().zip(rest) {
        *slot = parse_component(token)?;
    }
    Some((space, components, alpha))
}

/// A number, a percentage (100% is 1 in every space `color()` names) or `none`.
fn parse_component(token: &str) -> Option<f64> {
    if token.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
    let value = match token.strip_suffix('%') {
        Some(percent) => percent.parse::<f64>().ok()? / 100.0,
        None => token.parse::<f64>().ok()?,
    };
    value.is_finite().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(input: &str) -> [f64; 3] {
        parse(input).unwrap_or_else(|| panic!("did not parse {input}")).rgb
    }

    #[test]
    fn srgb_is_the_identity() {
        assert_eq!(rgb("srgb 1 0.5 0"), [1.0, 0.5, 0.0]);
        assert_eq!(rgb("srgb 100% 50% none"), [1.0, 0.5, 0.0]);
    }

    #[test]
    fn display_p3_red_is_outside_srgb() {
        let rgb = rgb("display-p3 1 0 0");
        assert!(rgb[0] > 1.0 && rgb[1] < 0.0, "{rgb:?}");
    }

    #[test]
    fn white_is_white_in_every_space() {
        for input in [
            "srgb-linear 1 1 1",
            "display-p3 1 1 1",
            "a98-rgb 1 1 1",
            "prophoto-rgb 1 1 1",
            "rec2020 1 1 1",
            "xyz 0.95046 1 1.08906",
            "xyz-d50 0.96430 1 0.82511",
        ] {
            for channel in rgb(input) {
                assert!((channel - 1.0).abs() < 1e-3, "{input}");
            }
        }
    }

    #[test]
    fn reads_alpha() {
        assert_eq!(parse("srgb 1 0 0 / 0.25").unwrap().alpha, 0.25);
        assert_eq!(parse("srgb 1 0 0/50%").unwrap().alpha, 0.5);
    }

    #[test]
    fn rejects_malformed_input() {
        for input in [
            "",
            "srgb 1 0",
            "srgb 1 0 0 0",
            "nope 1 0 0",
            "srgb 1 0 0 /",
            "srgb a b c",
            "from srgb 1 0 0",
            "--custom 1 0 0",
        ] {
            assert!(parse(input).is_none(), "{input}");
        }
    }
}
