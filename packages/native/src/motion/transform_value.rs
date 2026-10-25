//! Transform lists as animated values, CSS Transforms 1 "Interpolation of transform functions".
//!
//! A list is not a vector of numbers a track can blend channel by channel: which functions pair up
//! depends on the two lists. So a track holds the keyframes' lists whole (`Layout::Transform`) and a
//! position along them, and [`blend`] decides, for the pair a position falls between, what the list
//! there is.

use gpui::{blend_rotations, list_matrix, px, size, TransformFunction, TransformLength};

/// The list `progress` of the way from `from` to `to`. Progress outside 0 to 1 extrapolates.
///
/// As Chrome does (`TransformOperations::Blend`, third_party/blink/renderer/platform/transforms/
/// transform_operations.cc): the functions that pair up from the start of the lists are blended one by one,
/// and what is left of both lists from the first pair that does not is blended as two matrices
/// (`TransformFunction::Interpolated`). When one list is a run of functions that all pair up with the
/// start of the other, the rest of the longer one blends with identity functions of its own type; `none`
/// is the empty list, so it blends with every list in that way.
///
/// A matrix that cannot be decomposed (a `matrix()` or `matrix3d()` pair, or the matrices left over, that
/// flatten space) does not blend: Chrome then steps the whole list at the middle, unless a fraction of the
/// box is in what is left over, where the blend is decided once the box is known and steps only that part.
pub(super) fn blend(from: &[TransformFunction], to: &[TransformFunction], progress: f64) -> Vec<TransformFunction> {
    let shared = from.len().min(to.len());
    let longest = from.len().max(to.len());
    let paired = from.iter().zip(to).take_while(|(from, to)| pairs(from, to)).count();
    let blended = if paired == shared { longest } else { paired };
    if blended < longest && !has_fraction(&from[blended..]) && !has_fraction(&to[blended..]) {
        if is_singular(&from[blended..]) || is_singular(&to[blended..]) {
            return (if progress < 0.5 { from } else { to }).to_vec();
        }
    }
    let matrix_pair_is_singular = (0..blended).any(|index| match (from.get(index), to.get(index)) {
        (Some(from), Some(to)) if is_matrix(from) => is_singular(std::slice::from_ref(from)) || is_singular(std::slice::from_ref(to)),
        _ => false,
    });
    if matrix_pair_is_singular {
        return (if progress < 0.5 { from } else { to }).to_vec();
    }
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

fn has_fraction(list: &[TransformFunction]) -> bool {
    list.iter().any(|function| match function {
        TransformFunction::Translate(x, y) | TransformFunction::Translate3d(x, y, _) => x.fraction != 0.0 || y.fraction != 0.0,
        TransformFunction::Interpolated { from, to, .. } => has_fraction(from) || has_fraction(to),
        _ => false,
    })
}

/// Whether the matrix of `list` flattens space, which no decomposition can take apart.
fn is_singular(list: &[TransformFunction]) -> bool {
    list_matrix(list, size(px(0.0), px(0.0))).inverse().is_none()
}

fn is_matrix(function: &TransformFunction) -> bool {
    matches!(function, TransformFunction::Matrix(_) | TransformFunction::Matrix3d(_) | TransformFunction::Interpolated { .. })
}

/// What Blink calls the primitive type of a function (`TransformOperation::PrimitiveType`): two functions
/// blend one by one when they share it. A `translate()` and a `translateZ()` are both `Translate3D`, and a
/// `rotate()` and a `rotateX()` both `Rotate3D`; every other function is only its own kind, and a `matrix()`
/// is not a `matrix3d()`. What an earlier blend of matrices left (`Interpolated`) is a matrix of the
/// plane, or of space, as Blink's `MatrixTransformOperation::Blend` makes it.
#[derive(PartialEq)]
enum Primitive {
    Translate,
    Scale,
    Rotate,
    Skew,
    SkewX,
    SkewY,
    Perspective,
    Matrix,
    Matrix3d,
}

fn primitive(function: &TransformFunction) -> Primitive {
    use TransformFunction::*;
    match function {
        Translate(..) | Translate3d(..) => Primitive::Translate,
        Scale(..) | Scale3d(..) => Primitive::Scale,
        Rotate(_) | Rotate3d(..) => Primitive::Rotate,
        Skew(..) => Primitive::Skew,
        SkewX(_) => Primitive::SkewX,
        SkewY(_) => Primitive::SkewY,
        Perspective(_) => Primitive::Perspective,
        Matrix(_) => Primitive::Matrix,
        Matrix3d(_) => Primitive::Matrix3d,
        Interpolated { .. } if function.is_3d() => Primitive::Matrix3d,
        Interpolated { .. } => Primitive::Matrix,
    }
}

fn pairs(from: &TransformFunction, to: &TransformFunction) -> bool {
    primitive(from) == primitive(to)
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
        Matrix(_) => Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        Matrix3d(_) => Matrix3d(IDENTITY_3D),
        Interpolated { .. } if function.is_3d() => Matrix3d(IDENTITY_3D),
        Interpolated { .. } => Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
    }
}

