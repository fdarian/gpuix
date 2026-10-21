//! The displays attached to the machine (`getAllDisplays`, `getPrimaryDisplay`) and the rule that
//! picks the one a window opens on.

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use napi_derive::napi;

/// A rectangle in CSS px.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct Rectangle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A size in CSS px.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// A display, as Electron's `Display`. Coordinates are CSS px in one global space: the origin is
/// the top-left of the primary display, y points down, and a display left of or above it has
/// negative coordinates.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct Display {
    pub id: u32,
    /// The name the OS gives the display, such as "Built-in Retina Display".
    pub label: String,
    pub bounds: Rectangle,
    /// `bounds` without the menu bar and the Dock.
    pub work_area: Rectangle,
    /// Device pixels per CSS px.
    pub scale_factor: f64,
    /// Degrees clockwise: 0, 90, 180 or 270.
    pub rotation: f64,
    /// True for the display built into the machine.
    pub internal: bool,
    /// The size of `bounds`.
    pub size: Size,
    /// The size of `workArea`.
    pub work_area_size: Size,
}

impl Display {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.bounds.x
            && x < self.bounds.x + self.bounds.width
            && y >= self.bounds.y
            && y < self.bounds.y + self.bounds.height
    }

    fn squared_distance_to(&self, x: f64, y: f64) -> f64 {
        let dx = (self.bounds.x - x).max(0.0).max(x - (self.bounds.x + self.bounds.width));
        let dy = (self.bounds.y - y).max(0.0).max(y - (self.bounds.y + self.bounds.height));
        dx * dx + dy * dy
    }
}

/// The display that contains the point, with its right and bottom edges belonging to the
/// neighbour as in a DOM rect, else the one nearest to it (the first of equals). `None` only when
/// there are no displays.
pub(crate) fn display_for_point(displays: &[Display], x: f64, y: f64) -> Option<&Display> {
    displays.iter().find(|display| display.contains(x, y)).or_else(|| {
        displays.iter().min_by(|a, b| {
            a.squared_distance_to(x, y)
                .total_cmp(&b.squared_distance_to(x, y))
        })
    })
}

#[cfg(target_os = "macos")]
fn rectangle(bounds: gpui::Bounds<gpui::Pixels>) -> Rectangle {
    Rectangle {
        x: bounds.origin.x.into(),
        y: bounds.origin.y.into(),
        width: bounds.size.width.into(),
        height: bounds.size.height.into(),
    }
}

#[cfg(target_os = "macos")]
fn size_of(rectangle: &Rectangle) -> Size {
    Size {
        width: rectangle.width,
        height: rectangle.height,
    }
}

/// `None` when AppKit has no screen for the display (a mirrored one): it shows what another
/// display shows and has no frame of its own.
#[cfg(target_os = "macos")]
fn describe(display: &gpui_macos::MacDisplay) -> napi::Result<Option<Display>> {
    use gpui::PlatformDisplay;

    let Some(screen) = display.screen() else {
        return Ok(None);
    };
    let id = u64::from(display.id());
    let id = u32::try_from(id).map_err(|_| {
        napi::Error::from_reason(format!("Display id {id} does not fit in 32 bits"))
    })?;
    let bounds = rectangle(screen.frame);
    let work_area = rectangle(screen.visible_frame);
    Ok(Some(Display {
        id,
        label: screen.localized_name.to_string(),
        size: size_of(&bounds),
        work_area_size: size_of(&work_area),
        bounds,
        work_area,
        scale_factor: f64::from(screen.backing_scale_factor),
        rotation: display.rotation(),
        internal: display.is_builtin(),
    }))
}

