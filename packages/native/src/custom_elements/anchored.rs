/// Anchored custom element for deferred, trigger-relative floating layers.
use std::cell::Cell;
use std::rc::Rc;

use super::{CustomElement, CustomElementFactory, CustomRenderContext};

pub struct AnchoredFactory;

impl CustomElementFactory for AnchoredFactory {
    fn element_type(&self) -> &str {
        "anchored"
    }

    fn create(&self, id: u64) -> Box<dyn CustomElement> {
        Box::new(AnchoredElement {
            id,
            ..AnchoredElement::default()
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum AnchorPoint {
    TopLeft,
    TopCenter,
    TopRight,
    RightCenter,
    BottomRight,
    BottomCenter,
    BottomLeft,
    LeftCenter,
}

impl AnchorPoint {
    fn from_str(value: &str) -> Option<Self> {
        Some(match value {
            "topLeft" => Self::TopLeft,
            "topCenter" => Self::TopCenter,
            "topRight" => Self::TopRight,
            "rightCenter" => Self::RightCenter,
            "bottomRight" => Self::BottomRight,
            "bottomCenter" => Self::BottomCenter,
            "bottomLeft" => Self::BottomLeft,
            "leftCenter" => Self::LeftCenter,
            _ => return None,
        })
    }

    fn as_gpui(self) -> gpui::Anchor {
        match self {
            Self::TopLeft => gpui::Anchor::TopLeft,
            Self::TopCenter => gpui::Anchor::TopCenter,
            Self::TopRight => gpui::Anchor::TopRight,
            Self::RightCenter => gpui::Anchor::RightCenter,
            Self::BottomRight => gpui::Anchor::BottomRight,
            Self::BottomCenter => gpui::Anchor::BottomCenter,
            Self::BottomLeft => gpui::Anchor::BottomLeft,
            Self::LeftCenter => gpui::Anchor::LeftCenter,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Side {
    Top,
    Right,
    #[default]
    Bottom,
    Left,
}

impl Side {
    fn from_str(value: &str) -> Self {
        match value {
            "top" => Self::Top,
            "right" => Self::Right,
            "left" => Self::Left,
            _ => Self::Bottom,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Alignment {
    #[default]
    Start,
    Center,
    End,
}

impl Alignment {
    fn from_str(value: &str) -> Self {
        match value {
            "center" => Self::Center,
            "end" => Self::End,
            _ => Self::Start,
        }
    }
}

/// `Flip` is floating-ui's flip + shift, `Snap` only shifts. `Switch` is `Flip`, and with an
/// explicit `anchor` corner it picks gpui's corner switching instead of snapping.
#[derive(Debug, Clone, Copy, Default)]
enum FitMode {
    #[default]
    Flip,
    Switch,
    Snap,
}

impl FitMode {
    fn from_str(value: &str) -> Self {
        match value {
            "snap" => Self::Snap,
            "switch" => Self::Switch,
            _ => Self::Flip,
        }
    }
}

/// Where the last placement settled and how much room it left, in whole pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PlacementReport {
    side: Side,
    align: Alignment,
    available_width: i32,
    available_height: i32,
    anchor_width: i32,
    anchor_height: i32,
}

impl Side {
    fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Right => "right",
            Self::Bottom => "bottom",
            Self::Left => "left",
        }
    }
}

impl Alignment {
    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
        }
    }
}

/// Written during prepaint, read by the next `render`.
#[derive(Debug, Default)]
struct PlacementState {
    latest: Cell<Option<PlacementReport>>,
    emitted: Cell<Option<PlacementReport>>,
}

#[derive(Debug, Clone)]
pub struct AnchoredElement {
    position: Option<(f32, f32)>,
    side: Side,
    align: Alignment,
    anchor: Option<AnchorPoint>,
    gap: f32,
    offset: (f32, f32),
    fit: FitMode,
    snap_margin: f32,
    deferred: bool,
    priority: usize,
    /// `fill: "window"`: cover the whole viewport, like a dialog portal.
    fill_window: bool,
    /// `constrainToAvailable`: cap the layer's size to the room placement left.
    constrain_to_available: bool,
    placement_state: Rc<PlacementState>,
    id: u64,
}

impl Default for AnchoredElement {
    fn default() -> Self {
        Self {
            position: None,
            side: Side::Bottom,
            align: Alignment::Start,
            anchor: None,
            gap: 0.0,
            offset: (0.0, 0.0),
            fit: FitMode::Flip,
            snap_margin: 8.0,
            deferred: true,
            priority: 1,
            fill_window: false,
            constrain_to_available: false,
            placement_state: Rc::default(),
            id: 0,
        }
    }
}

impl AnchoredElement {
    fn resolved_anchor(&self) -> AnchorPoint {
        if let Some(anchor) = self.anchor {
            return anchor;
        }
        match (self.side, self.align) {
            (Side::Top, Alignment::Start) => AnchorPoint::BottomLeft,
            (Side::Top, Alignment::Center) => AnchorPoint::BottomCenter,
            (Side::Top, Alignment::End) => AnchorPoint::BottomRight,
            (Side::Right, Alignment::Start) => AnchorPoint::TopLeft,
            (Side::Right, Alignment::Center) => AnchorPoint::LeftCenter,
            (Side::Right, Alignment::End) => AnchorPoint::BottomLeft,
            (Side::Bottom, Alignment::Start) => AnchorPoint::TopLeft,
            (Side::Bottom, Alignment::Center) => AnchorPoint::TopCenter,
            (Side::Bottom, Alignment::End) => AnchorPoint::TopRight,
            (Side::Left, Alignment::Start) => AnchorPoint::TopRight,
            (Side::Left, Alignment::Center) => AnchorPoint::RightCenter,
            (Side::Left, Alignment::End) => AnchorPoint::BottomRight,
        }
    }

    fn resolved_offset(&self) -> gpui::Point<gpui::Pixels> {
        let (side_x, side_y) = match self.side {
            Side::Top => (0.0, -self.gap),
            Side::Right => (self.gap, 0.0),
            Side::Bottom => (0.0, self.gap),
            Side::Left => (-self.gap, 0.0),
        };
        gpui::point(
            gpui::px(side_x + self.offset.0),
            gpui::px(side_y + self.offset.1),
        )
    }

    /// Placement from side/align/gap, when no explicit `anchor` corner overrides it.
    fn placement(&self) -> Option<gpui::AnchoredPlacement> {
        if self.anchor.is_some() {
            return None;
        }
        Some(gpui::AnchoredPlacement {
            side: match self.side {
                Side::Top => gpui::PlacementSide::Top,
                Side::Right => gpui::PlacementSide::Right,
                Side::Bottom => gpui::PlacementSide::Bottom,
                Side::Left => gpui::PlacementSide::Left,
            },
            align: match self.align {
                Alignment::Start => gpui::PlacementAlign::Start,
                Alignment::Center => gpui::PlacementAlign::Center,
                Alignment::End => gpui::PlacementAlign::End,
            },
            gap: gpui::px(self.gap),
            offset: gpui::point(gpui::px(self.offset.0), gpui::px(self.offset.1)),
            flip: !matches!(self.fit, FitMode::Snap),
            margin: gpui::px(self.snap_margin),
        })
    }

    fn wrap_at_trigger(&self, layer: gpui::AnyElement) -> gpui::AnyElement {
        use gpui::prelude::*;

        match (self.side, self.align) {
            (Side::Top, Alignment::Start) => gpui::div()
                .absolute()
                .top_0()
                .left_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Top, Alignment::Center) => gpui::div()
                .absolute()
                .top_0()
                .left(gpui::relative(0.5))
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Top, Alignment::End) => gpui::div()
                .absolute()
                .top_0()
                .right_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Right, Alignment::Start) => gpui::div()
                .absolute()
                .top_0()
                .right_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Right, Alignment::Center) => gpui::div()
                .absolute()
                .top(gpui::relative(0.5))
                .right_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Right, Alignment::End) => gpui::div()
                .absolute()
                .bottom_0()
                .right_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Bottom, Alignment::Start) => gpui::div()
                .absolute()
                .bottom_0()
                .left_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Bottom, Alignment::Center) => gpui::div()
                .absolute()
                .bottom_0()
                .left(gpui::relative(0.5))
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Bottom, Alignment::End) => gpui::div()
                .absolute()
                .bottom_0()
                .right_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Left, Alignment::Start) => gpui::div()
                .absolute()
                .top_0()
                .left_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Left, Alignment::Center) => gpui::div()
                .absolute()
                .top(gpui::relative(0.5))
                .left_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Left, Alignment::End) => gpui::div()
                .absolute()
                .bottom_0()
                .left_0()
                .size_0()
                .child(layer)
                .into_any_element(),
        }
    }
}

