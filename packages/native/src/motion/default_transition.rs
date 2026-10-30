//! The transition Motion gives a key that its `transition` does not describe: `getDefaultTransition`
//! in motion-dom (`animation/utils/default-transitions.mjs`, the same in 12.x to 14.x), which
//! `animateMotionValue` applies when the transition holds only `delay` and the repeat fields.

use std::borrow::Cow;

use super::curve::{Curve, Ease, Spring, Transition};
use super::track::Goal;
use super::PropKey;

/// motion-dom's `transformProps`, which are the keys Motion runs as springs. `scaleZ` and `originZ` are not
/// among them.
const TRANSFORM_KEYS: [&str; 17] = [
    "transformPerspective",
    "x",
    "y",
    "z",
    "translateX",
    "translateY",
    "translateZ",
    "scale",
    "scaleX",
    "scaleY",
    "rotate",
    "rotateX",
    "rotateY",
    "rotateZ",
    "skew",
    "skewX",
    "skewY",
];

/// Motion's `ease`: a slightly shallower version of the browser's default easing.
const EASE: [f64; 4] = [0.25, 0.1, 0.35, 1.0];

/// What `getDefaultTransition(key, { keyframes })` answers. `key` is `None` for the animation of a
/// motion value, which Motion names `""`.
pub(super) fn curve(key: Option<PropKey>, keyframes: usize, ends_at_zero: bool) -> Curve {
    if keyframes > 2 {
        // Motion's `keyframesTransition` sets no easing, so the animation keeps its own `easeOut`.
        return Curve::Tween { duration: 0.8, ease: Ease::Name("easeOut".to_string()) };
    }
    let transform = matches!(key, Some(PropKey::Style(prop)) if TRANSFORM_KEYS.contains(&prop.name()));
    if !transform {
        return Curve::Tween { duration: 0.3, ease: Ease::CubicBezier(EASE) };
    }
    let scales = matches!(key, Some(PropKey::Style(prop)) if prop.name().starts_with("scale"));
    Curve::Spring(match (scales, ends_at_zero) {
        (false, _) => Spring::physics(500.0, 25.0, 10.0),
        (true, true) => Spring::physics(550.0, 2.0 * 550.0_f64.sqrt(), 10.0),
        (true, false) => Spring::physics(550.0, 30.0, 10.0),
    })
}

