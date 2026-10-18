/// Retained element tree — the Rust-side source of truth for the UI.
///
/// React's reconciler sends mutations (create, append, remove, etc.) via napi.
/// This tree stores those mutations. GpuixView builds ephemeral GPUI elements
/// from it, while virtual lists defer offscreen subtrees until layout requests them.
///
/// All IDs are u64 — JS generates them with an incrementing counter,
/// passes them as numbers across napi (no string allocation).
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock};

use crate::layout::containing_block;
use crate::resolved_style::{PointerEvents, Visibility};
use crate::style::StyleDesc;

/// What one `elements_at` walk carries along.
struct HitWalk<'a> {
    x: f64,
    y: f64,
    painted_box: &'a dyn Fn(u64) -> Option<crate::automation::ElementBounds>,
    /// Deferred `anchored` layers met so far, each with its priority, the order it was met in
    /// and its hits from the bottom up.
    layers: Vec<(u64, usize, Vec<u64>)>,
    /// For each positioned element above (and each `<virtual-list>`, whose rows are layout
    /// roots), whether its clip excludes the point from what is inside it. An absolute
    /// descendant takes the innermost one in place of the clip of the static boxes in between.
    frames: Vec<bool>,
    /// Only tell whether an `opacity`, `filter`, `mask` or `clip-path` value makes an effect
    /// group, which does not depend on the element's own `color` or font size.
    colors: crate::color::ColorContext,
    lengths: crate::css::length::LengthContext,
}

/// A stacking context being collected: the positioned descendants it was handed, with the
/// `z-index` and the place in tree order that `Window::end_stacking_context` sorts them by.
#[derive(Default)]
struct Context {
    next_ordinal: u32,
    lifted: Vec<(i32, u32, Vec<u64>)>,
}

impl Context {
    /// The tree order of a lifted element is the one it has before the elements inside it, so
    /// the ordinal is taken before they are visited.
    fn take_ordinal(&mut self) -> u32 {
        let ordinal = self.next_ordinal;
        self.next_ordinal += 1;
        ordinal
    }

    /// What the context paints, from the bottom up: its own box, the lifted elements with a
    /// negative index, its in-flow content, then the other lifted elements.
    fn paint_order(mut self, own: Option<u64>, in_flow: Vec<u64>) -> Vec<u64> {
        self.lifted.sort_by_key(|(z, ordinal, _)| (*z, *ordinal));
        let first_above = self.lifted.partition_point(|(z, _, _)| *z < 0);
        let above = self.lifted.split_off(first_above);
        let mut order: Vec<u64> = own.into_iter().collect();
        order.extend(self.lifted.into_iter().flat_map(|(_, _, hits)| hits));
        order.extend(in_flow);
        order.extend(above.into_iter().flat_map(|(_, _, hits)| hits));
        order
    }
}

pub struct RetainedElement {
    pub id: u64,
    pub element_type: String,
    /// Shared, not owned. `StyleDesc` is ~1.4 KB, and holding it inline made an
    /// unstyled element pay for a style it does not have: `RetainedElement` was
    /// 1624 bytes, and the `HashMap` stores values inline, so the whole table
    /// was that wide. Behind an `Arc` it was 248 bytes; cold metadata makes it 160,
    /// and `own_style` 168.
    ///
    /// The pointer is also shared between every element that declared the same
    /// style. A 10k-turn chat has 59 320 `setStyle` ops and 90 distinct styles.
    ///
    /// This is the effective style: the one the element declared, over the
    /// universal layer when there is one. Every reader wants it. Only `set_style`
    /// and `set_universal_style` write it (`RetainedTree::assign_style`).
    ///
    /// Read it with `.as_deref()` to get `Option<&StyleDesc>`. Never mutate
    /// through it; `Arc` has no `DerefMut`, so the compiler enforces that.
    pub style: Option<Arc<StyleDesc>>,
    /// What the element declared with `setStyle`, which the effective `style`
    /// is recomputed from when the universal layer changes. The same pointer as
    /// `style` while there is no layer.
    own_style: Option<Arc<StyleDesc>>,
    pub content: Option<String>,
    pub children: Vec<u64>,
    pub parent: Option<u64>,
    /// Most host nodes have neither listeners nor custom props.
    extras: Option<Box<ElementExtras>>,
    /// Take keyboard focus the first time this element gets a focus handle.
    /// Without it an `<input>` is dead until the user clicks it.
    pub auto_focus: bool,
    /// The `display` the last build found for the element once its `@container` blocks were
    /// folded in, which its parent decided its children by (`display: none` takes no part in
    /// layout, `inline-block` is a box on a line of its parent's text). The outer `None` until
    /// an element with such blocks is built, and again whenever its style is replaced. A build
    /// that finds another answer asks for the frame to be built again. Read it through
    /// [`Self::display`].
    pub container_display: std::cell::RefCell<Option<Option<String>>>,
    /// Counts the times the effective style of the element was replaced: all that a build of
    /// the child of a `display: contents` element reads of it is its style, and not the
    /// revisions that its descendants move. Not zero once the element has a style.
    pub style_serial: u64,
    /// Last mutation applied to this element or one of its descendants.
    pub subtree_revision: u64,
    /// Last change to the searchable TEXT of this subtree: content, structure,
    /// or the set of nested `highlight` declarations.
    ///
    /// Separate from `subtree_revision` because `highlight` is itself a custom
    /// prop: keying the group cache on the general revision means every find-bar
    /// keystroke re-walks and re-folds the whole subtree, which is the one case
    /// the cache exists for. Style, `activeIndex`, colours and a native
    /// element's own props never move this.
    pub search_revision: u64,
    /// Stable locator id from the React `testId` prop.
    pub test_id: Option<String>,
}

#[derive(Default)]
struct ElementExtras {
    events: HashSet<String>,
    custom_props: HashMap<String, serde_json::Value>,
    /// The style before the `setStyle` that made the element declare a CSS
    /// transition. Native only tracks elements that declare one, and the
    /// transitions of a change are the ones of the style after it, so without
    /// this the first change would be the first style native sees.
    before_transition: Option<Arc<StyleDesc>>,
}

static EMPTY_EXTRAS: LazyLock<ElementExtras> = LazyLock::new(ElementExtras::default);

