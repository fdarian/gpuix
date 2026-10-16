//! CSS transitions and animations of the colours of inline boxes.
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
//! of a span is not run, and starts no event.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use web_time::Instant;

use super::{
    resolve_declared_style, CssAnimations, CssTransitions, InterpolateSize, KeyframesRegistry,
    ParentFont, Scope,
};
use crate::color::ColorContext;
use crate::events::state_styles::{folded, PointerStates};
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;
use crate::text::inline::Paragraph;

static EMPTY_STYLE: std::sync::LazyLock<StyleDesc> = std::sync::LazyLock::new(StyleDesc::default);

/// The clock and the state a frame's inline boxes are resolved with.
pub(crate) struct Timeline<'a> {
    pub transitions: &'a mut CssTransitions,
    pub animations: &'a mut CssAnimations,
    pub keyframes: &'a KeyframesRegistry,
    pub now: Instant,
}

/// The style of each inline box of a paragraph that has a transition or an
/// animation, as it shows in this frame. A box that has none is not here.
#[derive(Default)]
pub(crate) struct InlineStyles(HashMap<u64, StyleDesc>);

impl InlineStyles {
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

    /// Steps the transitions and animations of the boxes of `paragraph` to
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
            let mut font = *font;
            for id in &piece.boxes {
                let Some(element) = tree.elements.get(id) else {
                    continue;
                };
                let style = element.style.as_deref();
                if stepped.insert(*id) {
                    let transitions = timeline.transitions.wants(*id, style);
                    let animations = timeline.animations.wants(*id, style);
                    if transitions || animations {
                        let style = style.unwrap_or(&EMPTY_STYLE);
                        let resolved = step(
                            *id,
                            element.style_before_transition(),
                            style,
                            states(*id, style),
                            (transitions, animations),
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

/// One box's style after its transitions and then its animations, which come
/// last as they do for an element.
#[allow(clippy::too_many_arguments)]
fn step(
    id: u64,
    before: Option<&StyleDesc>,
    style: &StyleDesc,
    pointer: PointerStates,
    (transitions_wanted, animations_wanted): (bool, bool),
    (colors, font): (&ColorContext, &ParentFont),
    transitions: &mut CssTransitions,
    animations: &mut CssAnimations,
    keyframes: &KeyframesRegistry,
    now: Instant,
) -> StyleDesc {
    let declared = resolve_declared_style(
        transitions_wanted.then_some(transitions),
        before,
        colors,
        font,
        pointer,
        None,
        id,
        style,
        false,
        None,
        InterpolateSize::default(),
        Scope::InlineBox,
        now,
    );
    let animated = animations_wanted
        .then(|| {
            animations.update(
                id,
                &declared.style,
                &declared.style,
                colors,
                font,
                keyframes,
                None,
                InterpolateSize::default(),
                Scope::InlineBox,
                now,
            )
        })
        .flatten();
    match animated {
        Some(values) => {
            let mut resolved = declared.style;
            values.apply_to(&mut resolved, declared.transitions.as_ref());
            resolved
        }
        None => declared.style,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
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
            }
        }

        fn style(&mut self, id: u64, json: &str) {
            let style = serde_json::from_str(json).expect("a style");
            self.tree.set_style(id, Arc::new(style));
        }

        fn paragraph(&self) -> crate::text::inline::Paragraph {
            match plan_children(&self.tree, &self.tree.elements[&1]).remove(0) {
                Child::Paragraph(paragraph) => paragraph,
                Child::Block(_) => panic!("a paragraph"),
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
                    transitions: &mut self.transitions,
                    animations: &mut self.animations,
                    keyframes: &self.keyframes,
                    now: self.origin + Duration::from_millis(millis),
                },
            )
        }

        fn shown(&self, styles: &InlineStyles, id: u64) -> StyleDesc {
            let base = self.tree.elements[&id].style.as_deref().expect("a styled box");
            styles.declared(id, base, PointerStates::default()).into_owned()
        }

        fn runs(&self, styles: &InlineStyles) -> Vec<RunStyle> {
            run_styles(
                &self.tree,
                &self.paragraph(),
                &ColorContext::root(Scheme::Light),
                &LengthContext::initial(800.0, 600.0),
                &FontFeatureState::default(),
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
                r#"{{ "borderWidth": 2, "borderColor": "{all}", "borderLeftColor": "{left}",
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
    fn only_colours_move_on_an_inline_box() {
        let mut page = Page::new();
        let style = |color: &str, padding: u32| {
            format!(r#"{{ "color": "{color}", "paddingLeft": {padding}, "transition": "all 1s linear" }}"#)
        };
        page.style(3, &style("red", 0));
        page.frame(0);
        page.style(3, &style("blue", 40));
        page.frame(0);
        let middle = page.frame(500);
        let shown = page.shown(&middle, 3);
        assert_eq!(shown.padding_left, crate::css::length::LinearLength::px(40.0).into());
        mid_red_to_blue(&shown.color);
        let events: Vec<TransitionEvent> = page.transitions.take_events();
        assert!(!events.is_empty());
        assert!(events.iter().all(|event| event.property_name() == "color"), "{} events", events.len());
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
                r#"{{ "backgroundColor": "{colour}", "borderWidth": 2, "borderColor": "{colour}",
                    "outlineWidth": 1, "outlineColor": "{colour}",
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
        let insets = InsetSpec::of(&page.tree, &paragraph, &lengths, &states);
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
}
