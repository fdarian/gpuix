//! `content-visibility` (CSS Containment 2): `hidden` skips the content of an element, `auto`
//! skips it while the element is off screen.
//!
//! An element with a value other than `visible` has style, layout and paint containment. In this
//! renderer that is a style: [`contained`] gives the element the `overflow: clip` that clips its
//! content at the padding box (unless the author's `overflow` already clips), makes it the
//! containing block of its `position: absolute` descendants and a stacking context. A skipped
//! element also has size containment, sized by `contain-intrinsic-size`, and builds none of its
//! children: their DOM, animation and transition state and resolved styles are kept, they are not
//! destroyed as under `display: none`.
//!
//! Only the host elements (`div`, `span`, `text`, `label`, `form`) skip their content. Any other
//! element has the containment and builds as it did.
//!
//! `auto` skips while the element is not relevant to the user: near the viewport (its border box,
//! outset by 150% of the viewport on each side, clipped by what clips it and by the viewport,
//! still shows something) or holding the focus. [`Relevance`] keeps what is known of each such
//! element. The first answer comes in the frame that builds the element, from its prepaint, which
//! asks for the frame to be built again when the element is relevant; later answers come from the
//! boxes a frame painted ([`Relevance::sweep`]) and take effect in the next frame.
//!
//! A read of geometry inside a skipped subtree forces it, as Chrome does
//! (`DisplayLockUtilities::ScopedForcedUpdate`): [`Relevance::force`] makes [`skips`] answer no for
//! the skipped ancestors of the element read, so a layout-only pass builds and lays out what is
//! below them and publishes its boxes, and [`Relevance::unforce`] makes the next frame skip them
//! again. Nothing the forced pass builds is remembered, answered for or told.
//!
//! Layout containment is the `contain_layout` flag zed's style has (set by `layout::style::apply`
//! on every box with a value other than `visible`): the box is an independent formatting context,
//! so no margin of a child collapses through its top or bottom edge, and it is the containing block
//! of its `position: fixed` descendants as well as of the absolute ones ([`containing_block`]). A
//! non-atomic inline has neither.
//!
//! [`containing_block`]: crate::layout::containing_block

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use gpui::{Bounds, Pixels};
use rustc_hash::FxHashMap;
use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::css::length::LengthContext;
use crate::layout::containing_block::Position;
use crate::retained_tree::{RetainedElement, RetainedTree};
use crate::scroll::{Axes, Overflow};
use crate::style::StyleDesc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentVisibility {
    #[default]
    Visible,
    /// Skipped while the element is not relevant to the user.
    Auto,
    /// Always skipped.
    Hidden,
}

impl ContentVisibility {
    fn keyword(self) -> &'static str {
        match self {
            Self::Visible => "visible",
            Self::Auto => "auto",
            Self::Hidden => "hidden",
        }
    }

    pub(crate) fn contains(self) -> bool {
        self != Self::Visible
    }
}

impl<'de> Deserialize<'de> for ContentVisibility {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        match text.as_str() {
            "visible" => Ok(Self::Visible),
            "auto" => Ok(Self::Auto),
            "hidden" => Ok(Self::Hidden),
            _ => Err(de::Error::custom(format!(
                "invalid contentVisibility \"{text}\": expected \"visible\", \"auto\" or \"hidden\""
            ))),
        }
    }
}

impl Serialize for ContentVisibility {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.keyword())
    }
}

/// The style `style` has with the containment its `content-visibility` brings, or `None` when it
/// has none. Idempotent: the result is its own containment.
pub(crate) fn contained(style: &StyleDesc) -> Option<StyleDesc> {
    if !style.content_visibility.is_some_and(ContentVisibility::contains) {
        return None;
    }
    let mut contained = style.clone();
    // Paint containment clips at the padding box on both axes. A scroller or an `overflow: hidden`
    // already does, and `clip` beside `visible` becomes `clip` on both.
    let declared = Axes::declared(style);
    if !declared.x.has_offset() && !declared.y.has_offset() {
        let clip = |overflow| (overflow == Overflow::Visible).then(|| "clip".to_owned());
        if let Some(clip) = clip(declared.x) {
            contained.overflow_x = Some(clip);
        }
        if let Some(clip) = clip(declared.y) {
            contained.overflow_y = Some(clip);
        }
    }
    // Layout containment makes it the containing block of its absolute descendants, as a
    // positioned element is. A static one is laid out as `relative` with no offset: the insets
    // of a static element do nothing. Its `z-index` does nothing either, except for an item.
    if Position::parse(style.position.as_deref()) == Position::Static {
        contained.position = Some("relative".to_owned());
        contained.top = None;
        contained.right = None;
        contained.bottom = None;
        contained.left = None;
        contained.inset = None;
        contained.inset_block = None;
        contained.inset_inline = None;
        if !style.flex_or_grid_item {
            contained.z_index = None;
        }
    }
    Some(contained)
}