impl RetainedElement {
    /// The `display` the element has: what its style declares, or what its `@container` blocks
    /// made of it when the last build folded them in (`container_display`).
    pub fn display(&self) -> Option<std::borrow::Cow<'_, str>> {
        if let Some(folded) = &*self.container_display.borrow() {
            return folded.clone().map(std::borrow::Cow::Owned);
        }
        self.style
            .as_deref()
            .and_then(|style| style.display.as_deref())
            .map(std::borrow::Cow::Borrowed)
    }

    /// The element's `tabIndex`, when it has a usable one.
    pub fn tab_index(&self) -> Option<isize> {
        self.custom_props()
            .get("tabIndex")
            .and_then(|value| value.as_i64())
            .and_then(|index| isize::try_from(index).ok())
    }

    /// The DOM `draggable` attribute: whether a press that moves starts a drag
    /// of this element.
    pub fn draggable(&self) -> bool {
        self.custom_props()
            .get("draggable")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    /// Whether the element must own a gpui focus handle, which is what DOM calls
    /// focusable: an enabled text control, or any other element with a `tabIndex`.
    /// A key or focus listener does not make an element focusable; an ancestor's
    /// key listener still hears the keys of a focused descendant.
    pub fn needs_focus(&self) -> bool {
        if crate::focus::is_text_control(&self.element_type) {
            // `disabled` outranks `tabIndex`: a disabled control cannot be focused at all.
            !self.is_disabled()
        } else {
            self.tab_index().is_some()
        }
    }

    /// What this element's own style says about `visibility`: `Some(true)` for hidden,
    /// `Some(false)` for visible, `None` when it inherits.
    pub fn declared_hidden(&self) -> Option<bool> {
        match self.style.as_deref()?.visibility.as_deref()? {
            "hidden" => Some(true),
            "visible" => Some(false),
            _ => None,
        }
    }

    /// Whether this element itself carries `inert`. Its subtree is inert too;
    /// `RetainedTree::is_inert` answers for an element and its ancestors.
    pub fn has_inert_prop(&self) -> bool {
        self.custom_props()
            .get("inert")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    /// Whether the universal style layer applies to this element. CSS `*`
    /// matches elements, not text nodes: a `text` element that holds a string is
    /// one, a `<text>` that holds children is an element.
    fn takes_universal_style(&self) -> bool {
        !(self.element_type == "text" && self.content.is_some())
    }

    pub fn is_disabled(&self) -> bool {
        self.custom_props()
            .get("disabled")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    /// `Some(disabled)` for a form control, which is what `:disabled` and `:enabled` match, and
    /// `None` for every other element.
    ///
    /// `<input>` and `<textarea>` are controls, and so are the `button` and `select` types the
    /// tree is also handed in tests. The React and Solid `Button` and `Select` are `div`s that
    /// stand in for them: they say so with the `formControl` prop (`"enabled"` or `"disabled"`),
    /// which the host forwards for a `div`, unlike `disabled`. That keeps a plain `<div disabled>`
    /// from matching, as in a DOM.
    pub fn control_disabled(&self) -> Option<bool> {
        if is_control_type(&self.element_type) {
            return Some(self.is_disabled());
        }
        match self.custom_props().get(FORM_CONTROL_PROP)?.as_str()? {
            "disabled" => Some(true),
            "enabled" => Some(false),
            _ => None,
        }
    }

    pub fn new(id: u64, element_type: String, revision: u64) -> Self {
        Self {
            id,
            element_type,
            style: None,
            own_style: None,
            content: None,
            children: Vec::new(),
            parent: None,
            extras: None,
            auto_focus: false,
            container_display: Default::default(),
            style_serial: 0,
            subtree_revision: revision,
            search_revision: revision,
            test_id: None,
        }
    }

    pub fn events(&self) -> &HashSet<String> {
        &self.extras.as_deref().unwrap_or(&EMPTY_EXTRAS).events
    }

    pub fn custom_props(&self) -> &HashMap<String, serde_json::Value> {
        &self.extras.as_deref().unwrap_or(&EMPTY_EXTRAS).custom_props
    }

    pub fn style_before_transition(&self) -> Option<&StyleDesc> {
        self.extras.as_deref()?.before_transition.as_deref()
    }

    fn extras_mut(&mut self) -> &mut ElementExtras {
        self.extras.get_or_insert_with(Default::default)
    }

    fn release_empty_extras(&mut self) {
        if self
            .extras
            .as_ref()
            .is_some_and(|extras| {
                extras.events.is_empty()
                    && extras.custom_props.is_empty()
                    && extras.before_transition.is_none()
            })
        {
            self.extras = None;
        }
    }
}

/// The element map, keyed by the u64 counter JS allocates.
///
/// Deliberately not the std hasher. `mark_changed` probes this map once per
/// ancestor hop on every append, insert, style and text mutation, and
/// `build_virtual_list` probes it twice per child on every frame. The keys come
/// from our own counter, never from user input, so SipHash only costs time.
pub type ElementMap = rustc_hash::FxHashMap<u64, RetainedElement>;

/// One hash-consed style: the raw JSON that produced it, and the shared value.
///
/// The raw bytes are kept so a hash hit can be confirmed by comparing content.
/// A 64-bit hash collides eventually, and a collision here would paint one
/// element with another element's style, which is a bug nobody would find.
struct InternedStyle {
    raw: Box<[u8]>,
    style: Arc<StyleDesc>,
}

/// Below this many entries a sweep is not worth thinking about, so the table
/// never sweeps on a small app just because it grew from one style to two.
pub(crate) const STYLE_SWEEP_FLOOR: usize = 64;

/// Styles shared by content. Keyed by a hash of the raw payload, with a bucket
/// per key so collisions are resolved by comparing bytes.
///
/// Interning happens here, on arrival, rather than in the protocol. JS cannot
/// do it safely: `commitUpdate` resends the full style on every commit, and a
/// dragged element produces a distinct style every frame, so a JS-owned table
/// would grow without bound and the update path would send a definition plus a
/// reference where it sends one op today.
///
/// Separate from the element tree so `resolve_styles` can borrow it alone. That
/// is what makes a batch atomic: interning is the only fallible step, and the
/// borrow checker proves it cannot have touched an element.
#[derive(Default)]
pub struct StyleTable {
    entries: rustc_hash::FxHashMap<u64, Vec<InternedStyle>>,
    /// Entries currently held. Tracked rather than summed, so `maybe_sweep` is
    /// O(1) on the commits that do not sweep.
    count: usize,
    /// `count` right after the last sweep.
    swept_at: usize,
    /// The universal layer's result per own style. It lives here because it holds
    /// styles of the table, which `sweep` could not reclaim while it did.
    layered: crate::universal_style::LayerCache,
}

fn has_container_rules(element: &RetainedElement) -> bool {
    element.style.as_deref().is_some_and(|style| style.container_rules.is_some())
}

impl StyleTable {
    /// Parse a style payload, reusing the shared value when the same bytes
    /// arrived before. Hashing ~110 bytes is far cheaper than building the
    /// ~80 `Option` fields of a `StyleDesc`.
    pub fn intern(&mut self, raw: &[u8]) -> Result<Arc<StyleDesc>, String> {
        let mut hasher = rustc_hash::FxHasher::default();
        raw.hash(&mut hasher);
        let key = hasher.finish();
        if let Some(bucket) = self.entries.get(&key) {
            if let Some(hit) = bucket.iter().find(|entry| &*entry.raw == raw) {
                return Ok(hit.style.clone());
            }
        }
        let mut style: StyleDesc = serde_json::from_slice(raw).map_err(|error| error.to_string())?;
        // Read from the text: which container units a style names is a property of all of it,
        // `@container` blocks and state styles included.
        style.container_unit_axes = crate::css::length::LengthReads::container_axes(
            std::str::from_utf8(raw).map_err(|error| error.to_string())?,
        );
        crate::motion::validate_transitions(&style)?;
        crate::motion::validate_animations(&style)?;
        let shared = Arc::new(style);
        self.entries.entry(key).or_default().push(InternedStyle {
            raw: raw.into(),
            style: shared.clone(),
        });
        self.count += 1;
        Ok(shared)
    }

    /// Drop interned styles no element references any more.
    ///
    /// This is what makes the table safe on an interactive app. A drag creates
    /// one style per frame; each is released once the element stops referencing
    /// it. `strong_count == 1` means the table holds the last reference, so
    /// nothing can be reading it.
    pub fn sweep(&mut self) {
        self.layered.release_unused();
        let mut live = 0;
        self.entries.retain(|_, bucket| {
            bucket.retain(|entry| Arc::strong_count(&entry.style) > 1);
            live += bucket.len();
            !bucket.is_empty()
        });
        self.count = live;
        self.swept_at = live;
    }

    /// Sweep when the table has grown, or when the tree has shrunk under it.
    ///
    /// Growth alone is not enough. After a large mount `swept_at` is large, so
    /// destroying the root or remounting a smaller app leaves `count` below the
    /// next threshold forever, and the whole high-water table stays resident.
    /// Re-interning an existing style does not raise `count`, so nothing would
    /// ever reclaim it.
    ///
    /// `live_elements` closes that. A live style needs at least one element
    /// holding it, so the live style count can never exceed the element count;
    /// a table far larger than the tree is therefore mostly dead. After a sweep
    /// `count <= live_elements`, so this cannot thrash.
    ///
    /// Both arms are amortized: each scan is O(entries) and follows at least as
    /// many interns or as large a collapse.
    pub fn maybe_sweep(&mut self, live_elements: usize) {
        let grew = self.count >= self.swept_at.saturating_mul(2).max(STYLE_SWEEP_FLOOR);
        let tree_shrank = self.count > live_elements.saturating_mul(2).max(STYLE_SWEEP_FLOOR);
        if grew || tree_shrank {
            self.sweep();
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

/// Whether the frame must sample motion values for this element: the ones its
/// style binds, and the drag values of its `layout`.
fn reads_motion_values(props: &HashMap<String, serde_json::Value>) -> bool {
    props.contains_key("motionValues")
        || props
            .get("layout")
            .is_some_and(|layout| layout.get("dragOffset").is_some_and(|drag| !drag.is_null()))
}

/// What assigning a style changed, for the caller to mark the tree with.
#[derive(Clone, Copy, Default)]
struct StyleChange {
    changed: bool,
    regrouped: bool,
    text_reprocessed: bool,
    visibility_changed: bool,
}

impl StyleChange {
    fn absorb(&mut self, other: Self) {
        self.changed |= other.changed;
        self.regrouped |= other.regrouped;
        self.text_reprocessed |= other.text_reprocessed;
        self.visibility_changed |= other.visibility_changed;
    }
}

/// Whether two optional styles are the same allocation or equal in content.
fn same_style(a: Option<&Arc<StyleDesc>>, b: Option<&Arc<StyleDesc>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b) || **a == **b,
        _ => false,
    }
}

/// The prop a `div` that stands in for a form control carries. See
/// [`RetainedElement::control_disabled`].
const FORM_CONTROL_PROP: &str = "formControl";

fn is_control_type(element_type: &str) -> bool {
    matches!(element_type, "button" | "input" | "textarea" | "select")
}

/// The strict ancestors of the disabled and of the enabled form controls: the elements
/// `:has(:disabled)` and `:has(:enabled)` match.
#[derive(Default)]
pub(crate) struct ControlAncestors {
    pub disabled: HashSet<u64>,
    pub enabled: HashSet<u64>,
}

/// [`ControlAncestors`] of one frame, worked out the first time a style that declares
/// `hasDisabled` or `hasEnabled` asks. Cloning shares the answer, so a virtual list's rows read
/// the one the root render worked out.
#[derive(Clone, Default)]
pub(crate) struct ControlStyles {
    ancestors: std::sync::Arc<std::sync::OnceLock<ControlAncestors>>,
}

impl ControlStyles {
    fn ancestors(&self, tree: &RetainedTree) -> &ControlAncestors {
        self.ancestors.get_or_init(|| {
            let mut found = ControlAncestors::default();
            for control in &tree.control_ids {
                let Some(disabled) = tree.elements.get(control).and_then(RetainedElement::control_disabled) else {
                    continue;
                };
                let ancestors = if disabled { &mut found.disabled } else { &mut found.enabled };
                let mut current = tree.elements.get(control).and_then(|element| element.parent);
                // An ancestor already in the set has the rest of the chain in it too.
                while let Some(id) = current.filter(|id| ancestors.insert(*id)) {
                    current = tree.elements.get(&id).and_then(|element| element.parent);
                }
            }
            found
        })
    }

    /// `:has(:disabled)`: a strict descendant of `id` is a disabled form control.
    pub fn has_disabled(&self, tree: &RetainedTree, id: u64) -> bool {
        self.ancestors(tree).disabled.contains(&id)
    }

    /// `:has(:enabled)`: a strict descendant of `id` is an enabled form control.
    pub fn has_enabled(&self, tree: &RetainedTree, id: u64) -> bool {
        self.ancestors(tree).enabled.contains(&id)
    }
}

pub struct RetainedTree {
    /// Never shared by two trees of this process, unlike the tree's address,
    /// which a later tree can reuse. Caches that outlive a tree key on it.
    id: u64,
    pub elements: ElementMap,
    pub motion_ids: HashSet<u64>,
    /// Every `<img>`, so a frame can find the ones that `display: none` kept out
    /// of the build without walking the tree.
    pub image_ids: HashSet<u64>,
    /// Elements that bind motion values to style properties.
    pub motion_value_ids: HashSet<u64>,
    /// Every form control, which `ControlStyles` walks up from, so a frame does not scan
    /// every element for them.
    control_ids: HashSet<u64>,
    /// Elements that `needs_focus`, in id order. Kept as mutations arrive so the
    /// per-frame focus sync visits these few instead of every retained element:
    /// a large tree retains thousands, and nearly none are focusable.
    pub focusable_ids: std::collections::BTreeSet<u64>,
    /// Elements with an `onLayout`, `ResizeObserver` or `IntersectionObserver` listener, which
    /// `observers::frame_sweep` reads the published boxes of.
    pub geometry_ids: HashSet<u64>,
    /// How many elements carry `inert`. While it is zero, no mutation has to
    /// look at ancestors to decide what is focusable.
    inert_count: usize,
    /// Elements whose style declares a `visibility`, so a tree without any skips the ancestor walk.
    visibility_count: usize,
    /// How many `anchored` elements exist. Without any (and without an absolute or fixed
    /// element), an `elements_at` walk can drop a clipped subtree whole.
    anchored_count: usize,
    /// How many elements are `position: absolute` or `fixed` in some state, which a clip above may
    /// not reach.
    escaping_count: usize,
    /// The last `RetainedElement::style_serial` handed out.
    style_serials: u64,
    /// How many elements have `@container` blocks, so a tree without any skips looking for
    /// the children they keep from being built.
    container_rule_count: usize,
    pub styles: StyleTable,
    /// What each built element was painted with, for the walks that read the tree.
    pub(crate) resolved: std::cell::RefCell<crate::resolved_style::ResolvedStyles>,
    /// The style layer under every element's own style, a CSS `* { ... }` rule.
    universal: Option<Arc<StyleDesc>>,
    /// The root element ID set by appendChildToContainer.
    pub root_id: Option<u64>,
    /// Dim other focusables while a control has keyboard focus. Set per root.
    keyboard_focus_dim: bool,
    next_revision: u64,
    /// Something a frame would draw changed since the last `take_dirty`. A batch
    /// of resends, which is what a re-render with unchanged props produces,
    /// leaves it false, so the window is not asked to rebuild its element tree
    /// for a frame that would come out identical.
    dirty: bool,
    /// An element was given `autoFocus` since the last `take_auto_focus_request`.
    auto_focus_requested: bool,
    /// How many elements this tree has ever destroyed, for caches keyed by element id to know when
    /// to look for the ones that are gone.
    removals: u64,
}

impl RetainedTree {
    pub fn new() -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            elements: ElementMap::default(),
            motion_ids: HashSet::new(),
            image_ids: HashSet::new(),
            motion_value_ids: HashSet::new(),
            control_ids: HashSet::new(),
            focusable_ids: Default::default(),
            geometry_ids: Default::default(),
            inert_count: 0,
            visibility_count: 0,
            anchored_count: 0,
            escaping_count: 0,
            style_serials: 0,
            container_rule_count: 0,
            styles: StyleTable::default(),
            resolved: Default::default(),
            universal: None,
            root_id: None,
            keyboard_focus_dim: true,
            next_revision: 1,
            dirty: false,
            auto_focus_requested: false,
            removals: 0,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn removals(&self) -> u64 {
        self.removals
    }

    /// Whether a frame drawn now would differ from the last one, clearing the
    /// answer. Call once per applied batch.
    /// Whether any element has `@container` blocks.
    pub fn has_container_rules(&self) -> bool {
        self.container_rule_count > 0
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Whether an element was given `autoFocus` since the last call. Call once
    /// per applied batch, to focus it at commit.
    pub fn take_auto_focus_request(&mut self) -> bool {
        std::mem::take(&mut self.auto_focus_requested)
    }

    pub fn keyboard_focus_dim(&self) -> bool {
        self.keyboard_focus_dim
    }

    pub fn set_keyboard_focus_dim(&mut self, enabled: bool) {
        if self.keyboard_focus_dim != enabled {
            self.keyboard_focus_dim = enabled;
            self.dirty = true;
        }
    }

    pub fn set_root(&mut self, id: u64) {
        if self.root_id != Some(id) {
            self.root_id = Some(id);
            self.dirty = true;
        }
    }

    pub fn create_element(&mut self, id: u64, element_type: String) {
        self.anchored_count += usize::from(element_type == "anchored");
        if element_type == "img" {
            self.image_ids.insert(id);
        }
        if is_control_type(&element_type) {
            self.control_ids.insert(id);
        }
        self.dirty = true;
        let revision = self.take_revision();
        self.elements
            .insert(id, RetainedElement::new(id, element_type, revision));
        if self.universal.is_some() {
            self.refresh_effective_style(id);
        }
        self.track_focusable(id);
    }

    /// Whether `id` or any ancestor has `inert`.
    pub fn is_inert(&self, id: u64) -> bool {
        if self.inert_count == 0 {
            return false;
        }
        let mut current = Some(id);
        while let Some(current_id) = current {
            let Some(element) = self.elements.get(&current_id) else {
                return false;
            };
            if element.has_inert_prop() {
                return true;
            }
            current = element.parent;
        }
        false
    }

    /// Whether `id` ends up `visibility: hidden`: the nearest declaration at or above it wins.
    pub fn is_visibility_hidden(&self, id: u64) -> bool {
        if self.visibility_count == 0 {
            return false;
        }
        let mut current = Some(id);
        while let Some(current_id) = current {
            let Some(element) = self.elements.get(&current_id) else {
                return false;
            };
            if let Some(hidden) = element.declared_hidden() {
                return hidden;
            }
            current = element.parent;
        }
        false
    }

    /// Whether the nearest form control at or above `id` is disabled. HTML
    /// dispatches no `click`, `auxclick` or `dblclick` for such a target.
    pub fn in_disabled_form_control(&self, id: u64) -> bool {
        let mut current = Some(id);
        while let Some(current_id) = current {
            let Some(element) = self.elements.get(&current_id) else {
                return false;
            };
            if let Some(disabled) = element.control_disabled() {
                return disabled;
            }
            current = element.parent;
        }
        false
    }

    /// Bring `focusable_ids` in line with `id` after anything `needs_focus`
    /// reads has changed.
    fn track_focusable(&mut self, id: u64) {
        if !self.is_inert(id)
            && !self.is_visibility_hidden(id)
            && self
                .elements
                .get(&id)
                .is_some_and(RetainedElement::needs_focus)
        {
            self.focusable_ids.insert(id);
        } else {
            self.focusable_ids.remove(&id);
        }
    }

    /// Redo `track_focusable` for `id` and everything below it. Needed when an
    /// `inert` ancestor appears, goes away, or the subtree is moved under or
    /// out from under one.
    fn track_focusable_subtree(&mut self, id: u64) {
        let parent = self.elements.get(&id).and_then(|element| element.parent);
        let inert = parent.is_some_and(|parent| self.is_inert(parent));
        let hidden = parent.is_some_and(|parent| self.is_visibility_hidden(parent));
        self.track_focusable_below(id, inert, hidden);
    }

    fn track_focusable_below(&mut self, id: u64, inherited_inert: bool, inherited_hidden: bool) {
        let Some(element) = self.elements.get(&id) else {
            return;
        };
        let inert = inherited_inert || element.has_inert_prop();
        let hidden = element.declared_hidden().unwrap_or(inherited_hidden);
        let focusable = !inert && !hidden && element.needs_focus();
        let children = element.children.clone();
        if focusable {
            self.focusable_ids.insert(id);
        } else {
            self.focusable_ids.remove(&id);
        }
        for child in children {
            self.track_focusable_below(child, inert, hidden);
        }
    }

    /// Moves with every change to an element, so a cache of what depends on the tree can tell
    /// that it is current.
    pub fn revision(&self) -> u64 {
        self.next_revision
    }

    fn take_revision(&mut self) -> u64 {
        // Every content change takes a revision to invalidate caches, so this is
        // the one place that covers style, text, structure and custom props.
        self.dirty = true;
        let revision = self.next_revision;
        self.next_revision = self.next_revision.wrapping_add(1).max(1);
        revision
    }

    /// Invalidate `id` and its ancestors, including their searchable text.
    fn mark_changed(&mut self, id: u64) {
        self.mark_changed_detail(id, true);
    }

    /// Invalidate for rendering only. Use for changes that cannot move a glyph
    /// into or out of the searchable text: style, and a native element's own
    /// props, whose text is matched at paint and never enters a `GroupList`.
    fn mark_render_changed(&mut self, id: u64) {
        self.mark_changed_detail(id, false);
    }

    /// Invalidate `id`, its ancestors and everything below it, including their
    /// searchable text.
    fn mark_subtree_changed(&mut self, id: u64) {
        self.mark_changed(id);
        let revision = self.take_revision();
        let mut pending = vec![id];
        while let Some(current) = pending.pop() {
            let Some(element) = self.elements.get_mut(&current) else {
                continue;
            };
            element.subtree_revision = revision;
            element.search_revision = revision;
            pending.extend(element.children.iter().copied());
        }
    }

    fn mark_changed_detail(&mut self, id: u64, search: bool) {
        let revision = self.take_revision();
        let mut current = Some(id);
        while let Some(current_id) = current {
            let Some(element) = self.elements.get_mut(&current_id) else {
                break;
            };
            element.subtree_revision = revision;
            if search {
                element.search_revision = revision;
            }
            current = element.parent;
        }
    }

    /// Recursively destroy an element and all its children.
    /// Returns all destroyed IDs so the caller can clean up JS-side state.
    ///
    /// Unlinks from the parent before removing, then marks the parent chain
    /// changed so caches cannot serve text that is no longer in the tree.
    pub fn destroy_element(&mut self, id: u64) -> Vec<u64> {
        let parent_id = self.elements.get(&id).and_then(|element| element.parent);
        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.elements.get_mut(&parent_id) {
                parent.children.retain(|child| *child != id);
            }
        }
        let mut destroyed = Vec::new();
        self.destroy_element_recursive(id, &mut destroyed);
        if !destroyed.is_empty() {
            self.dirty = true;
        }
        if self.root_id == Some(id) {
            self.root_id = None;
        }
        if let Some(parent_id) = parent_id {
            self.mark_changed(parent_id);
        }
        destroyed
    }

    fn destroy_element_recursive(&mut self, id: u64, destroyed: &mut Vec<u64>) {
        if let Some(element) = self.elements.remove(&id) {
            self.removals += 1;
            self.motion_ids.remove(&id);
            self.image_ids.remove(&id);
            self.motion_value_ids.remove(&id);
            self.control_ids.remove(&id);
            self.focusable_ids.remove(&id);
            self.geometry_ids.remove(&id);
            self.inert_count -= usize::from(element.has_inert_prop());
            self.visibility_count -= usize::from(element.declared_hidden().is_some());
            self.anchored_count -= usize::from(element.element_type == "anchored");
            self.escaping_count -= usize::from(containing_block::may_escape(&element));
            self.container_rule_count -= usize::from(has_container_rules(&element));
            self.resolved.get_mut().forget(id);
            destroyed.push(id);
            for child_id in element.children {
                self.destroy_element_recursive(child_id, destroyed);
            }
        }
    }

    pub fn append_child(&mut self, parent_id: u64, child_id: u64) {
        // Remove from old parent if any
        let old_parent_id = self.elements.get(&child_id).and_then(|e| e.parent);
        if let Some(old_parent_id) = old_parent_id {
            if let Some(old_parent) = self.elements.get_mut(&old_parent_id) {
                old_parent.children.retain(|c| *c != child_id);
            }
        }
        // Set new parent
        if let Some(child) = self.elements.get_mut(&child_id) {
            child.parent = Some(parent_id);
        }
        // Add to new parent's children
        if let Some(parent) = self.elements.get_mut(&parent_id) {
            parent.children.push(child_id);
        }
        if self.inert_count > 0 || self.visibility_count > 0 {
            self.track_focusable_subtree(child_id);
        }
        if let Some(old_parent_id) = old_parent_id {
            self.mark_changed(old_parent_id);
        }
        self.mark_changed(parent_id);
    }

    pub fn insert_before(&mut self, parent_id: u64, child_id: u64, before_id: u64) {
        // Remove from old parent if any
        let old_parent_id = self.elements.get(&child_id).and_then(|e| e.parent);
        if let Some(old_parent_id) = old_parent_id {
            if let Some(old_parent) = self.elements.get_mut(&old_parent_id) {
                old_parent.children.retain(|c| *c != child_id);
            }
        }
        // Set new parent
        if let Some(child) = self.elements.get_mut(&child_id) {
            child.parent = Some(parent_id);
        }
        // Insert before the target
        if let Some(parent) = self.elements.get_mut(&parent_id) {
            let pos = parent
                .children
                .iter()
                .position(|c| *c == before_id)
                .unwrap_or(parent.children.len());
            parent.children.insert(pos, child_id);
        }
        if self.inert_count > 0 || self.visibility_count > 0 {
            self.track_focusable_subtree(child_id);
        }
        if let Some(old_parent_id) = old_parent_id {
            self.mark_changed(old_parent_id);
        }
        self.mark_changed(parent_id);
    }

    /// Takes an interned style. Pointer identity is the fast path and covers
    /// every resend, because `StyleTable::intern` returns the same `Arc` for
    /// the same bytes. It is not sufficient on its own: interning keys on raw
    /// bytes, so `{"color":"red","width":10}` and `{"width":10,"color":"red"}`
    /// are two `Arc`s holding the same style. Only a pointer miss pays for the
    /// ~80-field compare, which is what decides whether this repaints.
    ///
    /// The comparison is on the declared style, before the universal layer is
    /// put under it, so a resend costs no merge.
    pub fn set_style(&mut self, id: u64, style: Arc<StyleDesc>) {
        let Some(element) = self.elements.get_mut(&id) else {
            return;
        };
        let same = element
            .own_style
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &style) || **current == *style);
        if same {
            return;
        }
        element.own_style = Some(style);
        let change = self.refresh_effective_style(id);
        self.settle_style_change(id, change);
    }

    /// The style layer under every element's own style, as a CSS `* { ... }` rule
    /// would be, or none. Restyles every element once, as if each had been given
    /// its new effective style with `set_style`.
    pub fn set_universal_style(&mut self, universal: Option<Arc<StyleDesc>>) {
        if same_style(self.universal.as_ref(), universal.as_ref()) {
            return;
        }
        self.universal = universal;
        self.styles.layered.clear();
        let ids: Vec<u64> = self.elements.keys().copied().collect();
        let mut total = StyleChange::default();
        for id in ids {
            total.absorb(self.refresh_effective_style(id));
        }
        if total.visibility_changed {
            // One pass from the tops of the trees instead of a walk below every element.
            let tops: Vec<u64> = self
                .elements
                .values()
                .filter(|element| element.parent.is_none())
                .map(|element| element.id)
                .collect();
            for top in tops {
                self.track_focusable_subtree(top);
            }
        }
        if total.changed {
            let revision = self.take_revision();
            let search = total.regrouped || total.text_reprocessed;
            for element in self.elements.values_mut() {
                element.subtree_revision = revision;
                if search {
                    element.search_revision = revision;
                }
            }
        }
    }

    /// Recompute the effective style of `id` from the style it declared and the
    /// universal layer, and assign it.
    fn refresh_effective_style(&mut self, id: u64) -> StyleChange {
        let Some(element) = self.elements.get(&id) else {
            return StyleChange::default();
        };
        let effective = match (&self.universal, &element.own_style) {
            (Some(universal), own) if element.takes_universal_style() => Some(match own {
                Some(own) => self.styles.layered.effective(universal, own),
                None => universal.clone(),
            }),
            (_, own) => own.clone(),
        };
        self.assign_style(id, effective)
    }

    /// The one place `RetainedElement::style` is written, and the bookkeeping
    /// that depends on it: paragraph grouping, the style a transition starts
    /// from, containing-block escapes and `visibility`. Marking the tree
    /// changed is the caller's, because the universal layer does it once for
    /// all elements.
    fn assign_style(&mut self, id: u64, style: Option<Arc<StyleDesc>>) -> StyleChange {
        let Some(element) = self.elements.get_mut(&id) else {
            return StyleChange::default();
        };
        if same_style(element.style.as_ref(), style.as_ref()) {
            // Equal but not the same allocation: take the shared one.
            element.style = style;
            return StyleChange::default();
        }
        let was_declared = element.declared_hidden();
        let regrouped =
            !crate::text::inline::same_paragraphs(element.style.as_deref(), style.as_deref());
        let text_reprocessed = element.style.as_deref().and_then(|old| old.white_space.as_deref())
            != style.as_deref().and_then(|new| new.white_space.as_deref());
        let escaped_before = containing_block::may_escape(element);
        let container_rules_before = has_container_rules(element);
        let before = element.style.take().filter(|old| {
            !crate::motion::declares_transitions(old)
                && style.as_deref().is_some_and(crate::motion::declares_transitions)
        });
        if before.is_some() {
            element.extras_mut().before_transition = before;
        } else if let Some(extras) = element.extras.as_mut() {
            extras.before_transition = None;
            element.release_empty_extras();
        }
        element.style = style;
        self.style_serials += 1;
        element.style_serial = self.style_serials;
        *element.container_display.borrow_mut() = None;
        let escaped_after = containing_block::may_escape(element);
        self.escaping_count += usize::from(escaped_after);
        self.escaping_count -= usize::from(escaped_before);
        self.container_rule_count += usize::from(has_container_rules(element));
        self.container_rule_count -= usize::from(container_rules_before);
        let now_declared = element.declared_hidden();
        let visibility_changed = was_declared != now_declared;
        if visibility_changed {
            self.visibility_count = self.visibility_count + usize::from(now_declared.is_some())
                - usize::from(was_declared.is_some());
        }
        StyleChange {
            changed: true,
            regrouped,
            text_reprocessed,
            visibility_changed,
        }
    }

    fn settle_style_change(&mut self, id: u64, change: StyleChange) {
        if change.visibility_changed {
            self.track_focusable_subtree(id);
        }
        if change.text_reprocessed {
            // `white-space` is inherited and decides what text is left of a
            // string, so the searchable text of every descendant changes.
            self.mark_subtree_changed(id);
        } else if change.regrouped {
            // `display` decides which text nodes form one paragraph, and a
            // paragraph is what search matches.
            self.mark_changed(id);
        } else if change.changed {
            self.mark_render_changed(id);
        }
    }

    pub fn set_text(&mut self, id: u64, content: String) {
        let mut changed = false;
        if let Some(element) = self.elements.get_mut(&id) {
            if element.content.as_ref() != Some(&content) {
                element.content = Some(content);
                changed = true;
            }
        }
        if changed {
            if self.universal.is_some() {
                // The string makes this a text node, which no style layer reaches.
                let change = self.refresh_effective_style(id);
                self.settle_style_change(id, change);
            }
            self.mark_changed(id);
        }
    }

    pub fn set_event_listener(&mut self, id: u64, event_type: String, has_handler: bool) {
        let geometry = crate::observers::reports_geometry(&event_type);
        if let Some(element) = self.elements.get_mut(&id) {
            let changed = if has_handler {
                element.extras_mut().events.insert(event_type)
            } else if let Some(extras) = element.extras.as_mut() {
                let changed = extras.events.remove(&event_type);
                element.release_empty_extras();
                changed
            } else {
                false
            };
            // Which listeners exist decides how the element is built, so it takes a revision:
            // `retain` keys what it reuses by it. For a `<text>` it also decides whether it is
            // raw text (`is_raw_text`), which the paragraph builder and search read it by.
            if changed {
                if geometry {
                    let reports = self
                        .elements
                        .get(&id)
                        .is_some_and(|element| element.events().iter().any(|event| crate::observers::reports_geometry(event)));
                    if reports {
                        self.geometry_ids.insert(id);
                    } else {
                        self.geometry_ids.remove(&id);
                    }
                }
                if self
                    .elements
                    .get(&id)
                    .is_some_and(|element| element.element_type == "text")
                {
                    self.mark_changed(id);
                } else {
                    self.mark_render_changed(id);
                }
            }
        }
    }

    /// Set a custom prop on an element (for non-div/text elements).
    ///
    /// Custom props never change the searchable text of a subtree: a native
    /// element's strings are matched at paint, not collected into a group. The
    /// one exception is `highlight` itself appearing or disappearing, which
    /// changes which subtrees an ancestor skips.
    pub fn set_custom_prop(&mut self, id: u64, key: String, value: serde_json::Value) {
        let mut changed = false;
        let is_highlight = key == "highlight";
        let is_motion = key == "motion";
        let is_motion_values = key == "motionValues" || key == "layout";
        let affects_focusability = key == "tabIndex" || key == "disabled";
        let is_inert = key == "inert";
        let is_form_control = key == FORM_CONTROL_PROP;
        let was_inert = self
            .elements
            .get(&id)
            .is_some_and(RetainedElement::has_inert_prop);
        let was_declaration = self
            .elements
            .get(&id)
            .is_some_and(|element| element.custom_props().contains_key("highlight"));
        if let Some(element) = self.elements.get_mut(&id) {
            // `autoFocus` applies to every element type, so it is lifted out of
            // the custom-prop map that only custom elements read.
            if key == "autoFocus" {
                let auto_focus = value.as_bool().unwrap_or(false);
                self.dirty |= element.auto_focus != auto_focus;
                self.auto_focus_requested |= auto_focus && !element.auto_focus;
                element.auto_focus = auto_focus;
                return;
            }
            if key == "testId" {
                let test_id = value.as_str().map(str::to_string);
                self.dirty |= element.test_id != test_id;
                element.test_id = test_id;
                return;
            }
            if value.is_null() {
                changed = element
                    .extras
                    .as_mut()
                    .is_some_and(|extras| extras.custom_props.remove(&key).is_some());
                element.release_empty_extras();
            } else if element.custom_props().get(&key) != Some(&value) {
                element.extras_mut().custom_props.insert(key, value);
                changed = true;
            }
        }
        if !changed {
            return;
        }
        if affects_focusability {
            self.track_focusable(id);
            // A focusable `<text>` is a block rather than an inline box.
            if self
                .elements
                .get(&id)
                .is_some_and(|element| element.element_type == "text")
            {
                self.mark_changed(id);
            }
        }
        if is_form_control {
            if self.elements.get(&id).is_some_and(|element| element.control_disabled().is_some()) {
                self.control_ids.insert(id);
            } else {
                self.control_ids.remove(&id);
            }
        }
        if is_inert {
            let now_inert = self
                .elements
                .get(&id)
                .is_some_and(RetainedElement::has_inert_prop);
            self.inert_count = self.inert_count + usize::from(now_inert) - usize::from(was_inert);
            self.track_focusable_subtree(id);
        }
        if is_motion {
            if self
                .elements
                .get(&id)
                .is_some_and(|element| element.custom_props().contains_key("motion"))
            {
                self.motion_ids.insert(id);
            } else {
                self.motion_ids.remove(&id);
            }
        }
        if is_motion_values {
            if self
                .elements
                .get(&id)
                .is_some_and(|element| reads_motion_values(element.custom_props()))
            {
                self.motion_value_ids.insert(id);
            } else {
                self.motion_value_ids.remove(&id);
            }
        }
        self.mark_render_changed(id);
        let is_declaration = self
            .elements
            .get(&id)
            .is_some_and(|element| element.custom_props().contains_key("highlight"));
        if is_highlight && was_declaration != is_declaration {
            // Only the ancestors: `GroupList::collect` skips a nested
            // declaration's subtree, so which subtrees exist changed for them.
            // This element's own groups are unaffected by its own query.
            if let Some(parent) = self.elements.get(&id).and_then(|element| element.parent) {
                self.mark_changed(parent);
            }
        }
    }

    /// Read a custom prop value from an element.
    pub fn get_custom_prop(&self, id: u64, key: &str) -> Option<&serde_json::Value> {
        self.elements.get(&id)?.custom_props().get(key)
    }

    pub fn to_json(
        &self,
        bounds: &std::collections::HashMap<u64, crate::automation::ElementBounds>,
    ) -> serde_json::Value {
        self.to_json_detail(bounds, true)
    }

    /// Locator tree. Skip style maps so a 5k-row list is not 100ms of JSON.
    pub fn to_automation_json(
        &self,
        bounds: &std::collections::HashMap<u64, crate::automation::ElementBounds>,
    ) -> serde_json::Value {
        self.to_json_detail(bounds, false)
    }

    /// The elements whose painted box holds the point, topmost first: the DOM's
    /// `elementsFromPoint`.
    ///
    /// The order is the one gpui paints and hit tests in, and `layout::stacking` decides it for
    /// both. An element that forms a stacking context (the window root, a virtual-list row, an
    /// `anchored` layer, or one whose style says so) paints its own box, then the positioned
    /// descendants it was handed with a negative `z-index`, then its in-flow content in tree
    /// order, then the other lifted descendants by `(z-index, tree order)`. A lifted element (a
    /// positioned one, a flex or grid item with a `z-index`, an effect group) goes to the nearest
    /// context around it however many static boxes sit between.
    /// Where no context is active (below a `visibility: hidden` element) the children of one
    /// parent are ordered by `paint_rank`. A deferred `anchored` layer is drawn after the tree
    /// by `priority`, without the clip of the elements around it: its hits come first.
    ///
    /// `painted_box` is the last frame's box. An element that was not painted
    /// (outside a virtual list's window) has none and is never hit, though its
    /// descendants are still visited. A `visibility: hidden` element has its box
    /// and is skipped the same way, and `visibility: visible` on a descendant
    /// brings it back. What clips matches what is
    /// drawn: `overflow` of `hidden` or `scroll`, per axis, hides whatever lies
    /// outside the clipping box. `pointer-events: none` is inherited, like in
    /// CSS, and `auto` restores it.
    ///
    /// Styles are the ones each element was last built with (`resolved_style`), so a `hover`
    /// state, an `@container` block, a transition, a CSS animation or a motion value that sets
    /// `position`, `z-index`, `opacity`, `visibility`, `pointer-events` or `overflow` counts. An
    /// element that was never built (a virtual-list row outside the window) has its declared
    /// style.
    pub fn elements_at(
        &self,
        x: f64,
        y: f64,
        painted_box: &dyn Fn(u64) -> Option<crate::automation::ElementBounds>,
    ) -> Vec<u64> {
        let mut walk = HitWalk {
            x,
            y,
            painted_box,
            layers: Vec::new(),
            frames: Vec::new(),
            colors: crate::color::ColorContext::root(crate::color::Scheme::Light),
            lengths: crate::css::length::LengthContext::initial(0.0, 0.0),
        };
        let mut hits = match self.root_id {
            Some(root_id) => self.paint_order(root_id, false, false, false, false, &mut walk, None),
            None => Vec::new(),
        };
        hits.reverse();
        // A layer met later is drawn over an earlier one of the same priority, and one inside
        // another is drawn over it.
        walk.layers.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
        let mut ordered: Vec<u64> = walk
            .layers
            .into_iter()
            .flat_map(|(_, _, layer)| layer.into_iter().rev())
            .collect();
        ordered.extend(hits);
        ordered
    }

    /// The hits of `id` and what it paints, from the bottom up. `none` is the inherited
    /// `pointer-events: none`. `inert` is stronger: no `pointer-events: auto` below an `inert`
    /// element restores hits. `clipped` means an ancestor's clip excludes the point, so only
    /// deferred layers below can still be hit. `hidden` is the inherited `visibility`. `context` is the stacking context `id` is
    /// painted in, which collects the positioned elements below it, or none where nothing is
    /// lifted.
    #[allow(clippy::too_many_arguments)]
    fn paint_order(
        &self,
        id: u64,
        none: bool,
        inert: bool,
        clipped: bool,
        hidden: bool,
        walk: &mut HitWalk,
        context: Option<&mut Context>,
    ) -> Vec<u64> {
        let Some(element) = self.elements.get(&id) else {
            return Vec::new();
        };
        let resolved = self.resolved_of(element, &walk.colors, &walk.lengths);
        let own_box = (walk.painted_box)(id);
        let (x, y) = (walk.x, walk.y);
        let inside = |bounds: &crate::automation::ElementBounds, horizontal: bool, vertical: bool| {
            (!horizontal || (x >= bounds.x && x < bounds.x + bounds.width))
                && (!vertical || (y >= bounds.y && y < bounds.y + bounds.height))
        };

        let mut clipped = match containing_block::escape(self, element) {
            None => clipped,
            Some(gpui::Escape::Viewport) => false,
            Some(gpui::Escape::ContainingBlock) => walk.frames.last().copied().unwrap_or(false),
        };
        if let Some(bounds) = &own_box {
            let (clip_x, clip_y) = (resolved.overflow.x.clips(), resolved.overflow.y.clips());
            if (clip_x || clip_y) && !inside(bounds, clip_x, clip_y) {
                if self.anchored_count == 0 && self.escaping_count == 0 && !self.resolved.borrow().any_escaping() {
                    return Vec::new();
                }
                clipped = true;
            }
        }

        let none = match resolved.pointer_events {
            PointerEvents::None => true,
            PointerEvents::Auto => false,
            PointerEvents::Inherit => none,
        };
        let inert = inert || element.has_inert_prop();
        let none = none || inert;
        let hidden = match resolved.visibility {
            Visibility::Hidden => true,
            Visibility::Visible => false,
            Visibility::Inherit => hidden,
        };

        if element.element_type == "anchored" {
            let deferred = element
                .custom_props()
                .get("deferred")
                .and_then(|value| value.as_bool())
                .unwrap_or(true);
            if deferred {
                let priority = element
                    .custom_props()
                    .get("priority")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(1);
                let met = walk.layers.len();
                walk.layers.push((priority, met, Vec::new()));
                let mut layer = Context::default();
                walk.frames.push(false);
                let in_flow = self.paint_children(element, none, inert, false, hidden, walk, Some(&mut layer));
                walk.frames.pop();
                // The layer's own box is a hit too, below what it holds: no ancestor clips it.
                let own = (!none && !hidden && own_box.is_some_and(|bounds| inside(&bounds, true, true))).then_some(id);
                walk.layers[met].2 = layer.paint_order(own, in_flow);
                return Vec::new();
            }
        }

        let item = crate::layout::stacking::is_item(self, element);
        let mut own_context = (crate::layout::stacking::is_forced_context(self, element)
            || (!resolved.contents && crate::layout::stacking::creates_context(&resolved, item)))
        .then(Context::default);
        // Nothing under a hidden element is painted by a context above it: a `visible`
        // descendant is, in place, by `paint_rank`.
        let children_context = match own_context.as_mut() {
            Some(own) => Some(own),
            None if hidden => None,
            None => context,
        };

        let opens_frame = containing_block::is_positioned(self, element) || element.element_type == "virtual-list";
        if opens_frame {
            walk.frames.push(clipped);
        }
        let in_flow = self.paint_children(element, none, inert, clipped, hidden, walk, children_context);
        if opens_frame {
            walk.frames.pop();
        }
        let own = (!clipped && !none && !hidden && own_box.is_some_and(|bounds| inside(&bounds, true, true))).then_some(id);
        match own_context {
            Some(own_context) => own_context.paint_order(own, in_flow),
            None => own.into_iter().chain(in_flow).collect(),
        }
    }

    /// The in-flow hits of the children of `element`, from the bottom up. A positioned child
    /// that `context` takes is registered in it instead, and a child met with no context is
    /// placed among its siblings by its paint rank.
    #[allow(clippy::too_many_arguments)]
    fn paint_children(
        &self,
        element: &RetainedElement,
        none: bool,
        inert: bool,
        clipped: bool,
        hidden: bool,
        walk: &mut HitWalk,
        context: Option<&mut Context>,
    ) -> Vec<u64> {
        let mut in_flow = Vec::new();
        match context {
            Some(context) => {
                for &child in &element.children {
                    let lifted = self
                        .elements
                        .get(&child)
                        .and_then(|child| {
                            crate::layout::stacking::lifted_z(self, child, &walk.colors, &walk.lengths)
                        });
                    match lifted {
                        Some(z) => {
                            let ordinal = context.take_ordinal();
                            let hits =
                                self.paint_order(child, none, inert, clipped, hidden, walk, Some(&mut *context));
                            context.lifted.push((z, ordinal, hits));
                        }
                        None => in_flow.extend(self.paint_order(
                            child,
                            none,
                            inert,
                            clipped,
                            hidden,
                            walk,
                            Some(&mut *context),
                        )),
                    }
                }
            }
            None => {
                let mut ranked: Vec<(i32, u64)> = element
                    .children
                    .iter()
                    .map(|&child| {
                        let rank = self.elements.get(&child).map_or(0, |element| {
                            crate::layout::stacking::paint_rank(
                                &self.resolved_of(element, &walk.colors, &walk.lengths),
                                crate::layout::stacking::is_item(self, element),
                            )
                        });
                        (rank, child)
                    })
                    .collect();
                ranked.sort_by_key(|(rank, _)| *rank);
                for (_, child) in ranked {
                    in_flow.extend(self.paint_order(child, none, inert, clipped, hidden, walk, None));
                }
            }
        }
        in_flow
    }

    fn to_json_detail(
        &self,
        bounds: &std::collections::HashMap<u64, crate::automation::ElementBounds>,
        include_details: bool,
    ) -> serde_json::Value {
        match self.root_id {
            Some(root_id) => element_to_json(root_id, self, bounds, include_details),
            None => serde_json::Value::Null,
        }
    }
}

fn element_to_json(
    id: u64,
    tree: &RetainedTree,
    bounds: &std::collections::HashMap<u64, crate::automation::ElementBounds>,
    include_details: bool,
) -> serde_json::Value {
    let Some(element) = tree.elements.get(&id) else {
        return serde_json::Value::Null;
    };

    let mut obj = serde_json::Map::new();
    obj.insert(
        "type".to_string(),
        serde_json::Value::String(element.element_type.clone()),
    );
    obj.insert("id".to_string(), serde_json::json!(element.id));

    if let Some(ref test_id) = element.test_id {
        obj.insert(
            "testId".to_string(),
            serde_json::Value::String(test_id.clone()),
        );
    }

    if let Some(ref content) = element.content {
        obj.insert(
            "text".to_string(),
            serde_json::Value::String(content.clone()),
        );
    }

    if let Some(rect) = bounds.get(&id) {
        obj.insert(
            "bounds".to_string(),
            serde_json::json!({
                "x": rect.x,
                "y": rect.y,
                "width": rect.width,
                "height": rect.height,
            }),
        );
    }

    if include_details {
        if let Some(ref style) = element.style {
            // `as_ref`, not the `Arc`. Serializing the pointer needs serde's
            // `rc` feature, which we never asked for; it only compiles because
            // gpui happens to enable it, and would break when gpui stops.
            if let Ok(style_json) = serde_json::to_value(style.as_ref()) {
                if let serde_json::Value::Object(ref map) = style_json {
                    let filtered: serde_json::Map<String, serde_json::Value> = map
                        .iter()
                        .filter(|(_, v)| !v.is_null())
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    if !filtered.is_empty() {
                        obj.insert("style".to_string(), serde_json::Value::Object(filtered));
                    }
                }
            }
        }

        if !element.events().is_empty() {
            let mut events: Vec<String> = element.events().iter().cloned().collect();
            events.sort();
            obj.insert("events".to_string(), serde_json::json!(events));
        }

        if !element.custom_props().is_empty() {
            let custom: serde_json::Map<String, serde_json::Value> = element
                .custom_props()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            obj.insert("customProps".to_string(), serde_json::Value::Object(custom));
        }
    }

    if !element.children.is_empty() {
        let children: Vec<serde_json::Value> = element
            .children
            .iter()
            .map(|&cid| element_to_json(cid, tree, bounds, include_details))
            .filter(|v| !v.is_null())
            .collect();
        if !children.is_empty() {
            obj.insert("children".to_string(), serde_json::Value::Array(children));
        }
    }

    serde_json::Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(tree: &mut RetainedTree, json: &str) {
        crate::renderer::apply_batch_to_tree(tree, json.as_bytes()).expect("valid batch");
    }

    fn tree_with_child() -> RetainedTree {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "text".to_string());
        tree.create_element(3, "text".to_string());
        tree.append_child(1, 2);
        tree.append_child(2, 3);
        tree.set_text(3, "hello".to_string());
        tree
    }

    #[test]
    fn destroy_unlinks_from_parent() {
        let mut tree = tree_with_child();
        let destroyed = tree.destroy_element(2);
        assert_eq!(destroyed, vec![2, 3]);
        assert_eq!(tree.elements[&1].children, Vec::<u64>::new());
        assert!(!tree.elements.contains_key(&3));
    }

    #[test]
    fn destroy_bumps_the_parent_chain_revision() {
        let mut tree = tree_with_child();
        let before = tree.elements[&1].subtree_revision;
        tree.destroy_element(2);
        assert!(
            tree.elements[&1].subtree_revision > before,
            "destroying a child must invalidate a subtree_revision cache on the parent"
        );
    }

    #[test]
    fn destroying_the_root_clears_it() {
        let mut tree = tree_with_child();
        tree.root_id = Some(1);
        tree.destroy_element(1);
        assert_eq!(tree.root_id, None);
        assert!(tree.elements.is_empty());
    }

    /// The whole point of `search_revision`: `highlight` is a custom prop, so
    /// keying the group cache on `subtree_revision` means every find-bar
    /// keystroke re-walks and re-folds the subtree it exists to avoid.
    #[test]
    fn a_query_change_does_not_move_search_revision() {
        let mut tree = tree_with_child();
        tree.set_custom_prop(
            1,
            "highlight".to_string(),
            serde_json::json!({"query": "a"}),
        );
        let search = tree.elements[&1].search_revision;
        let subtree = tree.elements[&1].subtree_revision;

        tree.set_custom_prop(
            1,
            "highlight".to_string(),
            serde_json::json!({"query": "ab"}),
        );
        assert_eq!(tree.elements[&1].search_revision, search, "same text");
        assert!(
            tree.elements[&1].subtree_revision > subtree,
            "still repaints"
        );
    }

    #[test]
    fn style_does_not_move_search_revision() {
        let mut tree = tree_with_child();
        let search = tree.elements[&1].search_revision;
        let style = tree.styles.intern(br##"{"color":"#fff"}"##).unwrap();
        tree.set_style(3, style);
        assert_eq!(tree.elements[&1].search_revision, search);
        assert!(tree.elements[&1].subtree_revision > 0);
    }

    // ── Style interning ──────────────────────────────────────────────

    #[test]
    fn equal_style_bytes_share_one_allocation() {
        let mut tree = tree_with_child();
        let first = tree.styles.intern(br#"{"color":"red"}"#).unwrap();
        let second = tree.styles.intern(br#"{"color":"red"}"#).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(tree.styles.len(), 1);

        let other = tree.styles.intern(br#"{"color":"blue"}"#).unwrap();
        assert!(!Arc::ptr_eq(&first, &other));
        assert_eq!(tree.styles.len(), 2);
    }

    /// Re-sending the same style must not repaint. `commitUpdate` resends the
    /// full style on every commit, so without this every commit would dirty
    /// every element it touched.
    #[test]
    fn resending_the_same_style_is_not_a_change() {
        let mut tree = tree_with_child();
        let style = tree.styles.intern(br#"{"color":"red"}"#).unwrap();
        tree.set_style(3, style);
        let revision = tree.elements[&3].subtree_revision;

        let same = tree.styles.intern(br#"{"color":"red"}"#).unwrap();
        tree.set_style(3, same);
        assert_eq!(tree.elements[&3].subtree_revision, revision);

        let different = tree.styles.intern(br#"{"color":"blue"}"#).unwrap();
        tree.set_style(3, different);
        assert!(tree.elements[&3].subtree_revision > revision);
    }

    /// The table must not grow without bound. A dragged element produces a
    /// distinct style every frame, so the sweep is what keeps this safe.
    #[test]
    fn sweep_releases_styles_no_element_references() {
        let mut tree = tree_with_child();
        for frame in 0..50 {
            let payload = format!(r#"{{"left":{frame}}}"#);
            let style = tree.styles.intern(payload.as_bytes()).unwrap();
            tree.set_style(3, style);
            tree.styles.sweep();
            assert_eq!(tree.styles.len(), 1, "frame {frame} leaked a style");
        }
    }

    /// A drag through the batch entry point must keep the style table bounded.
    #[test]
    fn a_drag_through_apply_batch_stays_bounded() {
        let mut tree = tree_with_child();
        for frame in 0..1_000 {
            apply(
                &mut tree,
                &format!(r#"[["setStyle",3,{{"left":{frame}}}]]"#),
            );
            assert!(
                tree.styles.len() <= STYLE_SWEEP_FLOOR * 2,
                "frame {frame} grew the table to {}",
                tree.styles.len(),
            );
        }
    }

    /// Growth alone never fires again once the tree collapses: `swept_at` stays
    /// at the high-water mark and `count` cannot climb past it, because
    /// re-interning an existing style does not add an entry. Destroying the
    /// tree must reclaim the table anyway.
    #[test]
    fn destroying_the_tree_releases_its_styles() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.root_id = Some(1);

        let wide = STYLE_SWEEP_FLOOR * 4;
        for index in 0..wide {
            let child = 100 + index as u64;
            tree.create_element(child, "div".to_string());
            tree.append_child(1, child);
            let style = tree
                .styles
                .intern(format!(r#"{{"left":{index}}}"#).as_bytes())
                .unwrap();
            tree.set_style(child, style);
        }
        assert_eq!(tree.styles.len(), wide, "every style is still live");

        tree.destroy_element(1);
        assert!(tree.elements.is_empty(), "the tree is gone");

        // Exactly what `apply_batch_to_tree` does after a batch that destroyed
        // the root. Nothing new is interned, so only the element count can
        // trigger this.
        let live_elements = tree.elements.len();
        tree.styles.maybe_sweep(live_elements);
        assert_eq!(tree.styles.len(), 0, "the destroyed tree kept its styles");
    }

    /// A style table larger than the tree must not re-sweep on every commit.
    #[test]
    fn the_shrink_sweep_does_not_thrash() {
        let mut tree = RetainedTree::new();
        for index in 0..(STYLE_SWEEP_FLOOR * 4) {
            let id = index as u64 + 1;
            tree.create_element(id, "div".to_string());
            let style = tree
                .styles
                .intern(format!(r#"{{"left":{index}}}"#).as_bytes())
                .unwrap();
            tree.set_style(id, style);
        }
        let live = tree.styles.len();
        let live_elements = tree.elements.len();
        tree.styles.maybe_sweep(live_elements);
        assert_eq!(tree.styles.len(), live, "a full table must survive a sweep");
    }

    #[test]
    fn sweep_keeps_a_style_two_elements_still_use() {
        let mut tree = tree_with_child();
        let style = tree.styles.intern(br#"{"color":"red"}"#).unwrap();
        tree.set_style(2, style.clone());
        tree.set_style(3, style);
        tree.styles.sweep();
        assert_eq!(tree.styles.len(), 1);

        let replacement = tree.styles.intern(br#"{"color":"blue"}"#).unwrap();
        tree.set_style(3, replacement);
        tree.styles.sweep();
        assert_eq!(tree.styles.len(), 2, "element 2 still holds red");

        let replacement = tree.styles.intern(br#"{"color":"blue"}"#).unwrap();
        tree.set_style(2, replacement);
        tree.styles.sweep();
        assert_eq!(tree.styles.len(), 1, "red is now unreferenced");
    }

    /// Sharing must never let one element's animation restyle another.
    #[test]
    fn a_shared_style_is_never_mutated_through() {
        let mut tree = tree_with_child();
        let style = tree.styles.intern(br#"{"color":"red"}"#).unwrap();
        tree.set_style(2, style.clone());
        tree.set_style(3, style);

        // This is what the motion path does: copy out, then mutate the copy.
        let mut animated = tree.elements[&3].style.as_deref().cloned().unwrap();
        animated.color = Some("green".to_string());

        assert_eq!(
            tree.elements[&2].style.as_ref().unwrap().color.as_deref(),
            Some("red")
        );
        assert_eq!(
            tree.elements[&3].style.as_ref().unwrap().color.as_deref(),
            Some("red")
        );
    }

    /// A nested declaration appearing changes which subtrees an ancestor skips,
    /// so the ancestor's collected groups really did change.
    #[test]
    fn a_nested_declaration_appearing_moves_the_ancestor() {
        let mut tree = tree_with_child();
        let search = tree.elements[&1].search_revision;
        tree.set_custom_prop(
            2,
            "highlight".to_string(),
            serde_json::json!({"query": "a"}),
        );
        assert!(tree.elements[&1].search_revision > search);

        // Changing that nested query does not.
        let after = tree.elements[&1].search_revision;
        tree.set_custom_prop(
            2,
            "highlight".to_string(),
            serde_json::json!({"query": "b"}),
        );
        assert_eq!(tree.elements[&1].search_revision, after);
    }

    #[test]
    fn text_and_structure_move_search_revision() {
        let mut tree = tree_with_child();
        let search = tree.elements[&1].search_revision;
        tree.set_text(3, "changed".to_string());
        assert!(tree.elements[&1].search_revision > search);

        let after = tree.elements[&1].search_revision;
        tree.destroy_element(3);
        assert!(tree.elements[&1].search_revision > after);
    }

    /// `focusable_ids` stands in for a scan of every element, so it has to
    /// agree with `needs_focus` after any mutation that can change the answer.
    #[test]
    fn focusable_ids_follow_what_needs_focus_reads() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "input".to_string());
        tree.create_element(3, "div".to_string());
        tree.create_element(4, "div".to_string());
        tree.append_child(1, 2);
        tree.append_child(1, 3);
        tree.append_child(1, 4);
        let focusable =
            |tree: &RetainedTree| tree.focusable_ids.iter().copied().collect::<Vec<_>>();
        assert_eq!(focusable(&tree), vec![2], "enabled inputs take focus");

        // Listeners have nothing to do with focus; a tabIndex does.
        tree.set_event_listener(3, "keydown".to_string(), true);
        tree.set_event_listener(3, "focus".to_string(), true);
        tree.set_event_listener(1, "click".to_string(), true);
        assert_eq!(focusable(&tree), vec![2]);
        tree.set_custom_prop(4, "tabIndex".to_string(), serde_json::json!(0));
        assert_eq!(focusable(&tree), vec![2, 4]);

        // A disabled input is out of the set, and back in when re-enabled.
        tree.set_custom_prop(2, "disabled".to_string(), serde_json::json!(true));
        assert_eq!(focusable(&tree), vec![4]);
        tree.set_custom_prop(2, "disabled".to_string(), serde_json::json!(false));
        assert_eq!(focusable(&tree), vec![2, 4]);

        tree.set_custom_prop(4, "tabIndex".to_string(), serde_json::Value::Null);
        assert_eq!(focusable(&tree), vec![2]);

        tree.destroy_element(1);
        assert!(
            tree.focusable_ids.is_empty(),
            "destroying a subtree forgets its ids"
        );
    }

    /// 1 div > [2 input, 3 div(formControl) > 4 div, 5 div(plain `disabled`) > 6 div].
    fn control_tree() -> RetainedTree {
        let mut tree = RetainedTree::new();
        for (id, element_type) in [(1, "div"), (2, "input"), (3, "div"), (4, "div"), (5, "div"), (6, "div")] {
            tree.create_element(id, element_type.to_string());
        }
        for (parent, child) in [(1, 2), (1, 3), (3, 4), (1, 5), (5, 6)] {
            tree.append_child(parent, child);
        }
        tree
    }

    #[test]
    fn only_form_controls_are_disabled_or_enabled() {
        let mut tree = control_tree();
        tree.set_custom_prop(3, "formControl".to_string(), serde_json::json!("enabled"));
        tree.set_custom_prop(5, "disabled".to_string(), serde_json::json!(true));
        let state = |tree: &RetainedTree, id: u64| tree.elements[&id].control_disabled();
        assert_eq!(state(&tree, 2), Some(false), "an input is a control");
        assert_eq!(state(&tree, 3), Some(false), "so is a div standing in for a button");
        assert_eq!(state(&tree, 5), None, "a plain div with `disabled` is not one, as in a DOM");
        assert_eq!(state(&tree, 1), None);

        tree.set_custom_prop(2, "disabled".to_string(), serde_json::json!(true));
        tree.set_custom_prop(3, "formControl".to_string(), serde_json::json!("disabled"));
        assert_eq!((state(&tree, 2), state(&tree, 3)), (Some(true), Some(true)));
        tree.set_custom_prop(3, "formControl".to_string(), serde_json::json!("nonsense"));
        assert_eq!(state(&tree, 3), None);
    }

    #[test]
    fn in_disabled_form_control_agrees_with_control_disabled() {
        let mut tree = control_tree();
        tree.set_custom_prop(3, "formControl".to_string(), serde_json::json!("disabled"));
        tree.set_custom_prop(5, "disabled".to_string(), serde_json::json!(true));
        assert!(tree.in_disabled_form_control(3) && tree.in_disabled_form_control(4));
        assert!(!tree.in_disabled_form_control(5) && !tree.in_disabled_form_control(6));
        tree.set_custom_prop(3, "formControl".to_string(), serde_json::json!("enabled"));
        assert!(!tree.in_disabled_form_control(4));
    }

    #[test]
    fn the_ancestors_of_controls_follow_what_the_controls_become() {
        let mut tree = control_tree();
        tree.set_custom_prop(3, "formControl".to_string(), serde_json::json!("enabled"));
        let ancestors = |tree: &RetainedTree| {
            let styles = ControlStyles::default();
            let ids = |has: &dyn Fn(u64) -> bool| (1..=6).filter(|id| has(*id)).collect::<Vec<_>>();
            (
                ids(&|id| styles.has_disabled(tree, id)),
                ids(&|id| styles.has_enabled(tree, id)),
            )
        };
        assert_eq!(ancestors(&tree), (vec![], vec![1]), "a control is not its own ancestor");

        tree.set_custom_prop(2, "disabled".to_string(), serde_json::json!(true));
        assert_eq!(ancestors(&tree), (vec![1], vec![1]));
        tree.set_custom_prop(3, "formControl".to_string(), serde_json::json!("disabled"));
        assert_eq!(ancestors(&tree), (vec![1], vec![]));

        tree.append_child(3, 2);
        assert_eq!(ancestors(&tree), (vec![1, 3], vec![]), "a nested control marks every ancestor");
        tree.set_custom_prop(3, "formControl".to_string(), serde_json::Value::Null);
        assert_eq!(ancestors(&tree), (vec![1, 3], vec![]));
        tree.destroy_element(2);
        assert_eq!(ancestors(&tree), (vec![], vec![]), "a destroyed control marks nothing");
    }

    #[test]
    fn visibility_hidden_takes_an_element_out_of_focusable_ids_until_a_descendant_shows() {
        let mut tree = RetainedTree::new();
        for id in 1..=4 {
            tree.create_element(id, "div".to_string());
        }
        tree.append_child(1, 2);
        tree.append_child(2, 3);
        tree.append_child(1, 4);
        for id in [2, 3, 4] {
            tree.set_custom_prop(id, "tabIndex".to_string(), serde_json::json!(0));
        }
        let focusable =
            |tree: &RetainedTree| tree.focusable_ids.iter().copied().collect::<Vec<_>>();
        let visibility = |value: &str| {
            Arc::new(StyleDesc {
                visibility: Some(value.to_string()),
                ..Default::default()
            })
        };
        assert_eq!(focusable(&tree), vec![2, 3, 4]);

        tree.set_style(2, visibility("hidden"));
        assert_eq!(focusable(&tree), vec![4], "the element and its descendants");

        tree.set_style(3, visibility("visible"));
        assert_eq!(focusable(&tree), vec![3, 4]);

        // Moving a subtree under a hidden element, or out of one, follows the new ancestors.
        tree.append_child(2, 4);
        assert_eq!(focusable(&tree), vec![3]);
        tree.append_child(1, 4);
        assert_eq!(focusable(&tree), vec![3, 4]);

        tree.set_style(2, Arc::new(StyleDesc::default()));
        assert_eq!(focusable(&tree), vec![2, 3, 4]);
        assert!(!tree.is_visibility_hidden(3));
    }

    #[test]
    fn an_inert_ancestor_takes_its_subtree_out_of_focusable_ids() {
        let mut tree = RetainedTree::new();
        for id in 1..=5 {
            tree.create_element(id, if id == 3 { "input" } else { "div" }.to_string());
        }
        tree.append_child(1, 2);
        tree.append_child(2, 3);
        tree.append_child(2, 4);
        tree.append_child(1, 5);
        for id in [2, 4, 5] {
            tree.set_custom_prop(id, "tabIndex".to_string(), serde_json::json!(0));
        }
        let focusable =
            |tree: &RetainedTree| tree.focusable_ids.iter().copied().collect::<Vec<_>>();
        assert_eq!(focusable(&tree), vec![2, 3, 4, 5]);

        tree.set_custom_prop(2, "inert".to_string(), serde_json::json!(true));
        assert_eq!(focusable(&tree), vec![5], "the element and its descendants");
        assert!(tree.is_inert(3) && !tree.is_inert(5));

        // A prop set below an inert ancestor does not bring the element back.
        tree.set_custom_prop(4, "tabIndex".to_string(), serde_json::json!(-1));
        tree.set_custom_prop(4, "inert".to_string(), serde_json::json!(false));
        assert_eq!(focusable(&tree), vec![5]);

        // Moving a subtree out, or in, follows the new ancestors.
        tree.append_child(1, 4);
        assert_eq!(focusable(&tree), vec![4, 5]);
        tree.append_child(2, 5);
        assert_eq!(focusable(&tree), vec![4]);

        tree.set_custom_prop(2, "inert".to_string(), serde_json::json!(false));
        assert_eq!(focusable(&tree), vec![2, 3, 4, 5]);
        tree.set_custom_prop(2, "inert".to_string(), serde_json::Value::Null);
        assert!(!tree.is_inert(3));
    }

    #[test]
    fn an_inert_subtree_is_not_hit_even_below_pointer_events_auto() {
        let (mut tree, painted) = hit_test_tree();
        assert!(tree.elements_at(70.0, 70.0, &painted).contains(&6));
        tree.set_custom_prop(5, "inert".to_string(), serde_json::json!(true));
        assert!(!tree.elements_at(70.0, 70.0, &painted).contains(&6));
        tree.set_custom_prop(5, "inert".to_string(), serde_json::json!(false));
        assert!(tree.elements_at(70.0, 70.0, &painted).contains(&6));
    }

    /// A page with a clipping scroller, overlapping siblings, and an overlay that
    /// does not take the pointer:
    ///
    /// ```text
    /// 1 root      0,0   100x100
    ///   2 scroller  0,0   50x50   overflow: hidden
    ///     3 row       0,40  50x30   (sticks out of the scroller's bottom)
    ///   4 sibling   40,0  50x50   (paints over the scroller)
    ///   5 overlay   0,0   100x100 pointer-events: none
    ///     6 button    60,60 20x20   pointer-events: auto
    /// ```
    fn hit_test_tree() -> (RetainedTree, impl Fn(u64) -> Option<crate::automation::ElementBounds>) {
        let mut tree = RetainedTree::new();
        for id in 1..=6 {
            tree.create_element(id, "div".to_string());
        }
        tree.set_root(1);
        for (parent, child) in [(1, 2), (2, 3), (1, 4), (1, 5), (5, 6)] {
            tree.append_child(parent, child);
        }
        let styled = |overflow: Option<&str>, pointer_events: Option<&str>| {
            Arc::new(StyleDesc {
                overflow: overflow.map(str::to_string),
                pointer_events: pointer_events.map(str::to_string),
                ..Default::default()
            })
        };
        tree.set_style(2, styled(Some("hidden"), None));
        tree.set_style(5, styled(None, Some("none")));
        tree.set_style(6, styled(None, Some("auto")));
        let boxes = [
            (1, (0.0, 0.0, 100.0, 100.0)),
            (2, (0.0, 0.0, 50.0, 50.0)),
            (3, (0.0, 40.0, 50.0, 30.0)),
            (4, (40.0, 0.0, 50.0, 50.0)),
            (5, (0.0, 0.0, 100.0, 100.0)),
            (6, (60.0, 60.0, 20.0, 20.0)),
        ];
        let lookup = move |id: u64| {
            boxes
                .iter()
                .find(|(candidate, _)| *candidate == id)
                .map(|(_, (x, y, width, height))| crate::automation::ElementBounds {
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                    client_left: 0.0,
                    client_top: 0.0,
                    border_right: 0.0,
                    border_bottom: 0.0,
                    padding_left: 0.0,
                    padding_top: 0.0,
                    padding_right: 0.0,
                    padding_bottom: 0.0,
                })
        };
        (tree, lookup)
    }

    #[test]
    fn elements_at_lists_what_is_under_the_point_topmost_first() {
        let (tree, painted) = hit_test_tree();
        // The later sibling paints over the scroller, and both sit in the root.
        assert_eq!(tree.elements_at(45.0, 10.0, &painted), vec![4, 2, 1]);
        // A descendant is above its ancestor.
        assert_eq!(tree.elements_at(10.0, 45.0, &painted), vec![3, 2, 1]);
        assert_eq!(tree.elements_at(500.0, 500.0, &painted), Vec::<u64>::new());
    }

    /// `1` (relative, the whole page) holds `2`, a 50 square clip at (100, 100), which holds `3`,
    /// a 40 square at (10, 10) of the page, far outside the clip. `clip` and `box_position` are
    /// their `position`.
    fn escaping_tree(
        clip_position: Option<&str>,
        box_position: &str,
    ) -> (RetainedTree, impl Fn(u64) -> Option<crate::automation::ElementBounds>) {
        let mut tree = RetainedTree::new();
        for id in 1..=3 {
            tree.create_element(id, "div".to_string());
        }
        tree.set_root(1);
        tree.append_child(1, 2);
        tree.append_child(2, 3);
        let styled = |position: Option<&str>, overflow: Option<&str>| {
            Arc::new(StyleDesc {
                position: position.map(str::to_string),
                overflow: overflow.map(str::to_string),
                ..Default::default()
            })
        };
        tree.set_style(1, styled(Some("relative"), None));
        tree.set_style(2, styled(clip_position, Some("hidden")));
        tree.set_style(3, styled(Some(box_position), None));
        let boxes = [
            (1, (0.0, 0.0, 200.0, 200.0)),
            (2, (100.0, 100.0, 50.0, 50.0)),
            (3, (10.0, 10.0, 40.0, 40.0)),
        ];
        let lookup = move |id: u64| {
            boxes
                .iter()
                .find(|(candidate, _)| *candidate == id)
                .map(|(_, (x, y, width, height))| crate::automation::ElementBounds {
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                    client_left: 0.0,
                    client_top: 0.0,
                    border_right: 0.0,
                    border_bottom: 0.0,
                    padding_left: 0.0,
                    padding_top: 0.0,
                    padding_right: 0.0,
                    padding_bottom: 0.0,
                })
        };
        (tree, lookup)
    }

    #[test]
    fn elements_at_hits_an_absolute_box_outside_a_static_clip() {
        let (tree, painted) = escaping_tree(None, "absolute");
        assert_eq!(tree.elements_at(20.0, 20.0, &painted), vec![3, 1]);
    }

    #[test]
    fn elements_at_clips_an_absolute_box_by_a_positioned_clip() {
        let (tree, painted) = escaping_tree(Some("relative"), "absolute");
        assert_eq!(tree.elements_at(20.0, 20.0, &painted), vec![1]);
    }

    #[test]
    fn elements_at_hits_a_fixed_box_outside_a_positioned_clip() {
        let (tree, painted) = escaping_tree(Some("relative"), "fixed");
        assert_eq!(tree.elements_at(20.0, 20.0, &painted), vec![3, 1]);
    }

    #[test]
    fn elements_at_clips_an_in_flow_box_by_a_static_clip_again_when_it_stops_escaping() {
        let (mut tree, painted) = escaping_tree(None, "absolute");
        let in_flow = Arc::new(StyleDesc {
            position: Some("static".to_string()),
            ..Default::default()
        });
        tree.set_style(3, in_flow);
        assert_eq!(tree.elements_at(20.0, 20.0, &painted), vec![1]);
    }

    fn built(tree: &RetainedTree, id: u64, json: &str) {
        let style: StyleDesc = serde_json::from_str(json).unwrap();
        let resolved = crate::resolved_style::Resolved::of(
            &tree.elements[&id],
            Some(&style),
            &crate::color::ColorContext::root(crate::color::Scheme::Light),
            &crate::css::length::LengthContext::for_tests(),
        );
        tree.resolved.borrow_mut().record(id, resolved);
    }

    #[test]
    fn elements_at_follows_the_position_and_overflow_an_element_was_built_with() {
        // A state made the static box absolute: it leaves the clip it was declared inside.
        let (tree, painted) = escaping_tree(None, "static");
        assert_eq!(tree.elements_at(20.0, 20.0, &painted), vec![1]);
        built(&tree, 3, r#"{"position":"absolute"}"#);
        assert_eq!(tree.elements_at(20.0, 20.0, &painted), vec![3, 1]);

        // A state made the clip a containing block, so the absolute box is cut by it again.
        let (tree, painted) = escaping_tree(None, "absolute");
        assert_eq!(tree.elements_at(20.0, 20.0, &painted), vec![3, 1]);
        built(&tree, 2, r#"{"position":"relative","overflow":"hidden"}"#);
        assert_eq!(tree.elements_at(20.0, 20.0, &painted), vec![1]);

        // And a clip that a state removed clips nothing.
        let (tree, painted) = escaping_tree(None, "static");
        built(&tree, 2, r#"{"overflow":"visible"}"#);
        assert_eq!(tree.elements_at(20.0, 20.0, &painted), vec![3, 1]);
    }

    #[test]
    fn elements_at_follows_the_pointer_events_and_visibility_an_element_was_built_with() {
        let (tree, painted) = hit_test_tree();
        assert_eq!(tree.elements_at(45.0, 10.0, &painted), vec![4, 2, 1]);
        built(&tree, 4, r#"{"pointerEvents":"none"}"#);
        assert_eq!(tree.elements_at(45.0, 10.0, &painted), vec![2, 1]);
        built(&tree, 4, r#"{"visibility":"hidden"}"#);
        assert_eq!(tree.elements_at(45.0, 10.0, &painted), vec![2, 1]);
        built(&tree, 4, "{}");
        assert_eq!(tree.elements_at(45.0, 10.0, &painted), vec![4, 2, 1]);
    }

    #[test]
    fn elements_at_orders_by_the_z_index_and_opacity_an_element_was_built_with() {
        // 2 and 3 are siblings that overlap at (45, 45) in the root; the later one is on top.
        let mut tree = RetainedTree::new();
        for id in 1..=3 {
            tree.create_element(id, "div".to_string());
            tree.set_style(id, Arc::new(StyleDesc::default()));
        }
        tree.set_root(1);
        tree.append_child(1, 2);
        tree.append_child(1, 3);
        let painted = |id: u64| {
            Some(crate::automation::ElementBounds {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                client_left: 0.0,
                client_top: 0.0,
                border_right: 0.0,
                border_bottom: 0.0,
                padding_left: 0.0,
                padding_top: 0.0,
                padding_right: 0.0,
                padding_bottom: 0.0,
            })
            .filter(|_| id != 0)
        };
        assert_eq!(tree.elements_at(45.0, 45.0, &painted), vec![3, 2, 1]);
        // A state gave the first one a `z-index` as a positioned box.
        built(&tree, 2, r#"{"position":"relative","zIndex":1}"#);
        assert_eq!(tree.elements_at(45.0, 45.0, &painted), vec![2, 3, 1]);
        // A motion value made the second one translucent, which lifts it to the 0 layer.
        built(&tree, 2, "{}");
        built(&tree, 3, r#"{"opacity":0.5}"#);
        assert_eq!(tree.elements_at(45.0, 45.0, &painted), vec![3, 2, 1]);
        built(&tree, 2, r#"{"position":"absolute"}"#);
        built(&tree, 3, "{}");
        assert_eq!(tree.elements_at(45.0, 45.0, &painted), vec![2, 3, 1]);
    }

    #[test]
    fn the_count_of_escaping_elements_follows_styles_and_destruction() {
        let (mut tree, _) = escaping_tree(None, "fixed");
        assert_eq!(tree.escaping_count, 1);
        tree.set_style(2, Arc::new(StyleDesc { position: Some("absolute".into()), ..Default::default() }));
        assert_eq!(tree.escaping_count, 2);
        tree.destroy_element(2);
        assert_eq!(tree.escaping_count, 0);
    }

    #[test]
    fn elements_at_stops_at_the_clip_edge() {
        let (tree, painted) = hit_test_tree();
        // y 60 is inside the row but below the scroller that clips it.
        assert_eq!(tree.elements_at(10.0, 60.0, &painted), vec![1]);
    }

    #[test]
    fn elements_at_skips_what_does_not_take_the_pointer() {
        let (tree, painted) = hit_test_tree();
        // The overlay covers everything yet is never hit...
        assert_eq!(tree.elements_at(10.0, 90.0, &painted), vec![1]);
        // ...while its child opts back in.
        assert_eq!(tree.elements_at(70.0, 70.0, &painted), vec![6, 1]);
    }

    #[test]
    fn elements_at_never_hits_what_was_not_painted() {
        let (tree, painted) = hit_test_tree();
        let without_the_sibling = |id: u64| if id == 4 { None } else { painted(id) };
        assert_eq!(tree.elements_at(45.0, 10.0, &without_the_sibling), vec![2, 1]);
    }

    #[test]
    fn elements_at_skips_visibility_hidden_until_a_descendant_shows_again() {
        let (mut tree, painted) = hit_test_tree();
        let visibility = |value: &str| {
            Arc::new(StyleDesc {
                visibility: Some(value.to_string()),
                ..Default::default()
            })
        };
        // The sibling and the scroller are hidden, so the pointer reaches the root under them.
        tree.set_style(4, visibility("hidden"));
        assert_eq!(tree.elements_at(45.0, 10.0, &painted), vec![2, 1]);
        tree.set_style(2, visibility("hidden"));
        assert_eq!(tree.elements_at(45.0, 10.0, &painted), vec![1]);
        // The row inherits it, and `visible` undoes it for the row alone.
        assert_eq!(tree.elements_at(10.0, 45.0, &painted), vec![1]);
        tree.set_style(3, visibility("visible"));
        assert_eq!(tree.elements_at(10.0, 45.0, &painted), vec![3, 1]);
    }

    fn boxed(
        boxes: &[(u64, (f64, f64, f64, f64))],
    ) -> impl Fn(u64) -> Option<crate::automation::ElementBounds> + '_ {
        move |id| {
            boxes
                .iter()
                .find(|(candidate, _)| *candidate == id)
                .map(|(_, (x, y, width, height))| crate::automation::ElementBounds {
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                    client_left: 0.0,
                    client_top: 0.0,
                    border_right: 0.0,
                    border_bottom: 0.0,
                    padding_left: 0.0,
                    padding_top: 0.0,
                    padding_right: 0.0,
                    padding_bottom: 0.0,
                })
        }
    }

    /// A popover lives inside a clipping scroller, early in the tree, yet gpui
    /// paints it after everything else and outside that clip.
    #[test]
    fn elements_at_puts_a_deferred_layer_above_the_tree_and_outside_its_clip() {
        let mut tree = RetainedTree::new();
        for (id, kind) in [
            (1, "div"),
            (2, "div"),
            (3, "anchored"),
            (4, "div"),
            (5, "div"),
        ] {
            tree.create_element(id, kind.to_string());
        }
        tree.set_root(1);
        for (parent, child) in [(1, 2), (2, 3), (3, 4), (1, 5)] {
            tree.append_child(parent, child);
        }
        tree.set_style(
            2,
            Arc::new(StyleDesc {
                overflow: Some("hidden".to_string()),
                ..Default::default()
            }),
        );
        let boxes = [
            (1, (0.0, 0.0, 100.0, 100.0)),
            (2, (0.0, 0.0, 20.0, 20.0)),
            (4, (50.0, 50.0, 30.0, 30.0)),
            (5, (40.0, 40.0, 60.0, 60.0)),
        ];
        // The popover content is outside its scroller's clip and under a later
        // sibling, and is still what the point lands on.
        assert_eq!(tree.elements_at(60.0, 60.0, &boxed(&boxes)), vec![4, 5, 1]);

        // Ordinary clipping is untouched.
        tree.destroy_element(3);
        tree.create_element(6, "div".to_string());
        tree.append_child(2, 6);
        let with_clipped_child = [(1, (0.0, 0.0, 100.0, 100.0)), (2, (0.0, 0.0, 20.0, 20.0)), (6, (0.0, 0.0, 90.0, 90.0))];
        assert_eq!(tree.elements_at(60.0, 60.0, &boxed(&with_clipped_child)), vec![1]);
    }

    #[test]
    fn elements_at_orders_deferred_layers_by_priority() {
        let mut tree = RetainedTree::new();
        for (id, kind) in [(1, "div"), (2, "anchored"), (3, "div"), (4, "anchored"), (5, "div")] {
            tree.create_element(id, kind.to_string());
        }
        tree.set_root(1);
        for (parent, child) in [(1, 2), (2, 3), (1, 4), (4, 5)] {
            tree.append_child(parent, child);
        }
        let boxes = [
            (1, (0.0, 0.0, 100.0, 100.0)),
            (3, (0.0, 0.0, 100.0, 100.0)),
            (5, (0.0, 0.0, 100.0, 100.0)),
        ];
        // Equal priority: the layer later in the tree paints over the earlier.
        assert_eq!(tree.elements_at(10.0, 10.0, &boxed(&boxes)), vec![5, 3, 1]);

        tree.set_custom_prop(2, "priority".to_string(), serde_json::json!(5));
        assert_eq!(tree.elements_at(10.0, 10.0, &boxed(&boxes)), vec![3, 5, 1]);

        // Not deferred: painted in place, in tree order, under the clip.
        tree.set_custom_prop(2, "deferred".to_string(), serde_json::json!(false));
        assert_eq!(tree.elements_at(10.0, 10.0, &boxed(&boxes)), vec![5, 3, 1]);
    }

    /// A tree from `(id, parent, type, style)`, the first node being the root. Every box is the
    /// whole 100 square, so the order of the hits is the paint order.
    fn stacked(
        nodes: Vec<(u64, u64, &str, StyleDesc)>,
    ) -> (RetainedTree, impl Fn(u64) -> Option<crate::automation::ElementBounds>) {
        let mut tree = RetainedTree::new();
        for (id, _, kind, style) in &nodes {
            tree.create_element(*id, kind.to_string());
            tree.set_style(*id, Arc::new(style.clone()));
        }
        tree.set_root(nodes[0].0);
        for (id, parent, _, _) in &nodes {
            if *parent != 0 {
                tree.append_child(*parent, *id);
            }
        }
        let ids: Vec<u64> = nodes.iter().map(|(id, ..)| *id).collect();
        let lookup = move |id: u64| {
            ids.contains(&id).then_some(crate::automation::ElementBounds {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                client_left: 0.0,
                client_top: 0.0,
                border_right: 0.0,
                border_bottom: 0.0,
                padding_left: 0.0,
                padding_top: 0.0,
                padding_right: 0.0,
                padding_bottom: 0.0,
            })
        };
        (tree, lookup)
    }

    fn flow() -> StyleDesc {
        StyleDesc::default()
    }

    fn at(position: &str, z_index: Option<f64>) -> StyleDesc {
        StyleDesc {
            position: Some(position.to_string()),
            z_index,
            ..Default::default()
        }
    }

    fn hits(nodes: Vec<(u64, u64, &str, StyleDesc)>) -> Vec<u64> {
        let (tree, painted) = stacked(nodes);
        tree.elements_at(50.0, 50.0, &painted)
    }

    #[test]
    fn elements_at_puts_a_z_indexed_descendant_above_the_later_sibling_of_its_ancestor() {
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", flow()),
            (3, 2, "div", at("relative", Some(5.0))),
            (4, 1, "div", flow()),
        ]);
        assert_eq!(order, [3, 4, 2, 1]);
    }

    #[test]
    fn elements_at_keeps_tree_order_between_positioned_elements_of_different_parents() {
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", flow()),
            (3, 2, "div", at("absolute", Some(1.0))),
            (4, 1, "div", flow()),
            (5, 4, "div", at("absolute", Some(1.0))),
        ]);
        assert_eq!(order, [5, 3, 4, 2, 1]);

        // A higher index in the earlier parent wins, and z-index 0 ties with auto.
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", flow()),
            (3, 2, "div", at("absolute", Some(2.0))),
            (4, 1, "div", flow()),
            (5, 4, "div", at("absolute", Some(1.0))),
        ]);
        assert_eq!(order, [3, 5, 4, 2, 1]);
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", at("absolute", None)),
            (3, 1, "div", at("absolute", Some(0.0))),
            (4, 1, "div", at("absolute", None)),
        ]);
        assert_eq!(order, [4, 3, 2, 1]);
    }

    #[test]
    fn elements_at_puts_a_negative_index_above_the_context_and_below_its_content() {
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", flow()),
            (3, 2, "div", at("absolute", Some(-1.0))),
            (4, 1, "div", flow()),
        ]);
        assert_eq!(order, [4, 2, 3, 1]);
    }

    #[test]
    fn elements_at_lets_a_positioned_ancestor_with_auto_z_hand_its_descendants_to_the_context() {
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", at("relative", None)),
            (3, 2, "div", at("absolute", Some(5.0))),
            (4, 1, "div", at("absolute", Some(2.0))),
        ]);
        assert_eq!(order, [3, 4, 2, 1]);
    }

    #[test]
    fn elements_at_traps_the_z_index_inside_a_context() {
        // 2 is a context by its z-index, 3 inside it cannot rise above 4.
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", at("relative", Some(1.0))),
            (3, 2, "div", at("absolute", Some(100.0))),
            (4, 1, "div", at("absolute", Some(2.0))),
        ]);
        assert_eq!(order, [4, 3, 2, 1]);

        // So does an effect group on a static box.
        let faded = StyleDesc { opacity: Some(0.5), ..Default::default() };
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", faded),
            (3, 2, "div", at("absolute", Some(100.0))),
            (4, 1, "div", at("absolute", Some(2.0))),
        ]);
        assert_eq!(order, [4, 3, 2, 1]);

        // A positioned element with a z-index is itself ordered in the context around it, and
        // its negative child is above its own box.
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", at("relative", Some(3.0))),
            (3, 2, "div", at("absolute", Some(-1.0))),
            (4, 1, "div", at("absolute", Some(2.0))),
        ]);
        assert_eq!(order, [3, 2, 4, 1]);
    }

    fn laid_out(display: &str) -> StyleDesc {
        StyleDesc { display: Some(display.to_string()), ..Default::default() }
    }

    fn item(z_index: Option<f64>) -> StyleDesc {
        StyleDesc { z_index, ..Default::default() }
    }

    #[test]
    fn elements_at_puts_a_flex_or_grid_item_with_a_z_index_above_the_later_sibling_of_its_container() {
        for display in ["flex", "grid"] {
            let order = hits(vec![
                (1, 0, "div", flow()),
                (2, 1, "div", laid_out(display)),
                (3, 2, "div", item(Some(5.0))),
                (4, 1, "div", flow()),
            ]);
            assert_eq!(order, [3, 4, 2, 1], "{display}");
        }
    }

    #[test]
    fn elements_at_leaves_a_static_box_with_a_z_index_in_tree_order_unless_it_is_an_item() {
        // z-index auto on an item, and an index in a block container: tree order.
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", laid_out("flex")),
            (3, 2, "div", item(None)),
            (4, 1, "div", flow()),
        ]);
        assert_eq!(order, [4, 3, 2, 1]);
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", laid_out("block")),
            (3, 2, "div", item(Some(5.0))),
            (4, 1, "div", flow()),
        ]);
        assert_eq!(order, [4, 3, 2, 1]);
    }

    #[test]
    fn elements_at_gives_a_display_contents_parents_children_to_the_flex_container() {
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", laid_out("flex")),
            (3, 2, "div", laid_out("contents")),
            (5, 3, "div", item(Some(5.0))),
            (4, 1, "div", flow()),
        ]);
        assert_eq!(order, [5, 4, 3, 2, 1]);
    }

    #[test]
    fn elements_at_orders_a_flex_item_by_its_z_index_among_positioned_boxes() {
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", laid_out("flex")),
            (3, 2, "div", item(Some(2.0))),
            (4, 1, "div", at("absolute", Some(1.0))),
            (5, 1, "div", at("absolute", Some(3.0))),
        ]);
        assert_eq!(order, [5, 3, 4, 2, 1]);
        // A negative index is below the in-flow boxes: the container's own box is above it.
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", laid_out("flex")),
            (3, 2, "div", item(Some(-1.0))),
            (4, 1, "div", flow()),
        ]);
        assert_eq!(order, [4, 2, 3, 1]);
    }

    #[test]
    fn elements_at_paints_a_translucent_static_box_with_the_positioned_ones() {
        let faded = StyleDesc { opacity: Some(0.5), ..Default::default() };
        // In the z=0 layer, in tree order with the positioned boxes, over the in-flow ones.
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", faded.clone()),
            (3, 1, "div", flow()),
            (4, 1, "div", at("relative", None)),
            (5, 1, "div", faded.clone()),
        ]);
        assert_eq!(order, [5, 4, 2, 3, 1]);
        // Under a positioned box with a higher index, over one with a lower.
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", at("absolute", Some(-1.0))),
            (3, 1, "div", faded.clone()),
            (4, 1, "div", at("absolute", Some(1.0))),
        ]);
        assert_eq!(order, [4, 3, 2, 1]);
        // From inside its own context, and from a deeper static box.
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", flow()),
            (3, 2, "div", faded),
            (4, 3, "div", at("absolute", Some(9.0))),
            (5, 1, "div", flow()),
        ]);
        assert_eq!(order, [4, 3, 5, 2, 1]);
    }

    #[test]
    fn elements_at_orders_the_children_of_a_hidden_element_by_paint_rank() {
        // Nothing is lifted out of a `visibility: hidden` element: 3 stays among its siblings.
        // The hidden box is no hit, and its `visible` descendants are.
        let hidden = StyleDesc { visibility: Some("hidden".to_string()), ..Default::default() };
        let shown = |style: StyleDesc| StyleDesc { visibility: Some("visible".to_string()), ..style };
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", hidden),
            (3, 2, "div", shown(at("absolute", Some(-1.0)))),
            (4, 2, "div", shown(flow())),
            (5, 2, "div", shown(at("relative", None))),
            (6, 1, "div", at("absolute", Some(2.0))),
        ]);
        // From the bottom: 1, 3 (negative), 4, 5 (positioned), then 6 in the context.
        assert_eq!(order, [6, 5, 4, 3, 1]);
    }

    #[test]
    fn elements_at_keeps_the_z_index_of_a_virtual_list_row_inside_the_row() {
        // The rows are layout roots: 5 does not rise above the later row 4.
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "virtual-list", flow()),
            (3, 2, "div", flow()),
            (5, 3, "div", at("absolute", Some(9.0))),
            (4, 2, "div", flow()),
        ]);
        assert_eq!(order, [4, 5, 3, 2, 1]);

        // Inside a row z-index works.
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "virtual-list", flow()),
            (3, 2, "div", flow()),
            (4, 3, "div", flow()),
            (5, 3, "div", at("absolute", Some(9.0))),
            (6, 4, "div", at("absolute", Some(10.0))),
        ]);
        assert_eq!(order, [6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn elements_at_orders_by_z_index_inside_a_deferred_layer() {
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "anchored", flow()),
            (3, 2, "div", flow()),
            (4, 3, "div", at("absolute", Some(7.0))),
            (5, 2, "div", flow()),
            (6, 1, "div", at("absolute", Some(99.0))),
        ]);
        // The layer (2, which is hit too) is above the tree whatever 6 asks for, and 4 rises above 5 inside it.
        assert_eq!(order, [4, 5, 3, 2, 6, 1]);
    }

    #[test]
    fn elements_at_lifts_a_fixed_box_and_makes_it_a_context() {
        let order = hits(vec![
            (1, 0, "div", flow()),
            (2, 1, "div", flow()),
            (3, 2, "div", at("fixed", None)),
            (4, 3, "div", at("absolute", Some(-1.0))),
            (5, 1, "div", flow()),
        ]);
        // 3 is above 5 and its negative child is above 3's own box.
        assert_eq!(order, [4, 3, 5, 2, 1]);
    }

    #[test]
    fn cold_event_and_prop_storage_is_released_when_empty() {
        let mut tree = tree_with_child();
        assert!(tree.elements[&1].extras.is_none());

        tree.set_event_listener(1, "click".to_string(), true);
        tree.set_custom_prop(1, "aria-label".to_string(), serde_json::json!("button"));
        assert!(tree.elements[&1].events().contains("click"));
        assert_eq!(
            tree.get_custom_prop(1, "aria-label"),
            Some(&serde_json::json!("button"))
        );

        tree.set_event_listener(1, "click".to_string(), false);
        assert!(tree.elements[&1].extras.is_some());
        tree.set_custom_prop(1, "aria-label".to_string(), serde_json::Value::Null);
        assert!(tree.elements[&1].extras.is_none());
    }

    #[test]
    fn the_style_before_a_transition_declaration_is_kept_until_the_next_style() {
        let mut tree = tree_with_child();
        let intern = |tree: &mut RetainedTree, json: &str| tree.styles.intern(json.as_bytes()).unwrap();
        let plain = intern(&mut tree, r#"{"opacity":0}"#);
        tree.set_style(1, plain);
        assert!(tree.elements[&1].style_before_transition().is_none(), "the first style has nothing before it");

        let declared = intern(&mut tree, r#"{"opacity":1,"transition":"opacity 1s"}"#);
        tree.set_style(1, declared);
        assert_eq!(tree.elements[&1].style_before_transition().unwrap().opacity, Some(0.0));

        let again = intern(&mut tree, r#"{"opacity":0.5,"transition":"opacity 1s"}"#);
        tree.set_style(1, again);
        assert!(tree.elements[&1].style_before_transition().is_none());
        assert!(tree.elements[&1].extras.is_none());

        let hover = intern(&mut tree, r#"{"opacity":0.5,"hover":{"transition":"opacity 1s"}}"#);
        tree.set_style(1, hover);
        let plain = intern(&mut tree, r#"{"opacity":0.5}"#);
        tree.set_style(1, plain);
        assert!(tree.elements[&1].style_before_transition().is_none());
    }

    #[test]
    fn set_text_bumps_every_ancestor() {
        let mut tree = tree_with_child();
        let before = tree.elements[&1].subtree_revision;
        tree.set_text(3, "changed".to_string());
        assert!(tree.elements[&1].subtree_revision > before);
        // An unchanged value must not invalidate anything.
        let after = tree.elements[&1].subtree_revision;
        tree.set_text(3, "changed".to_string());
        assert_eq!(tree.elements[&1].subtree_revision, after);
    }

    #[test]
    fn a_listener_appearing_or_going_takes_a_revision_and_a_resend_does_not() {
        let mut tree = tree_with_child();
        let before = tree.elements[&1].subtree_revision;
        tree.set_event_listener(1, "layout".to_string(), true);
        let added = tree.elements[&1].subtree_revision;
        assert!(added > before, "which listeners exist decides how the element is built");
        tree.set_event_listener(1, "layout".to_string(), true);
        assert_eq!(tree.elements[&1].subtree_revision, added);
        tree.set_event_listener(1, "layout".to_string(), false);
        assert!(tree.elements[&1].subtree_revision > added);
    }

    // ── The universal style layer ────────────────────────────────────

    fn layered_tree() -> RetainedTree {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "div".to_string());
        tree.create_element(3, "text".to_string());
        tree.append_child(1, 2);
        tree.append_child(1, 3);
        tree.set_text(3, "hello".to_string());
        apply(&mut tree, r##"[["setStyle",2,{"color":"#222222","padding":4}]]"##);
        tree
    }

    fn style_of(tree: &RetainedTree, id: u64) -> Option<&StyleDesc> {
        tree.elements[&id].style.as_deref()
    }

    #[test]
    fn the_universal_layer_is_the_lowest_precedence_per_property() {
        let mut tree = layered_tree();
        apply(
            &mut tree,
            r##"[["setUniversalStyle",{"color":"#111111","opacity":0.5,"paddingTop":9}]]"##,
        );
        let own = style_of(&tree, 2).unwrap();
        assert_eq!(own.color.as_deref(), Some("#222222"), "the element's own value wins");
        assert_eq!(own.opacity, Some(0.5), "an unset property comes from the layer");
        assert_eq!(own.padding_top, None, "an own shorthand shadows a layer longhand");
        assert!(own.padding.is_some());
    }

    #[test]
    fn an_element_with_no_style_and_one_created_later_get_the_layer() {
        let mut tree = layered_tree();
        apply(&mut tree, r##"[["setUniversalStyle",{"opacity":0.5}]]"##);
        assert_eq!(style_of(&tree, 1).unwrap().opacity, Some(0.5), "never given a style");
        apply(&mut tree, r##"[["createElement",9,"div"]]"##);
        assert_eq!(style_of(&tree, 9).unwrap().opacity, Some(0.5));
        assert!(tree.elements[&9].own_style.is_none());
        apply(&mut tree, r##"[["setStyle",9,{"width":10}]]"##);
        let later = style_of(&tree, 9).unwrap();
        assert_eq!((later.opacity, later.width.is_some()), (Some(0.5), true));
    }

    #[test]
    fn a_text_node_never_gets_the_layer_but_a_text_element_does() {
        let mut tree = layered_tree();
        apply(&mut tree, r##"[["setUniversalStyle",{"opacity":0.5}]]"##);
        assert!(style_of(&tree, 3).is_none(), "a string is not an element");
        assert!(crate::text::inline::is_raw_text(&tree.elements[&3]));

        // A text node is created before its string arrives.
        apply(&mut tree, r##"[["createElement",4,"text"],["createElement",5,"text"],["appendChild",4,5],["setText",5,"x"]]"##);
        assert_eq!(style_of(&tree, 4).unwrap().opacity, Some(0.5), "a `<text>` element");
        assert!(style_of(&tree, 5).is_none());
        assert!(crate::text::inline::is_raw_text(&tree.elements[&5]));
    }

    #[test]
    fn clearing_the_layer_restores_the_own_styles_exactly() {
        let mut tree = layered_tree();
        let own = tree.elements[&2].style.clone().unwrap();
        apply(&mut tree, r##"[["setUniversalStyle",{"opacity":0.5}]]"##);
        assert!(!Arc::ptr_eq(style_of_arc(&tree, 2), &own));
        apply(&mut tree, r##"[["setUniversalStyle",null]]"##);
        assert!(Arc::ptr_eq(style_of_arc(&tree, 2), &own), "the pointer, not a copy");
        assert!(style_of(&tree, 1).is_none(), "an element that had no style has none again");
        assert_eq!(tree.styles.layered.len(), 0);
    }

    fn style_of_arc(tree: &RetainedTree, id: u64) -> &Arc<StyleDesc> {
        tree.elements[&id].style.as_ref().unwrap()
    }

    #[test]
    fn changing_the_layer_restyles_every_element_once() {
        let mut tree = layered_tree();
        tree.take_dirty();
        let revision = tree.next_revision;
        apply(&mut tree, r##"[["setUniversalStyle",{"opacity":0.5}]]"##);
        assert!(tree.take_dirty());
        assert_eq!(tree.next_revision, revision + 1, "one revision for the whole tree");
        assert!(tree.elements.values().all(|element| element.subtree_revision == revision));
        assert_eq!(style_of(&tree, 2).unwrap().opacity, Some(0.5));

        // The same layer again, byte for byte or only by content, is nothing.
        apply(&mut tree, r##"[["setUniversalStyle",{"opacity":0.5}]]"##);
        apply(&mut tree, r##"[["setUniversalStyle",{ "opacity" : 0.50 }]]"##);
        assert_eq!(tree.next_revision, revision + 1);
        assert!(!tree.take_dirty());
    }

    #[test]
    fn a_layer_change_that_no_element_shows_repaints_nothing() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        apply(&mut tree, r##"[["setStyle",1,{"opacity":0.2}],["setUniversalStyle",{"opacity":0.5}]]"##);
        let revision = tree.elements[&1].subtree_revision;
        tree.take_dirty();
        apply(&mut tree, r##"[["setUniversalStyle",{"opacity":0.9}]]"##);
        assert_eq!(tree.elements[&1].subtree_revision, revision, "its own opacity wins either way");
        assert!(!tree.take_dirty());
    }

    #[test]
    fn elements_with_the_same_own_style_share_one_layered_style() {
        let mut tree = RetainedTree::new();
        for id in 1..=20 {
            tree.create_element(id, "div".to_string());
        }
        apply(&mut tree, r##"[["setUniversalStyle",{"opacity":0.5}]]"##);
        for id in 1..=20 {
            let width = if id % 2 == 0 { 10 } else { 20 };
            apply(&mut tree, &format!(r##"[["setStyle",{id},{{"width":{width}}}]]"##));
        }
        assert_eq!(tree.styles.layered.len(), 2);
        assert!(Arc::ptr_eq(style_of_arc(&tree, 1), style_of_arc(&tree, 3)));
        assert!(!Arc::ptr_eq(style_of_arc(&tree, 1), style_of_arc(&tree, 2)));
    }

    #[test]
    fn sweeping_releases_the_layered_styles_no_element_holds() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        apply(&mut tree, r##"[["setUniversalStyle",{"opacity":0.5}]]"##);
        for frame in 0..30 {
            apply(&mut tree, &format!(r##"[["setStyle",1,{{"left":{frame}}}]]"##));
            tree.styles.sweep();
            assert_eq!(tree.styles.layered.len(), 1, "frame {frame} leaked a layered style");
            // The layer, and the style the element declares.
            assert_eq!(tree.styles.len(), 2, "frame {frame} leaked a style");
        }
    }

    #[test]
    fn an_invalid_layer_rejects_the_batch_and_changes_nothing() {
        let mut tree = layered_tree();
        let revision = tree.next_revision;
        let error = crate::renderer::apply_batch_to_tree(
            &mut tree,
            br##"[["setStyle",2,{"color":"#abcdef"}],["setUniversalStyle",{"color":5}]]"##,
        )
        .expect_err("a layer is validated like any style");
        assert!(error.starts_with("Batch op 1 setUniversalStyle parse error:"), "{error}");
        assert_eq!(style_of(&tree, 2).unwrap().color.as_deref(), Some("#222222"));
        assert_eq!(tree.next_revision, revision);
        assert!(tree.universal.is_none());

        let error = crate::renderer::apply_batch_to_tree(
            &mut tree,
            br#"[["setUniversalStyle",{"transition":"width not-a-time"}]]"#,
        )
        .expect_err("transitions are validated too");
        assert!(error.contains("setUniversalStyle parse error"), "{error}");

        let error = crate::renderer::apply_batch_to_tree(&mut tree, br#"[["setUniversalStyle"]]"#)
            .expect_err("a missing style is not a null one");
        assert!(error.contains("missing style"), "{error}");
        assert!(tree.universal.is_none());
    }

    #[test]
    fn the_layer_is_ordered_with_the_element_mutations_of_its_batch() {
        let mut tree = RetainedTree::new();
        apply(
            &mut tree,
            r##"[["createElement",1,"div"],["setUniversalStyle",{"opacity":0.5}],["createElement",2,"div"]]"##,
        );
        assert_eq!(style_of(&tree, 1).unwrap().opacity, Some(0.5));
        assert_eq!(style_of(&tree, 2).unwrap().opacity, Some(0.5));
        apply(&mut tree, r##"[["setUniversalStyle",null],["createElement",3,"div"]]"##);
        assert!(tree.elements.values().all(|element| element.style.is_none()));
    }

    #[test]
    fn a_layer_that_introduces_a_transition_keeps_the_style_before_it() {
        let mut tree = layered_tree();
        apply(&mut tree, r##"[["setUniversalStyle",{"backgroundColor":"#ff0000"}]]"##);
        assert!(tree.elements[&2].style_before_transition().is_none());
        apply(
            &mut tree,
            r##"[["setUniversalStyle",{"backgroundColor":"#0000ff","transition":"background-color 1s"}]]"##,
        );
        for id in [1, 2] {
            let before = tree.elements[&id].style_before_transition().unwrap();
            assert_eq!(before.background_color.as_deref(), Some("#ff0000"), "element {id}");
            assert!(!crate::motion::declares_transitions(before));
        }

        // An element's own transition replaces the layer's.
        apply(&mut tree, r##"[["setStyle",2,{"transition":"opacity 2s"}]]"##);
        let own = style_of(&tree, 2).unwrap();
        assert_eq!(own.transition.as_deref(), Some("opacity 2s"));
        assert_eq!(own.background_color.as_deref(), Some("#0000ff"));
        assert!(tree.elements[&2].style_before_transition().is_none());

        // Dropping the layer drops the declaration, and nothing is left behind.
        apply(&mut tree, r##"[["setUniversalStyle",null],["setStyle",2,{"color":"#222222"}]]"##);
        assert!(tree.elements[&1].style_before_transition().is_none());
        assert!(tree.elements[&1].extras.is_none());
    }

    #[test]
    fn the_layer_feeds_the_counts_that_gate_the_tree_walks() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "div".to_string());
        tree.append_child(1, 2);
        tree.set_custom_prop(2, "tabIndex".to_string(), serde_json::json!(0));
        assert!(tree.focusable_ids.contains(&2));

        apply(&mut tree, r##"[["setUniversalStyle",{"visibility":"hidden","position":"absolute"}]]"##);
        assert!(tree.focusable_ids.is_empty(), "a hidden element cannot take focus");
        assert!(tree.is_visibility_hidden(2));
        assert_eq!((tree.visibility_count, tree.escaping_count), (2, 2));

        apply(&mut tree, r##"[["createElement",3,"div"],["appendChild",1,3]]"##);
        assert_eq!((tree.visibility_count, tree.escaping_count), (3, 3));
        tree.destroy_element(3);

        apply(&mut tree, r##"[["setStyle",2,{"visibility":"visible"}]]"##);
        assert!(tree.focusable_ids.contains(&2), "its own value wins");

        apply(&mut tree, r##"[["setUniversalStyle",null]]"##);
        assert_eq!((tree.visibility_count, tree.escaping_count), (1, 0));
        tree.destroy_element(1);
        assert_eq!((tree.visibility_count, tree.escaping_count), (0, 0));
    }
}
