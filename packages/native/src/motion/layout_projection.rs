//! Where a `layout` box is shown against where layout put it, as Motion's projection builds it
//! (`notifyLayoutUpdate`, `calcBoxDelta`, `mixTargetDelta` in motion-dom's `create-projection-node`).
//!
//! A box is a `min` corner and a `size`. The projection of a box is a translation of its centre
//! and a scale of its size about it. It starts as the delta from the layout the box has to the
//! box that was shown when the layout changed, and eases to nothing.

use gpui::TransformationMatrix;

pub(crate) type Vec2 = [f64; 2];

/// A translation under this, in px, is rounding noise (Motion's `translate` snaps to 0 within 0.01).
const MOVED: f64 = 0.01;

/// A scale within this of 1 is 1 (Motion's `scale` snaps within 1e-4).
const SCALED: f64 = 1e-4;

pub(crate) fn sub(a: Vec2, b: Vec2) -> Vec2 {
    [a[0] - b[0], a[1] - b[1]]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub(crate) min: Vec2,
    pub(crate) size: Vec2,
}

impl Rect {
    pub(crate) fn center(self) -> Vec2 {
        [self.min[0] + self.size[0] / 2.0, self.min[1] + self.size[1] / 2.0]
    }
}

/// What a `layout` prop animates: `true` is both, `"position"` the corner, `"size"` the size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axes {
    Both,
    Position,
    Size,
}