/// Whether an element whose `content-visibility` is `value` skips its content in the build about
/// to be made. The one place that decides, so that the build and the walks over the tree cannot
/// disagree. A forced element (see [`Relevance::force`]) builds its content though it is skipped.
pub(crate) fn skips(relevance: &Relevance, value: ContentVisibility, id: u64) -> bool {
    contains_size(relevance, value, id) && !relevance.is_forced(id)
}

/// Whether the box of an element whose `content-visibility` is `value` has size containment in
/// the build about to be made: it is skipped, or would be but for being forced, whose content is
/// laid out in the box the skipped element has.
pub(crate) fn contains_size(relevance: &Relevance, value: ContentVisibility, id: u64) -> bool {
    match value {
        ContentVisibility::Hidden => true,
        ContentVisibility::Auto => relevance.is_skipped(id),
        ContentVisibility::Visible => false,
    }
}

/// [`skips`] as the last build of the element did it, which is what the walks over the tree that
/// say what is built read.
fn skipped_as_built(relevance: &Relevance, value: ContentVisibility, id: u64) -> bool {
    match value {
        ContentVisibility::Hidden => true,
        ContentVisibility::Auto => relevance.was_skipped(id),
        ContentVisibility::Visible => false,
    }
}

/// Gives the box of a skipped element its size: size containment on both axes, and the content
/// box `contain-intrinsic-size` says. An `auto` element has `auto` on both axes whatever its
/// `contain-intrinsic-size` says.
pub(crate) fn contain_skipped<E: gpui::Styled>(
    element: E,
    id: u64,
    style: &StyleDesc,
    lengths: &LengthContext,
    relevance: &Relevance,
) -> E {
    let mut element = crate::layout::container::contain_size(element, crate::layout::container::ContainerType::Size);
    // The automatic minimum size of a flex or grid item that is size contained is nothing, not the
    // size it is given in place of its content: Chrome shrinks a skipped item down to its padding.
    let minimum = &mut element.style().min_size;
    if matches!(minimum.width, None | Some(gpui::Length::Auto)) {
        minimum.width = Some(gpui::px(0.0).into());
    }
    if matches!(minimum.height, None | Some(gpui::Length::Auto)) {
        minimum.height = Some(gpui::px(0.0).into());
    }
    let mut axes = crate::css::contain_intrinsic::axes(style);
    if style.content_visibility == Some(ContentVisibility::Auto) {
        axes.width.auto = true;
        axes.height.auto = true;
    }
    let remembered = relevance.remembered(id);
    let size = |px: f64| gpui::px(px as f32);
    element.style().contain_intrinsic_width =
        axes.width.skipped_size(remembered.map(|size| size.0), lengths).map(size);
    element.style().contain_intrinsic_height =
        axes.height.skipped_size(remembered.map(|size| size.1), lengths).map(size);
    element
}

/// Whether the size an element has when it is not skipped is to be remembered: it is `auto` or
/// its `contain-intrinsic-size` has `auto` on an axis.
pub(crate) fn remembers_size(style: &StyleDesc) -> bool {
    style.content_visibility == Some(ContentVisibility::Auto) || {
        let axes = crate::css::contain_intrinsic::axes(style);
        axes.width.auto || axes.height.auto
    }
}

/// How far past the viewport an element is still near it, as a fraction of the viewport's side.
const NEAR: f32 = 1.5;

/// How far past the viewport the box of an element can be and be near, the same on every side:
/// the largest of the margins, so that a subtree this far from what shows is certainly not near.
pub(crate) fn proximity_reach(viewport_width: f64, viewport_height: f64) -> f32 {
    NEAR * viewport_width.max(viewport_height) as f32
}

/// Asks, when the element is prepainted, whether an `auto` element that has not been asked is
/// near, and remembers the content box of one that is laid out without being skipped. An element in
/// a culled subtree is not near, see [`proximity_reach`].
///
/// `skipped_matches` is the search that has matches in the content a skipped element does not
/// build, and where the element is in the document: it announces them, see
/// [`crate::text::search::announce_skipped`].
pub(crate) fn watch<E: gpui::InteractiveElement>(
    element: E,
    id: u64,
    relevance: Relevance,
    skipped_matches: Option<(Arc<crate::text::HighlightContext>, crate::text::selection::DocPos)>,
) -> E {
    element.on_prepainted_layout(move |bounds, layout_id, window, _| {
        if let Some((highlight, at)) = &skipped_matches {
            if window.visibility() != gpui::Visibility::Hidden {
                crate::text::search::announce_skipped(window.window_handle().window_id(), highlight, id, at);
            }
        }
        if relevance.wants_answer(id) {
            let near = !window.is_culling()
                && is_near(bounds, [window.content_mask().bounds], window.viewport_size());
            if relevance.answer(id, near) {
                window.request_redraw_before_present();
            }
        }
        let content = crate::layout::container::content_box(bounds, layout_id, window);
        relevance.remember(id, content.width, content.height);
    })
}

