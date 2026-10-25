//! The universal style layer: the declarations of a CSS `* { ... }` rule.
//!
//! One style sits under the own style of every element. It has the lowest
//! precedence, so an element's own declaration, and its state styles, win per
//! property. It is a declaration on each element, not an inherited value.
//!
//! Not a stylesheet: there is one layer and no selector, so there is no
//! specificity or cascade between rules to decide.

use std::sync::Arc;

use crate::events::state_styles::overlay;
use crate::style::StyleDesc;

/// `own` over `universal`. Shorthands and longhands interplay exactly as they do
/// for a `hover` style over its base (`overlay`): an own `padding` shadows a
/// universal `paddingTop`.
///
/// A state refinement present on both sides is merged the same way, the
/// element's winning per property, as `*:hover` and `div:hover` would be; one
/// present on a single side is used as it is.
pub fn layer_under(universal: &StyleDesc, own: &StyleDesc) -> StyleDesc {
    let mut layered = overlay(universal, own);
    layered.hover = refine(universal.hover.as_deref(), own.hover.as_deref());
    layered.active = refine(universal.active.as_deref(), own.active.as_deref());
    layered.focus_visible = refine(universal.focus_visible.as_deref(), own.focus_visible.as_deref());
    layered.focus_within = refine(universal.focus_within.as_deref(), own.focus_within.as_deref());
    layered.group_hover = refine(universal.group_hover.as_deref(), own.group_hover.as_deref());
    layered.group_active = refine(universal.group_active.as_deref(), own.group_active.as_deref());
    layered.group_focus_within = refine(
        universal.group_focus_within.as_deref(),
        own.group_focus_within.as_deref(),
    );
    layered.has_hover = refine(universal.has_hover.as_deref(), own.has_hover.as_deref());
    layered.has_active = refine(universal.has_active.as_deref(), own.has_active.as_deref());
    layered.has_focus = refine(universal.has_focus.as_deref(), own.has_focus.as_deref());
    layered.has_focus_visible = refine(
        universal.has_focus_visible.as_deref(),
        own.has_focus_visible.as_deref(),
    );
    layered.disabled = refine(universal.disabled.as_deref(), own.disabled.as_deref());
    layered.enabled = refine(universal.enabled.as_deref(), own.enabled.as_deref());
    layered.has_disabled = refine(universal.has_disabled.as_deref(), own.has_disabled.as_deref());
    layered.has_enabled = refine(universal.has_enabled.as_deref(), own.has_enabled.as_deref());
    // The universal blocks come first, so the element's own win when both match.
    layered.container_rules = crate::layout::container::ContainerRules::concat(
        universal.container_rules.as_ref(),
        own.container_rules.as_ref(),
    );
    layered
}

fn refine(universal: Option<&StyleDesc>, own: Option<&StyleDesc>) -> Option<Box<StyleDesc>> {
    match (universal, own) {
        (Some(universal), Some(own)) => Some(Box::new(overlay(universal, own))),
        (Some(only), None) | (None, Some(only)) => Some(Box::new(only.clone())),
        (None, None) => None,
    }
}

struct Layered {
    /// Held so the address the entry is keyed by cannot be reused by another style.
    _own: Arc<StyleDesc>,
    effective: Arc<StyleDesc>,
}

/// The layered style of each own style, so the thousands of elements that share
/// one own style (a 10k-turn chat has 90) share one layered style too.
///
/// Valid for one universal style: the tree clears it when the layer changes.
#[derive(Default)]
pub struct LayerCache {
    entries: rustc_hash::FxHashMap<usize, Layered>,
}