impl Axes {
    /// Whether layout moved `to` from `from` in a way this animates.
    pub(crate) fn changed(self, from: Rect, to: Rect) -> bool {
        let moved = sub(to.min, from.min).iter().any(|distance| distance.abs() >= MOVED);
        let resized = sub(to.size, from.size).iter().any(|distance| distance.abs() >= MOVED);
        match self {
            Self::Both => moved || resized,
            Self::Position => moved,
            Self::Size => resized,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Projection {
    /// How far the centre of the shown box is from the centre of the layout box.
    pub(crate) translate: Vec2,
    /// The size of the shown box over the size of the layout box.
    pub(crate) scale: Vec2,
}

impl Projection {
    pub(crate) const NONE: Self = Self {
        translate: [0.0; 2],
        scale: [1.0; 2],
    };

    /// The channels a glide animates, from here to `NONE`.
    pub(crate) const CHANNELS: usize = 4;

    /// The projection that shows `layout` as `shown`, once `shown` has been cut down to what
    /// `axes` animates: the size of `layout` for `Position`, and its corner for `Size`.
    pub(crate) fn between(shown: Rect, layout: Rect, axes: Axes) -> Self {
        let shown = match axes {
            Axes::Both => shown,
            Axes::Position => Rect {
                min: shown.min,
                size: layout.size,
            },
            Axes::Size => Rect {
                min: layout.min,
                size: shown.size,
            },
        };
        let (from, to) = (shown.center(), layout.center());
        let mut translate = sub(from, to);
        let mut scale = [1.0; 2];
        for axis in 0..2 {
            if layout.size[axis] > 0.0 && shown.size[axis] >= 0.0 {
                scale[axis] = shown.size[axis] / layout.size[axis];
            }
            if (scale[axis] - 1.0).abs() < SCALED {
                scale[axis] = 1.0;
            }
            if translate[axis].abs() < MOVED {
                translate[axis] = 0.0;
            }
        }
        Self { translate, scale }
    }

    pub(crate) fn is_none(self) -> bool {
        self.translate.iter().all(|distance| distance.abs() < MOVED)
            && self.scale.iter().all(|scale| (scale - 1.0).abs() < SCALED)
    }

    pub(crate) fn to_channels(self) -> [f64; Self::CHANNELS] {
        [self.translate[0], self.translate[1], self.scale[0], self.scale[1]]
    }

    pub(crate) fn of_channels(channels: &[f64]) -> Self {
        Self {
            translate: [channels[0], channels[1]],
            scale: [channels[2], channels[3]],
        }
    }

    /// The box `layout` is shown as.
    pub(crate) fn shown(self, layout: Rect) -> Rect {
        let size = [layout.size[0] * self.scale[0], layout.size[1] * self.scale[1]];
        let centre = layout.center();
        Rect {
            min: [
                centre[0] + self.translate[0] - size[0] / 2.0,
                centre[1] + self.translate[1] - size[1] / 2.0,
            ],
            size,
        }
    }

    /// The map of the plane that scales about `centre` and moves by the translation.
    pub(crate) fn matrix_about(self, centre: Vec2) -> TransformationMatrix {
        let [sx, sy] = [self.scale[0] as f32, self.scale[1] as f32];
        TransformationMatrix::affine(
            [[sx, 0.0], [0.0, sy]],
            [
                (centre[0] + self.translate[0]) as f32 - sx * centre[0] as f32,
                (centre[1] + self.translate[1]) as f32 - sy * centre[1] as f32,
            ],
        )
    }
}

/// The matrix a `layout` box puts in the ambient transform to be shown as `net` when another
/// one encloses it, whose own net projection was `enclosing_net` and which left `enclosing_inside`
/// as the ambient transform inside it. The enclosing box carries everything inside it along; a
/// box that is itself a `layout` one is shown at its own layout box instead, as Motion
/// projects it, so the enclosing projection is taken back out before its own is applied.
///
/// `ambient` is the transform where the box is now. It differs from `enclosing_inside` by the
/// transforms of elements between the two boxes, and the projection to take back is the one seen
/// from inside them.
pub(crate) fn inside(
    enclosing_net: TransformationMatrix,
    enclosing_inside: TransformationMatrix,
    ambient: TransformationMatrix,
    net: TransformationMatrix,
) -> TransformationMatrix {
    let Some(undo) = enclosing_net.inverse() else {
        return net;
    };
    let Some(between) = enclosing_inside.inverse().map(|inverse| inverse.compose(ambient)) else {
        return net;
    };
    if is_unit(between) {
        return undo.compose(net);
    }
    match between.inverse() {
        Some(back) => back.compose(undo).compose(between).compose(net),
        None => net,
    }
}

fn is_unit(matrix: TransformationMatrix) -> bool {
    let unit = TransformationMatrix::unit();
    matrix
        .rows
        .iter()
        .flatten()
        .zip(unit.rows.iter().flatten())
        .all(|(a, b)| (a - b).abs() < 1e-3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            min: [x, y],
            size: [width, height],
        }
    }

    #[test]
    fn a_projection_shows_the_old_box_and_unprojects_to_layout() {
        let shown = rect(10.0, 20.0, 100.0, 50.0);
        let layout = rect(200.0, 100.0, 300.0, 200.0);
        let projection = Projection::between(shown, layout, Axes::Both);
        assert_eq!(projection.scale, [1.0 / 3.0, 0.25]);
        assert_eq!(projection.translate, [-290.0, -155.0]);
        let again = projection.shown(layout);
        for axis in 0..2 {
            assert!((again.min[axis] - shown.min[axis]).abs() < 1e-9);
            assert!((again.size[axis] - shown.size[axis]).abs() < 1e-9);
        }
        assert_eq!(Projection::NONE.shown(layout), layout);
    }

    #[test]
    fn position_keeps_the_new_size_and_size_keeps_the_new_corner() {
        let shown = rect(10.0, 20.0, 100.0, 50.0);
        let layout = rect(200.0, 100.0, 300.0, 200.0);

        let position = Projection::between(shown, layout, Axes::Position);
        assert_eq!(position.scale, [1.0, 1.0]);
        assert_eq!(position.translate, [-190.0, -80.0]);

        let size = Projection::between(shown, layout, Axes::Size);
        assert_eq!(size.scale, [1.0 / 3.0, 0.25]);
        let kept = size.shown(layout);
        assert!((kept.min[0] - 200.0).abs() < 1e-9 && (kept.min[1] - 100.0).abs() < 1e-9);
        assert!((kept.size[0] - 100.0).abs() < 1e-9 && (kept.size[1] - 50.0).abs() < 1e-9);
    }

    #[test]
    fn a_move_alone_is_no_size_animation_and_a_resize_alone_is_no_position_one() {
        let before = rect(0.0, 0.0, 100.0, 50.0);
        let moved = rect(40.0, 0.0, 100.0, 50.0);
        let grown = rect(0.0, 0.0, 100.0, 80.0);
        assert!(Projection::between(before, moved, Axes::Size).is_none());
        assert!(!Projection::between(before, moved, Axes::Position).is_none());
        assert!(Projection::between(before, grown, Axes::Position).is_none());
        assert!(Axes::Position.changed(before, moved) && !Axes::Size.changed(before, moved));
        assert!(Axes::Size.changed(before, grown) && !Axes::Position.changed(before, rect(0.0, 0.0, 100.0, 50.004)));
        assert!(Axes::Both.changed(before, grown));
    }

    #[test]
    fn noise_and_a_box_with_no_size_are_not_a_projection() {
        let layout = rect(0.0, 0.0, 100.0, 50.0);
        let noisy = rect(0.004, -0.004, 100.00001, 50.0);
        assert!(Projection::between(noisy, layout, Axes::Both).is_none());
        let flat = rect(0.0, 0.0, 0.0, 0.0);
        let projection = Projection::between(rect(5.0, 5.0, 10.0, 10.0), flat, Axes::Both);
        assert_eq!(projection.scale, [1.0, 1.0]);
    }

    #[test]
    fn the_matrix_scales_about_the_centre() {
        let layout = rect(100.0, 100.0, 100.0, 40.0);
        let projection = Projection {
            translate: [10.0, -5.0],
            scale: [0.5, 2.0],
        };
        let matrix = projection.matrix_about(layout.center());
        let rows = matrix.rows;
        let map = |x: f32, y: f32| {
            (
                rows[0][0] * x + rows[0][1] * y + rows[0][2],
                rows[1][0] * x + rows[1][1] * y + rows[1][2],
            )
        };
        let shown = projection.shown(layout);
        let (left, top) = map(100.0, 100.0);
        let (right, bottom) = map(200.0, 140.0);
        assert!((f64::from(left) - shown.min[0]).abs() < 1e-4);
        assert!((f64::from(top) - shown.min[1]).abs() < 1e-4);
        assert!((f64::from(right) - (shown.min[0] + shown.size[0])).abs() < 1e-4);
        assert!((f64::from(bottom) - (shown.min[1] + shown.size[1])).abs() < 1e-4);
    }

    fn apply(matrix: TransformationMatrix, point: (f32, f32)) -> (f32, f32) {
        let rows = matrix.rows;
        (
            rows[0][0] * point.0 + rows[0][1] * point.1 + rows[0][2],
            rows[1][0] * point.0 + rows[1][1] * point.1 + rows[1][2],
        )
    }

    #[test]
    fn a_layout_box_inside_another_is_shown_at_its_own_projection() {
        let outer = Projection {
            translate: [30.0, 0.0],
            scale: [2.0, 2.0],
        }
        .matrix_about([100.0, 100.0]);
        let own = Projection {
            translate: [-5.0, 7.0],
            scale: [0.5, 0.5],
        }
        .matrix_about([120.0, 110.0]);
        // Nothing between them: the ambient inside the outer box is the outer projection.
        let put = inside(outer, outer, outer, own);
        let (x, y) = apply(outer.compose(put), (123.0, 98.0));
        let (want_x, want_y) = apply(own, (123.0, 98.0));
        assert!((x - want_x).abs() < 1e-3 && (y - want_y).abs() < 1e-3, "{x} {y} against {want_x} {want_y}");
    }

    #[test]
    fn what_lies_between_the_boxes_is_not_undone() {
        let outer = Projection {
            translate: [30.0, 10.0],
            scale: [2.0, 2.0],
        }
        .matrix_about([100.0, 100.0]);
        let own = Projection {
            translate: [-5.0, 7.0],
            scale: [1.0, 1.0],
        }
        .matrix_about([120.0, 110.0]);
        // A box between them that moves the content by 50 px to the right.
        let between = TransformationMatrix::affine([[1.0, 0.0], [0.0, 1.0]], [50.0, 0.0]);
        let ambient = outer.compose(between);
        let put = inside(outer, outer, ambient, own);
        // Drawn through the ambient transform, the box lands where the between-transform alone would
        // put it and its own projection after that.
        let (x, y) = apply(ambient.compose(put), (123.0, 98.0));
        let (want_x, want_y) = apply(between.compose(own), (123.0, 98.0));
        assert!((x - want_x).abs() < 1e-3 && (y - want_y).abs() < 1e-3, "{x} {y} against {want_x} {want_y}");
    }
}