/// Whether the border box `border_box`, outset by [`NEAR`] of the viewport on each side and cut
/// by `clips` (the boxes of what clips it) and by the viewport, has an area left. An element at
/// the very edge of the margin is not near, as it is not in Chrome.
pub(crate) fn is_near(
    border_box: Bounds<Pixels>,
    clips: impl IntoIterator<Item = Bounds<Pixels>>,
    viewport: gpui::Size<Pixels>,
) -> bool {
    let (width, height) = (f32::from(viewport.width), f32::from(viewport.height));
    let (margin_x, margin_y) = (NEAR * width, NEAR * height);
    let edges = |bounds: Bounds<Pixels>| {
        let (left, top) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        (left, top, left + f32::from(bounds.size.width), top + f32::from(bounds.size.height))
    };
    let (mut left, mut top, mut right, mut bottom) = edges(border_box);
    left -= margin_x;
    right += margin_x;
    top -= margin_y;
    bottom += margin_y;
    let viewport_edges = (0.0, 0.0, width, height);
    for (clip_left, clip_top, clip_right, clip_bottom) in clips.into_iter().map(edges).chain([viewport_edges]) {
        left = left.max(clip_left);
        top = top.max(clip_top);
        right = right.min(clip_right);
        bottom = bottom.min(clip_bottom);
    }
    left < right && top < bottom
}

/// What is known of the elements that skip their content depending on where they are, and of the
/// size elements remember for `contain-intrinsic-size: auto`. Shared by the tree, which answers the
/// walks over it, the build, which asks it and tells it what it built, and the prepaint of the
/// elements, which answer whether they are near.
#[derive(Clone, Default)]
pub(crate) struct Relevance(Arc<Shared>);

#[derive(Default)]
struct Shared {
    /// How many elements `known` has, readable without the lock: almost every element of a page
    /// asks and has nothing to ask about.
    count: AtomicUsize,
    /// How many elements are forced, for the same reason.
    forced: AtomicUsize,
    known: Mutex<Known>,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Known> {
        self.known.lock().unwrap()
    }

    fn counted(&self, known: &Known) {
        self.count.store(known.elements.len(), Ordering::Relaxed);
    }
}

#[derive(Default)]
struct Known {
    elements: FxHashMap<u64, Element>,
    /// The elements a read of geometry below them lays out the content of, see [`Relevance::force`].
    forced: Vec<u64>,
    /// The last forced layout pass of a read, while nothing has happened to the window since.
    forced_read: Option<ForcedRead>,
    /// The focused element and its ancestors, which hold the focus.
    focus_within: Arc<[u64]>,
    /// The `auto` elements around the active match of a text search, which find-in-page activates.
    found: Arc<[u64]>,
    clips: Option<Clips>,
}

/// A layout pass that a read forced the content of `forced` for, and the window's state right
/// after it, trailing invalidation included.
struct ForcedRead {
    forced: Vec<u64>,
    stamp: gpui::LayoutStamp,
}

#[derive(Default)]
struct Element {
    /// Whether the element is `content-visibility: auto`.
    auto: bool,
    /// Whether the element is near the viewport as far as the last answer knows, or `None` while
    /// nothing has answered. An element nothing has answered for is skipped.
    near: Option<bool>,
    /// What the last build did with the content of the element.
    skipped_in_build: bool,
    /// The state the last `contentvisibilityautostatechange` told, or `None` before the first.
    notified: Option<bool>,
    /// The content box the element had when it was last laid out without being skipped.
    remembered: Option<(f64, f64)>,
    /// A frame was asked for because the state it wants is not the one it was built with.
    rebuild_requested: bool,
}

impl Element {
    fn wanted_skipped(&self, known: &Holds, id: u64) -> bool {
        !(self.near == Some(true) || known.focus_within.contains(&id) || known.found.contains(&id))
    }
}

/// What keeps an `auto` element from skipping its content besides being near: the focus within it
/// and a match of a search in it that is active.
struct Holds<'a> {
    focus_within: &'a [u64],
    found: &'a [u64],
}

impl Known {
    fn holds(&self) -> Holds<'_> {
        Holds { focus_within: &self.focus_within, found: &self.found }
    }
}

/// The ancestors that clip each element, for one revision of one tree.
struct Clips {
    stamp: (u64, u64),
    ancestors: FxHashMap<u64, Vec<crate::observers::ClippingAncestor>>,
}

/// What a sweep over the painted boxes found.
#[derive(Default)]
pub(crate) struct Swept {
    /// A frame has to be built, because the content of an element is to be skipped or shown.
    pub rebuild: bool,
    /// The elements whose state is not the one last told, with the state, in no order.
    pub changes: Vec<(u64, bool)>,
}

impl Relevance {
    pub(crate) fn is_empty(&self) -> bool {
        self.0.count.load(Ordering::Relaxed) == 0
    }

    /// The build of `id` is about to decide, for an `auto` element: whether it skips.
    pub(crate) fn is_skipped(&self, id: u64) -> bool {
        let known = self.0.lock();
        known.elements.get(&id).is_none_or(|element| element.wanted_skipped(&known.holds(), id))
    }

    /// Whether the last build of `id` skipped its content. An element never built is skipped.
    pub(crate) fn was_skipped(&self, id: u64) -> bool {
        self.0.lock().elements.get(&id).is_none_or(|element| element.skipped_in_build)
    }