impl LayerCache {
    pub fn effective(&mut self, universal: &StyleDesc, own: &Arc<StyleDesc>) -> Arc<StyleDesc> {
        self.entries
            .entry(Arc::as_ptr(own) as usize)
            .or_insert_with(|| Layered {
                _own: own.clone(),
                effective: Arc::new(layer_under(universal, own)),
            })
            .effective
            .clone()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Forget the layered styles no element holds. An entry also holds its own
    /// style, which would stay out of `StyleTable::sweep`'s reach for as long.
    pub fn release_unused(&mut self) {
        self.entries
            .retain(|_, entry| Arc::strong_count(&entry.effective) > 1);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    fn holds(&self, own: &Arc<StyleDesc>) -> bool {
        self.entries
            .get(&(Arc::as_ptr(own) as usize))
            .is_some_and(|entry| Arc::ptr_eq(&entry._own, own))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn an_own_value_beats_the_universal_one_and_the_rest_comes_through() {
        let universal = style(r##"{ "color": "#111111", "opacity": 0.5, "cursor": "pointer" }"##);
        let own = style(r##"{ "color": "#222222", "width": 10 }"##);
        let layered = layer_under(&universal, &own);
        assert_eq!(layered.color.as_deref(), Some("#222222"));
        assert_eq!(layered.opacity, universal.opacity);
        assert_eq!(layered.cursor.as_deref(), Some("pointer"));
        assert_eq!(layered.width, own.width);
    }

    #[test]
    fn an_own_shorthand_shadows_the_universal_longhands_it_covers() {
        let universal = style(r#"{ "paddingTop": 4, "paddingLeft": 6, "transitionDuration": "1s" }"#);
        let own = style(r#"{ "padding": 10, "transition": "color 200ms" }"#);
        let layered = layer_under(&universal, &own);
        assert_eq!(layered.padding, own.padding);
        assert_eq!(layered.padding_top, None);
        assert_eq!(layered.padding_left, None);
        assert_eq!(layered.transition_duration, None);
    }

    #[test]
    fn an_own_longhand_leaves_the_universal_shorthand_for_the_other_sides() {
        let universal = style(r#"{ "padding": 10 }"#);
        let own = style(r#"{ "paddingTop": 4 }"#);
        let layered = layer_under(&universal, &own);
        assert_eq!(layered.padding, universal.padding);
        assert_eq!(layered.padding_top, own.padding_top);
    }

    #[test]
    fn an_invalid_own_colour_falls_through_to_the_universal_one() {
        let universal = style(r##"{ "backgroundColor": "#336699" }"##);
        let own = style(r#"{ "backgroundColor": "not-a-colour" }"#);
        assert_eq!(
            layer_under(&universal, &own).background_color.as_deref(),
            Some("#336699")
        );
    }

    #[test]
    fn a_refinement_on_both_sides_merges_with_the_element_winning() {
        let universal = style(
            r##"{ "hover": { "backgroundColor": "#aaaaaa", "opacity": 0.5 },
                  "focusVisible": { "outlineWidth": 2 } }"##,
        );
        let own = style(
            r##"{ "hover": { "backgroundColor": "#bbbbbb", "cursor": "pointer" },
                  "groupHover": { "color": "#cccccc" } }"##,
        );
        let layered = layer_under(&universal, &own);
        let hover = layered.hover.as_deref().unwrap();
        assert_eq!(hover.background_color.as_deref(), Some("#bbbbbb"));
        assert_eq!(hover.opacity, Some(0.5));
        assert_eq!(hover.cursor.as_deref(), Some("pointer"));
        assert_eq!(layered.focus_visible, universal.focus_visible);
        assert_eq!(layered.group_hover, own.group_hover);
        assert_eq!(layered.active, None);
    }

    #[test]
    fn the_relational_refinements_merge_like_the_others() {
        let universal = style(
            r##"{ "hasHover": { "backgroundColor": "#aaaaaa", "opacity": 0.5 },
                  "hasFocusVisible": { "outlineWidth": 2 } }"##,
        );
        let own = style(
            r##"{ "hasHover": { "backgroundColor": "#bbbbbb" },
                  "hasActive": { "color": "#cccccc" },
                  "hasFocus": { "color": "#dddddd" } }"##,
        );
        let layered = layer_under(&universal, &own);
        let hover = layered.has_hover.as_deref().unwrap();
        assert_eq!(hover.background_color.as_deref(), Some("#bbbbbb"));
        assert_eq!(hover.opacity, Some(0.5));
        assert_eq!(layered.has_focus_visible, universal.has_focus_visible);
        assert_eq!(layered.has_active, own.has_active);
        assert_eq!(layered.has_focus, own.has_focus);
    }

    #[test]
    fn the_control_refinements_merge_like_the_others() {
        let universal = style(
            r##"{ "disabled": { "opacity": 0.5, "color": "#aaaaaa" },
                  "hasEnabled": { "outlineWidth": 2 } }"##,
        );
        let own = style(
            r##"{ "disabled": { "color": "#bbbbbb" },
                  "enabled": { "color": "#cccccc" },
                  "hasDisabled": { "color": "#dddddd" } }"##,
        );
        let layered = layer_under(&universal, &own);
        let disabled = layered.disabled.as_deref().unwrap();
        assert_eq!((disabled.color.as_deref(), disabled.opacity), (Some("#bbbbbb"), Some(0.5)));
        assert_eq!(layered.has_enabled, universal.has_enabled);
        assert_eq!(layered.enabled, own.enabled);
        assert_eq!(layered.has_disabled, own.has_disabled);
    }

    #[test]
    fn container_blocks_layer_the_universal_ones_first_so_the_elements_own_win() {
        use crate::css::container_query::{Containers, Measured, Needs};

        struct Wide;
        impl Containers for Wide {
            fn container(&self, _: Option<&str>, _: Needs) -> Option<Measured> {
                Some(Measured {
                    width: Some(1000.0),
                    height: Some(1000.0),
                    lengths: crate::css::length::LengthContext::initial(1000.0, 1000.0),
                    custom: Default::default(),
                })
            }
        }
        let universal = style(
            r##"{ "@container": { "(min-width: 1px)": { "color": "#aaaaaa", "opacity": 0.5 },
                                  "(min-width: 9999px)": { "width": 7 } } }"##,
        );
        let own = style(r##"{ "@container": { "(min-width: 1px)": { "color": "#bbbbbb" } } }"##);
        let folded = crate::layout::container::fold(&layer_under(&universal, &own), &Wide).unwrap();
        assert_eq!(folded.color.as_deref(), Some("#bbbbbb"));
        assert_eq!(folded.opacity, Some(0.5), "what only the universal block sets still applies");
        assert_eq!(folded.width, None);
        // One side alone is used as it is.
        let only_universal = layer_under(&universal, &style("{}"));
        assert_eq!(only_universal.container_rules, universal.container_rules);
    }

    #[test]
    fn one_layered_style_serves_every_element_with_the_same_own_style() {
        let universal = style(r#"{ "opacity": 0.5 }"#);
        let own = Arc::new(style(r#"{ "width": 10 }"#));
        let mut cache = LayerCache::default();
        let first = cache.effective(&universal, &own);
        let second = cache.effective(&universal, &own);
        assert!(Arc::ptr_eq(&first, &second));
        let other = cache.effective(&universal, &Arc::new(style(r#"{ "width": 10 }"#)));
        assert!(!Arc::ptr_eq(&first, &other), "keyed by the allocation, not by content");
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn release_unused_keeps_what_an_element_holds() {
        let universal = style(r#"{ "opacity": 0.5 }"#);
        let held = Arc::new(style(r#"{ "width": 10 }"#));
        let dropped = Arc::new(style(r#"{ "width": 20 }"#));
        let mut cache = LayerCache::default();
        let _element_holds = cache.effective(&universal, &held);
        drop(cache.effective(&universal, &dropped));
        cache.release_unused();
        assert!(cache.holds(&held));
        assert!(!cache.holds(&dropped));
    }

    /// What the renderer does with an element's style each frame, minus the
    /// parts that do not decide a transition.
    struct Frames {
        transitions: crate::motion::CssTransitions,
        origin: web_time::Instant,
        window: crate::color::ColorContext,
    }

    impl Frames {
        fn new() -> Self {
            Self {
                transitions: Default::default(),
                origin: web_time::Instant::now(),
                window: crate::color::ColorContext::root(crate::color::Scheme::Light),
            }
        }

        fn dark(&mut self) {
            self.window = crate::color::ColorContext::root(crate::color::Scheme::Dark);
        }

        fn background(&mut self, tree: &crate::retained_tree::RetainedTree, id: u64, millis: u64) -> String {
            let element = &tree.elements[&id];
            let style = element.style.as_deref().expect("a styled element");
            let wanted = self.transitions.wants(id, Some(style));
            crate::motion::resolve_declared_style(
                wanted.then_some(&mut self.transitions),
                None,
                element.style_before_transition(),
                &self.window,
                &crate::motion::ParentValues::initial(),
                Default::default(),
                None,
                id,
                style,
                crate::motion::FocusVisible::default(),
                None,
                crate::motion::InterpolateSize::default(),
                crate::motion::Scope::Element,
                self.origin + std::time::Duration::from_millis(millis),
            )
            .style
            .background_color
            .expect("a background")
        }
    }

    fn rgba(text: &str) -> gpui::Rgba {
        crate::color::parse_color_rgba(text).unwrap()
    }

    fn apply(tree: &mut crate::retained_tree::RetainedTree, json: &str) {
        crate::renderer::apply_batch_to_tree(tree, json.as_bytes()).expect("valid batch");
    }

    fn halfway(text: &str) -> bool {
        let colour = rgba(text);
        colour.r > 0.2 && colour.b > 0.2
    }

    /// The theme cross-fade: one universal `transition`, elements that never
    /// declared one, and the window flipping its appearance.
    #[test]
    fn a_universal_transition_fades_elements_through_an_appearance_flip() {
        let mut tree = crate::retained_tree::RetainedTree::new();
        let themed = r##"{"colorScheme":"light dark","backgroundColor":"light-dark(#ff0000, #0000ff)"}"##;
        apply(
            &mut tree,
            &format!(
                r##"[["createElement",1,"div"],["createElement",2,"div"],["appendChild",1,2],
                    ["setStyle",1,{themed}],["setStyle",2,{themed}],
                    ["setUniversalStyle",{{"transition":"background-color 1s linear"}}],
                    ["createElement",3,"div"],["appendChild",1,3],["setStyle",3,{themed}]]"##
            ),
        );
        let mut frames = Frames::new();
        for id in [1, 2, 3] {
            frames.background(&tree, id, 0);
        }

        frames.dark();
        for id in [1, 2, 3] {
            assert!(
                rgba(&frames.background(&tree, id, 0)).r > 0.9,
                "element {id} starts from the light colour"
            );
            let middle = frames.background(&tree, id, 500);
            assert!(halfway(&middle), "element {id} is {middle} halfway");
            let end = frames.background(&tree, id, 1000);
            assert_eq!(end, "light-dark(#ff0000, #0000ff)", "element {id} has arrived");
        }
    }

    #[test]
    fn an_element_with_its_own_transition_does_not_fade_with_the_universal_one() {
        let mut tree = crate::retained_tree::RetainedTree::new();
        let themed = r##""colorScheme":"light dark","backgroundColor":"light-dark(#ff0000, #0000ff)""##;
        apply(
            &mut tree,
            &format!(
                r##"[["createElement",1,"div"],["createElement",2,"div"],
                    ["setStyle",1,{{{themed}}}],
                    ["setStyle",2,{{{themed},"transition":"opacity 1s"}}],
                    ["setUniversalStyle",{{"transition":"background-color 1s linear"}}]]"##
            ),
        );
        let mut frames = Frames::new();
        frames.background(&tree, 1, 0);
        frames.background(&tree, 2, 0);
        frames.dark();
        assert!(halfway(&{
            frames.background(&tree, 1, 0);
            frames.background(&tree, 1, 500)
        }));
        assert_eq!(
            frames.background(&tree, 2, 500),
            "light-dark(#ff0000, #0000ff)",
            "its own transition list replaces the universal one"
        );
    }

    /// The same-change rule: the layer that declares the transition also moves the colour.
    #[test]
    fn a_layer_that_declares_a_transition_and_moves_a_colour_together_runs_it() {
        let mut tree = crate::retained_tree::RetainedTree::new();
        apply(
            &mut tree,
            r##"[["createElement",1,"div"],["createElement",2,"div"],["setStyle",2,{"width":10}],
                ["setUniversalStyle",{"backgroundColor":"#ff0000"}]]"##,
        );
        let mut frames = Frames::new();
        assert_eq!(frames.background(&tree, 1, 0), "#ff0000");
        assert_eq!(frames.background(&tree, 2, 0), "#ff0000");

        apply(
            &mut tree,
            r##"[["setUniversalStyle",{"backgroundColor":"#0000ff","transition":"background-color 1s linear"}]]"##,
        );
        for id in [1, 2] {
            let start = rgba(&frames.background(&tree, id, 0));
            assert!(start.r > 0.99 && start.b < 0.01, "element {id} leaves red");
            assert!(halfway(&frames.background(&tree, id, 500)), "element {id}");
            assert_eq!(frames.background(&tree, id, 1000), "#0000ff");
        }
    }
}
