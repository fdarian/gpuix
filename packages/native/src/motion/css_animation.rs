//! CSS animation declarations (CSS Animations Level 1): the `animation`
//! shorthand and its eight longhands, parsed into one [`Animation`] per name.
//!
//! A bad value is rejected when the style is set ([`validate_animations`]), so
//! it fails where it is written instead of silently never running. A name with
//! no keyframes defined is not an error, as in CSS: it just animates nothing.

// Nothing reads the resolved lists until the runtime lands; the tests do.
#![cfg_attr(not(test), allow(dead_code))]

use super::css_transition::{
    check_seconds, is_timing_function, parse_delay, parse_duration, parse_list, parse_time,
    split_outside_parens, split_whitespace_outside_parens,
};
use super::curve::{parse_timing_function, Ease};
use crate::style::StyleDesc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum IterationCount {
    /// Never negative. Fractions run a partial last iteration.
    Finite(f64),
    Infinite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Direction {
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FillMode {
    None,
    Forwards,
    Backwards,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlayState {
    Running,
    Paused,
}

/// One entry of an element's `animation-*` lists, with the lists already
/// repeated or truncated to the length of `animation-name`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Animation {
    /// `None` for `animation-name: none`: the slot exists but animates nothing.
    pub name: Option<String>,
    /// Seconds, never negative. `auto` is `0s`, as CSS Animations 1 treats it.
    pub duration: f64,
    /// Seconds. A negative delay starts the animation part-way through.
    pub delay: f64,
    pub timing: Ease,
    pub iteration_count: IterationCount,
    pub direction: Direction,
    pub fill_mode: FillMode,
    pub play_state: PlayState,
}

/// The resolved `animation-*` lists of one element. Empty when it sets none.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct AnimationList(pub Vec<Animation>);

impl AnimationList {
    pub(super) fn parse(style: &StyleDesc) -> Result<Self, String> {
        if animation_fields(style).into_iter().all(Option::is_none) {
            return Ok(Self::default());
        }
        let mut lists = match style.animation.as_deref() {
            Some(text) => {
                Lists::parse_shorthand(text).map_err(|error| format!("animation: {error}"))?
            }
            None => Lists::initial(),
        };
        if let Some(text) = style.animation_name.as_deref() {
            lists.names = parse_list(text, parse_name_or_none)
                .map_err(|error| format!("animationName: {error}"))?;
        }
        if let Some(text) = style.animation_duration.as_deref() {
            lists.durations = parse_list(text, parse_animation_duration)
                .map_err(|error| format!("animationDuration: {error}"))?;
        }
        if let Some(text) = style.animation_timing_function.as_deref() {
            lists.timings = parse_list(text, parse_timing_function)
                .map_err(|error| format!("animationTimingFunction: {error}"))?;
        }
        if let Some(text) = style.animation_delay.as_deref() {
            lists.delays = parse_list(text, parse_delay)
                .map_err(|error| format!("animationDelay: {error}"))?;
        }
        if let Some(text) = style.animation_iteration_count.as_deref() {
            lists.counts = parse_list(text, parse_iteration_count)
                .map_err(|error| format!("animationIterationCount: {error}"))?;
        }
        if let Some(text) = style.animation_direction.as_deref() {
            lists.directions = parse_list(text, |item| {
                parse_direction(item).ok_or_else(|| {
                    format!(
                        "`{item}` is not a direction (normal, reverse, alternate, alternate-reverse)"
                    )
                })
            })
            .map_err(|error| format!("animationDirection: {error}"))?;
        }
        if let Some(text) = style.animation_fill_mode.as_deref() {
            lists.fills = parse_list(text, |item| {
                parse_fill_mode(item).ok_or_else(|| {
                    format!("`{item}` is not a fill mode (none, forwards, backwards, both)")
                })
            })
            .map_err(|error| format!("animationFillMode: {error}"))?;
        }
        if let Some(text) = style.animation_play_state.as_deref() {
            lists.states = parse_list(text, |item| {
                parse_play_state(item)
                    .ok_or_else(|| format!("`{item}` is not a play state (running, paused)"))
            })
            .map_err(|error| format!("animationPlayState: {error}"))?;
        }
        Ok(lists.into_list())
    }
}

fn animation_fields(style: &StyleDesc) -> [&Option<String>; 9] {
    [
        &style.animation,
        &style.animation_name,
        &style.animation_duration,
        &style.animation_timing_function,
        &style.animation_delay,
        &style.animation_iteration_count,
        &style.animation_direction,
        &style.animation_fill_mode,
        &style.animation_play_state,
    ]
}

/// The eight `animation-*` lists. `names` sets the count and the others repeat
/// or truncate to it, as in CSS.
struct Lists {
    names: Vec<Option<String>>,
    durations: Vec<f64>,
    timings: Vec<Ease>,
    delays: Vec<f64>,
    counts: Vec<IterationCount>,
    directions: Vec<Direction>,
    fills: Vec<FillMode>,
    states: Vec<PlayState>,
}

fn default_timing() -> Ease {
    Ease::CubicBezier([0.25, 0.1, 0.25, 1.0])
}

impl Lists {
    fn initial() -> Self {
        Self {
            names: vec![None],
            durations: vec![0.0],
            timings: vec![default_timing()],
            delays: vec![0.0],
            counts: vec![IterationCount::Finite(1.0)],
            directions: vec![Direction::Normal],
            fills: vec![FillMode::None],
            states: vec![PlayState::Running],
        }
    }

    fn parse_shorthand(text: &str) -> Result<Self, String> {
        match text.trim().to_ascii_lowercase().as_str() {
            "initial" | "unset" => return Ok(Self::initial()),
            "inherit" | "revert" | "revert-layer" => {
                return Err(format!("{} is not supported", text.trim()));
            }
            _ => {}
        }
        let mut lists = Self {
            names: Vec::new(),
            durations: Vec::new(),
            timings: Vec::new(),
            delays: Vec::new(),
            counts: Vec::new(),
            directions: Vec::new(),
            fills: Vec::new(),
            states: Vec::new(),
        };
        for item in split_outside_parens(text, ',')? {
            let single = Single::parse(item)?;
            lists.names.push(single.name);
            lists.durations.push(single.duration.unwrap_or(0.0));
            lists.timings.push(single.timing.unwrap_or_else(default_timing));
            lists.delays.push(single.delay.unwrap_or(0.0));
            lists
                .counts
                .push(single.count.unwrap_or(IterationCount::Finite(1.0)));
            lists.directions.push(single.direction.unwrap_or(Direction::Normal));
            lists.fills.push(single.fill.unwrap_or(FillMode::None));
            lists.states.push(single.state.unwrap_or(PlayState::Running));
        }
        Ok(lists)
    }

    fn into_list(self) -> AnimationList {
        AnimationList(
            (0..self.names.len())
                .map(|index| Animation {
                    name: self.names[index].clone(),
                    duration: self.durations[index % self.durations.len()],
                    delay: self.delays[index % self.delays.len()],
                    timing: self.timings[index % self.timings.len()].clone(),
                    iteration_count: self.counts[index % self.counts.len()],
                    direction: self.directions[index % self.directions.len()],
                    fill_mode: self.fills[index % self.fills.len()],
                    play_state: self.states[index % self.states.len()],
                })
                .collect(),
        )
    }
}

/// One comma-separated item of the shorthand, each part set at most once.
#[derive(Default)]
struct Single {
    name: Option<String>,
    duration: Option<f64>,
    delay: Option<f64>,
    timing: Option<Ease>,
    count: Option<IterationCount>,
    direction: Option<Direction>,
    fill: Option<FillMode>,
    state: Option<PlayState>,
}

impl Single {
    /// Parts may come in any order, except that the first `<time>` is the
    /// duration and the second the delay. A keyword that fits a longhand not
    /// yet set goes to that longhand and only otherwise names the animation,
    /// so `animation: none` fills with `none` and `2s paused spin` names `spin`.
    fn parse(item: &str) -> Result<Self, String> {
        let mut single = Self::default();
        let mut named = false;
        for token in split_whitespace_outside_parens(item) {
            if let Some(time) = parse_time(token) {
                if single.duration.is_none() {
                    if time < 0.0 {
                        return Err(format!("a duration cannot be negative: {token}"));
                    }
                    single.duration = Some(check_seconds(time, token)?);
                } else if single.delay.is_none() {
                    single.delay = Some(check_seconds(time, token)?);
                } else {
                    return Err(format!("too many times in `{item}`"));
                }
            } else if is_timing_function(token) {
                if single.timing.replace(parse_timing_function(token)?).is_some() {
                    return Err(format!("two timing functions in `{item}`"));
                }
            } else if single.count.is_none() && looks_like_count(token) {
                single.count = Some(parse_iteration_count(token)?);
            } else if let Some(direction) = parse_direction(token).filter(|_| single.direction.is_none()) {
                single.direction = Some(direction);
            } else if let Some(fill) = parse_fill_mode(token).filter(|_| single.fill.is_none()) {
                single.fill = Some(fill);
            } else if let Some(state) = parse_play_state(token).filter(|_| single.state.is_none()) {
                single.state = Some(state);
            } else if named {
                return Err(format!("two names in `{item}`"));
            } else {
                single.name = parse_name_or_none(token)?;
                named = true;
            }
        }
        Ok(single)
    }
}

fn looks_like_count(token: &str) -> bool {
    token.eq_ignore_ascii_case("infinite")
        || token
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_digit() || matches!(first, '.' | '+' | '-'))
            && token.parse::<f64>().is_ok()
}

fn parse_iteration_count(text: &str) -> Result<IterationCount, String> {
    if text.eq_ignore_ascii_case("infinite") {
        return Ok(IterationCount::Infinite);
    }
    let count = text
        .parse::<f64>()
        .ok()
        .filter(|count| count.is_finite())
        .ok_or_else(|| format!("not an iteration count: {text}"))?;
    if count < 0.0 {
        return Err(format!("an iteration count cannot be negative: {text}"));
    }
    Ok(IterationCount::Finite(count))
}

fn parse_direction(text: &str) -> Option<Direction> {
    Some(match text.to_ascii_lowercase().as_str() {
        "normal" => Direction::Normal,
        "reverse" => Direction::Reverse,
        "alternate" => Direction::Alternate,
        "alternate-reverse" => Direction::AlternateReverse,
        _ => return None,
    })
}

fn parse_fill_mode(text: &str) -> Option<FillMode> {
    Some(match text.to_ascii_lowercase().as_str() {
        "none" => FillMode::None,
        "forwards" => FillMode::Forwards,
        "backwards" => FillMode::Backwards,
        "both" => FillMode::Both,
        _ => return None,
    })
}

fn parse_play_state(text: &str) -> Option<PlayState> {
    Some(match text.to_ascii_lowercase().as_str() {
        "running" => PlayState::Running,
        "paused" => PlayState::Paused,
        _ => return None,
    })
}

/// `auto` is `0s` in CSS Animations 1.
fn parse_animation_duration(text: &str) -> Result<f64, String> {
    if text.eq_ignore_ascii_case("auto") {
        return Ok(0.0);
    }
    parse_duration(text)
}

fn parse_name_or_none(text: &str) -> Result<Option<String>, String> {
    if text.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    check_keyframes_name(text)?;
    Ok(Some(text.to_string()))
}

/// A CSS identifier, case-sensitive, that is not a CSS-wide keyword. Also what
/// `defineKeyframes` accepts, so every defined name can be written in a style.
pub(super) fn check_keyframes_name(name: &str) -> Result<(), String> {
    let mut characters = name.chars();
    let starts_well = match characters.next() {
        Some('-') => characters
            .next()
            .is_some_and(|second| second.is_alphabetic() || matches!(second, '_' | '-')),
        Some(first) => first.is_alphabetic() || first == '_',
        None => false,
    };
    let valid = starts_well
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'));
    if !valid {
        return Err(format!("`{name}` is not a valid animation name (a CSS identifier)"));
    }
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "none" | "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default"
    ) {
        return Err(format!("`{name}` cannot be an animation name"));
    }
    Ok(())
}

