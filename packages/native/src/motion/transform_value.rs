//! Transform lists as animated values, CSS Transforms 1 "Interpolation of transform functions".
//!
//! A list is not a vector of numbers a track can blend channel by channel: which functions pair up
//! depends on the two lists. So a track holds the keyframes' lists whole (`Layout::Transform`) and a
//! position along them, and [`blend`] decides, for the pair a position falls between, what the list
//! there is.

use gpui::{TransformFunction, TransformLength};

/// The list `progress` of the way from `from` to `to`. Progress outside 0 to 1 extrapolates.
///
/// As Chrome does: the functions that pair up from the start of the lists are blended one by one, and
/// what is left of both lists from the first pair that does not is blended as two matrices
/// (`TransformFunction::Interpolated`). When one list is a run of functions that all pair up with the
/// start of the other, the rest of the longer one blends with identity functions of its own type; `none`
/// is the empty list, so it blends with every list in that way.
pub(super) fn blend(from: &[TransformFunction], to: &[TransformFunction], progress: f64) -> Vec<TransformFunction> {
    let shared = from.len().min(to.len());
    let longest = from.len().max(to.len());
    let paired = from.iter().zip(to).take_while(|(from, to)| pairs(from, to)).count();
    let blended = if paired == shared { longest } else { paired };
    let mut list: Vec<TransformFunction> = (0..blended)
        .map(|index| match (from.get(index), to.get(index)) {
            (Some(from), Some(to)) => blend_pair(from, to, progress),
            (Some(from), None) => blend_pair(from, &identity(from), progress),
            (None, Some(to)) => blend_pair(&identity(to), to, progress),
            (None, None) => unreachable!("the index is within the longer list"),
        })
        .collect();
    if blended < longest {
        list.push(TransformFunction::Interpolated {
            from: from[blended..].to_vec(),
            to: to[blended..].to_vec(),
            progress: progress as f32,
        });
    }
    list
}

/// Whether two functions blend by their own numbers. `translate` and `translateX` are both
/// `Translate`, as they are one primitive in CSS, and likewise `scale`; `skewX`, `skewY` and `skew`
/// are three that do not pair with each other. Matrices blend through their decomposition, and
/// so does what an earlier blend left in the middle of a transition.
fn pairs(from: &TransformFunction, to: &TransformFunction) -> bool {
    use TransformFunction::*;
    matches!(
        (from, to),
        (Translate(..), Translate(..))
            | (Scale(..), Scale(..))
            | (Rotate(_), Rotate(_))
            | (Skew(..), Skew(..))
            | (SkewX(_), SkewX(_))
            | (SkewY(_), SkewY(_))
            | (Matrix(_) | Interpolated { .. }, Matrix(_) | Interpolated { .. })
    )
}

/// The function of `function`'s type that does nothing.
fn identity(function: &TransformFunction) -> TransformFunction {
    use TransformFunction::*;
    match function {
        Translate(..) => Translate(TransformLength::default(), TransformLength::default()),
        Scale(..) => Scale(1.0, 1.0),
        Rotate(_) => Rotate(0.0),
        Skew(..) => Skew(0.0, 0.0),
        SkewX(_) => SkewX(0.0),
        SkewY(_) => SkewY(0.0),
        Translate3d(..) => Translate3d(TransformLength::default(), TransformLength::default(), 0.0),
        Scale3d(..) => Scale3d(1.0, 1.0, 1.0),
        Rotate3d(x, y, z, _) => Rotate3d(*x, *y, *z, 0.0),
        Perspective(_) => Perspective(None),
        Matrix3d(_) => Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        Matrix(_) | Interpolated { .. } => Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
    }
}

fn mix(from: f32, to: f32, progress: f64) -> f32 {
    (f64::from(from) + (f64::from(to) - f64::from(from)) * progress) as f32
}

fn mix_length(from: &TransformLength, to: &TransformLength, progress: f64) -> TransformLength {
    TransformLength { px: mix(from.px, to.px, progress), fraction: mix(from.fraction, to.fraction, progress) }
}

