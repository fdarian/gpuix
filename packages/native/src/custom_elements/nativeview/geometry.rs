//! How the native view of a `<nativeview>` is cut and who it takes the pointer from.
use gpui::{size, CornerRadius, Corners, Pixels, Size};

use crate::custom_elements::native_surface::uniform_circle;

/// The sides of `[top, right, bottom, left]`, the order `css::border::used_widths` answers in.
const TOP: usize = 0;
const RIGHT: usize = 1;
const BOTTOM: usize = 2;
const LEFT: usize = 3;

/// What the element's style says about the cut of its content: the radii of its border box, the
/// borders inside them, and whether it clips at all.
#[derive(Clone, Debug)]
pub(super) struct Cut {
    radii: Corners<CornerRadius>,
    borders: [Pixels; 4],
    clips: bool,
}

impl Cut {
    pub(super) fn new(radii: Corners<CornerRadius>, borders: [Pixels; 4], clips: bool) -> Self {
        Self { radii, borders, clips }
    }

    /// The radius the native view is cut to, over the padding box `padding_box`. An element that
    /// does not clip (`overflow: visible`) cuts nothing, and its radius does not reach the view.
    pub(super) fn corner_radius(&self, padding_box: Size<Pixels>, rem: Pixels) -> Pixels {
        if !self.clips {
            return Pixels::ZERO;
        }
        inner_radius(&self.radii, self.borders, padding_box, rem)
    }
}

/// The radius of the padding box when its four corners are one circle: each outer radius less
/// the border it runs along, on each axis. Zero when they are not, as a surface has one radius.
fn inner_radius(
    radii: &Corners<CornerRadius>,
    borders: [Pixels; 4],
    padding_box: Size<Pixels>,
    rem: Pixels,
) -> Pixels {
    // Fractions are of the border box the radii are declared on.
    let border_box = size(
        padding_box.width + borders[LEFT] + borders[RIGHT],
        padding_box.height + borders[TOP] + borders[BOTTOM],
    );
    let inner = |radius: CornerRadius, horizontal: Pixels, vertical: Pixels| {
        let outer = radius.to_pixels(border_box, rem);
        // `CornerRadius`: no vertical radius is the horizontal one.
        let outer_vertical = if outer.height == Pixels::ZERO { outer.width } else { outer.height };
        let width = (outer.width - horizontal).max(Pixels::ZERO);
        let height = (outer_vertical - vertical).max(Pixels::ZERO);
        if width == Pixels::ZERO || height == Pixels::ZERO {
            size(Pixels::ZERO, Pixels::ZERO)
        } else {
            size(width, height)
        }
    };
    uniform_circle([
        inner(radii.top_left, borders[LEFT], borders[TOP]),
        inner(radii.top_right, borders[RIGHT], borders[TOP]),
        inner(radii.bottom_right, borders[RIGHT], borders[BOTTOM]),
        inner(radii.bottom_left, borders[LEFT], borders[BOTTOM]),
    ])
}

/// Whether the native view takes the pointer: it asked to (`nativeEvents`) and the box is a hit
/// target. Under `pointer-events: none` the events are GPUI's, to what is below.
pub(super) fn captures_pointer(native_events: bool, pointer_events_none: bool) -> bool {
    native_events && !pointer_events_none
}

#[cfg(test)]
mod tests {
    use gpui::{px, relative, rems};

    use super::*;

    const REM: Pixels = px(16.);

    fn all(radius: impl Into<CornerRadius>) -> Corners<CornerRadius> {
        radius.into().on_all_corners()
    }

    fn cut(radii: Corners<CornerRadius>, borders: [f32; 4], clips: bool) -> Cut {
        Cut::new(radii, borders.map(px), clips)
    }

    fn radius_of(cut: &Cut, width: f32, height: f32) -> Pixels {
        cut.corner_radius(size(px(width), px(height)), REM)
    }

    #[test]
    fn the_view_is_cut_to_the_radius_less_the_border() {
        assert_eq!(radius_of(&cut(all(px(12.)), [2.; 4], true), 100., 60.), px(10.));
        assert_eq!(radius_of(&cut(all(px(12.)), [0.; 4], true), 100., 60.), px(12.));
    }

