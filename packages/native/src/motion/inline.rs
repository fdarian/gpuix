//! CSS transitions and animations, and `motion`, of the colours of inline boxes.
//!
//! A `<span>` has no GPUI element, so `build_element` never runs for it: its
//! paragraph reads the style of each box while it builds the text, and paints the
//! box's colours itself. This feeds those styles through the machinery elements
//! use ([`CssTransitions`], [`CssAnimations`]), keyed by the box's id, so the
//! state lives and dies with the node and a paragraph reads the sampled colours
//! of the frame.
//!
//! Only colours move ([`Scope::InlineBox`]): they are what a paragraph can change
//! without laying out again. A transition or animation of a size, padding or font
//! of a span is not run, and starts no event. `motion` (`motion.span`) runs
//! between the transitions and the CSS animations, as it does for an element, and
//! a description that names any other property never mounts
//! ([`MotionState::new_inline`]).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use web_time::Instant;

use super::{
    resolve_declared_style, CssAnimations, CssTransitions, InterpolateSize, KeyframesRegistry,
    MotionState, MotionValueStore, ParentFont, Scope,
};
use crate::color::ColorContext;
use crate::events::state_styles::{folded, PointerStates};
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;
use crate::text::inline::Paragraph;

static EMPTY_STYLE: std::sync::LazyLock<StyleDesc> = std::sync::LazyLock::new(StyleDesc::default);

/// The clock and the state a frame's inline boxes are resolved with.
pub(crate) struct Timeline<'a> {
    /// What the motion values the frame sampled are, for the boxes whose `style` binds some.
    pub values: &'a MotionValueStore,
    pub transitions: &'a mut CssTransitions,
    pub animations: &'a mut CssAnimations,
    pub keyframes: &'a KeyframesRegistry,
    pub motion: &'a HashMap<u64, MotionState>,
    pub now: Instant,
}

/// The style of each inline box of a paragraph that has a transition or an
/// animation, as it shows in this frame. A box that has none is not here.
#[derive(Default)]
pub(crate) struct InlineStyles(HashMap<u64, StyleDesc>);

impl InlineStyles {
    /// Whether a box shows something other than what it declares, or declares an animation: the
    /// paragraph paints differently from one frame to the next without the tree changing.
    pub(crate) fn is_active(&self, tree: &RetainedTree, states: &dyn Fn(u64, &StyleDesc) -> PointerStates) -> bool {
        self.0.iter().any(|(id, sampled)| {
            tree.elements.get(id).and_then(|element| element.style.as_deref()).is_none_or(|style| {
                *sampled != *folded(style, states(*id, style)) || super::css_animation::declares_animation(style)
            })
        })
    }

    /// The style the box `id` declares. A box that declares none and has `motion`
    /// (`<motion.span animate>`) has the one motion sampled for it.
    pub(crate) fn declared_by<'a>(&'a self, tree: &'a RetainedTree, id: u64) -> Option<&'a StyleDesc> {
        tree.elements
            .get(&id)?
            .style
            .as_deref()
            .or_else(|| self.0.get(&id))
    }

    /// `base` as the box `id` shows it: the sampled style when a transition or an
    /// animation is on it, else `base` with `states` folded in.
    pub(crate) fn declared<'a>(
        &'a self,
        id: u64,
        base: &'a StyleDesc,
        states: PointerStates,
    ) -> Cow<'a, StyleDesc> {
        match self.0.get(&id) {
            Some(sampled) => Cow::Borrowed(sampled),
            None => folded(base, states),
        }
    }

    /// Steps the transitions, `motion` and animations of the boxes of `paragraph` to
    /// `timeline.now`. `colors` and `font` are the container's, as its children
    /// inherit them; a box inherits from the boxes around it as they show in this
    /// frame, so a span follows the colour of a transitioning parent.
    pub(crate) fn resolve(
        tree: &RetainedTree,
        paragraph: &Paragraph,
        colors: &ColorContext,
        font: &ParentFont,
        states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
        timeline: Timeline,
    ) -> Self {
        let mut sampled = HashMap::new();
        // A box is declared the same way around every piece of text it holds, so
        // the first piece that holds it steps it.
        let mut stepped = HashSet::new();
        for piece in &paragraph.pieces {
            let mut colors = *colors;
            let mut font = font.clone();
            for id in &piece.boxes {
                let Some(element) = tree.elements.get(id) else {
                    continue;
                };
                let style = element.style.as_deref();
                if stepped.insert(*id) {
                    let transitions = timeline.transitions.wants(*id, style);
                    let animations = timeline.animations.wants(*id, style);
                    let motion = timeline.motion.get(id);
                    let binding = element.custom_props().get("motionValues");
                    if transitions || animations || motion.is_some() || binding.is_some() {
                        let style = style.unwrap_or(&EMPTY_STYLE);
                        let resolved = step(
                            *id,
                            element.style_before_transition(),
                            style,
                            states(*id, style),
                            (transitions, animations),
                            (motion, binding, timeline.values),
                            (&colors, &font),
                            &mut *timeline.transitions,
                            &mut *timeline.animations,
                            timeline.keyframes,
                            timeline.now,
                        );
                        sampled.insert(*id, resolved);
                    }
                }
                let declared = match (sampled.get(id), style) {
                    (Some(sampled), _) => Cow::Borrowed(sampled),
                    (None, Some(base)) => folded(base, states(*id, base)),
                    (None, None) => continue,
                };
                colors = colors.descend(&declared);
                font = font.descend(Some(&declared));
            }
        }
        Self(sampled)
    }
}

