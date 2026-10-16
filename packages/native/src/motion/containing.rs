//! Whether a percentage `height` has anything to resolve against.
//!
//! In CSS a percentage height whose containing block has no definite height
//! behaves as `auto`. Taffy does the same at layout, but motion resolves a
//! percentage itself (a px to `%` transition runs in px), and the parent's
//! painted height is no answer for an auto parent: it is made of the child.
//! The build walks the tree top down, so each element hands its children what
//! it is as a containing block.
//!
//! The rule follows CSS Sizing 3 "definite" for the layouts gpuix builds from a
//! style alone: a length, a percentage that itself resolves, an absolute box
//! with both `top` and `bottom`, and a flex item the container sizes. A grid
//! item is never taken as definite, though a grid with fixed rows sizes it.

use super::property::Axis;
use crate::style::{DimensionValue, StyleDesc};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Layout {
    Other,
    Flex {
        column: bool,
        wrap: bool,
        /// `align-items` is `stretch` (or unset, which is `normal`).
        stretch: bool,
    },
}

impl Layout {
    fn of(style: &StyleDesc) -> Self {
        if style.display.as_deref() != Some("flex") {
            return Self::Other;
        }
        Self::Flex {
            column: matches!(
                style.flex_direction.as_deref(),
                Some("column" | "column-reverse")
            ),
            wrap: matches!(style.flex_wrap.as_deref(), Some("wrap" | "wrap-reverse")),
            stretch: matches!(
                style.align_items.as_deref(),
                None | Some("stretch" | "normal")
            ),
        }
    }
}

/// What an element is as the containing block of its children, as far as its
/// style and its own container tell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Container {
    height_definite: bool,
    layout: Layout,
}

impl Container {
    /// The window, which the elements at the top of a tree sit in: its height
    /// is the viewport's.
    pub(crate) const WINDOW: Self = Self {
        height_definite: true,
        layout: Layout::Other,
    };

    /// Whether a percentage `height` on an element of `style` in this container
    /// resolves. An absolute box has the padding box of its containing block to
    /// resolve against, which always has a height.
    pub(crate) fn resolves_percent_height(self, style: Option<&StyleDesc>) -> bool {
        self.height_definite || style.is_some_and(is_out_of_flow)
    }

    /// The axis a `flexBasis` percentage of a child is a share of: the main axis
    /// of a flex container. A child of anything else has no flex basis in effect, and
    /// its percentage is taken of the width.
    pub(crate) fn flex_main(self) -> Axis {
        match self.layout {
            Layout::Flex { column: true, .. } => Axis::Height,
            Layout::Flex { .. } | Layout::Other => Axis::Width,
        }
    }

    /// The container an element of `style` makes for its children.
    pub(crate) fn within(self, style: &StyleDesc) -> Self {
        Self {
            height_definite: self.gives_definite_height(style),
            layout: Layout::of(style),
        }
    }

    fn gives_definite_height(self, style: &StyleDesc) -> bool {
        match &style.height {
            Some(DimensionValue::Pixels(_)) => true,
            Some(DimensionValue::Percentage(_)) => self.resolves_percent_height(Some(style)),
            _ if is_out_of_flow(style) => has_both_vertical_insets(style),
            _ => self.sizes_item_height(style),
        }
    }

    /// Whether the flex container hands an item with an auto height a definite
    /// one: the post-flexing main size of a column, the stretched cross size of
    /// a single-line row.
    fn sizes_item_height(self, style: &StyleDesc) -> bool {
        if !self.height_definite {
            return false;
        }
        match self.layout {
            Layout::Other => false,
            Layout::Flex { column: true, .. } => true,
            Layout::Flex { wrap: true, .. } => false,
            Layout::Flex { stretch, .. } => match style.align_self.as_deref() {
                None | Some("auto" | "normal") => stretch,
                Some("stretch") => true,
                Some(_) => false,
            },
        }
    }
}

fn is_out_of_flow(style: &StyleDesc) -> bool {
    matches!(style.position.as_deref(), Some("absolute" | "fixed"))
}