    pub(crate) fn remembered(&self, id: u64) -> Option<(f64, f64)> {
        self.0.lock().elements.get(&id)?.remembered
    }

    /// What the state of an `auto` element adds to the key of the element: whether it is to skip
    /// when it is built, which builds it again when that changes. `declared` is whether its style
    /// says `auto`, for an element no build has seen yet.
    pub(crate) fn key(&self, id: u64, declared: bool) -> Option<u64> {
        if self.is_forced(id) {
            return Some(8);
        }
        if !declared && self.is_empty() {
            return None;
        }
        let known = self.0.lock();
        let skipped = match known.elements.get(&id) {
            Some(element) if element.auto => element.wanted_skipped(&known.holds(), id),
            _ if declared => true,
            _ => return None,
        };
        Some(4 | u64::from(skipped) << 1)
    }

    /// The build of `id` has decided: `skipped` is what it does with the content. `auto` and
    /// `remembers` are what its style says. An element that is neither is forgotten.
    pub(crate) fn built(&self, id: u64, auto: bool, remembers: bool, skipped: bool) {
        // A forced build is not a build the frames see: what the state says is what they built.
        if self.is_forced(id) || (!auto && !remembers && self.is_empty()) {
            return;
        }
        let mut known = self.0.lock();
        if !auto && !remembers {
            known.elements.remove(&id);
            self.0.counted(&known);
            return;
        }
        let element = known.elements.entry(id).or_default();
        if element.auto != auto {
            element.near = None;
            element.notified = None;
            element.rebuild_requested = false;
        }
        element.auto = auto;
        element.skipped_in_build = skipped;
        element.rebuild_requested = false;
        self.0.counted(&known);
    }

    /// Whether prepaint is to answer for `id`: nothing has. Later answers are the sweep's, which
    /// reads the layout the frame ended with: an answer from a pass that is built again would
    /// chase the layout the passes change as the entries are laid out, a pass at a time.
    pub(crate) fn wants_answer(&self, id: u64) -> bool {
        !self.is_forced(id) && self.0.lock().elements.get(&id).is_some_and(|element| element.auto && element.near.is_none())
    }

    /// Prepaint found out whether `id` is near. True if the build has to be made again because it
    /// skipped an element that is near.
    pub(crate) fn answer(&self, id: u64, near: bool) -> bool {
        let mut known = self.0.lock();
        let (focus_within, found) = (known.focus_within.clone(), known.found.clone());
        let Some(element) = known.elements.get_mut(&id).filter(|element| element.auto) else {
            return false;
        };
        element.near = Some(near);
        element.wanted_skipped(&Holds { focus_within: &focus_within, found: &found }, id) != element.skipped_in_build
    }

    /// `id` was laid out without being skipped, with this content box.
    pub(crate) fn remember(&self, id: u64, width: f64, height: f64) {
        if self.is_forced(id) {
            return;
        }
        if let Some(element) = self.0.lock().elements.get_mut(&id) {
            if !element.skipped_in_build {
                element.remembered = Some((width, height));
            }
        }
    }

    /// The ids that hold the focus: the focused element and its ancestors.
    pub(crate) fn set_focus_within(&self, within: &Arc<[u64]>) {
        let mut known = self.0.lock();
        if !Arc::ptr_eq(&known.focus_within, within) && *known.focus_within != **within {
            known.focus_within = within.clone();
        }
    }

    /// The `auto` elements around the active match of a text search, which stay built for as long
    /// as it is active: Chrome activates the display lock of a match it makes active
    /// (`ActivateFindInPageMatchRangeIfNeeded`).
    pub(crate) fn set_found(&self, around: Vec<u64>) {
        let mut known = self.0.lock();
        if *known.found != *around {
            known.found = around.into();
        }
    }

    /// The `auto` elements that are to skip or show their content otherwise than their last build
    /// did, which that build has to be made again for.
    pub(crate) fn pending(&self) -> Vec<u64> {
        let known = self.0.lock();
        known
            .elements
            .iter()
            .filter(|(id, element)| element.auto && element.wanted_skipped(&known.holds(), **id) != element.skipped_in_build)
            .map(|(id, _)| *id)
            .chain(known.forced.iter().copied())
            .collect()
    }

    pub(crate) fn any_forced(&self) -> bool {
        self.0.forced.load(Ordering::Relaxed) != 0
    }

    pub(crate) fn is_forced(&self, id: u64) -> bool {
        self.0.forced.load(Ordering::Relaxed) != 0 && self.0.lock().forced.contains(&id)
    }

    /// Makes the builds that follow lay out the content of `ids`, whether they skip it or not, until
    /// [`Relevance::unforce`]. They are a source of the frame, so the roots above them are built.
    pub(crate) fn force(&self, ids: Vec<u64>) {
        let mut known = self.0.lock();
        known.forced = ids;
        self.0.forced.store(known.forced.len(), Ordering::Relaxed);
    }

    pub(crate) fn unforce(&self) {
        self.force(Vec::new());
    }

