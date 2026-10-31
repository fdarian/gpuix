//! A `<nativeview>` in a window of the test platform, built and painted as the renderer does and
//! read from the frames the platform recorded. `GPUIX_RETAIN_VERIFY`'s check runs on every frame.
use std::sync::{Arc, Mutex};

use gpui::AppContext as _;

use super::test_api::{NativeviewSurfaceReading, NativeSurfaceEventReading};
use crate::element_tree::EventPayload;
use crate::renderer::{EventCallback, GpuixView};
use crate::retained_tree::RetainedTree;

struct El {
    id: u64,
    parent: Option<u64>,
    element_type: &'static str,
    style: String,
    props: Vec<(&'static str, serde_json::Value)>,
    events: Vec<&'static str>,
}

fn el(id: u64, parent: Option<u64>, element_type: &'static str, style: &str) -> El {
    El { id, parent, element_type, style: style.to_owned(), props: Vec::new(), events: Vec::new() }
}

impl El {
    fn prop(mut self, key: &'static str, value: serde_json::Value) -> Self {
        self.props.push((key, value));
        self
    }

    fn listening(mut self, event: &'static str) -> Self {
        self.events.push(event);
        self
    }
}

struct Page {
    tree: Arc<Mutex<RetainedTree>>,
    window: gpui::WindowHandle<GpuixView>,
    told: Arc<Mutex<Vec<EventPayload>>>,
}

impl Page {
    fn new(cx: &mut gpui::TestAppContext, elements: Vec<El>) -> Self {
        Self::with_setup(cx, elements, |_| {})
    }

    /// `setup` runs before the first frame.
    fn with_setup(cx: &mut gpui::TestAppContext, elements: Vec<El>, setup: impl FnOnce(&mut gpui::TestAppContext)) -> Self {
        cx.update(|cx| cx.set_global(super::SurfaceLog::default()));
        setup(cx);
        let tree = Arc::new(Mutex::new(RetainedTree::new()));
        {
            let mut tree = tree.lock().unwrap();
            for element in &elements {
                tree.create_element(element.id, element.element_type.to_owned());
                let style = tree.styles.intern(element.style.as_bytes()).unwrap();
                tree.set_style(element.id, style);
                for (key, value) in &element.props {
                    tree.set_custom_prop(element.id, (*key).to_owned(), value.clone());
                }
                for event in &element.events {
                    tree.set_event_listener(element.id, (*event).to_owned(), true);
                }
            }
            for element in &elements {
                if let Some(parent) = element.parent {
                    tree.append_child(parent, element.id);
                }
            }
            tree.set_root(elements[0].id);
        }
        let told = Arc::new(Mutex::new(Vec::new()));
        let sink = told.clone();
        let callback: EventCallback = Arc::new(move |payload| sink.lock().unwrap().push(payload));
        let view_tree = tree.clone();
        let window = cx.add_window(|window, _| {
            crate::events::enable_dom_hit_testing(window);
            crate::layout::container::allow_redraw_passes(window);
            let mut view = GpuixView::new(
                view_tree,
                Some(callback),
                "test".to_owned(),
                crate::text::SharedSelection::default(),
                crate::events::state::PointerRequests::default(),
            );
            view.clock.pause();
            view.retain.set_planning(true);
            view.retain.set_verify(true);
            view
        });
        let page = Self { tree, window, told };
        for _ in 0..4 {
            page.draw(cx);
        }
        page
    }

    /// A frame of a view that notified, then the check that nothing judged unchanged was built
    /// differently.
    fn draw(&self, cx: &mut gpui::TestAppContext) {
        self.window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.update_window(self.window.into(), |_, window, app| window.draw(app).clear(app)).unwrap();
        cx.run_until_parked();
        let mismatches = self.window.update(cx, |view, _, _| view.retain.mismatches().to_vec()).unwrap();
        assert!(mismatches.is_empty(), "{mismatches:?}");
        let replayed =
            cx.update_window(self.window.into(), |_, window, _| window.retention_mismatches().to_vec()).unwrap();
        assert!(replayed.is_empty(), "{replayed:?}");
    }