#[cfg(target_os = "macos")]
pub(crate) fn all_displays() -> napi::Result<Vec<Display>> {
    let mut displays = Vec::new();
    for display in gpui_macos::MacDisplay::all() {
        if let Some(described) = describe(&display)? {
            displays.push(described);
        }
    }
    Ok(displays)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn all_displays() -> napi::Result<Vec<Display>> {
    Err(napi::Error::from_reason(
        "Reading the displays is only implemented on macOS",
    ))
}

/// Every display, in the order the OS lists them.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[napi]
pub fn get_all_displays() -> napi::Result<Vec<Display>> {
    all_displays()
}

/// The display with the menu bar. Its `bounds` start at 0, 0.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[napi]
pub fn get_primary_display() -> napi::Result<Display> {
    #[cfg(target_os = "macos")]
    {
        let primary = gpui_macos::MacDisplay::primary();
        describe(&primary)?.ok_or_else(|| {
            napi::Error::from_reason("The primary display has no screen to describe")
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        all_displays().map(|_| unreachable!("all_displays fails off macOS"))
    }
}

#[cfg(test)]
pub(crate) fn test_display(id: u32, x: f64, y: f64, width: f64, height: f64) -> Display {
    let bounds = Rectangle {
        x,
        y,
        width,
        height,
    };
    let size = Size { width, height };
    Display {
        id,
        label: format!("display {id}"),
        work_area: bounds,
        bounds,
        scale_factor: 2.0,
        rotation: 0.0,
        internal: id == 1,
        size,
        work_area_size: size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> Vec<Display> {
        vec![
            test_display(1, 0.0, 0.0, 1512.0, 982.0),
            test_display(2, 1512.0, -200.0, 2560.0, 1440.0),
        ]
    }

    fn id_at(displays: &[Display], x: f64, y: f64) -> Option<u32> {
        display_for_point(displays, x, y).map(|display| display.id)
    }

    #[test]
    fn a_point_inside_belongs_to_its_display() {
        let displays = pair();
        assert_eq!(id_at(&displays, 100.0, 100.0), Some(1));
        assert_eq!(id_at(&displays, 2000.0, 300.0), Some(2));
    }

    #[test]
    fn the_top_left_corner_is_inside() {
        let displays = pair();
        assert_eq!(id_at(&displays, 0.0, 0.0), Some(1));
        assert_eq!(id_at(&displays, 1512.0, -200.0), Some(2));
    }

    #[test]
    fn the_right_edge_belongs_to_the_neighbour() {
        let displays = pair();
        assert_eq!(id_at(&displays, 1511.99, 500.0), Some(1));
        assert_eq!(id_at(&displays, 1512.0, 500.0), Some(2));
    }

    #[test]
    fn the_bottom_edge_belongs_to_the_neighbour() {
        let stacked = vec![
            test_display(1, 0.0, 0.0, 1000.0, 800.0),
            test_display(2, 0.0, 800.0, 1000.0, 600.0),
        ];
        assert_eq!(id_at(&stacked, 500.0, 799.99), Some(1));
        assert_eq!(id_at(&stacked, 500.0, 800.0), Some(2));
    }

    #[test]
    fn the_far_edge_of_the_last_display_is_outside_and_snaps_back_to_it() {
        let displays = pair();
        assert!(!displays[1].contains(4072.0, 0.0));
        assert_eq!(id_at(&displays, 4072.0, 0.0), Some(2));
    }

    #[test]
    fn a_point_on_no_display_takes_the_nearest_by_distance_to_the_rect() {
        let displays = pair();
        // Left of the primary: 40 from it, 1552 from the second display.
        assert_eq!(id_at(&displays, -40.0, 500.0), Some(1));
        // Above the second display: 300 from it, far from the primary.
        assert_eq!(id_at(&displays, 3000.0, -500.0), Some(2));
        // Below the primary: 318 from it, 1412 across and 60 down from the second.
        assert_eq!(id_at(&displays, 100.0, 1300.0), Some(1));
        // Below the second display: 100 from it, 488 across and 358 down from the primary.
        assert_eq!(id_at(&displays, 2000.0, 1340.0), Some(2));
    }

    #[test]
    fn nearest_measures_to_the_rect_not_to_its_origin() {
        let displays = vec![
            test_display(1, 0.0, 0.0, 1000.0, 1000.0),
            test_display(2, 1100.0, 0.0, 100.0, 100.0),
        ];
        // 50 from the first display's edge, and 50 across and 400 down from the second; the
        // second display's origin is the nearer of the two origins.
        assert_eq!(id_at(&displays, 1050.0, 500.0), Some(1));
    }

    #[test]
    fn equal_distances_take_the_first_display() {
        let displays = vec![
            test_display(7, 0.0, 0.0, 100.0, 100.0),
            test_display(8, 300.0, 0.0, 100.0, 100.0),
        ];
        assert_eq!(id_at(&displays, 200.0, 50.0), Some(7));
    }

    #[test]
    fn no_displays_is_none() {
        assert_eq!(id_at(&[], 0.0, 0.0), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_primary_display_is_one_of_all_and_sits_at_the_origin() {
        let all = get_all_displays().expect("displays");
        let primary = get_primary_display().expect("primary display");
        assert!(
            all.contains(&primary),
            "getAllDisplays {all:?} lacks the primary {primary:?}"
        );
        assert_eq!((primary.bounds.x, primary.bounds.y), (0.0, 0.0));

        for display in &all {
            let bounds = &display.bounds;
            let area = &display.work_area;
            assert!(
                area.x >= bounds.x
                    && area.y >= bounds.y
                    && area.x + area.width <= bounds.x + bounds.width
                    && area.y + area.height <= bounds.y + bounds.height,
                "work area outside bounds: {display:?}"
            );
            assert_eq!(
                display.size,
                Size {
                    width: bounds.width,
                    height: bounds.height
                }
            );
            assert_eq!(
                display.work_area_size,
                Size {
                    width: area.width,
                    height: area.height
                }
            );
        }
    }
}