impl CustomElement for AnchoredElement {
    fn render(
        &mut self,
        ctx: CustomRenderContext,
        window: &mut gpui::Window,
        _cx: &mut gpui::Context<crate::renderer::GpuixView>,
    ) -> gpui::AnyElement {
        use gpui::prelude::*;

        // The overlay, not the trigger, is what a user clicks, so the recorded
        // box has to be this content div after `anchored` placed it. Only gpui
        // knows where that is, so it reports its own painted box instead of
        // carrying a `bounds_tracker` child.
        let mut content = gpui::div()
            .id(crate::events::ids::host_element_id(ctx.id))
            .flex_col();
        content = crate::events::ids::host_hitbox(content, ctx.pointer_events_none);
        content = crate::automation::track_own_bounds(content, ctx.id, ctx.observe.clone());
        if let Some(style) = ctx.style {
            content = crate::renderer::apply_interactive_styles(content, style);
        }
        if self.fill_window {
            // `anchored()` is absolute and sized by its content, so a percentage
            // size has nothing to resolve against. A resize runs gpui's
            // `Window::bounds_changed`, which refreshes the window, so reading
            // the viewport here tracks it in the same frame, with no JS polling.
            // Applied after the JS style so it always wins over width/height.
            let viewport = window.viewport_size();
            content = content.w(viewport.width).h(viewport.height);
        }
        if self.constrain_to_available {
            if let Some(report) = self.placement_state.latest.get() {
                content = content
                    .max_w(gpui::px(report.available_width as f32))
                    .max_h(gpui::px(report.available_height as f32));
            }
        }
        content = crate::accessibility::apply_accessibility(content, ctx.id, ctx.props, None);
        content = super::wire_host_events(content, &ctx);
        // Deferred overlays paint over the window blur. A missing fill lets the
        // page show through the card. Force an opaque surface when JS omitted
        // one. An explicit transparent fill opts out: gpui paints no quad for it,
        // and a full-window layer such as a dialog portal must paint nothing.
        let declares_fill = ctx
            .style
            .and_then(crate::style::StyleDesc::resolved_background)
            .is_some();
        if !declares_fill {
            content = content.bg(gpui::rgb(0x1A1A1A));
        }
        for child in ctx.children {
            content = content.child(child);
        }

        let mut anchored = gpui::anchored();
        if self.fill_window {
            // Already the window: no offset, no snapping.
            anchored = anchored
                .anchor(gpui::Anchor::TopLeft)
                .position(gpui::point(gpui::px(0.0), gpui::px(0.0)));
        } else if let Some(placement) = self.placement() {
            if let Some((x, y)) = self.position {
                anchored = anchored.position(gpui::point(gpui::px(x), gpui::px(y)));
            }
            let state = self.placement_state.clone();
            let notify_view = _cx.entity_id();
            let constrain = self.constrain_to_available;
            let callback = ctx.event_callback.clone();
            let element_id = ctx.id;
            let reports_placement = ctx.events.contains("placement");
            anchored = anchored
                .placement(placement)
                .on_placement(move |resolved, _window, app| {
                    let report = PlacementReport {
                        side: match resolved.side {
                            gpui::PlacementSide::Top => Side::Top,
                            gpui::PlacementSide::Right => Side::Right,
                            gpui::PlacementSide::Bottom => Side::Bottom,
                            gpui::PlacementSide::Left => Side::Left,
                        },
                        align: match resolved.align {
                            gpui::PlacementAlign::Start => Alignment::Start,
                            gpui::PlacementAlign::Center => Alignment::Center,
                            gpui::PlacementAlign::End => Alignment::End,
                        },
                        available_width: f32::from(resolved.available_width).round() as i32,
                        available_height: f32::from(resolved.available_height).round() as i32,
                        anchor_width: f32::from(resolved.anchor_size.width).round() as i32,
                        anchor_height: f32::from(resolved.anchor_size.height).round() as i32,
                    };
                    let previous = state.latest.replace(Some(report));
                    let anchor_changed = crate::layout::anchor_size::record(
                        element_id,
                        (report.anchor_width, report.anchor_height),
                    );
                    if (constrain && previous != Some(report))
                        || (anchor_changed && crate::layout::anchor_size::uses_anchor_size(element_id))
                    {
                        // The layer was built with the previous size cap.
                        app.notify(notify_view);
                    }
                    if reports_placement && state.emitted.get() != Some(report) {
                        state.emitted.set(Some(report));
                        crate::renderer::emit_event_full(
                            &callback,
                            element_id,
                            "placement",
                            |payload| {
                                payload.side = Some(report.side.as_str().to_string());
                                payload.align = Some(report.align.as_str().to_string());
                                payload.available_width = Some(report.available_width as f64);
                                payload.available_height = Some(report.available_height as f64);
                                payload.anchor_width = Some(report.anchor_width as f64);
                                payload.anchor_height = Some(report.anchor_height as f64);
                            },
                        );
                    }
                });
        } else {
            anchored = anchored
                .anchor(self.resolved_anchor().as_gpui())
                .offset(self.resolved_offset());
            if let Some((x, y)) = self.position {
                anchored = anchored.position(gpui::point(gpui::px(x), gpui::px(y)));
            }
            if !matches!(self.fit, FitMode::Switch) {
                anchored = anchored.snap_to_window_with_margin(gpui::px(self.snap_margin));
            }
        }

        let anchored = anchored.child(content);
        let layer = if self.deferred {
            gpui::deferred(anchored)
                .with_priority(self.priority)
                .into_any_element()
        } else {
            anchored.into_any_element()
        };

        if self.position.is_some() || self.fill_window {
            layer
        } else if self.placement().is_some() {
            // Covers the trigger, so placement can measure the box it flips around.
            gpui::div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(layer)
                .into_any_element()
        } else {
            self.wrap_at_trigger(layer)
        }
    }