/// One box's style after its transitions, then `motion`, then its animations,
/// which come last as they do for an element.
#[allow(clippy::too_many_arguments)]
fn step(
    id: u64,
    before: Option<&StyleDesc>,
    style: &StyleDesc,
    pointer: PointerStates,
    (transitions_wanted, animations_wanted): (bool, bool),
    (motion, binding, values): (Option<&MotionState>, Option<&serde_json::Value>, &MotionValueStore),
    (colors, font): (&ColorContext, &ParentFont),
    transitions: &mut CssTransitions,
    animations: &mut CssAnimations,
    keyframes: &KeyframesRegistry,
    now: Instant,
) -> StyleDesc {
    let declared = resolve_declared_style(
        transitions_wanted.then_some(transitions),
        animations_wanted.then_some((&mut *animations, keyframes)),
        before,
        colors,
        font,
        pointer,
        motion,
        id,
        style,
        super::FocusVisible::default(),
        None,
        InterpolateSize::default(),
        Scope::InlineBox,
        now,
    );
    let moved = motion.and_then(|state| state.visible_style(now)).map(|values| {
        let mut moved = declared.style.clone();
        values.apply_to(&mut moved);
        moved
    });
    // Motion values are inline style as well, and come after `motion`.
    let bound = binding.map(|binding| {
        let mut bound = moved.clone().unwrap_or_else(|| declared.style.clone());
        values.apply_in(binding, &mut bound, font, Scope::InlineBox);
        bound
    });
    let moved = bound.or(moved);
    let underlying = moved.as_ref().unwrap_or(&declared.style);
    let animated = animations_wanted
        .then(|| {
            animations.update(
                id,
                &declared.style,
                underlying,
                colors,
                font,
                None,
                InterpolateSize::default(),
                Scope::InlineBox,
                now,
            )
        })
        .flatten();
    match animated {
        Some(values) => {
            let mut resolved = underlying.clone();
            values.apply_to(&mut resolved, declared.transitions.as_ref());
            resolved
        }
        None => moved.unwrap_or(declared.style),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::css::length::LinearLength;
    use crate::text::inline_insets::InsetSpec;
    use std::rc::Rc;
    use crate::color::Scheme;
    use crate::css::font_features::FontFeatureState;
    use crate::css::length::LengthContext;
    use crate::motion::TransitionEvent;
    use crate::text::inline::{plan_children, Child};
    use crate::text::paragraph::{run_styles, RunStyle};

    /// `div(1) > ["b" in span(3) > ["c" in span(5)]]`, with the texts as 4 and 6.
    struct Page {
        origin: Instant,
        tree: RetainedTree,
        transitions: CssTransitions,
        animations: CssAnimations,
        keyframes: KeyframesRegistry,
        motion: HashMap<u64, MotionState>,
        values: MotionValueStore,
    }

    impl Page {
        fn new() -> Self {
            let mut tree = RetainedTree::new();
            for (id, kind) in [(1, "div"), (3, "span"), (4, "text"), (5, "span"), (6, "text")] {
                tree.create_element(id, kind.to_string());
            }
            for (parent, child) in [(1, 3), (3, 4), (3, 5), (5, 6)] {
                tree.append_child(parent, child);
            }
            tree.set_text(4, "b".to_string());
            tree.set_text(6, "c".to_string());
            Self {
                origin: Instant::now(),
                tree,
                transitions: CssTransitions::default(),
                animations: CssAnimations::default(),
                keyframes: KeyframesRegistry::default(),
                motion: HashMap::new(),
                values: MotionValueStore::default(),
            }
        }

        /// What the renderer does for a gesture on a span: `sync_motion_states` tells its state.
        fn gesture(&mut self, id: u64, gestures: crate::motion::Gestures, millis: u64) {
            let base = self.tree.elements[&id].style.clone();
            let now = self.origin + Duration::from_millis(millis);
            self.motion
                .get_mut(&id)
                .expect("a mounted state")
                .set_gestures(
                    crate::motion::GestureStates { own: gestures, ..Default::default() },
                    crate::motion::GestureBase::Style(base.as_deref()),
                    None,
                    0,
                    now,
                )
                .expect("a valid gesture");
        }

        /// What `sync_motion_states` does for a `motion.span`: mounts the state, or
        /// retargets the one it has.
        fn describe(&mut self, id: u64, json: &str) -> Result<(), String> {
            self.describe_at(id, json, 0)
        }

        fn describe_at(&mut self, id: u64, json: &str, millis: u64) -> Result<(), String> {
            let now = self.origin + Duration::from_millis(millis);
            let source: serde_json::Value = serde_json::from_str(json).expect("a description");
            let base = self.tree.elements[&id].style.clone();
            let colors = crate::motion::Colors::of(&ColorContext::root(Scheme::Light), base.as_deref());
            match self.motion.get_mut(&id) {
                Some(state) => state.sync_inline(&source, base.as_deref(), colors, now),
                None => {
                    let state = MotionState::new_inline(&source, base.as_deref(), colors, now)?;
                    self.motion.insert(id, state);
                    Ok(())
                }
            }
        }

        fn motion_frame(&mut self, id: u64, millis: u64) -> crate::motion::MotionFrame {
            let now = self.origin + Duration::from_millis(millis);
            self.motion.get_mut(&id).expect("a mounted state").frame(now)
        }

        fn style(&mut self, id: u64, json: &str) {
            let style = serde_json::from_str(json).expect("a style");
            self.tree.set_style(id, Arc::new(style));
        }

        fn paragraph(&self) -> crate::text::inline::Paragraph {
            match plan_children(&self.tree, &self.tree.elements[&1]).remove(0) {
                Child::Paragraph(paragraph) => paragraph,
                Child::Block(_) | Child::AnonymousCell(_) | Child::AnonymousTable(_) => panic!("a paragraph"),
            }
        }

        fn frame(&mut self, millis: u64) -> InlineStyles {
            let paragraph = self.paragraph();
            InlineStyles::resolve(
                &self.tree,
                &paragraph,
                &ColorContext::root(Scheme::Light),
                &ParentFont::initial(),
                &|_, _| PointerStates::default(),
                Timeline {
                    values: &self.values,
                    transitions: &mut self.transitions,
                    animations: &mut self.animations,
                    keyframes: &self.keyframes,
                    motion: &self.motion,
                    now: self.origin + Duration::from_millis(millis),
                },
            )
        }

        fn shown(&self, styles: &InlineStyles, id: u64) -> StyleDesc {
            let base = self.tree.elements[&id].style.as_deref().expect("a styled box");
            styles.declared(id, base, PointerStates::default()).into_owned()
        }

        fn groups(&self, styles: &InlineStyles) -> Vec<crate::text::paragraph::OpacityGroup> {
            crate::text::paragraph::styles(
                &self.tree,
                &self.paragraph(),
                &ColorContext::root(Scheme::Light),
                &LengthContext::initial(800.0, 600.0),
                &FontFeatureState::default(),
                Default::default(),
                &|_, _| PointerStates::default(),
                styles,
            )
            .groups
        }

        fn runs(&self, styles: &InlineStyles) -> Vec<RunStyle> {
            run_styles(
                &self.tree,
                &self.paragraph(),
                &ColorContext::root(Scheme::Light),
                &LengthContext::initial(800.0, 600.0),
                &FontFeatureState::default(),
                Default::default(),
                &|_, _| PointerStates::default(),
                styles,
            )
        }
    }

    fn rgb(value: &Option<String>) -> [f32; 3] {
        let color = ColorContext::root(Scheme::Light)
            .resolve(value.as_deref().expect("a colour"))
            .expect("a valid colour");
        [color.r, color.g, color.b]
    }

    /// Strictly between the red and the blue it goes from and to.
    fn mid_red_to_blue(value: &Option<String>) {
        let [red, _, blue] = rgb(value);
        assert!(red > 0.05 && red < 0.95, "red is {red}");
        assert!(blue > 0.05 && blue < 0.95, "blue is {blue}");
    }

    fn is(value: &Option<String>, expected: &str) {
        let expected = ColorContext::root(Scheme::Light).resolve(expected).expect("a colour");
        for (actual, expected) in rgb(value).iter().zip([expected.r, expected.g, expected.b]) {
            assert!((actual - expected).abs() < 0.01, "{value:?} is not {expected}");
        }
    }

    #[test]
    fn a_background_between_two_colours_shows_neither_while_it_transitions() {
        let mut page = Page::new();
        let style = |color: &str| {
            format!(r#"{{ "backgroundColor": "{color}", "transition": "background-color 1s linear" }}"#)
        };
        page.style(3, &style("red"));
        page.frame(0);
        page.style(3, &style("blue"));

        let start = page.frame(0);
        is(&page.shown(&start, 3).background_color, "red");
        let middle = page.frame(500);
        mid_red_to_blue(&page.shown(&middle, 3).background_color);
        assert!(page.transitions.take_active(), "the window needs another frame");
        let end = page.frame(1000);
        is(&page.shown(&end, 3).background_color, "blue");
    }

    #[test]
    fn the_text_colour_and_the_decoration_colour_transition() {
        let mut page = Page::new();
        let style = |text: &str, line: &str| {
            format!(
                r#"{{ "color": "{text}", "textDecorationLine": "underline", "textDecorationColor": "{line}",
                    "transition": "color 1s linear, text-decoration-color 1s linear" }}"#
            )
        };
        page.style(3, &style("red", "red"));
        page.frame(0);
        page.style(3, &style("blue", "lime"));
        page.frame(0);

        let middle = page.frame(500);
        let shown = page.shown(&middle, 3);
        mid_red_to_blue(&shown.color);
        let [red, green, blue] = rgb(&shown.text_decoration_color);
        assert!(red > 0.05 && red < 0.95 && green > 0.05 && green < 0.95 && blue < 0.01, "{red} {green} {blue}");
        // The runs paint what the style shows.
        let runs = page.runs(&middle);
        let text = gpui::Rgba::from(runs[0].color.expect("the run is coloured"));
        assert!((text.b - rgb(&shown.color)[2]).abs() < 0.01);
        let line = gpui::Rgba::from(
            runs[0].underline.expect("the run is underlined").color.expect("the line is coloured"),
        );
        assert!((line.g - green).abs() < 0.01);
    }

    #[test]
    fn border_colours_transition_as_a_whole_and_by_side() {
        let mut page = Page::new();
        let style = |all: &str, left: &str| {
            format!(
                r#"{{ "borderWidth": 2, "borderStyle": "solid", "borderColor": "{all}", "borderLeftColor": "{left}",
                    "transition": "border-color 1s linear" }}"#
            )
        };
        page.style(3, &style("red", "red"));
        page.frame(0);
        page.style(3, &style("blue", "red"));
        page.frame(0);
        let middle = page.frame(500);
        let shown = page.shown(&middle, 3);
        mid_red_to_blue(&shown.border_top_color);
        mid_red_to_blue(&shown.border_bottom_color);
        is(&shown.border_left_color, "red");

        page.style(3, &style("blue", "blue"));
        page.frame(1000);
        let later = page.frame(1500);
        let shown = page.shown(&later, 3);
        // A side that is not moving is the shorthand's again.
        is(&shown.border_top_color.or(shown.border_color), "blue");
        mid_red_to_blue(&shown.border_left_color);
    }

    #[test]
    fn a_colour_without_a_transition_is_the_new_colour_at_once() {
        let mut page = Page::new();
        page.style(3, r#"{ "backgroundColor": "red" }"#);
        page.frame(0);
        page.style(3, r#"{ "backgroundColor": "blue" }"#);
        let styles = page.frame(0);
        is(&page.shown(&styles, 3).background_color, "blue");
        assert!(styles.0.is_empty(), "nothing was stepped");
        assert!(!page.transitions.take_active());
    }

    #[test]
    fn a_transition_declared_with_the_new_colour_runs() {
        let mut page = Page::new();
        page.style(3, r#"{ "backgroundColor": "red" }"#);
        page.frame(0);
        page.style(3, r#"{ "backgroundColor": "blue", "transition": "background-color 1s linear" }"#);
        page.frame(0);
        let middle = page.frame(500);
        mid_red_to_blue(&page.shown(&middle, 3).background_color);
    }

    #[test]
    fn a_keyframe_animation_moves_the_colour() {
        let mut page = Page::new();
        page.keyframes
            .define(
                "wash",
                r#"{ "from": { "backgroundColor": "red" }, "to": { "backgroundColor": "blue" } }"#,
            )
            .unwrap();
        page.style(3, r#"{ "backgroundColor": "lime", "animation": "wash 1s linear infinite" }"#);
        let start = page.frame(0);
        is(&page.shown(&start, 3).background_color, "red");
        let middle = page.frame(500);
        mid_red_to_blue(&page.shown(&middle, 3).background_color);
        assert!(page.animations.take_active());
        let again = page.frame(1250);
        let [red, _, blue] = rgb(&page.shown(&again, 3).background_color);
        assert!(red > blue, "the second lap is a quarter in: {red} {blue}");
    }

    #[test]
    fn a_colour_change_under_a_running_animation_of_it_starts_no_transition() {
        let mut page = Page::new();
        page.keyframes
            .define(
                "wash",
                r#"{ "from": { "backgroundColor": "red" }, "to": { "backgroundColor": "red" } }"#,
            )
            .unwrap();
        let style = |color: &str| {
            format!(
                r#"{{ "backgroundColor": "{color}", "transition": "background-color 1s linear", "animation": "wash 10s linear" }}"#
            )
        };
        page.style(3, &style("lime"));
        page.frame(0);
        page.style(3, &style("blue"));
        for millis in [0, 500, 5000] {
            let frame = page.frame(millis);
            is(&page.shown(&frame, 3).background_color, "red");
        }
        assert!(page.transitions.take_events().is_empty());
        let ended = page.frame(10_500);
        is(&page.shown(&ended, 3).background_color, "blue");
        assert!(page.transitions.take_events().is_empty());
    }

    #[test]
    fn a_legacy_colour_blends_in_srgb_as_it_does_on_an_element() {
        let mut page = Page::new();
        page.style(3, r#"{ "backgroundColor": "red", "transition": "background-color 1s linear" }"#);
        page.frame(0);
        page.style(3, r#"{ "backgroundColor": "blue", "transition": "background-color 1s linear" }"#);
        page.frame(0);
        let middle = page.frame(500);
        let [red, green, blue] = rgb(&page.shown(&middle, 3).background_color);
        assert!((red - 0.5).abs() < 0.01 && (blue - 0.5).abs() < 0.01 && green < 0.01, "{red} {green} {blue}");
    }

    #[test]
    fn what_an_inline_box_shows_moves_with_a_transition_and_the_rest_does_not() {
        let mut page = Page::new();
        let style = |color: &str, padding: u32, width: u32| {
            format!(
                r#"{{ "color": "{color}", "paddingLeft": {padding}, "width": {width}, "transition": "all 1s linear" }}"#
            )
        };
        page.style(3, &style("red", 0, 0));
        page.frame(0);
        page.style(3, &style("blue", 40, 100));
        page.frame(0);
        let middle = page.frame(500);
        let shown = page.shown(&middle, 3);
        assert_eq!(shown.padding_left, crate::css::length::LinearLength::px(20.0).into());
        assert_eq!(shown.width, Some(crate::style::DimensionValue::Pixels(100.0)), "a span has no width to move");
        mid_red_to_blue(&shown.color);
        let events: Vec<TransitionEvent> = page.transitions.take_events();
        let names: std::collections::BTreeSet<_> = events.iter().map(|event| event.property_name()).collect();
        assert!(names.contains("color") && names.contains("padding-left"), "{names:?}");
        assert!(!names.contains("width"), "{names:?}");
    }

    #[test]
    fn an_inner_box_follows_the_colour_of_a_transitioning_outer_one() {
        let mut page = Page::new();
        let outer = |color: &str| format!(r#"{{ "color": "{color}", "transition": "color 1s linear" }}"#);
        page.style(3, &outer("red"));
        // A weight of its own keeps the runs apart: the colour is all it inherits.
        page.style(5, r#"{ "fontWeight": "bold" }"#);
        page.frame(0);
        page.style(3, &outer("blue"));
        page.frame(0);
        let middle = page.frame(500);
        let runs = page.runs(&middle);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].color, runs[1].color);
        let color = runs[1].color.expect("the inner run is coloured");
        let outer_color = page.shown(&middle, 3).color;
        let shown = gpui::Rgba::from(color);
        let [red, _, blue] = rgb(&outer_color);
        assert!((shown.r - red).abs() < 0.01 && (shown.b - blue).abs() < 0.01);
        mid_red_to_blue(&outer_color);
    }

    #[test]
    fn the_painted_background_border_and_outline_are_the_sampled_colours() {
        use crate::text::inline_decoration::of;
        use crate::text::inline_insets::InsetSpec;

        let mut page = Page::new();
        let style = |colour: &str| {
            format!(
                r#"{{ "backgroundColor": "{colour}", "borderWidth": 2, "borderStyle": "solid", "borderColor": "{colour}",
                    "outlineWidth": 1, "outlineStyle": "solid", "outlineColor": "{colour}",
                    "transition": "background-color 1s linear, border-color 1s linear, outline-color 1s linear" }}"#
            )
        };
        page.style(3, &style("red"));
        page.frame(0);
        page.style(3, &style("blue"));
        page.frame(0);
        let styles = page.frame(500);

        let paragraph = page.paragraph();
        let lengths = LengthContext::initial(800.0, 600.0);
        let states = |_: u64, _: &StyleDesc| PointerStates::default();
        let insets = InsetSpec::of(&page.tree, &paragraph, &crate::color::ColorContext::root(crate::color::Scheme::Light), &lengths, &states);
        let decorations = of(
            &page.tree,
            &paragraph,
            &ColorContext::root(Scheme::Light),
            &lengths,
            &states,
            &styles,
            &insets,
        );
        let decoration = &decorations[0];
        let between = |color: gpui::Hsla| {
            let color = gpui::Rgba::from(color);
            assert!(color.r > 0.05 && color.r < 0.95 && color.b > 0.05 && color.b < 0.95, "{color:?}");
        };
        between(decoration.paint.background.expect("a background"));
        between(decoration.paint.colors.top);
        between(decoration.paint.colors.left);
        between(decoration.outline.expect("an outline").color);
    }

    const WASH: &str = r#"{ "type": "tween", "duration": 1, "ease": "linear" }"#;

    fn description(initial: &str, animate: &str) -> String {
        format!(r#"{{ "generation": 1, "initial": {initial}, "animate": {animate}, "transition": {WASH} }}"#)
    }

    #[test]
    fn motion_moves_the_background_of_a_span_from_initial_to_animate() {
        let mut page = Page::new();
        page.style(3, r#"{ "backgroundColor": "lime" }"#);
        page.describe(3, &description(r#"{ "backgroundColor": "red" }"#, r#"{ "backgroundColor": "blue" }"#))
            .expect("a description a span can run");

        // The first frame shows `initial`, not `animate` and not the declared lime.
        let start = page.frame(0);
        is(&page.shown(&start, 3).background_color, "red");
        assert!(page.motion_frame(3, 0).active);

        let middle = page.frame(500);
        mid_red_to_blue(&page.shown(&middle, 3).background_color);
        assert!(page.motion_frame(3, 500).active);

        let end = page.frame(1000);
        is(&page.shown(&end, 3).background_color, "blue");
        let done = page.motion_frame(3, 1000);
        assert!(!done.active && done.just_settled);
        assert_eq!(done.generation, 1);
    }

    #[test]
    fn the_paragraph_paints_the_colour_motion_sampled() {
        let mut page = Page::new();
        page.style(3, r#"{ "color": "lime" }"#);
        page.describe(3, &description(r#"{ "color": "red" }"#, r#"{ "color": "blue" }"#))
            .expect("a description a span can run");
        let first = page.frame(0);
        let runs = page.runs(&first);
        let shown = gpui::Rgba::from(runs[0].color.expect("the run is coloured"));
        assert!(shown.r > 0.99 && shown.b < 0.01, "{shown:?}");
        let middle = page.frame(500);
        let runs = page.runs(&middle);
        let shown = gpui::Rgba::from(runs[0].color.expect("the run is coloured"));
        assert!(shown.r > 0.05 && shown.r < 0.95 && shown.b > 0.05 && shown.b < 0.95, "{shown:?}");
    }

    #[test]
    fn without_initial_a_span_animates_from_its_own_style() {
        let mut page = Page::new();
        page.style(3, r#"{ "backgroundColor": "red" }"#);
        page.describe(
            3,
            &format!(r#"{{ "generation": 1, "animate": {{ "backgroundColor": "blue" }}, "transition": {WASH} }}"#),
        )
        .expect("a description a span can run");
        let start = page.frame(0);
        is(&page.shown(&start, 3).background_color, "red");
        let middle = page.frame(500);
        mid_red_to_blue(&page.shown(&middle, 3).background_color);
    }

    #[test]
    fn initial_false_shows_a_spans_animate_at_once() {
        let mut page = Page::new();
        page.style(3, r#"{ "backgroundColor": "red" }"#);
        page.describe(
            3,
            &format!(
                r#"{{ "generation": 1, "initial": false, "animate": {{ "backgroundColor": "blue" }}, "transition": {WASH} }}"#
            ),
        )
        .expect("a description a span can run");
        let start = page.frame(0);
        is(&page.shown(&start, 3).background_color, "blue");
        assert!(!page.motion_frame(3, 0).active);
    }

    #[test]
    fn a_new_target_moves_a_span_from_where_it_is() {
        let mut page = Page::new();
        page.style(3, r#"{ "backgroundColor": "red" }"#);
        page.describe(3, &description(r#"{ "backgroundColor": "red" }"#, r#"{ "backgroundColor": "blue" }"#))
            .unwrap();
        page.frame(0);
        let middle = page.frame(500);
        mid_red_to_blue(&page.shown(&middle, 3).background_color);
        page.describe_at(
            3,
            &format!(
                r#"{{ "generation": 2, "initial": {{ "backgroundColor": "red" }},
                    "animate": {{ "backgroundColor": "lime" }}, "transition": {WASH} }}"#
            ),
            500,
        )
        .unwrap();
        let retargeted = page.frame(500);
        mid_red_to_blue(&page.shown(&retargeted, 3).background_color);
        let end = page.frame(1500);
        is(&page.shown(&end, 3).background_color, "lime");
        let done = page.motion_frame(3, 1500);
        assert!(done.just_settled);
        assert_eq!(done.generation, 2);
    }

    #[test]
    fn an_exit_target_settles_with_its_generation() {
        let mut page = Page::new();
        page.style(3, r#"{ "color": "blue" }"#);
        page.describe(3, &description(r#"false"#, r#"{ "color": "blue" }"#)).unwrap();
        page.frame(0);
        page.motion_frame(3, 0);
        page.describe_at(
            3,
            &format!(
                r#"{{ "generation": 2, "isExit": true, "initial": false,
                    "animate": {{ "color": "red" }}, "transition": {WASH} }}"#
            ),
            0,
        )
        .unwrap();
        let middle = page.frame(500);
        mid_red_to_blue(&page.shown(&middle, 3).color);
        let leaving = page.motion_frame(3, 500);
        assert!(leaving.active && !leaving.just_settled, "the span stays while it leaves");
        let gone = page.frame(1000);
        is(&page.shown(&gone, 3).color, "red");
        let done = page.motion_frame(3, 1000);
        assert!(done.just_settled);
        assert_eq!(done.generation, 2);
    }

    #[test]
    fn a_property_a_span_cannot_move_is_an_error_naming_it() {
        let mut page = Page::new();
        page.style(3, r#"{ "color": "red" }"#);
        let animate = page
            .describe(3, &description("false", r#"{ "color": "red", "height": 12 }"#))
            .expect_err("a height does not move on a span");
        assert!(animate.contains("animate") && animate.contains("height") && animate.contains("span"), "{animate}");
        let initial = page
            .describe(3, &description(r#"{ "flexGrow": 1 }"#, r#"{ "color": "red" }"#))
            .expect_err("a flex does not move on a span");
        assert!(initial.contains("initial") && initial.contains("flexGrow"), "{initial}");
        assert!(page.motion.is_empty());

        page.describe(3, &description("false", r#"{ "color": "red" }"#)).unwrap();
        let retarget = page
            .describe(3, &description("false", r#"{ "width": 12 }"#))
            .expect_err("a retarget is checked too");
        assert!(retarget.contains("width"), "{retarget}");
        let shown = page.frame(0);
        is(&page.shown(&shown, 3).color, "red");
    }

    #[test]
    fn motion_beats_a_transition_of_the_same_colour() {
        let mut page = Page::new();
        let style = |color: &str| {
            format!(r#"{{ "backgroundColor": "{color}", "transition": "background-color 10s linear" }}"#)
        };
        page.style(3, &style("red"));
        page.describe(3, &description("false", r#"{ "backgroundColor": "blue" }"#)).unwrap();
        page.frame(0);
        page.style(3, &style("lime"));
        let styles = page.frame(1000);
        is(&page.shown(&styles, 3).background_color, "blue");
        assert!(page.transitions.take_events().is_empty());
    }

    #[test]
    fn a_keyframe_animation_overrides_motion() {
        let mut page = Page::new();
        page.keyframes
            .define("wash", r#"{ "from": { "color": "lime" }, "to": { "color": "lime" } }"#)
            .unwrap();
        page.style(3, r#"{ "color": "red", "animation": "wash 10s linear" }"#);
        page.describe(3, &description("false", r#"{ "color": "blue" }"#)).unwrap();
        let styles = page.frame(0);
        is(&page.shown(&styles, 3).color, "lime");
    }

    #[test]
    fn an_inner_box_follows_the_colour_motion_gives_the_outer_one() {
        let mut page = Page::new();
        page.style(3, r#"{ "color": "lime" }"#);
        page.style(5, r#"{ "fontWeight": "bold" }"#);
        page.describe(3, &description(r#"{ "color": "red" }"#, r#"{ "color": "blue" }"#)).unwrap();
        let middle = page.frame(500);
        let runs = page.runs(&middle);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].color, runs[1].color);
        mid_red_to_blue(&page.shown(&middle, 3).color);
    }

    #[test]
    fn a_span_that_declares_no_style_is_painted_with_the_colour_motion_gives_it() {
        use crate::text::inline_decoration::of;
        use crate::text::inline_insets::InsetSpec;

        let mut page = Page::new();
        page.describe(
            3,
            &description(r#"{ "color": "red", "backgroundColor": "red" }"#, r#"{ "color": "blue", "backgroundColor": "blue" }"#),
        )
        .unwrap();
        assert!(page.tree.elements[&3].style.is_none());
        let end = page.frame(1000);

        let runs = page.runs(&end);
        let shown = gpui::Rgba::from(runs[0].color.expect("the run is coloured"));
        assert!(shown.b > 0.99 && shown.r < 0.01, "{shown:?}");

        let paragraph = page.paragraph();
        let lengths = LengthContext::initial(800.0, 600.0);
        let states = |_: u64, _: &StyleDesc| PointerStates::default();
        let colors = ColorContext::root(Scheme::Light);
        let insets = InsetSpec::of(&page.tree, &paragraph, &colors, &lengths, &states);
        let decorations = of(&page.tree, &paragraph, &colors, &lengths, &states, &end, &insets);
        let background = gpui::Rgba::from(decorations[0].paint.background.expect("a background"));
        assert!(background.b > 0.99 && background.r < 0.01, "{background:?}");
    }

    #[test]
    fn opacity_moves_on_a_span_and_the_paragraph_fades_the_group_it_is() {
        let mut page = Page::new();
        page.style(3, r#"{ "backgroundColor": "red", "color": "blue" }"#);
        page.describe(3, &description(r#"{ "opacity": 0 }"#, r#"{ "opacity": 1 }"#))
            .expect("opacity is a property a span can animate");

        let start = page.frame(0);
        assert_eq!(page.shown(&start, 3).opacity, Some(0.0));
        let opacity = |page: &Page, styles: &InlineStyles| {
            page.groups(styles).first().map(|group| group.opacity)
        };
        assert_eq!(opacity(&page, &start), Some(0.0));

        let middle = page.frame(500);
        let halfway = opacity(&page, &middle).expect("a group");
        assert!(halfway > 0.4 && halfway < 0.6, "{halfway}");
        let end = page.frame(1000);
        assert_eq!(page.shown(&end, 3).opacity, Some(1.0));
        assert_eq!(opacity(&page, &end), None, "a box at full opacity is no group");
        assert!(page.motion_frame(3, 1000).just_settled);
    }

    #[test]
    fn a_css_transition_of_the_opacity_of_a_span_runs() {
        let mut page = Page::new();
        let style = |opacity: f64| format!(r#"{{ "opacity": {opacity}, "transition": "opacity 1s linear" }}"#);
        page.style(3, &style(0.0));
        page.frame(0);
        page.style(3, &style(1.0));
        page.frame(0);
        let middle = page.frame(500);
        let opacity = page.shown(&middle, 3).opacity.expect("an opacity");
        assert!(opacity > 0.4 && opacity < 0.6, "{opacity}");
    }

    #[test]
    fn the_state_of_a_removed_box_is_dropped() {
        let mut page = Page::new();
        page.style(3, r#"{ "backgroundColor": "red", "transition": "background-color 1s linear" }"#);
        page.frame(0);
        page.style(3, r#"{ "backgroundColor": "blue", "transition": "background-color 1s linear" }"#);
        page.frame(0);
        page.frame(500);
        let tree = &page.tree;
        page.transitions.retain(|id| id != 3 && tree.elements.contains_key(&id));
        assert!(!page.transitions.wants(3, None), "nothing is left to settle");
    }

    fn placed(styles: &InlineStyles, page: &Page) -> Rc<crate::text::inline_insets::Insets> {
        let lengths = LengthContext::initial(800.0, 600.0);
        let insets = InsetSpec::of_moving(
            &page.tree,
            &page.paragraph(),
            &ColorContext::root(Scheme::Light),
            &lengths,
            &|_, _| PointerStates::default(),
            styles,
            false,
        );
        insets.at(None)
    }

    #[test]
    fn the_padding_and_font_of_a_span_move_and_the_paragraph_reads_them_as_the_frame_has_them() {
        let mut page = Page::new();
        page.style(3, r#"{ "paddingLeft": 0, "fontSize": 10, "borderLeftWidth": 0, "borderLeftStyle": "solid" }"#);
        page.describe(
            3,
            &description(
                r#"{ "paddingLeft": 0, "fontSize": 10, "borderLeftWidth": 0 }"#,
                r#"{ "paddingLeft": 20, "fontSize": 30, "borderLeftWidth": 4 }"#,
            ),
        )
        .expect("an inline box can move its padding, font and border");
        let middle = page.frame(500);
        let style = page.shown(&middle, 3);
        assert_eq!(style.padding_left, Some(LinearLength::px(10.0)));
        assert_eq!(style.font_size, Some(LinearLength::px(20.0)));
        assert_eq!(style.border_left_width, Some(LinearLength::px(2.0)));
        let inset = placed(&middle, &page).text();
        assert_eq!(inset.len(), 1);
        assert_eq!(f32::from(inset[0].leading), 12.0, "padding and border push the text");
        let end = page.frame(1000);
        assert_eq!(f32::from(placed(&end, &page).text()[0].leading), 24.0);
    }

    fn bind(page: &mut Page, id: u64, json: &str) -> serde_json::Value {
        let binding: serde_json::Value = serde_json::from_str(json).expect("a binding");
        page.tree.set_custom_prop(id, "motionValues".to_string(), binding.clone());
        binding
    }

    #[test]
    fn a_motion_value_in_the_style_of_a_span_moves_what_a_span_can_move() {
        let mut page = Page::new();
        page.style(3, r#"{ "backgroundColor": "red" }"#);
        page.values.create_source(1.0, &serde_json::json!(0.5)).expect("a value");
        page.values.create_source(2.0, &serde_json::json!(16)).expect("a value");
        page.values.create_source(3.0, &serde_json::json!(300)).expect("a value");
        let binding = bind(&mut page, 3, r#"{ "opacity": 1, "paddingLeft": 2, "width": 3 }"#);
        page.values.frame([&binding], &[], page.origin);
        let styles = page.frame(0);
        let style = page.shown(&styles, 3);
        assert_eq!(style.opacity, Some(0.5));
        assert_eq!(style.padding_left, Some(LinearLength::px(16.0)));
        assert_eq!(style.width, None, "a width is nothing a span has");
        assert_eq!(f32::from(placed(&styles, &page).text()[0].leading), 16.0);
    }

    #[test]
    fn a_span_with_only_a_binding_has_a_style_of_its_own_for_the_frame() {
        let mut page = Page::new();
        page.values.create_source(1.0, &serde_json::json!(0.25)).expect("a value");
        let binding = bind(&mut page, 3, r#"{ "opacity": 1 }"#);
        page.values.frame([&binding], &[], page.origin);
        let styles = page.frame(0);
        assert_eq!(styles.declared_by(&page.tree, 3).and_then(|style| style.opacity), Some(0.25));
    }

    fn hovered() -> crate::motion::Gestures {
        crate::motion::Gestures { hover: true, ..Default::default() }
    }

    #[test]
    fn a_span_goes_to_its_hover_target_and_back_to_its_own_style() {
        let mut page = Page::new();
        page.style(3, r##"{ "color": "#ff0000", "paddingLeft": 4 }"##);
        page.describe(
            3,
            r##"{ "animate": {}, "whileHover": { "color": "#0000ff", "paddingLeft": 24 },
                 "transition": { "duration": 1, "ease": "linear" } }"##,
        )
        .expect("a hover target");
        let rest = page.frame(0);
        assert_eq!(page.shown(&rest, 3).padding_left, Some(LinearLength::px(4.0)), "no gesture, no track");

        page.gesture(3, hovered(), 0);
        let middle = page.frame(500);
        assert_eq!(page.shown(&middle, 3).padding_left, Some(LinearLength::px(14.0)));
        mid_red_to_blue(&page.shown(&middle, 3).color);
        let hovered_end = page.frame(1000);
        assert_eq!(page.shown(&hovered_end, 3).padding_left, Some(LinearLength::px(24.0)));

        page.gesture(3, Default::default(), 1000);
        let back = page.frame(1500);
        assert_eq!(page.shown(&back, 3).padding_left, Some(LinearLength::px(14.0)), "it goes back to the 4px of its style");
        let rested = page.frame(2000);
        assert_eq!(page.shown(&rested, 3).padding_left, Some(LinearLength::px(4.0)));
        assert_eq!(rgb(&page.shown(&rested, 3).color), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_tap_beats_a_hover_and_a_hover_beats_a_focus() {
        let mut page = Page::new();
        page.style(3, r#"{ "paddingLeft": 0 }"#);
        page.describe(
            3,
            r#"{ "animate": {}, "whileHover": { "paddingLeft": 10 }, "whileTap": { "paddingLeft": 20 },
                 "whileFocus": { "paddingLeft": 30 }, "transition": { "duration": 0 } }"#,
        )
        .expect("targets");
        let at = |page: &mut Page, gestures, millis| {
            page.gesture(3, gestures, millis);
            let styles = page.frame(millis + 10);
            page.shown(&styles, 3).padding_left
        };
        // Framer's order, lowest first: animate, whileInView, whileFocus, whileHover, whileTap.
        let focus = crate::motion::Gestures { focus: true, ..Default::default() };
        let hover = crate::motion::Gestures { hover: true, ..focus };
        let all = crate::motion::Gestures { tap: true, ..hover };
        assert_eq!(at(&mut page, focus, 0), Some(LinearLength::px(30.0)));
        assert_eq!(at(&mut page, hover, 20), Some(LinearLength::px(10.0)));
        assert_eq!(at(&mut page, all, 40), Some(LinearLength::px(20.0)));
        assert_eq!(at(&mut page, hover, 60), Some(LinearLength::px(10.0)));
    }

    #[test]
    fn a_while_target_with_a_transition_of_its_own_animates_with_it() {
        let mut page = Page::new();
        page.style(3, r#"{ "paddingLeft": 0 }"#);
        page.describe(
            3,
            r#"{ "animate": {}, "whileHover": { "paddingLeft": 10, "transition": { "duration": 4, "ease": "linear" } },
                 "transition": { "duration": 1, "ease": "linear" } }"#,
        )
        .expect("a target");
        page.gesture(3, hovered(), 0);
        let styles = page.frame(1000);
        assert_eq!(page.shown(&styles, 3).padding_left, Some(LinearLength::px(2.5)), "a quarter of the way in 4s");
        page.gesture(3, Default::default(), 1000);
        let styles = page.frame(1500);
        assert!(
            page.shown(&styles, 3).padding_left.is_some_and(|left| left.px < 2.5),
            "it goes back with the transition of the element"
        );
    }

    #[test]
    fn a_while_target_takes_the_same_properties_as_animate() {
        let mut page = Page::new();
        page.style(3, "{}");
        let error = page
            .describe(3, r#"{ "animate": {}, "whileHover": { "width": 10 } }"#)
            .expect_err("a span has no width");
        assert!(error.contains("whileHover") && error.contains("width"), "{error}");
    }

    #[test]
    fn in_view_is_a_gesture_that_the_description_turns_on() {
        let mut page = Page::new();
        page.style(3, r#"{ "paddingLeft": 2 }"#);
        let describe = |in_view: bool| {
            format!(
                r#"{{ "animate": {{}}, "whileInView": {{ "paddingLeft": 12 }}, "inView": {in_view},
                     "transition": {{ "duration": 0 }} }}"#
            )
        };
        page.describe(3, &describe(false)).expect("a target");
        let out = page.frame(0);
        assert_eq!(page.shown(&out, 3).padding_left, Some(LinearLength::px(2.0)));
        page.describe_at(3, &describe(true), 10).expect("a target");
        let seen = page.frame(20);
        assert_eq!(page.shown(&seen, 3).padding_left, Some(LinearLength::px(12.0)));
        page.describe_at(3, &describe(false), 30).expect("a target");
        let gone = page.frame(40);
        assert_eq!(page.shown(&gone, 3).padding_left, Some(LinearLength::px(2.0)));
    }
}