const IDENTITY_3D: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

fn mix(from: f32, to: f32, progress: f64) -> f32 {
    (f64::from(from) + (f64::from(to) - f64::from(from)) * progress) as f32
}

fn mix_length(from: &TransformLength, to: &TransformLength, progress: f64) -> TransformLength {
    TransformLength { px: mix(from.px, to.px, progress), fraction: mix(from.fraction, to.fraction, progress) }
}

/// The turn of a rotation function as `rotate3d()` has it, `[x, y, z, angle]`.
fn turn_of(function: &TransformFunction) -> [f32; 4] {
    match function {
        TransformFunction::Rotate(angle) => [0.0, 0.0, 1.0, *angle],
        TransformFunction::Rotate3d(x, y, z, angle) => [*x, *y, *z, *angle],
        other => unreachable!("{other:?} is no rotation"),
    }
}

/// Two functions that [`pairs`].
fn blend_pair(from: &TransformFunction, to: &TransformFunction, progress: f64) -> TransformFunction {
    use TransformFunction::*;
    match (from, to) {
        (Translate(from_x, from_y), Translate(to_x, to_y)) => {
            Translate(mix_length(from_x, to_x, progress), mix_length(from_y, to_y, progress))
        }
        (Translate3d(..) | Translate(..), Translate3d(..) | Translate(..)) => {
            let parts = |function: &TransformFunction| match function {
                Translate(x, y) => (*x, *y, 0.0),
                Translate3d(x, y, z) => (*x, *y, *z),
                _ => unreachable!("a translation was matched"),
            };
            let ((from_x, from_y, from_z), (to_x, to_y, to_z)) = (parts(from), parts(to));
            Translate3d(
                mix_length(&from_x, &to_x, progress),
                mix_length(&from_y, &to_y, progress),
                mix(from_z, to_z, progress),
            )
        }
        (Scale(from_x, from_y), Scale(to_x, to_y)) => Scale(mix(*from_x, *to_x, progress), mix(*from_y, *to_y, progress)),
        (Scale3d(..) | Scale(..), Scale3d(..) | Scale(..)) => {
            let parts = |function: &TransformFunction| match function {
                Scale(x, y) => (*x, *y, 1.0),
                Scale3d(x, y, z) => (*x, *y, *z),
                _ => unreachable!("a scaling was matched"),
            };
            let ((from_x, from_y, from_z), (to_x, to_y, to_z)) = (parts(from), parts(to));
            Scale3d(mix(from_x, to_x, progress), mix(from_y, to_y, progress), mix(from_z, to_z, progress))
        }
        (Rotate(from), Rotate(to)) => Rotate(mix(*from, *to, progress)),
        (Rotate3d(..) | Rotate(..), Rotate3d(..) | Rotate(..)) => {
            let [x, y, z, angle] = blend_rotations(turn_of(from), turn_of(to), progress);
            Rotate3d(x, y, z, angle)
        }
        (Skew(from_x, from_y), Skew(to_x, to_y)) => Skew(mix(*from_x, *to_x, progress), mix(*from_y, *to_y, progress)),
        (SkewX(from), SkewX(to)) => SkewX(mix(*from, *to, progress)),
        (SkewY(from), SkewY(to)) => SkewY(mix(*from, *to, progress)),
        (Perspective(from), Perspective(to)) => Perspective(blend_perspective(*from, *to, progress)),
        _ => Interpolated { from: vec![from.clone()], to: vec![to.clone()], progress: progress as f32 },
    }
}