    /// The elements the last forced pass of a read laid out the content of, if the window is as
    /// that read left it: nothing invalidated it and no frame or pass was built since, so the boxes
    /// it published are still the window's (Chrome's locked subtree stays clean while nothing
    /// dirties it). A frame skips them again and a mutation, a scroll write or a resize
    /// invalidates, so each ends it.
    pub(crate) fn forced_by_last_read(&self, now: gpui::LayoutStamp) -> Option<Vec<u64>> {
        let known = self.0.lock();
        known.forced_read.as_ref().filter(|read| read.stamp == now).map(|read| read.forced.clone())
    }

    /// Records the pass that forced `forced`, with the stamp of the window after the read made
    /// it skip them again.
    pub(crate) fn remember_forced_read(&self, forced: Vec<u64>, stamp: gpui::LayoutStamp) {
        self.0.lock().forced_read = Some(ForcedRead { forced, stamp });
    }

    /// Drops what is known of elements the tree no longer has.
    pub(crate) fn retain(&self, mut keep: impl FnMut(u64) -> bool) {
        if self.is_empty() {
            return;
        }
        let mut known = self.0.lock();
        known.elements.retain(|id, _| keep(*id));
        self.0.counted(&known);
    }

    /// Reads the boxes the last frame painted: remembers the size of the elements that were not
    /// skipped, decides whether each `auto` element is near, and says which have a state that was
    /// not told and which are to be built again.
    pub(crate) fn sweep(
        &self,
        window: gpui::WindowId,
        viewport: gpui::Size<Pixels>,
        tree: &crate::retained_tree::RetainedTree,
    ) -> Swept {
        let mut known = self.0.lock();
        let known = &mut *known;
        let mut swept = Swept::default();
        if known.elements.is_empty() {
            return swept;
        }
        let stamp = (tree.id(), tree.revision());
        if known.clips.as_ref().is_none_or(|clips| clips.stamp != stamp) {
            known.clips = Some(Clips { stamp, ancestors: FxHashMap::default() });
        }
        let clips = known.clips.as_mut().expect("the clips were just made");
        let holds = Holds { focus_within: &known.focus_within, found: &known.found };
        for (id, element) in known.elements.iter_mut() {
            let Some(painted) = crate::automation::get_bounds(window, *id) else {
                continue;
            };
            if !element.skipped_in_build {
                let width = painted.width - painted.padding_left - painted.padding_right - painted.client_left - painted.border_right;
                let height = painted.height - painted.padding_top - painted.padding_bottom - painted.client_top - painted.border_bottom;
                element.remembered = Some((width.max(0.0), height.max(0.0)));
            }
            if !element.auto {
                continue;
            }
            // Found when the element is first swept: an element that is built does not change the tree's revision.
            let ancestors = clips.ancestors.entry(*id).or_insert_with(|| crate::observers::clipping_ancestors(tree, *id, None));
            let boxes = crate::observers::clip_rects(window, ancestors).into_iter().map(|(_, bounds)| bounds);
            element.near = Some(is_near(crate::observers::border_box(&painted), boxes, viewport));
            let wanted = element.wanted_skipped(&holds, *id);
            if wanted != element.skipped_in_build {
                swept.rebuild |= !std::mem::replace(&mut element.rebuild_requested, true);
            } else {
                element.rebuild_requested = false;
                if element.notified != Some(wanted) {
                    element.notified = Some(wanted);
                    swept.changes.push((*id, wanted));
                }
            }
        }
        swept
    }
}

/// What `element` was last built with, the declared value for one that never was.
fn value_of(tree: &RetainedTree, element: &RetainedElement) -> ContentVisibility {
    tree.content_visibility_of(element)
}

/// The strict ancestors of `id` that skip their content, as the last build did or as the next
/// would: what a read of the geometry of `id` has to lay out for. Outermost first.
pub(crate) fn skipped_ancestors(tree: &RetainedTree, id: u64) -> Vec<u64> {
    let mut skipped = Vec::new();
    let mut current = tree.elements.get(&id).and_then(|element| element.parent).and_then(|parent| tree.elements.get(&parent));
    while let Some(ancestor) = current {
        let value = value_of(tree, ancestor);
        if value.contains()
            && crate::text::inline::is_host_container(ancestor)
            && !crate::layout::contents::is_display_contents(ancestor)
            && (skipped_as_built(&tree.relevance, value, ancestor.id) || skips(&tree.relevance, value, ancestor.id))
        {
            skipped.push(ancestor.id);
        }
        current = ancestor.parent.and_then(|parent| tree.elements.get(&parent));
    }
    skipped.reverse();
    skipped
}

/// Whether a strict ancestor of `element` is forced, so that `element` is laid out for a read of
/// geometry and nothing of what it runs advances: no animation or transition starts or steps below
/// a skipped element.
pub(crate) fn is_forced_by_ancestor(tree: &RetainedTree, element: &RetainedElement) -> bool {
    if !tree.relevance.any_forced() {
        return false;
    }
    let mut current = element.parent.and_then(|parent| tree.elements.get(&parent));
    while let Some(ancestor) = current {
        if tree.relevance.is_forced(ancestor.id) {
            return true;
        }
        current = ancestor.parent.and_then(|parent| tree.elements.get(&parent));
    }
    false
}

