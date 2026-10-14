use crate::style::StyleDesc;

/// Where a child paints among its siblings, lowest first. Layout is not affected.
///
/// CSS paints negative `z-index` positioned boxes, then in-flow boxes, then
/// positioned boxes by `z-index` (`auto` is 0) in tree order. Only an author-set
/// `position` other than `static` counts as positioned. Every parent is treated as
/// its own stacking context, so a `z-index` never reorders boxes against cousins.
pub(crate) fn paint_rank(style: Option<&StyleDesc>) -> i32 {
    let Some(style) = style else {
        return 0;
    };
    if !matches!(
        style.position.as_deref(),
        Some("relative") | Some("absolute") | Some("fixed") | Some("sticky")
    ) {
        return 0;
    }
    let z_index = style
        .z_index
        .map_or(0, |z| z.round().clamp(-1_000_000.0, 1_000_000.0) as i32);
    if z_index < 0 {
        z_index
    } else {
        z_index + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(position: Option<&str>, z_index: Option<f64>) -> StyleDesc {
        StyleDesc {
            position: position.map(str::to_string),
            z_index,
            ..Default::default()
        }
    }

    #[test]
    fn in_flow_children_paint_below_positioned_ones() {
        assert_eq!(paint_rank(None), 0);
        assert_eq!(paint_rank(Some(&style(None, Some(5.0)))), 0);
        assert_eq!(paint_rank(Some(&style(Some("static"), None))), 0);
        assert!(paint_rank(Some(&style(Some("relative"), None))) > 0);
        assert!(paint_rank(Some(&style(Some("absolute"), None))) > 0);
    }

    #[test]
    fn z_index_orders_positioned_children() {
        let rank = |z| paint_rank(Some(&style(Some("absolute"), z)));
        assert!(rank(Some(-1.0)) < 0);
        assert!(rank(Some(-2.0)) < rank(Some(-1.0)));
        assert_eq!(rank(None), rank(Some(0.0)));
        assert!(rank(Some(1.0)) > rank(Some(0.0)));
    }
}