impl Transition {
    /// This transition for `key` animating `goal`: itself, or, if it only holds `delay` and the repeat
    /// fields, Motion's default curve with those.
    pub(super) fn for_key<'a>(&'a self, key: PropKey, goal: &Goal) -> Cow<'a, Transition> {
        if !self.defaulted {
            return Cow::Borrowed(self);
        }
        Cow::Owned(self.with_curve(curve(Some(key), goal.keyframe_count(), goal.ends_at_zero())))
    }

    /// [`for_key`](Self::for_key) for the animation of a motion value, which has no key.
    pub(super) fn for_value(self, keyframes: usize) -> Transition {
        if !self.defaulted {
            return self;
        }
        let curve = curve(None, keyframes, false);
        self.with_curve(curve)
    }

    fn with_curve(&self, curve: Curve) -> Transition {
        Transition { curve, defaulted: false, ..self.clone() }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::value::AnimValue;
    use super::super::{AnimProp, Target, Transitions};
    use super::*;

    fn goal(frames: &[f64]) -> Goal {
        let frames = frames.iter().map(|frame| Some(AnimValue::Number(*frame))).collect();
        Goal::keyframes("test", frames).unwrap()
    }

    fn curve_of(transition: serde_json::Value, key: &str, goal: &Goal) -> (Curve, f64) {
        let transitions = Transitions::parse(Some(&transition), Target::Style, false).unwrap();
        let key = PropKey::from_name(Target::Style, key).unwrap();
        let resolved = transitions.for_prop(key, goal);
        (resolved.curve.clone(), resolved.delay)
    }

    fn tween(duration: f64, ease: Ease) -> Curve {
        Curve::Tween { duration, ease }
    }

    fn ease_out() -> Ease {
        Ease::Name("easeOut".to_string())
    }

    fn is_spring(curve: &Curve, stiffness: f64, damping: f64) -> bool {
        matches!(curve, Curve::Spring(spring) if spring.stiffness == stiffness && spring.damping == damping)
    }

    #[test]
    fn a_transform_key_is_a_spring_and_anything_else_a_cubic_bezier() {
        let one = goal(&[100.0]);
        for key in ["x", "y", "z", "translateX", "translateY", "translateZ", "rotate", "rotateX", "rotateY", "rotateZ", "skew", "skewX", "skewY", "transformPerspective"] {
            let (curve, _) = curve_of(json!({}), key, &one);
            assert!(is_spring(&curve, 500.0, 25.0), "{key}: {curve:?}");
        }
        for key in ["width", "height", "opacity", "backgroundColor", "paddingTop", "scaleZ", "originX", "originZ", "fontSize"] {
            let (curve, _) = curve_of(json!({}), key, &one);
            assert_eq!(curve, tween(0.3, Ease::CubicBezier([0.25, 0.1, 0.35, 1.0])), "{key}");
        }
    }

    #[test]
    fn scale_is_damped_less_unless_it_ends_at_zero() {
        for key in ["scale", "scaleX", "scaleY"] {
            let (curve, _) = curve_of(json!({}), key, &goal(&[2.0]));
            assert!(is_spring(&curve, 550.0, 30.0), "{key}: {curve:?}");
            let (curve, _) = curve_of(json!({}), key, &goal(&[1.0, 0.0]));
            assert!(is_spring(&curve, 550.0, 2.0 * 550.0_f64.sqrt()), "{key}: {curve:?}");
        }
    }

    #[test]
    fn more_than_two_keyframes_run_for_eight_tenths_whatever_the_key() {
        for key in ["x", "scale", "width", "opacity"] {
            let (curve, _) = curve_of(json!({}), key, &goal(&[0.0, 1.0, 0.0]));
            assert_eq!(curve, tween(0.8, ease_out()), "{key}");
        }
        let (curve, _) = curve_of(json!({}), "x", &goal(&[0.0, 1.0]));
        assert!(is_spring(&curve, 500.0, 25.0), "two keyframes stay a spring: {curve:?}");
    }

    #[test]
    fn a_transition_of_only_a_delay_or_a_repeat_keeps_the_default_curve() {
        let one = goal(&[100.0]);
        let (curve, delay) = curve_of(json!({ "delay": 0.4 }), "x", &one);
        assert!(is_spring(&curve, 500.0, 25.0));
        assert_eq!(delay, 0.4);
        let (curve, delay) = curve_of(json!({ "delay": 0.4 }), "width", &one);
        assert_eq!(curve, tween(0.3, Ease::CubicBezier([0.25, 0.1, 0.35, 1.0])));
        assert_eq!(delay, 0.4);
        let transitions = Transitions::parse(Some(&json!({ "repeat": 2, "repeatType": "reverse" })), Target::Style, false).unwrap();
        let resolved = transitions.for_prop(PropKey::Style(AnimProp::from_name("x").unwrap()), &one);
        assert!(resolved.repeat.repeats());
        assert!(is_spring(&resolved.curve, 500.0, 25.0));
    }

    #[test]
    fn describing_the_curve_in_any_way_replaces_the_default() {
        let one = goal(&[100.0]);
        for transition in [
            json!({ "duration": 0.5 }),
            json!({ "ease": "linear" }),
            json!({ "type": "tween" }),
            json!({ "default": { "duration": 0.5 } }),
            json!({ "x": { "duration": 0.5 } }),
        ] {
            let (curve, _) = curve_of(transition.clone(), "x", &one);
            assert!(matches!(curve, Curve::Tween { .. }), "{transition}: {curve:?}");
        }
        let (curve, _) = curve_of(json!({ "type": "spring" }), "width", &one);
        assert!(matches!(curve, Curve::Spring(_)));
        let (curve, _) = curve_of(json!({ "type": "tween" }), "x", &one);
        assert_eq!(curve, tween(0.3, ease_out()));
    }

    #[test]
    fn an_entry_for_one_key_describes_the_curve_of_the_others() {
        // Motion hands every key without an entry the whole transition, which then counts as described.
        let one = goal(&[100.0]);
        let (curve, _) = curve_of(json!({ "width": { "duration": 0.5 } }), "x", &one);
        assert_eq!(curve, tween(0.3, ease_out()));
        let (curve, delay) = curve_of(json!({ "delay": 0.2, "width": { "duration": 0.5 } }), "x", &one);
        assert_eq!((curve, delay), (tween(0.3, ease_out()), 0.2));
        let (curve, delay) = curve_of(json!({ "x": { "delay": 0.2 } }), "x", &one);
        assert!(is_spring(&curve, 500.0, 25.0));
        assert_eq!(delay, 0.2);
    }

    #[test]
    fn default_stands_in_for_the_whole_transition_with_its_own_delay_only() {
        let one = goal(&[100.0]);
        let (curve, delay) = curve_of(json!({ "delay": 0.4, "default": { "duration": 0.5 } }), "x", &one);
        assert_eq!((curve, delay), (tween(0.5, ease_out()), 0.0));
        let (curve, delay) = curve_of(json!({ "delay": 0.4, "default": { "delay": 0.1 } }), "x", &one);
        assert!(is_spring(&curve, 500.0, 25.0));
        assert_eq!(delay, 0.1);
        let (curve, _) = curve_of(json!({ "default": {} }), "width", &one);
        assert_eq!(curve, tween(0.3, Ease::CubicBezier([0.25, 0.1, 0.35, 1.0])));
    }

    #[test]
    fn a_motion_value_has_no_key_so_it_never_springs() {
        let value = |keyframes: usize| Transition::undescribed().for_value(keyframes).curve;
        assert_eq!(value(2), tween(0.3, Ease::CubicBezier([0.25, 0.1, 0.35, 1.0])));
        assert_eq!(value(3), tween(0.8, ease_out()));
        let described = Transition::parse(json!({ "duration": 0.5 })).unwrap();
        assert_eq!(described.for_value(2).curve, tween(0.5, ease_out()));
    }
}
