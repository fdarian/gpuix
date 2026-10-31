//! What the elements that show a native surface (`<webview>`, `<nativeview>`) share.

/// The radius a surface is cut to, if the corners are one circle.
pub(crate) fn uniform_radius(
    radii: &gpui::Corners<gpui::CornerRadius>,
    size: gpui::Size<gpui::Pixels>,
    rem: gpui::Pixels,
) -> gpui::Pixels {
    let corners = [radii.top_left, radii.top_right, radii.bottom_right, radii.bottom_left]
        .map(|corner| corner.to_pixels(size, rem));
    uniform_circle(corners)
}

/// The radius of four corners that are all the same circle, zero otherwise. A corner with a
/// vertical radius of zero is circular (`CornerRadius::y`).
pub(crate) fn uniform_circle(corners: [gpui::Size<gpui::Pixels>; 4]) -> gpui::Pixels {
    let circle = |corner: gpui::Size<gpui::Pixels>| {
        let vertical = if corner.height == gpui::Pixels::ZERO { corner.width } else { corner.height };
        (corner.width == vertical).then_some(corner.width)
    };
    match circle(corners[0]) {
        Some(radius) if corners.iter().all(|corner| circle(*corner) == Some(radius)) => radius,
        _ => gpui::Pixels::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{px, size};

    fn circle(radius: f32) -> gpui::Size<gpui::Pixels> {
        size(px(radius), px(radius))
    }

    #[test]
    fn four_equal_circles_are_a_radius() {
        assert_eq!(uniform_circle([circle(8.); 4]), px(8.));
    }

    #[test]
    fn square_corners_are_no_radius() {
        assert_eq!(uniform_circle([circle(0.); 4]), px(0.));
    }

    #[test]
    fn corners_that_differ_are_no_radius() {
        assert_eq!(uniform_circle([circle(8.), circle(8.), circle(8.), circle(4.)]), px(0.));
        assert_eq!(uniform_circle([circle(8.), circle(8.), circle(8.), circle(0.)]), px(0.));
    }

    #[test]
    fn an_ellipse_is_no_radius() {
        assert_eq!(uniform_circle([size(px(8.), px(4.)); 4]), px(0.));
    }

    #[test]
    fn a_corner_without_a_vertical_radius_is_a_circle() {
        assert_eq!(uniform_circle([size(px(6.), px(0.)); 4]), px(6.));
    }
}