/// What find-in-page makes of the content of `element`: Chrome's `FindBuffer` leaves out the content
/// of a locked element that find-in-page cannot activate, which is a `hidden` one, and searches the
/// content of an `auto` one whether it is skipped or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Found {
    Searched,
    /// Searched, and shown when a match in it is the active one.
    Activatable,
    Left,
}

pub(crate) fn found_in(element: &RetainedElement) -> Found {
    if !crate::text::inline::is_host_container(element) || crate::layout::contents::is_display_contents(element) {
        return Found::Searched;
    }
    // The declared value, which is what moves the search revision: the groups of a search are cached
    // by it.
    match element.style.as_deref().and_then(|style| style.content_visibility).unwrap_or_default() {
        ContentVisibility::Visible => Found::Searched,
        ContentVisibility::Auto => Found::Activatable,
        ContentVisibility::Hidden => Found::Left,
    }
}

/// Whether `element` is a host element that skips its content, as it was last built.
pub(crate) fn skips_content(tree: &RetainedTree, element: &RetainedElement) -> bool {
    crate::text::inline::is_host_container(element)
        && !crate::layout::contents::is_display_contents(element)
        && skipped_as_built(&tree.relevance, value_of(tree, element), element.id)
}

/// Whether a strict ancestor of `element` skips its content, so nothing is built for it.
pub(crate) fn is_skipped_by_ancestor(tree: &RetainedTree, element: &RetainedElement) -> bool {
    let mut current = element.parent.and_then(|parent| tree.elements.get(&parent));
    while let Some(ancestor) = current {
        if skips_content(tree, ancestor) {
            return true;
        }
        current = ancestor.parent.and_then(|parent| tree.elements.get(&parent));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap_or_else(|error| panic!("{json}: {error}"))
    }

    fn contained_of(json: &str) -> StyleDesc {
        contained(&style(json)).expect("an element with content-visibility is contained")
    }

    #[test]
    fn visible_contains_nothing() {
        assert!(contained(&style("{}")).is_none());
        assert!(contained(&style(r#"{"contentVisibility":"visible"}"#)).is_none());
        assert!(serde_json::from_str::<StyleDesc>(r#"{"contentVisibility":"maybe"}"#).is_err());
    }

    #[test]
    fn the_content_is_clipped_unless_the_author_already_clips_it() {
        let both = contained_of(r#"{"contentVisibility":"auto"}"#);
        assert_eq!((both.overflow_x.as_deref(), both.overflow_y.as_deref()), (Some("clip"), Some("clip")));
        let scroller = contained_of(r#"{"contentVisibility":"auto","overflowY":"scroll"}"#);
        assert_eq!((scroller.overflow_x, scroller.overflow_y.as_deref()), (None, Some("scroll")));
        let hidden = contained_of(r#"{"contentVisibility":"hidden","overflow":"hidden"}"#);
        assert_eq!(hidden.overflow.as_deref(), Some("hidden"));
        assert!(hidden.overflow_x.is_none() && hidden.overflow_y.is_none());
        let beside_clip = contained_of(r#"{"contentVisibility":"auto","overflowX":"clip"}"#);
        assert_eq!(
            (beside_clip.overflow_x.as_deref(), beside_clip.overflow_y.as_deref()),
            (Some("clip"), Some("clip"))
        );
    }

    #[test]
    fn a_static_element_becomes_a_containing_block_without_an_offset() {
        let static_box = contained_of(r#"{"contentVisibility":"hidden","top":5,"inset":3,"zIndex":4}"#);
        assert_eq!(static_box.position.as_deref(), Some("relative"));
        assert!(static_box.top.is_none() && static_box.inset.is_none() && static_box.z_index.is_none());
        let positioned = contained_of(r#"{"contentVisibility":"hidden","position":"absolute","top":5,"zIndex":4}"#);
        assert_eq!(positioned.position.as_deref(), Some("absolute"));
        assert!(positioned.top.is_some() && positioned.z_index == Some(4.0));
        let item = StyleDesc { flex_or_grid_item: true, ..style(r#"{"contentVisibility":"auto","zIndex":2}"#) };
        assert_eq!(contained(&item).unwrap().z_index, Some(2.0));
    }

    #[test]
    fn a_box_has_layout_containment_for_any_value_but_visible() {
        use crate::color::{ColorContext, Scheme};
        use gpui::Styled;

        let (colors, lengths) = (ColorContext::root(Scheme::Light), LengthContext::for_tests());
        let flag = |json: &str| {
            let mut element = crate::layout::style::apply(gpui::div(), &contained_of_or_plain(json), &colors, &lengths);
            element.style().contain_layout
        };
        assert_eq!(flag(r#"{"contentVisibility":"auto"}"#), Some(true));
        assert_eq!(flag(r#"{"contentVisibility":"hidden"}"#), Some(true));
        assert_eq!(flag(r#"{"contentVisibility":"visible"}"#), None);
        assert_eq!(flag("{}"), None);
    }

    fn contained_of_or_plain(json: &str) -> StyleDesc {
        let declared = style(json);
        contained(&declared).unwrap_or(declared)
    }

    #[test]
    fn containing_twice_changes_nothing() {
        let once = contained_of(r#"{"contentVisibility":"auto","top":1}"#);
        assert_eq!(contained(&once).unwrap(), once);
    }

    #[test]
    fn the_walks_over_the_tree_see_a_stacking_context_that_is_a_containing_block() {
        use crate::color::{ColorContext, Scheme};
        use crate::layout::{containing_block, stacking};

        let (colors, lengths) = (ColorContext::root(Scheme::Light), LengthContext::for_tests());
        let mut tree = RetainedTree::new();
        for (id, json) in [(1, r#"{"contentVisibility":"auto"}"#), (2, r#"{"position":"absolute"}"#), (3, "{}")] {
            tree.create_element(id, "div".to_owned());
            tree.set_style(id, std::sync::Arc::new(style(json)));
        }
        tree.set_root(1);
        tree.append_child(1, 2);
        tree.append_child(1, 3);
        let contained = &tree.elements[&1];
        let resolved = tree.resolved_of(contained, &colors, &lengths);
        assert!(stacking::creates_context(&resolved, false));
        assert_eq!(stacking::lift_z(&resolved, false), Some(0));
        assert!(containing_block::is_positioned(&tree, contained));
        assert_eq!(containing_block::reaching_ancestors(&tree, 2), vec![1]);
        assert!(!containing_block::is_positioned(&tree, &tree.elements[&3]));
        assert!(resolved.overflow.x.clips() && resolved.overflow.y.clips());
    }

    fn square(left: f32, top: f32, side: f32) -> Bounds<Pixels> {
        Bounds {
            origin: gpui::point(gpui::px(left), gpui::px(top)),
            size: gpui::size(gpui::px(side), gpui::px(side)),
        }
    }

    const VIEWPORT: gpui::Size<Pixels> = gpui::Size { width: gpui::px(1000.0), height: gpui::px(800.0) };

    #[test]
    fn an_element_is_near_up_to_a_viewport_and_a_half_past_each_edge() {
        let near = |left, top, clips: &[Bounds<Pixels>]| is_near(square(left, top, 50.0), clips.iter().copied(), VIEWPORT);
        assert!(near(100.0, 100.0, &[]));
        // The margin is 1.5 times the side of the axis: 1200 below, 1500 to the right.
        assert!(near(0.0, 1999.0, &[]));
        assert!(!near(0.0, 2000.0, &[]));
        assert!(near(0.0, -1249.0, &[]));
        assert!(!near(0.0, -1250.0, &[]));
        assert!(near(2499.0, 0.0, &[]));
        assert!(!near(2500.0, 0.0, &[]));
        assert!(!near(-1550.0, 0.0, &[]));
    }

    #[test]
    fn what_clips_an_element_cuts_the_margin_short() {
        let far = square(0.0, 1900.0, 50.0);
        assert!(is_near(far, [], VIEWPORT));
        // A scroller whose visible part ends before the margin of the element begins.
        assert!(!is_near(far, [square(0.0, 0.0, 400.0)], VIEWPORT));
        assert!(is_near(far, [square(0.0, 300.0, 600.0)], VIEWPORT));
        // The margin does not reach past the viewport, whatever the clip lets through.
        assert!(!is_near(far, [square(0.0, 1500.0, 400.0)], VIEWPORT));
    }

    #[test]
    fn auto_skips_until_something_says_it_is_near() {
        let relevance = Relevance::default();
        assert!(skips(&relevance, ContentVisibility::Hidden, 1));
        assert!(!skips(&relevance, ContentVisibility::Visible, 1));
        // Never built: skipped.
        assert!(skips(&relevance, ContentVisibility::Auto, 1));
        relevance.built(1, true, true, true);
        assert!(relevance.wants_answer(1));
        assert!(skips(&relevance, ContentVisibility::Auto, 1));
        assert!(relevance.answer(1, true), "near and skipped: build again");
        assert!(!skips(&relevance, ContentVisibility::Auto, 1));
        assert!(relevance.was_skipped(1), "the last build skipped it still");
        relevance.built(1, true, true, false);
        assert!(!relevance.was_skipped(1));
        // The first answer is the only one prepaint gives.
        assert!(!relevance.wants_answer(1));
    }

    #[test]
    fn an_answer_that_is_far_asks_for_nothing() {
        let relevance = Relevance::default();
        relevance.built(1, true, true, true);
        assert!(!relevance.answer(1, false));
        assert!(!relevance.wants_answer(1));
        assert!(skips(&relevance, ContentVisibility::Auto, 1));
    }

    #[test]
    fn the_focus_keeps_an_element_from_skipping() {
        let relevance = Relevance::default();
        relevance.built(1, true, true, true);
        relevance.answer(1, false);
        assert!(skips(&relevance, ContentVisibility::Auto, 1));
        relevance.set_focus_within(&Arc::from([5_u64, 1, 0]));
        assert!(!skips(&relevance, ContentVisibility::Auto, 1));
        relevance.set_focus_within(&Arc::from([]));
        assert!(skips(&relevance, ContentVisibility::Auto, 1));
    }

    #[test]
    fn the_active_match_of_a_search_keeps_an_element_from_skipping() {
        let relevance = Relevance::default();
        relevance.built(1, true, true, true);
        relevance.answer(1, false);
        relevance.set_found(vec![1]);
        assert!(!skips(&relevance, ContentVisibility::Auto, 1));
        assert_eq!(relevance.pending(), vec![1], "the build that skipped it is made again");
        relevance.set_found(Vec::new());
        assert!(skips(&relevance, ContentVisibility::Auto, 1));
        assert!(relevance.pending().is_empty());
    }

    #[test]
    fn a_forced_element_builds_its_content_in_its_skipped_box_and_leaves_no_trace() {
        let relevance = Relevance::default();
        relevance.built(1, true, true, true);
        relevance.answer(1, false);
        assert!(skips(&relevance, ContentVisibility::Hidden, 2));
        relevance.force(vec![1, 2]);
        for (value, id) in [(ContentVisibility::Auto, 1), (ContentVisibility::Hidden, 2)] {
            assert!(!skips(&relevance, value, id), "its content is built");
            assert!(contains_size(&relevance, value, id), "in the box it has skipped");
        }
        assert!(relevance.pending().contains(&1) && relevance.pending().contains(&2), "a source of the frame, hidden or not");
        assert!(relevance.key(2, false).is_some(), "and part of the key of a hidden element");
        // What the forced build tells is not what the frames built.
        relevance.built(1, true, true, false);
        relevance.remember(1, 30.0, 40.0);
        relevance.unforce();
        assert!(relevance.was_skipped(1));
        assert_eq!(relevance.remembered(1), None);
        assert!(skips(&relevance, ContentVisibility::Auto, 1) && skips(&relevance, ContentVisibility::Hidden, 2));
        assert!(relevance.pending().is_empty());
        assert_eq!(relevance.key(2, false), None);
    }

    #[test]
    fn the_skipped_ancestors_are_the_ones_a_read_has_to_force() {
        let mut tree = RetainedTree::new();
        for (id, json) in [
            (1, "{}"),
            (2, r#"{"contentVisibility":"hidden"}"#),
            (3, r#"{"contentVisibility":"auto"}"#),
            (4, r#"{"contentVisibility":"auto"}"#),
            (5, "{}"),
        ] {
            tree.create_element(id, "div".to_owned());
            tree.set_style(id, std::sync::Arc::new(style(json)));
        }
        tree.set_root(1);
        for (parent, child) in [(1, 2), (2, 3), (3, 5), (1, 4)] {
            tree.append_child(parent, child);
        }
        // Element 3 was never built: skipped. The one in the other, then, outermost first.
        assert_eq!(skipped_ancestors(&tree, 5), vec![2, 3]);
        assert_eq!(skipped_ancestors(&tree, 3), vec![2], "its own box is no concern of a read");
        assert!(skipped_ancestors(&tree, 2).is_empty());
        // An `auto` element the last build did not skip, and that is near, is read through.
        tree.relevance.built(4, true, true, false);
        tree.relevance.answer(4, true);
        tree.create_element(6, "div".to_owned());
        tree.append_child(4, 6);
        assert!(skipped_ancestors(&tree, 6).is_empty());
        tree.relevance.built(4, true, true, true);
        tree.relevance.answer(4, false);
        assert_eq!(skipped_ancestors(&tree, 6), vec![4]);
    }

    #[test]
    fn an_element_that_stops_being_auto_is_forgotten() {
        let relevance = Relevance::default();
        relevance.built(1, true, true, true);
        relevance.answer(1, true);
        relevance.built(1, false, false, false);
        assert!(relevance.is_empty());
        relevance.built(1, true, true, true);
        assert!(relevance.wants_answer(1));
    }

    #[test]
    fn the_size_is_only_remembered_while_the_content_is_built() {
        let relevance = Relevance::default();
        relevance.built(1, true, true, false);
        relevance.remember(1, 30.0, 40.0);
        assert_eq!(relevance.remembered(1), Some((30.0, 40.0)));
        relevance.built(1, true, true, true);
        relevance.remember(1, 1.0, 1.0);
        assert_eq!(relevance.remembered(1), Some((30.0, 40.0)));
    }

    #[test]
    fn auto_in_the_intrinsic_size_is_remembered_by_any_value() {
        let remembers = |json| remembers_size(&style(json));
        assert!(remembers(r#"{"contentVisibility":"auto"}"#));
        assert!(remembers(r#"{"contentVisibility":"hidden","containIntrinsicSize":"auto 40px"}"#));
        assert!(remembers(r#"{"containIntrinsicHeight":"auto 40px"}"#));
        assert!(!remembers(r#"{"contentVisibility":"hidden","containIntrinsicSize":"40px"}"#));
        assert!(!remembers("{}"));
    }
}