    fn set_prop(&mut self, key: &str, value: serde_json::Value) {
        match key {
            "position" => {
                self.position = value.as_object().and_then(|position| {
                    Some((
                        position.get("x")?.as_f64()? as f32,
                        position.get("y")?.as_f64()? as f32,
                    ))
                });
            }
            "side" => self.side = value.as_str().map(Side::from_str).unwrap_or_default(),
            "align" => self.align = value.as_str().map(Alignment::from_str).unwrap_or_default(),
            "anchor" => self.anchor = value.as_str().and_then(AnchorPoint::from_str),
            "gap" => self.gap = value.as_f64().unwrap_or(0.0) as f32,
            "offset" => {
                self.offset = value
                    .as_object()
                    .map(|offset| {
                        (
                            offset
                                .get("x")
                                .and_then(|value| value.as_f64())
                                .unwrap_or(0.0) as f32,
                            offset
                                .get("y")
                                .and_then(|value| value.as_f64())
                                .unwrap_or(0.0) as f32,
                        )
                    })
                    .unwrap_or_default();
            }
            "fit" => self.fit = value.as_str().map(FitMode::from_str).unwrap_or_default(),
            "snapMargin" => self.snap_margin = value.as_f64().unwrap_or(8.0) as f32,
            "deferred" => self.deferred = value.as_bool().unwrap_or(true),
            "priority" => {
                self.priority = value
                    .as_u64()
                    .and_then(|priority| usize::try_from(priority).ok())
                    .unwrap_or(1);
            }
            "fill" => self.fill_window = value.as_str() == Some("window"),
            "constrainToAvailable" => {
                self.constrain_to_available = value.as_bool().unwrap_or(false)
            }
            _ => {}
        }
    }

    fn supported_props(&self) -> &'static [&'static str] {
        &[
            "position",
            "side",
            "align",
            "anchor",
            "gap",
            "offset",
            "fit",
            "snapMargin",
            "deferred",
            "priority",
            "fill",
            "constrainToAvailable",
        ]
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["placement"]
    }

    fn destroy(&mut self) {
        crate::layout::anchor_size::forget(self.id);
    }
}