/// Two functions that [`pairs`].
fn blend_pair(from: &TransformFunction, to: &TransformFunction, progress: f64) -> TransformFunction {
    use TransformFunction::*;
    match (from, to) {
        (Translate(from_x, from_y), Translate(to_x, to_y)) => {
            Translate(mix_length(from_x, to_x, progress), mix_length(from_y, to_y, progress))
        }
        (Scale(from_x, from_y), Scale(to_x, to_y)) => Scale(mix(*from_x, *to_x, progress), mix(*from_y, *to_y, progress)),
        (Rotate(from), Rotate(to)) => Rotate(mix(*from, *to, progress)),
        (Skew(from_x, from_y), Skew(to_x, to_y)) => Skew(mix(*from_x, *to_x, progress), mix(*from_y, *to_y, progress)),
        (SkewX(from), SkewX(to)) => SkewX(mix(*from, *to, progress)),
        (SkewY(from), SkewY(to)) => SkewY(mix(*from, *to, progress)),
        (Matrix(_) | Interpolated { .. }, Matrix(_) | Interpolated { .. }) => {
            Interpolated { from: vec![from.clone()], to: vec![to.clone()], progress: progress as f32 }
        }
        _ => unreachable!("{from:?} and {to:?} do not pair"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::length::LengthContext;
    use crate::css::transform::{parse_transform, Value};

    fn list(text: &str) -> Vec<TransformFunction> {
        match parse_transform(text, &LengthContext::for_tests()) {
            Some(Value::List(functions)) => functions,
            Some(Value::None) => Vec::new(),
            None => panic!("{text} does not parse"),
        }
    }

    fn at(from: &str, to: &str, progress: f64) -> Vec<TransformFunction> {
        blend(&list(from), &list(to), progress)
    }

    fn px(value: f32) -> TransformLength {
        TransformLength::pixels(value)
    }

    #[test]
    fn translations_blend_their_pixels_and_their_fractions_apart() {
        let zero = TransformLength::default();
        assert_eq!(
            at("translate(10px, 0)", "translate(50%, 20px)", 0.5),
            [TransformFunction::Translate(TransformLength { px: 5.0, fraction: 0.25 }, px(10.0))]
        );
        assert_eq!(at("translateX(40px)", "translateY(60px)", 0.5), [TransformFunction::Translate(px(20.0), px(30.0))]);
        assert_eq!(at("translate(0, 0)", "translate(100px)", 1.0), [TransformFunction::Translate(px(100.0), zero)]);
    }

    #[test]
    fn a_rotation_turns_the_long_way_round_with_no_shortest_path() {
        let [TransformFunction::Rotate(angle)] = at("rotate(0deg)", "rotate(270deg)", 0.5)[..] else {
            panic!("one rotation")
        };
        assert!((angle - 135f32.to_radians()).abs() < 1e-5);
    }

    #[test]
    fn none_blends_as_the_identity_of_the_other_list() {
        assert_eq!(
            at("none", "scale(3) rotate(90deg)", 0.5),
            [TransformFunction::Scale(2.0, 2.0), TransformFunction::Rotate(45f32.to_radians())]
        );
        assert_eq!(at("none", "none", 0.5), []);
        assert_eq!(at("translate(10px) skewX(20deg)", "none", 0.5).len(), 2);
    }

    #[test]
    fn the_longer_list_blends_its_tail_with_identity_functions() {
        let blended = at("translate(10px)", "translate(110px) rotate(90deg)", 0.5);
        assert_eq!(blended, [TransformFunction::Translate(px(60.0), TransformLength::default()), TransformFunction::Rotate(45f32.to_radians())]);
    }

    #[test]
    fn lists_that_stop_pairing_blend_the_rest_as_matrices() {
        let blended = at("scale(1, 3) rotate(30deg)", "scale(3, 1) translate(40px)", 0.5);
        assert_eq!(blended.len(), 2);
        assert_eq!(blended[0], TransformFunction::Scale(2.0, 2.0));
        let TransformFunction::Interpolated { from, to, progress } = &blended[1] else { panic!("{:?}", blended[1]) };
        assert_eq!((from.clone(), to.clone(), *progress), (list("rotate(30deg)"), list("translate(40px)"), 0.5));
    }

    #[test]
    fn a_mismatch_in_the_first_function_makes_one_matrix_of_both_lists() {
        let blended = at("rotate(30deg) translate(10px)", "scale(2) translate(110px)", 0.25);
        assert_eq!(
            blended,
            [TransformFunction::Interpolated {
                from: list("rotate(30deg) translate(10px)"),
                to: list("scale(2) translate(110px)"),
                progress: 0.25,
            }]
        );
    }

    #[test]
    fn the_skews_pair_only_with_their_own_kind() {
        assert_eq!(at("skewX(10deg)", "skewX(50deg)", 0.5), [TransformFunction::SkewX(30f32.to_radians())]);
        assert_eq!(at("skewY(10deg)", "skewY(50deg)", 0.5), [TransformFunction::SkewY(30f32.to_radians())]);
        assert!(matches!(at("skewX(10deg)", "skewY(50deg)", 0.5)[..], [TransformFunction::Interpolated { .. }]));
        assert!(matches!(at("skew(10deg)", "skewX(50deg)", 0.5)[..], [TransformFunction::Interpolated { .. }]));
        assert!(matches!(at("skew(10deg, 5deg)", "skew(50deg)", 0.5)[..], [TransformFunction::Skew(..)]));
    }

    #[test]
    fn matrices_blend_through_their_decomposition() {
        assert!(matches!(at("matrix(1, 0, 0, 1, 0, 0)", "matrix(0, 1, -1, 0, 50, 20)", 0.5)[..], [TransformFunction::Interpolated { .. }]));
        assert!(matches!(at("none", "matrix(2, 0, 0, 3, 10, 20)", 0.5)[..], [TransformFunction::Interpolated { .. }]));
    }

    #[test]
    fn the_ends_are_the_lists_themselves() {
        let to = list("translate(110px, -40px) rotate(0.5rad)");
        assert_eq!(blend(&list("translate(10px, 20px) rotate(0deg)"), &to, 1.0), to);
        let from = list("scale(2) skewX(10deg)");
        assert_eq!(blend(&from, &list("scale(5) skewX(30deg)"), 0.0), from);
    }

    #[test]
    fn overshoot_extrapolates() {
        assert_eq!(at("translate(0)", "translate(100px)", 1.5), [TransformFunction::Translate(px(150.0), TransformLength::default())]);
        assert_eq!(at("scale(1)", "scale(2)", -0.5), [TransformFunction::Scale(0.5, 0.5)]);
    }
}
