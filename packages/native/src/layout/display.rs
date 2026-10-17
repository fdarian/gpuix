//! The `display` values that name an inner display type, and the inline-level
//! ones that CSS Display 3 blockifies. gpui has one box type per inner display
//! type, so the inline-level spelling of a value only matters to the inline
//! planner ([`crate::text::inline`]): everywhere else `inline-flex` is `flex`.

/// `inline-block`, `inline-flex`, `inline-grid` or `inline-table`: a box that sits on a line of
/// text as one unit (an atomic inline).
pub(crate) fn is_atomic_inline(display: Option<&str>) -> bool {
    matches!(display, Some("inline-block" | "inline-flex" | "inline-grid" | "inline-table"))
}

/// The inner display type, which is what gpui lays the box out as. A
/// flex or grid item or an absolutely positioned box has its `display`
/// blockified, and for these values that is the same mapping.
pub(crate) fn inner(display: Option<&str>) -> Option<&str> {
    match display {
        Some("inline-block") => Some("block"),
        Some("inline-flex") => Some("flex"),
        Some("inline-grid") => Some("grid"),
        Some("inline-table") => Some("table"),
        display => display,
    }
}

/// Whether the box lays its children out as flex or grid items.
pub(crate) fn lays_out_items(display: Option<&str>) -> bool {
    matches!(inner(display), Some("flex" | "grid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_level_values_blockify_to_their_inner_type() {
        assert_eq!(inner(Some("inline-block")), Some("block"));
        assert_eq!(inner(Some("inline-flex")), Some("flex"));
        assert_eq!(inner(Some("inline-grid")), Some("grid"));
        assert_eq!(inner(Some("inline-table")), Some("table"));
        assert_eq!(inner(Some("flex")), Some("flex"));
        assert_eq!(inner(None), None);
    }

    #[test]
    fn only_the_four_inline_level_values_are_atomic() {
        for display in ["inline-block", "inline-flex", "inline-grid", "inline-table"] {
            assert!(is_atomic_inline(Some(display)), "{display}");
        }
        for display in ["inline", "block", "flex", "grid", "table", "table-cell", "contents", "none"] {
            assert!(!is_atomic_inline(Some(display)), "{display}");
        }
        assert!(!is_atomic_inline(None));
    }

    #[test]
    fn a_flex_or_grid_container_lays_out_items_whatever_its_level() {
        for display in ["flex", "grid", "inline-flex", "inline-grid"] {
            assert!(lays_out_items(Some(display)), "{display}");
        }
        for display in ["block", "inline-block", "inline"] {
            assert!(!lays_out_items(Some(display)), "{display}");
        }
    }
}