/// Blink's `PerspectiveTransformOperation::Blend`: the distances blend as their reciprocals (CSS Transforms 2
/// "Interpolation of transform functions"), `none` being a distance without end and so a reciprocal of 0, and
/// a distance under a pixel counting as a pixel. A reciprocal that comes out 0 or less is `none` again.
pub(crate) fn blend_perspective(from: Option<f32>, to: Option<f32>, progress: f64) -> Option<f32> {
    let reciprocal = |distance: Option<f32>| distance.map_or(0.0, |distance| 1.0 / f64::from(distance).max(1.0));
    let (from, to) = (reciprocal(from), reciprocal(to));
    let blended = from + (to - from) * progress;
    (blended > 0.0 && blended.is_normal()).then(|| (1.0 / blended) as f32)
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
    fn the_3d_translations_scalings_and_turns_pair_with_their_plane_forms() {
        let zero = TransformLength::default();
        assert_eq!(
            at("translateZ(10px)", "translate3d(20px, 40px, 30px)", 0.5),
            [TransformFunction::Translate3d(px(10.0), px(20.0), 20.0)]
        );
        assert_eq!(at("translate(10px, 20px)", "translateZ(8px)", 0.5), [TransformFunction::Translate3d(px(5.0), px(10.0), 4.0)]);
        assert_eq!(at("scale(2)", "scale3d(4, 4, 3)", 0.5), [TransformFunction::Scale3d(3.0, 3.0, 2.0)]);
        assert_eq!(at("scaleZ(3)", "scale(5)", 0.5), [TransformFunction::Scale3d(3.0, 3.0, 2.0)]);
        assert_eq!(
            at("none", "translateZ(10px)", 0.5),
            [TransformFunction::Translate3d(zero, zero, 5.0)]
        );
        assert_eq!(at("scale3d(3, 3, 3)", "none", 0.5), [TransformFunction::Scale3d(2.0, 2.0, 2.0)]);
    }

    #[test]
    fn turns_about_one_axis_blend_their_angles_whatever_they_are_called() {
        let [TransformFunction::Rotate3d(x, y, z, angle)] = at("rotateY(0deg)", "rotate3d(0, 5, 0, 180deg)", 0.5)[..] else {
            panic!("one rotation")
        };
        assert_eq!((x, y, z), (0.0, 1.0, 0.0));
        assert!((angle - 90f32.to_radians()).abs() < 1e-5);
        let [TransformFunction::Rotate(angle)] = at("rotate(0deg)", "rotateZ(270deg)", 0.5)[..] else { panic!("one rotation") };
        assert!((angle - 135f32.to_radians()).abs() < 1e-5);
        let [TransformFunction::Rotate3d(x, y, z, angle)] = at("none", "rotateX(40deg)", 0.5)[..] else { panic!("one rotation") };
        assert_eq!((x, y, z), (1.0, 0.0, 0.0));
        assert!((angle - 20f32.to_radians()).abs() < 1e-5);
    }

    #[test]
    fn turns_about_two_axes_blend_as_quaternions_and_stay_one_function() {
        let blended = at("rotateX(90deg)", "rotateY(90deg)", 0.5);
        let [TransformFunction::Rotate3d(x, y, z, angle)] = blended[..] else { panic!("one rotation: {blended:?}") };
        assert!((x - y).abs() < 1e-5 && z.abs() < 1e-5 && (x - 0.5f32.sqrt()).abs() < 1e-5);
        assert!((angle - 2.0 * (2f32 / 3.0).sqrt().acos()).abs() < 1e-5);
    }

    #[test]
    fn perspectives_blend_as_the_reciprocals_of_their_distances() {
        let distance = |blended: Vec<TransformFunction>| match blended[..] {
            [TransformFunction::Perspective(distance)] => distance,
            _ => panic!("one perspective: {blended:?}"),
        };
        // 1/100 and 1/400 meet at 1/160.
        let middle = distance(at("perspective(100px)", "perspective(400px)", 0.5)).unwrap();
        assert!((middle - 160.0).abs() < 1e-3, "{middle}");
        // `none` is the reciprocal 0: 1/100 halfway to it is 1/200.
        let to_none = distance(at("perspective(100px)", "perspective(none)", 0.5)).unwrap();
        assert!((to_none - 200.0).abs() < 1e-3, "{to_none}");
        assert_eq!(distance(at("perspective(none)", "perspective(none)", 0.5)), None);
        assert_eq!(distance(at("perspective(100px)", "perspective(none)", 1.0)), None);
        // A distance under a pixel counts as a pixel.
        let tiny = distance(at("perspective(0)", "perspective(0.5px)", 0.3)).unwrap();
        assert!((tiny - 1.0).abs() < 1e-6, "{tiny}");
        // `none` blends as the other list's perspective turned off.
        let from_nothing = distance(at("none", "perspective(100px)", 0.5)).unwrap();
        assert!((from_nothing - 200.0).abs() < 1e-3, "{from_nothing}");
    }

    #[test]
    fn a_matrix_and_a_matrix3d_do_not_pair_and_blend_the_rest_of_both_lists_as_one_matrix() {
        let blended = at("matrix(1, 0, 0, 1, 0, 0) translate(10px)", "matrix3d(1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1) translate(30px)", 0.5);
        assert!(matches!(blended[..], [TransformFunction::Interpolated { .. }]), "{blended:?}");
        let blended = at("matrix3d(1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1) translate(10px)", "matrix3d(1,0,0,0, 0,1,0,0, 0,0,1,0, 5,0,0,1) translate(30px)", 0.5);
        assert!(matches!(blended[..], [TransformFunction::Interpolated { .. }, TransformFunction::Translate(..)]), "{blended:?}");
    }

    #[test]
    fn a_plane_function_and_a_3d_one_that_do_not_pair_blend_as_matrices() {
        let blended = at("translateX(10px)", "rotateY(45deg)", 0.5);
        assert!(matches!(blended[..], [TransformFunction::Interpolated { .. }]), "{blended:?}");
        assert!(blended[0].is_3d());
    }

    #[test]
    fn a_singular_matrix_in_what_is_left_over_steps_the_whole_list_at_the_middle() {
        let from = list("translate(10px) scale(0)");
        let to = list("translate(50px) rotate(30deg)");
        assert_eq!(blend(&from, &to, 0.25), from);
        assert_eq!(blend(&from, &to, 0.75), to);
        // A fraction of the box makes the blend wait for the box: only the matrices step.
        let sized = list("translate(10%) rotate(30deg)");
        assert!(matches!(blend(&list("scale(0)"), &sized, 0.25)[..], [TransformFunction::Interpolated { .. }]));
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