fn has_both_vertical_insets(style: &StyleDesc) -> bool {
    let set = |inset: &Option<DimensionValue>| {
        matches!(
            inset,
            Some(DimensionValue::Pixels(_) | DimensionValue::Percentage(_))
        )
    };
    set(&style.top) && set(&style.bottom)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    /// The container the chain of `styles` makes, outermost first, below the window.
    fn below(styles: &[&str]) -> Container {
        styles
            .iter()
            .fold(Container::WINDOW, |container, json| container.within(&style(json)))
    }

    fn resolves(container: Container, child: &str) -> bool {
        container.resolves_percent_height(Some(&style(child)))
    }

    #[test]
    fn a_root_resolves_against_the_window() {
        assert!(Container::WINDOW.resolves_percent_height(None));
        assert!(resolves(Container::WINDOW, r#"{ "height": "50%" }"#));
    }

    #[test]
    fn an_auto_height_parent_gives_nothing_to_resolve() {
        assert!(!resolves(below(&["{}"]), r#"{ "height": "50%" }"#));
        assert!(!resolves(below(&[r#"{ "display": "block" }"#]), "{}"));
    }

    #[test]
    fn a_parent_with_a_height_in_px_resolves() {
        assert!(resolves(below(&[r#"{ "height": 200 }"#]), "{}"));
    }

    #[test]
    fn a_percentage_parent_resolves_when_its_own_container_does() {
        let tall = r#"{ "height": "100%" }"#;
        assert!(resolves(below(&[tall, tall]), "{}"));
        assert!(!resolves(below(&["{}", tall]), "{}"));
    }

    #[test]
    fn an_absolute_box_resolves_against_its_containing_block_whatever_the_parent_is() {
        let auto_parent = below(&["{}"]);
        assert!(resolves(auto_parent, r#"{ "position": "absolute" }"#));
        assert!(resolves(auto_parent, r#"{ "position": "fixed" }"#));
    }

    #[test]
    fn an_absolute_parent_with_top_and_bottom_is_definite() {
        let both = r#"{ "position": "absolute", "top": 0, "bottom": "10%" }"#;
        assert!(resolves(below(&[both]), "{}"));
        let one = r#"{ "position": "absolute", "top": 0 }"#;
        assert!(!resolves(below(&[one]), "{}"));
    }

    #[test]
    fn a_column_flex_item_is_definite_in_a_definite_container() {
        let column = r#"{ "display": "flex", "flexDirection": "column", "height": 200 }"#;
        assert!(resolves(below(&[column, r#"{ "flexGrow": 1 }"#]), "{}"));
        assert!(resolves(below(&[column, "{}"]), "{}"));
        let auto_column = r#"{ "display": "flex", "flexDirection": "column" }"#;
        assert!(!resolves(below(&[auto_column, "{}"]), "{}"));
    }

    #[test]
    fn a_row_flex_item_is_definite_only_when_it_stretches() {
        let row = r#"{ "display": "flex", "height": 200 }"#;
        assert!(resolves(below(&[row, "{}"]), "{}"));
        assert!(resolves(below(&[row, r#"{ "alignSelf": "stretch" }"#]), "{}"));
        assert!(!resolves(below(&[row, r#"{ "alignSelf": "flex-start" }"#]), "{}"));
        let centred = r#"{ "display": "flex", "height": 200, "alignItems": "center" }"#;
        assert!(!resolves(below(&[centred, "{}"]), "{}"));
        assert!(resolves(below(&[centred, r#"{ "alignSelf": "stretch" }"#]), "{}"));
        let wrapping = r#"{ "display": "flex", "height": 200, "flexWrap": "wrap" }"#;
        assert!(!resolves(below(&[wrapping, "{}"]), "{}"));
    }

    #[test]
    fn a_block_child_of_a_definite_parent_is_not_definite() {
        assert!(!resolves(below(&[r#"{ "height": 200 }"#, "{}"]), "{}"));
    }

    #[test]
    fn the_flex_main_axis_is_the_direction_of_a_flex_container_and_the_width_of_anything_else() {
        let main = |container: &str| below(&[container]).flex_main();
        assert_eq!(main(r#"{ "display": "flex" }"#), Axis::Width);
        assert_eq!(main(r#"{ "display": "flex", "flexDirection": "row-reverse" }"#), Axis::Width);
        assert_eq!(main(r#"{ "display": "flex", "flexDirection": "column" }"#), Axis::Height);
        assert_eq!(main(r#"{ "display": "flex", "flexDirection": "column-reverse" }"#), Axis::Height);
        // `flex-direction` means nothing on a block.
        assert_eq!(main(r#"{ "flexDirection": "column" }"#), Axis::Width);
        assert_eq!(Container::WINDOW.flex_main(), Axis::Width);
    }
}
