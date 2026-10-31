//! A `HostBox` keeps a layout node id across frames, and the layout engine gives its nodes other ids
//! when it moves them to a smaller store: an id from before can then be another node's.
use gpui::{div, px, AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Window};

use super::{track_host_bounds, HostPlace};

const HOSTS: u64 = 10;

/// Hosts between a run of leaf nodes before them and one after them. Nodes are created in the order
/// of the children, so the hosts sit in the low slots of the engine's store, and the leaves before
/// them leave holes that the move closes: the id of a host is then the id of another host. The
/// leaves are out of flow, so the hosts do not move when they go.
struct Page {
    leading: usize,
    trailing: usize,
    first_host_width: f32,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let leaf = |name: &'static str, index: usize| div().id((name, index)).absolute().w(px(4.0)).h(px(1.0));
        div()
            .flex()
            .flex_col()
            .items_start()
            .w(px(400.0))
            .h(px(300.0))
            .children((0..self.leading).map(|index| leaf("leading", index)))
            .children((0..HOSTS).map(|host| {
                let width = if host == 0 { self.first_host_width } else { 100.0 };
                track_host_bounds(div().id(("host", host as usize)).w(px(width)).h(px(10.0)), host, None, false, false, None)
            }))
            .children((0..self.trailing).map(|index| leaf("trailing", index)))
    }
}

fn draw(cx: &mut gpui::TestAppContext, window: gpui::WindowHandle<Page>) {
    window.update(cx, |_, _, cx| cx.notify()).unwrap();
    cx.update_window(window.into(), |_, window, app| window.draw(app).clear(app)).unwrap();
}

#[gpui::test]
fn a_place_taken_before_the_layout_engine_moved_its_nodes_is_not_taken_for_unmoved_after(cx: &mut gpui::TestAppContext) {
    let window = cx.add_window(|_, _| Page { leading: 2, trailing: 6_000, first_host_width: 100.0 });
    for _ in 0..3 {
        draw(cx, window);
    }
    let (place, engine) = cx.update_window(window.into(), |_, window, _| (HostPlace::of(window, 0), window.layout_engine_id())).unwrap();

    // The first host gets wider, which moves its own box and no other.
    window
        .update(cx, |page, _, _| {
            page.leading = 0;
            page.trailing = 0;
            page.first_host_width = 130.0;
        })
        .unwrap();
    let mut moved = false;
    for _ in 0..6 {
        draw(cx, window);
        moved = cx.update_window(window.into(), |_, window, _| window.layout_engine_id() != engine).unwrap();
        if moved {
            break;
        }
    }
    assert!(moved, "the engine moved its nodes to a store of the size of the small page");

    let unmoved = cx.update_window(window.into(), |_, window, _| place.unmoved(window, true)).unwrap();
    assert!(!unmoved, "the host's box changed, and the id the place kept names another node now");
}