    #[test]
    fn a_radius_the_border_uses_up_is_square() {
        assert_eq!(radius_of(&cut(all(px(4.)), [4.; 4], true), 100., 60.), px(0.));
        assert_eq!(radius_of(&cut(all(px(4.)), [9.; 4], true), 100., 60.), px(0.));
    }

    #[test]
    fn borders_that_differ_across_and_down_make_an_ellipse_which_is_square() {
        // 12 - 2 across and 12 - 3 down.
        assert_eq!(radius_of(&cut(all(px(12.)), [3., 2., 3., 2.], true), 100., 60.), px(0.));
    }

    #[test]
    fn an_elliptical_outer_radius_can_leave_a_circle() {
        let ellipse = CornerRadius { x: px(12.).into(), y: px(11.).into() };
        // 12 - 2 across and 11 - 1 down.
        assert_eq!(radius_of(&cut(all(ellipse), [1., 2., 1., 2.], true), 100., 60.), px(10.));
    }

    #[test]
    fn corners_that_differ_are_square() {
        let radii = Corners {
            top_left: px(12.).into(),
            top_right: px(12.).into(),
            bottom_right: px(12.).into(),
            bottom_left: px(6.).into(),
        };
        assert_eq!(radius_of(&cut(radii, [0.; 4], true), 100., 60.), px(0.));
    }

    #[test]
    fn a_square_corner_among_round_ones_makes_all_square() {
        let radii = Corners {
            top_left: px(12.).into(),
            top_right: px(12.).into(),
            bottom_right: px(12.).into(),
            bottom_left: px(0.).into(),
        };
        assert_eq!(radius_of(&cut(radii, [0.; 4], true), 100., 60.), px(0.));
    }

    #[test]
    fn an_elliptical_radius_is_square() {
        let ellipse = CornerRadius { x: px(8.).into(), y: px(4.).into() };
        assert_eq!(radius_of(&cut(all(ellipse), [0.; 4], true), 100., 60.), px(0.));
        assert_eq!(radius_of(&cut(all(ellipse), [1.; 4], true), 100., 60.), px(0.));
    }

    #[test]
    fn a_corner_without_a_vertical_radius_is_a_circle() {
        let circle = CornerRadius { x: px(8.).into(), y: px(0.).into() };
        assert_eq!(radius_of(&cut(all(circle), [2.; 4], true), 100., 60.), px(6.));
        // The vertical radius is 8 too, so a wider border on one side is an ellipse.
        assert_eq!(radius_of(&cut(all(circle), [1., 1., 1., 3.], true), 100., 60.), px(0.));
    }

    #[test]
    fn a_fraction_is_of_the_border_box() {
        // Half of a 104 x 104 border box is 52; the 2px border leaves 50.
        let half = CornerRadius::from(relative(0.5));
        assert_eq!(radius_of(&cut(all(half), [2.; 4], true), 100., 100.), px(50.));
        // On a box that is not square it is an ellipse.
        assert_eq!(radius_of(&cut(all(half), [2.; 4], true), 200., 100.), px(0.));
    }

    #[test]
    fn a_rem_radius_is_of_the_windows_rem() {
        assert_eq!(radius_of(&cut(all(rems(1.)), [0.; 4], true), 100., 60.), px(16.));
    }

    #[test]
    fn an_element_that_does_not_clip_does_not_cut_the_view() {
        assert_eq!(radius_of(&cut(all(px(12.)), [0.; 4], false), 100., 60.), px(0.));
        assert_eq!(radius_of(&cut(all(px(12.)), [2.; 4], false), 100., 60.), px(0.));
    }

    #[test]
    fn the_pointer_is_taken_when_asked_for_on_a_box_that_can_be_hit() {
        assert!(captures_pointer(true, false));
        assert!(!captures_pointer(false, false));
        assert!(!captures_pointer(true, true));
        assert!(!captures_pointer(false, true));
    }
}