    fn settle(&self, cx: &mut gpui::TestAppContext) {
        for _ in 0..4 {
            self.draw(cx);
        }
    }

    fn set_style(&self, id: u64, style: &str) {
        let mut tree = self.tree.lock().unwrap();
        let style = tree.styles.intern(style.as_bytes()).unwrap();
        tree.set_style(id, style);
    }

    fn set_prop(&self, id: u64, key: &str, value: serde_json::Value) {
        self.tree.lock().unwrap().set_custom_prop(id, key.to_owned(), value);
    }

    fn listen(&self, id: u64, event: &str, on: bool) {
        self.tree.lock().unwrap().set_event_listener(id, event.to_owned(), on);
    }

    fn destroy(&self, id: u64) {
        self.tree.lock().unwrap().destroy_element(id);
    }

    /// What `applyBatch` does with a `destroyElement`: no frame is drawn.
    fn destroy_in_batch(&self, id: u64) {
        let mut tree = self.tree.lock().unwrap();
        let batch = format!(r#"[["destroyElement", {id}]]"#);
        let destroyed = crate::renderer::apply_batch_to_tree(&mut tree, batch.as_bytes()).unwrap();
        let tree_id = tree.id();
        drop(tree);
        super::teardown::tell_destroyed(tree_id, &destroyed).unwrap();
    }

    fn surface(&self, cx: &mut gpui::TestAppContext, id: u64) -> Option<NativeviewSurfaceReading> {
        self.window.update(cx, |view, window, _| view.nativeview_surface(id, window).unwrap()).unwrap()
    }

    fn surface_events(&self, cx: &mut gpui::TestAppContext) -> Vec<String> {
        let events: Vec<NativeSurfaceEventReading> =
            cx.update(|cx| super::test_api::native_surface_events(cx));
        events.into_iter().map(|event| format!("{} {}", event.kind, event.handle)).collect()
    }

    fn warnings(&self, cx: &mut gpui::TestAppContext, id: u64) -> usize {
        self.window.update(cx, |view, _, _| view.nativeview_warnings(id).unwrap()).unwrap()
    }

    /// The events of `kind` JS was told of, as `"<element> <handle>"`.
    fn told(&self, kind: &str) -> Vec<String> {
        self.told
            .lock()
            .unwrap()
            .iter()
            .filter(|payload| payload.event_type == kind)
            .map(|payload| format!("{} {}", payload.element_id, payload.native_handle.as_deref().unwrap_or("-")))
            .collect()
    }

    fn bounds(&self, id: u64) -> crate::automation::ElementBounds {
        crate::automation::get_bounds(self.window.window_id(), id).expect("painted")
    }

    fn layers(&self, cx: &mut gpui::TestAppContext) -> Vec<gpui::RecordedLayer> {
        cx.update_window(self.window.into(), |_, window, _| {
            window.present_cached_frame();
            window.composition_frame().expect("a composed frame").layers
        })
        .unwrap()
    }
}

const ROOT: &str = r#"{"width": 400, "height": 300}"#;
const BOX: &str = r##"{"width": 120, "height": 80, "borderWidth": 4, "borderStyle": "solid", "borderColor": "#000000", "padding": 6}"##;

fn nativeview_page(cx: &mut gpui::TestAppContext, nativeview: El) -> Page {
    Page::new(cx, vec![el(1, None, "div", ROOT), nativeview])
}

fn nativeview(style: &str) -> El {
    el(2, Some(1), "nativeview", style)
}

#[gpui::test]
fn the_native_view_fills_the_padding_box(cx: &mut gpui::TestAppContext) {
    let page = nativeview_page(cx, nativeview(BOX));
    let outer = page.bounds(2);
    let surface = page.surface(cx, 2).expect("a placed surface");
    assert_eq!(surface.handle, "0");
    assert_eq!(
        (surface.bounds.x, surface.bounds.y, surface.bounds.width, surface.bounds.height),
        (outer.x + 4., outer.y + 4., 112., 72.)
    );
    assert_eq!(page.surface_events(cx), ["created 0"]);
}

#[gpui::test]
fn a_nativeview_lays_out_as_a_div(cx: &mut gpui::TestAppContext) {
    let layouts = [
        // A flex row with a gap, and an item after it.
        (r#"{"display": "flex", "gap": 10, "width": 400, "height": 300}"#, BOX),
        // A grid with a gap: the canvas is no grid item.
        (r#"{"display": "grid", "gridTemplateColumns": "1fr 1fr", "gap": 7, "width": 400, "height": 300}"#, BOX),
        // Sized by the content, in a flex column.
        (r#"{"display": "flex", "flexDirection": "column", "gap": 3, "width": 400, "height": 300}"#, r#"{"padding": 5}"#),
        // Sized by the content, in a block.
        (ROOT, r#"{"padding": 5, "borderWidth": 2, "borderStyle": "solid"}"#),
    ];
    for (root, style) in layouts {
        let boxes = |cx: &mut gpui::TestAppContext, element_type: &'static str| {
            let page = Page::new(
                cx,
                vec![
                    el(1, None, "div", root),
                    el(2, Some(1), element_type, style),
                    el(3, Some(2), "div", r#"{"width": 40, "height": 30}"#),
                    el(4, Some(1), "div", r#"{"width": 50, "height": 50}"#),
                ],
            );
            [1, 2, 3, 4].map(|id| {
                let bounds = page.bounds(id);
                (bounds.x, bounds.y, bounds.width, bounds.height)
            })
        };
        let div = boxes(cx, "div");
        let native = boxes(cx, "nativeview");
        assert_eq!(native, div, "root {root}, style {style}");
    }
}

#[gpui::test]
fn a_nativeview_is_a_block_whatever_its_display_says_of_inline(cx: &mut gpui::TestAppContext) {
    let page = Page::new(
        cx,
        vec![
            el(1, None, "div", ROOT),
            nativeview(r#"{"display": "inline", "width": 50, "height": 20}"#),
            el(3, Some(2), "text", "{}"),
        ],
    );
    assert!(page.surface(cx, 2).is_some());
}

#[gpui::test]
fn the_view_is_between_the_background_of_the_element_and_its_children(cx: &mut gpui::TestAppContext) {
    let page = Page::new(
        cx,
        vec![
            el(1, None, "div", ROOT),
            nativeview(r##"{"width": 100, "height": 100, "backgroundColor": "#ff0000"}"##),
            el(3, Some(2), "div", r##"{"width": 40, "height": 40, "backgroundColor": "#00ff00"}"##),
        ],
    );
    let layers = page.layers(cx);
    let kinds: Vec<_> = layers
        .iter()
        .map(|layer| match layer {
            gpui::RecordedLayer::Gpui { primitives, .. } => format!("gpui {primitives}"),
            gpui::RecordedLayer::Native { .. } => "native".to_string(),
        })
        .collect();
    assert_eq!(kinds, ["gpui 1", "native", "gpui 1"]);
}

#[gpui::test]
fn a_later_sibling_is_above_the_view(cx: &mut gpui::TestAppContext) {
    let page = Page::new(
        cx,
        vec![
            el(1, None, "div", ROOT),
            nativeview(r#"{"width": 100, "height": 100}"#),
            el(3, Some(1), "div", r##"{"position": "absolute", "left": 10, "top": 10, "width": 30, "height": 30, "backgroundColor": "#0000ff"}"##),
        ],
    );
    let layers = page.layers(cx);
    let kinds: Vec<_> = layers
        .iter()
        .map(|layer| match layer {
            gpui::RecordedLayer::Gpui { primitives, .. } => format!("gpui {primitives}"),
            gpui::RecordedLayer::Native { .. } => "native".to_string(),
        })
        .collect();
    assert_eq!(kinds, ["gpui 0", "native", "gpui 1"]);
}

#[gpui::test]
fn the_view_is_not_cut_by_an_element_that_does_not_clip(cx: &mut gpui::TestAppContext) {
    let page = nativeview_page(
        cx,
        nativeview(r#"{"width": 100, "height": 60, "borderRadius": 12, "borderWidth": 2, "borderStyle": "solid"}"#),
    );
    let surface = page.surface(cx, 2).unwrap();
    assert_eq!(surface.corner_radius, 0.);
    // Only the content mask around the element cuts it, and that is the window here.
    assert!(surface.clip.width >= 400. && surface.clip.height >= 300., "{}x{}", surface.clip.width, surface.clip.height);
}

#[gpui::test]
fn an_element_that_clips_cuts_the_view_to_its_inner_radius(cx: &mut gpui::TestAppContext) {
    for overflow in [r#""overflow": "hidden""#, r#""overflow": "clip""#, r#""overflow": "scroll""#, r#""overflowX": "hidden""#, r#""overflowY": "auto""#] {
        let style = format!(
            r#"{{"width": 100, "height": 60, "borderRadius": 12, "borderWidth": 2, "borderStyle": "solid", {overflow}}}"#
        );
        let page = nativeview_page(cx, nativeview(&style));
        let surface = page.surface(cx, 2).unwrap();
        assert_eq!(surface.corner_radius, 10., "{overflow}");
        let outer = page.bounds(2);
        // The mask of the element is its padding box.
        assert_eq!(
            (surface.clip.x, surface.clip.y, surface.clip.width, surface.clip.height),
            (outer.x + 2., outer.y + 2., 96., 56.),
            "{overflow}"
        );
    }
}

#[gpui::test]
fn corners_that_are_not_one_circle_leave_the_view_square(cx: &mut gpui::TestAppContext) {
    let page = nativeview_page(
        cx,
        nativeview(r#"{"width": 100, "height": 60, "overflow": "hidden", "borderRadius": 6, "borderTopLeftRadius": 12}"#),
    );
    assert_eq!(page.surface(cx, 2).unwrap().corner_radius, 0.);
}

#[gpui::test]
fn the_pointer_is_taken_only_when_asked_for(cx: &mut gpui::TestAppContext) {
    let page = nativeview_page(cx, nativeview(BOX));
    assert!(!page.surface(cx, 2).unwrap().captures_pointer);
    page.set_prop(2, "nativeEvents", serde_json::json!(true));
    page.settle(cx);
    assert!(page.surface(cx, 2).unwrap().captures_pointer);
    page.set_prop(2, "nativeEvents", serde_json::json!(false));
    page.settle(cx);
    assert!(!page.surface(cx, 2).unwrap().captures_pointer);
}

#[gpui::test]
fn pointer_events_none_leaves_the_pointer_to_gpui(cx: &mut gpui::TestAppContext) {
    let page = Page::new(
        cx,
        vec![
            el(1, None, "div", r#"{"width": 400, "height": 300, "pointerEvents": "none"}"#),
            el(2, Some(1), "nativeview", BOX).prop("nativeEvents", serde_json::json!(true)),
            el(3, Some(1), "nativeview", BOX).prop("nativeEvents", serde_json::json!(true)),
        ],
    );
    assert!(!page.surface(cx, 2).unwrap().captures_pointer);
    page.set_style(3, r#"{"width": 120, "height": 80, "pointerEvents": "auto"}"#);
    page.settle(cx);
    assert!(page.surface(cx, 3).unwrap().captures_pointer);
    assert!(!page.surface(cx, 2).unwrap().captures_pointer);
}

#[gpui::test]
fn attach_is_told_once_with_the_content_view(cx: &mut gpui::TestAppContext) {
    let page = nativeview_page(cx, nativeview(BOX).listening("attach"));
    page.settle(cx);
    page.draw(cx);
    assert_eq!(page.told("attach"), ["2 0"]);
    assert!(page.told("detach").is_empty());
}

#[gpui::test]
fn attach_waits_for_a_listener_and_is_told_once(cx: &mut gpui::TestAppContext) {
    let page = nativeview_page(cx, nativeview(BOX));
    assert!(page.told("attach").is_empty());
    page.listen(2, "attach", true);
    page.settle(cx);
    page.listen(2, "detach", true);
    page.listen(2, "attach", false);
    page.listen(2, "attach", true);
    page.settle(cx);
    assert_eq!(page.told("attach"), ["2 0"]);
}

#[gpui::test]
fn the_surface_is_made_with_no_listener(cx: &mut gpui::TestAppContext) {
    let page = nativeview_page(cx, nativeview(BOX));
    assert_eq!(page.surface_events(cx), ["created 0"]);
}

#[gpui::test]
fn destroying_the_element_tells_detach_and_removes_the_surface(cx: &mut gpui::TestAppContext) {
    let page = nativeview_page(cx, nativeview(BOX).listening("attach").listening("detach"));
    page.destroy(2);
    page.settle(cx);
    assert_eq!(page.told("attach"), ["2 0"]);
    assert_eq!(page.told("detach"), ["2 0"]);
    assert_eq!(page.surface_events(cx), ["created 0", "removed 0"]);
}

#[gpui::test]
fn destroying_an_element_that_was_never_attached_tells_nothing(cx: &mut gpui::TestAppContext) {
    let page = nativeview_page(cx, nativeview(BOX));
    page.destroy(2);
    page.settle(cx);
    assert!(page.told("attach").is_empty());
    assert!(page.told("detach").is_empty());
    assert_eq!(page.surface_events(cx), ["created 0", "removed 0"]);
}

#[gpui::test]
fn a_window_that_closes_with_the_element_mounted_tells_detach_once(cx: &mut gpui::TestAppContext) {
    let page = nativeview_page(cx, nativeview(BOX).listening("attach").listening("detach"));
    cx.update_window(page.window.into(), |_, window, _| window.remove_window()).unwrap();
    cx.run_until_parked();
    assert_eq!(page.told("attach"), ["2 0"]);
    assert_eq!(page.told("detach"), ["2 0"]);
}

#[gpui::test]
fn the_elements_in_a_destroyed_subtree_each_tell_detach(cx: &mut gpui::TestAppContext) {
    let page = Page::new(
        cx,
        vec![
            el(1, None, "div", ROOT),
            el(2, Some(1), "div", "{}"),
            el(3, Some(2), "nativeview", BOX).listening("attach").listening("detach"),
            el(4, Some(2), "nativeview", BOX).listening("attach").listening("detach"),
        ],
    );
    page.destroy(2);
    page.settle(cx);
    let mut detached = page.told("detach");
    detached.sort();
    assert_eq!(detached, ["3 0", "4 1"]);
}

#[gpui::test]
fn hiding_the_element_hides_the_view_and_does_not_detach(cx: &mut gpui::TestAppContext) {
    let hidden = [
        r#"{"width": 100, "height": 60, "display": "none"}"#,
        r#"{"width": 100, "height": 60, "visibility": "hidden"}"#,
        r#"{"width": 100, "height": 60, "contentVisibility": "hidden"}"#,
    ];
    for style in hidden {
        let page = nativeview_page(cx, nativeview(r#"{"width": 100, "height": 60}"#).listening("attach").listening("detach"));
        let before = page.surface(cx, 2).unwrap();
        page.set_style(2, style);
        page.settle(cx);
        assert!(page.surface(cx, 2).is_none(), "{style}");
        page.set_style(2, r#"{"width": 100, "height": 60}"#);
        page.settle(cx);
        let after = page.surface(cx, 2).unwrap();
        assert_eq!(after.handle, before.handle, "{style}");
        assert_eq!(page.told("attach"), ["2 0"], "{style}");
        assert!(page.told("detach").is_empty(), "{style}");
        assert_eq!(page.surface_events(cx), ["created 0"], "{style}");
    }
}

#[gpui::test]
fn an_element_hidden_from_the_start_makes_its_surface_when_it_shows(cx: &mut gpui::TestAppContext) {
    let page = nativeview_page(cx, nativeview(r#"{"width": 100, "height": 60, "display": "none"}"#).listening("attach"));
    assert!(page.surface_events(cx).is_empty());
    assert!(page.told("attach").is_empty());
    page.set_style(2, r#"{"width": 100, "height": 60}"#);
    page.settle(cx);
    assert_eq!(page.told("attach"), ["2 0"]);
    assert!(page.surface(cx, 2).is_some());
}

#[gpui::test]
fn moving_the_element_among_its_siblings_keeps_its_surface(cx: &mut gpui::TestAppContext) {
    let page = Page::new(
        cx,
        vec![
            el(1, None, "div", ROOT),
            el(2, Some(1), "nativeview", r#"{"width": 100, "height": 60}"#).listening("attach").listening("detach"),
            el(3, Some(1), "div", r#"{"width": 100, "height": 60}"#),
        ],
    );
    page.tree.lock().unwrap().insert_before(1, 3, 2);
    page.settle(cx);
    page.tree.lock().unwrap().insert_before(1, 2, 3);
    page.settle(cx);
    assert_eq!(page.told("attach"), ["2 0"]);
    assert!(page.told("detach").is_empty());
    assert_eq!(page.surface_events(cx), ["created 0"]);
    assert!(page.surface(cx, 2).is_some());
}

#[gpui::test]
fn a_platform_that_cannot_compose_leaves_an_empty_box_and_warns_once(cx: &mut gpui::TestAppContext) {
    let page = Page::with_setup(
        cx,
        vec![
            el(1, None, "div", ROOT),
            el(2, Some(1), "nativeview", r#"{"width": 100, "height": 60, "padding": 4}"#).listening("attach"),
            el(3, Some(2), "div", r#"{"width": 40, "height": 30}"#),
        ],
        |cx| cx.update(|cx| super::test_api::set_surfaces_available(cx, false)),
    );
    for _ in 0..5 {
        page.draw(cx);
    }
    assert_eq!(page.warnings(cx, 2), 1);
    assert!(page.surface(cx, 2).is_none());
    assert!(page.surface_events(cx).is_empty());
    assert!(page.told("attach").is_empty());
    assert_eq!(page.bounds(2).width, 100.);
    assert_eq!((page.bounds(3).width, page.bounds(3).height), (40., 30.));
    // Switching it back does not make it try again.
    cx.update(|cx| super::test_api::set_surfaces_available(cx, true));
    page.settle(cx);
    assert_eq!(page.warnings(cx, 2), 1);
    assert!(page.surface(cx, 2).is_none());
}

#[gpui::test]
fn an_element_that_cannot_attach_tells_nothing_on_destroy(cx: &mut gpui::TestAppContext) {
    let page = Page::with_setup(
        cx,
        vec![el(1, None, "div", ROOT), nativeview(BOX).listening("attach").listening("detach")],
        |cx| cx.update(|cx| super::test_api::set_surfaces_available(cx, false)),
    );
    page.destroy(2);
    page.settle(cx);
    assert!(page.told("detach").is_empty());
}

#[gpui::test]
fn the_box_is_hit_and_clicked_as_a_div(cx: &mut gpui::TestAppContext) {
    let page = Page::new(
        cx,
        vec![
            el(1, None, "div", ROOT),
            el(2, Some(1), "nativeview", r#"{"width": 100, "height": 100}"#).listening("click"),
            el(3, Some(2), "div", r#"{"width": 20, "height": 20}"#).listening("click"),
        ],
    );
    let bounds = page.bounds(2);
    let mut visual = gpui::VisualTestContext::from_window(page.window.into(), cx);
    visual.simulate_click(
        gpui::point(gpui::px((bounds.x + 60.) as f32), gpui::px((bounds.y + 60.) as f32)),
        Default::default(),
    );
    visual.simulate_click(
        gpui::point(gpui::px((bounds.x + 5.) as f32), gpui::px((bounds.y + 5.) as f32)),
        Default::default(),
    );
    let clicked: Vec<f64> = page
        .told
        .lock()
        .unwrap()
        .iter()
        .filter(|payload| payload.event_type == "click")
        .map(|payload| payload.element_id)
        .collect();
    assert_eq!(clicked, [2.0, 3.0]);
}

#[gpui::test]
fn the_layer_takes_no_row_of_a_flex_container(cx: &mut gpui::TestAppContext) {
    let page = Page::new(
        cx,
        vec![
            el(1, None, "div", ROOT),
            nativeview(r#"{"width": 100, "height": 100, "display": "flex", "flexDirection": "column", "gap": 6}"#),
            el(3, Some(2), "div", r#"{"width": 40, "height": 20}"#),
            el(4, Some(2), "div", r#"{"width": 40, "height": 20, "order": -1}"#),
        ],
    );
    assert_eq!((page.bounds(4).y, page.bounds(3).y), (page.bounds(2).y, page.bounds(2).y + 26.));
    assert!(page.surface(cx, 2).is_some());
}

#[gpui::test]
fn a_nativeview_is_retained_as_a_div_is(cx: &mut gpui::TestAppContext) {
    // The elements the frames build while only a sibling changes, and what the replays that were
    // compared with a build found.
    let mut built = Vec::new();
    for kind in ["div", "nativeview"] {
        let mut page_elements = vec![
            el(1, None, "div", ROOT),
            el(2, Some(1), kind, r#"{"width": 100, "height": 60, "overflow": "hidden", "borderRadius": 8}"#)
                .prop("nativeEvents", serde_json::json!(true)),
            el(3, Some(2), "div", r#"{"width": 20, "height": 20}"#),
            el(4, Some(1), "div", r#"{"width": 20, "height": 20}"#),
        ];
        if kind == "nativeview" {
            page_elements[1] = std::mem::replace(&mut page_elements[1], el(0, None, "div", "{}")).listening("attach");
        }
        let page = Page::new(cx, page_elements);
        let counters = |cx: &mut gpui::TestAppContext| {
            cx.update_window(page.window.into(), |_, window, _| window.retained_counters()).unwrap()
        };
        page.settle(cx);
        let before = counters(cx);
        let elements_before = crate::perf_counters::snapshot().elements_built;
        let placed = (kind == "nativeview").then(|| page.surface(cx, 2).unwrap());
        for width in [21, 22, 23] {
            page.set_style(4, &format!(r#"{{"width": {width}, "height": 20}}"#));
            page.draw(cx);
        }
        let after = counters(cx);
        assert!(after.verified > before.verified, "{kind}: {after:?}");
        assert_eq!(after.mismatches, 0);
        built.push(crate::perf_counters::snapshot().elements_built - elements_before);
        if let Some(placed) = placed {
            let replayed = page.surface(cx, 2).unwrap();
            assert_eq!(
                (placed.bounds.x, placed.bounds.width, placed.corner_radius, placed.captures_pointer),
                (replayed.bounds.x, replayed.bounds.width, replayed.corner_radius, replayed.captures_pointer)
            );
            assert_eq!(page.told("attach"), ["2 0"]);
            assert!(page.surface(cx, 2).is_some());
        }
    }
    assert_eq!(built[0], built[1], "a nativeview is built as often as a div: {built:?}");
}

#[gpui::test]
fn a_batch_that_destroys_the_element_tells_detach_before_any_frame(cx: &mut gpui::TestAppContext) {
    let page = nativeview_page(cx, nativeview(BOX).listening("attach").listening("detach"));
    page.destroy_in_batch(2);
    // Nothing was drawn since: it is the batch that told it, while JS still has its handlers.
    assert_eq!(page.told("detach"), ["2 0"]);
    page.settle(cx);
    assert_eq!(page.told("detach"), ["2 0"], "the frame that finds it gone tells nothing more");
    assert_eq!(page.surface_events(cx), ["created 0", "removed 0"]);
}

#[gpui::test]
fn a_batch_that_destroys_a_subtree_tells_detach_for_each_nativeview_in_it(cx: &mut gpui::TestAppContext) {
    let page = Page::new(
        cx,
        vec![
            el(1, None, "div", ROOT),
            el(2, Some(1), "div", "{}"),
            el(3, Some(2), "nativeview", BOX).listening("attach").listening("detach"),
            el(4, Some(3), "nativeview", BOX).listening("attach").listening("detach"),
            el(5, Some(1), "nativeview", BOX).listening("attach").listening("detach"),
        ],
    );
    page.destroy_in_batch(2);
    let mut detached = page.told("detach");
    detached.sort();
    assert_eq!(detached.len(), 2, "{detached:?}");
    assert!(detached[0].starts_with("3 ") && detached[1].starts_with("4 "), "{detached:?}");
    page.settle(cx);
    assert_eq!(page.told("detach").len(), 2);
    let removed = page.surface_events(cx).iter().filter(|event| event.starts_with("removed")).count();
    assert_eq!(removed, 2);
    assert!(page.surface(cx, 5).is_some());
}

#[gpui::test]
fn a_batch_that_destroys_something_else_leaves_the_nativeview(cx: &mut gpui::TestAppContext) {
    let page = Page::new(
        cx,
        vec![el(1, None, "div", ROOT), nativeview(BOX).listening("attach").listening("detach"), el(3, Some(1), "div", "{}")],
    );
    page.destroy_in_batch(3);
    page.settle(cx);
    assert!(page.told("detach").is_empty());
    assert!(page.surface(cx, 2).is_some());
}

#[gpui::test]
fn a_box_under_pointer_events_none_is_not_a_target_as_a_div_is_not(cx: &mut gpui::TestAppContext) {
    // A click on the box alone reaches nothing, and one on a child that is a target reaches the
    // child: the same as for a div.
    let mut clicks = Vec::new();
    for kind in ["div", "nativeview"] {
        let page = Page::new(
            cx,
            vec![
                el(1, None, "div", ROOT),
                el(2, Some(1), kind, r#"{"width": 100, "height": 100, "pointerEvents": "none"}"#)
                    .prop("nativeEvents", serde_json::json!(true))
                    .listening("click"),
                el(3, Some(2), "div", r#"{"width": 20, "height": 20, "pointerEvents": "auto"}"#).listening("click"),
            ],
        );
        let bounds = page.bounds(2);
        let mut visual = gpui::VisualTestContext::from_window(page.window.into(), cx);
        for offset in [60., 5.] {
            visual.simulate_click(
                gpui::point(gpui::px((bounds.x + offset) as f32), gpui::px((bounds.y + offset) as f32)),
                Default::default(),
            );
        }
        let clicked: Vec<f64> = page
            .told
            .lock()
            .unwrap()
            .iter()
            .filter(|payload| payload.event_type == "click")
            .map(|payload| payload.element_id)
            .collect();
        clicks.push(clicked);
    }
    assert_eq!(clicks[1], [3.0]);
    assert_eq!(clicks[1], clicks[0]);
}

#[gpui::test]
fn the_pointer_is_over_the_native_view_only_where_it_captures(cx: &mut gpui::TestAppContext) {
    let page = Page::new(
        cx,
        vec![
            el(1, None, "div", ROOT),
            el(2, Some(1), "nativeview", r#"{"width": 100, "height": 100}"#).prop("nativeEvents", serde_json::json!(true)),
            el(3, Some(1), "nativeview", r#"{"width": 100, "height": 100}"#),
        ],
    );
    let over = |cx: &mut gpui::TestAppContext, id: u64| {
        let bounds = page.bounds(id);
        let point = gpui::point(gpui::px((bounds.x + 50.) as f32), gpui::px((bounds.y + 50.) as f32));
        cx.update_window(page.window.into(), |_, window, _| window.native_surface_at(point).is_some()).unwrap()
    };
    assert!(over(cx, 2));
    assert!(!over(cx, 3));
}