/// Rejects a style whose `animation*` values, in it or in a state style, do
/// not parse.
pub(crate) fn validate_animations(style: &StyleDesc) -> Result<(), String> {
    AnimationList::parse(style)?;
    for (name, state) in [
        ("hover", &style.hover),
        ("active", &style.active),
        ("focusVisible", &style.focus_visible),
    ] {
        if let Some(state) = state {
            AnimationList::parse(state).map_err(|error| format!("{name}.{error}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    fn parse(json: &str) -> Vec<Animation> {
        AnimationList::parse(&style(json)).unwrap().0
    }

    fn error(json: &str) -> String {
        AnimationList::parse(&style(json)).unwrap_err()
    }

    #[test]
    fn a_style_without_animation_fields_has_no_animations() {
        assert!(parse(r#"{ "opacity": 1 }"#).is_empty());
    }

    #[test]
    fn the_shorthand_takes_its_parts_in_any_order() {
        let animations = parse(r#"{ "animation": "spin 2s linear 500ms infinite alternate both paused" }"#);
        assert_eq!(
            animations,
            vec![Animation {
                name: Some("spin".to_string()),
                duration: 2.0,
                delay: 0.5,
                timing: Ease::Name("linear".to_string()),
                iteration_count: IterationCount::Infinite,
                direction: Direction::Alternate,
                fill_mode: FillMode::Both,
                play_state: PlayState::Paused,
            }]
        );
        let reordered = parse(r#"{ "animation": "paused both alternate infinite 500ms linear 2s spin" }"#);
        // The first time is still the duration, so the times swap here.
        assert_eq!((reordered[0].duration, reordered[0].delay), (0.5, 2.0));
        assert_eq!(reordered[0].name.as_deref(), Some("spin"));
    }

    #[test]
    fn omitted_parts_take_their_initial_values() {
        let animations = parse(r#"{ "animation": "fade 1s" }"#);
        assert_eq!(animations[0].timing, Ease::CubicBezier([0.25, 0.1, 0.25, 1.0]));
        assert_eq!(animations[0].delay, 0.0);
        assert_eq!(animations[0].iteration_count, IterationCount::Finite(1.0));
        assert_eq!(animations[0].direction, Direction::Normal);
        assert_eq!(animations[0].fill_mode, FillMode::None);
        assert_eq!(animations[0].play_state, PlayState::Running);
    }

    #[test]
    fn a_comma_list_makes_one_animation_per_item() {
        let animations = parse(r#"{ "animation": "a 1s, b 2s cubic-bezier(0, 0, 1, 1) 3, c" }"#);
        assert_eq!(animations.len(), 3);
        assert_eq!(animations[1].name.as_deref(), Some("b"));
        assert_eq!(animations[1].timing, Ease::CubicBezier([0.0, 0.0, 1.0, 1.0]));
        assert_eq!(animations[1].iteration_count, IterationCount::Finite(3.0));
        assert_eq!(animations[2].duration, 0.0);
    }

    #[test]
    fn a_keyword_for_another_longhand_is_not_the_name() {
        let animations = parse(r#"{ "animation": "none" }"#);
        assert_eq!(animations.len(), 1);
        assert_eq!(animations[0].name, None);
        assert_eq!(animations[0].fill_mode, FillMode::None);

        // `forwards` is the fill mode; `reverse` is taken, so the second is the name.
        let animations = parse(r#"{ "animation": "1s reverse forwards" }"#);
        assert_eq!(animations[0].name, None);
        assert_eq!(animations[0].fill_mode, FillMode::Forwards);
        assert_eq!(animations[0].direction, Direction::Reverse);

        let animations = parse(r#"{ "animation": "forwards forwards 1s" }"#);
        assert_eq!(animations[0].name.as_deref(), Some("forwards"));
        assert_eq!(animations[0].fill_mode, FillMode::Forwards);
    }

    #[test]
    fn a_name_is_case_sensitive_and_keywords_are_not() {
        let animations = parse(r#"{ "animation": "Spin 1s INFINITE ALTERNATE" }"#);
        assert_eq!(animations[0].name.as_deref(), Some("Spin"));
        assert_eq!(animations[0].iteration_count, IterationCount::Infinite);
        assert_eq!(animations[0].direction, Direction::Alternate);
    }

    #[test]
    fn fractional_and_zero_counts_are_valid() {
        let animations = parse(r#"{ "animation": "spin 1s 2.5" }"#);
        assert_eq!(animations[0].iteration_count, IterationCount::Finite(2.5));
        let animations = parse(r#"{ "animationIterationCount": "0" }"#);
        assert_eq!(animations[0].iteration_count, IterationCount::Finite(0.0));
    }

    #[test]
    fn a_negative_delay_is_kept() {
        let animations = parse(r#"{ "animation": "spin 2s -500ms" }"#);
        assert_eq!(animations[0].delay, -0.5);
    }

    #[test]
    fn longhands_win_over_the_shorthand() {
        let animations = parse(
            r#"{ "animation": "spin 1s linear", "animationDuration": "3s", "animationPlayState": "paused" }"#,
        );
        assert_eq!(animations[0].name.as_deref(), Some("spin"));
        assert_eq!(animations[0].duration, 3.0);
        assert_eq!(animations[0].play_state, PlayState::Paused);
        assert_eq!(animations[0].timing, Ease::Name("linear".to_string()));
    }

    #[test]
    fn longhands_alone_start_from_initial_values() {
        let animations = parse(r#"{ "animationName": "spin", "animationDuration": "1s" }"#);
        assert_eq!(animations.len(), 1);
        assert_eq!(animations[0].fill_mode, FillMode::None);
        assert_eq!(animations[0].iteration_count, IterationCount::Finite(1.0));
    }

    #[test]
    fn shorter_lists_repeat_and_longer_ones_truncate_to_the_name_count() {
        let animations = parse(
            r#"{
                "animationName": "a, b, c",
                "animationDuration": "1s, 2s",
                "animationDirection": "reverse, normal, alternate, alternate-reverse"
            }"#,
        );
        assert_eq!(animations.len(), 3);
        let durations: Vec<f64> = animations.iter().map(|animation| animation.duration).collect();
        assert_eq!(durations, [1.0, 2.0, 1.0]);
        assert_eq!(animations[2].direction, Direction::Alternate);
    }

    #[test]
    fn name_none_keeps_its_slot_and_animates_nothing() {
        let animations = parse(r#"{ "animationName": "none, spin", "animationDuration": "1s, 2s" }"#);
        assert_eq!(animations[0].name, None);
        assert_eq!(animations[1].name.as_deref(), Some("spin"));
        assert_eq!(animations[1].duration, 2.0);
    }

    #[test]
    fn auto_duration_is_zero() {
        assert_eq!(parse(r#"{ "animationDuration": "auto" }"#)[0].duration, 0.0);
    }

    #[test]
    fn initial_and_unset_reset_the_shorthand() {
        let animations = parse(r#"{ "animation": "initial" }"#);
        assert_eq!(animations[0].name, None);
        assert!(error(r#"{ "animation": "inherit" }"#).contains("not supported"));
    }

    #[test]
    fn invalid_values_name_the_field() {
        assert!(error(r#"{ "animation": "spin 1s 2s 3s" }"#).starts_with("animation: too many times"));
        assert!(error(r#"{ "animation": "spin 1s ease linear" }"#).contains("two timing functions"));
        assert!(error(r#"{ "animation": "a b 1s" }"#).contains("two names"));
        assert!(error(r#"{ "animation": "spin -1s" }"#).contains("cannot be negative"));
        assert!(error(r#"{ "animation": "spin 1s -2" }"#).contains("cannot be negative"));
        assert!(error(r#"{ "animation": "1s bogus(" }"#).starts_with("animation:"));
        assert!(error(r#"{ "animation": "" }"#).starts_with("animation:"));
        assert!(error(r#"{ "animation": "a,,b" }"#).starts_with("animation:"));
        assert!(error(r#"{ "animationName": "3d" }"#).starts_with("animationName:"));
        assert!(error(r#"{ "animationName": "initial" }"#).starts_with("animationName:"));
        assert!(error(r#"{ "animationDuration": "fast" }"#).starts_with("animationDuration:"));
        assert!(error(r#"{ "animationDuration": "-1s" }"#).starts_with("animationDuration:"));
        assert!(error(r#"{ "animationTimingFunction": "wobble" }"#)
            .starts_with("animationTimingFunction:"));
        assert!(error(r#"{ "animationDelay": "1" }"#).starts_with("animationDelay:"));
        assert!(error(r#"{ "animationIterationCount": "many" }"#)
            .starts_with("animationIterationCount:"));
        assert!(error(r#"{ "animationIterationCount": "NaN" }"#)
            .starts_with("animationIterationCount:"));
        assert!(error(r#"{ "animationDirection": "sideways" }"#).starts_with("animationDirection:"));
        assert!(error(r#"{ "animationFillMode": "after" }"#).starts_with("animationFillMode:"));
        assert!(error(r#"{ "animationPlayState": "stopped" }"#).starts_with("animationPlayState:"));
    }

    #[test]
    fn validation_covers_state_styles() {
        assert!(validate_animations(&style(r#"{ "animation": "spin 1s" }"#)).is_ok());
        let error = validate_animations(&style(r#"{ "hover": { "animationDelay": "soon" } }"#))
            .unwrap_err();
        assert!(error.starts_with("hover.animationDelay:"), "{error}");
        assert!(validate_animations(&style(r#"{ "focusVisible": { "animation": "a b" } }"#)).is_err());
    }

    #[test]
    fn keyframes_names_are_identifiers() {
        for name in ["spin", "fade-in", "_x", "-vendor-spin", "Spin2", "é"] {
            assert!(check_keyframes_name(name).is_ok(), "{name}");
        }
        for name in ["", "2d", "-2", "a b", "a,b", "none", "Inherit", "a(b)"] {
            assert!(check_keyframes_name(name).is_err(), "{name}");
        }
    }
}
