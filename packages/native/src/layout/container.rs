//! Containment: `containerType`, `containerName`, `container` and the `@container`
//! blocks of a style, resolved against the query containers above an element.
//!
//! A container's content box is only known once layout has produced it, and the
//! style of its descendants depends on that box, so the build reads the box the
//! last layout recorded ([`ContainerStore`]) and layout checks the guess afterwards:
//! every container compares its new content box with the one the build assumed, for
//! the axes something read, and asks the window for another pass before present
//! when they differ (`Window::request_redraw_before_present`). A steady frame
//! therefore costs one comparison per container that was read, and nothing else.
//!
//! The query container of an element is never the element itself, so a container's
//! own style does not depend on its own size.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::css::container_query::{is_ident, Axes, ContainerQuery, Containers, Measured, Needs};
use crate::css::custom_properties::CustomProperties;
use crate::css::length::LengthContext;
use crate::retained_tree::{RetainedElement, RetainedTree};
use crate::style::StyleDesc;

// ── Declaration ───────────────────────────────────────────────────────────

/// `container-type`. `normal` makes no query container.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ContainerType {
    #[default]
    Normal,
    /// Queries and `cqw` / `cqi` see the content box width; the width no longer
    /// depends on the content.
    InlineSize,
    /// Both axes.
    Size,
}

impl ContainerType {
    fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "inline-size" => Some(Self::InlineSize),
            "size" => Some(Self::Size),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::InlineSize => "inline-size",
            Self::Size => "size",
        }
    }

    /// Whether a container of this type answers a query that reads `axes`. An element of type
    /// `normal` answers one that reads none (`style()`, or a name alone).
    fn contains(self, axes: Axes) -> bool {
        match self {
            Self::Normal => axes == Axes::NONE,
            Self::InlineSize => !axes.height,
            Self::Size => true,
        }
    }

    /// The axes a container of this type contains: the ones its size is not content-based in.
    pub(crate) fn axes(self) -> Axes {
        match self {
            Self::Normal => Axes::NONE,
            Self::InlineSize => Axes::WIDTH,
            Self::Size => Axes::BOTH,
        }
    }
}

impl<'de> Deserialize<'de> for ContainerType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            de::Error::custom(format!(
                "invalid containerType \"{text}\": expected \"normal\", \"inline-size\" or \"size\""
            ))
        })
    }
}

impl Serialize for ContainerType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

/// `container-name`: the identifiers a query can name the container by.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContainerNames(Arc<[String]>);

impl ContainerNames {
    fn parse(text: &str) -> Result<Self, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            return Err("expected a name, or \"none\"".to_owned());
        }
        if words == ["none"] {
            return Ok(Self::default());
        }
        for word in &words {
            let reserved = ["none", "and", "or", "not"].iter().any(|keyword| word.eq_ignore_ascii_case(keyword));
            if reserved || !is_ident(word) {
                return Err(format!("\"{word}\" is not a container name"));
            }
        }
        Ok(Self(words.into_iter().map(str::to_owned).collect()))
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        self.0.iter().any(|candidate| candidate == name)
    }

    fn text(&self) -> String {
        if self.0.is_empty() {
            "none".to_owned()
        } else {
            self.0.join(" ")
        }
    }
}

impl<'de> Deserialize<'de> for ContainerNames {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(|reason| de::Error::custom(format!("invalid containerName \"{text}\": {reason}")))
    }
}

impl Serialize for ContainerNames {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text())
    }
}

/// `container: <name> [ / <type> ]`, which sets both longhands; a type left out is `normal`.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerShorthand {
    names: ContainerNames,
    kind: ContainerType,
}

impl<'de> Deserialize<'de> for ContainerShorthand {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let invalid = |reason: String| de::Error::custom(format!("invalid container \"{text}\": {reason}"));
        let (names, kind) = match text.split_once('/') {
            Some((names, kind)) => (
                names,
                ContainerType::parse(kind)
                    .ok_or_else(|| invalid("expected \"normal\", \"inline-size\" or \"size\" after \"/\"".to_owned()))?,
            ),
            None => (text.as_str(), ContainerType::Normal),
        };
        Ok(Self { names: ContainerNames::parse(names).map_err(invalid)?, kind })
    }
}

impl Serialize for ContainerShorthand {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{} / {}", self.names.text(), self.kind.name()))
    }
}

/// What an element declares to be, if it is a query container.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Declared {
    pub names: ContainerNames,
    pub kind: ContainerType,
}

/// The container `style` makes its element, with the longhands winning over the
/// shorthand as everywhere else. `None` for `container-type: normal` with no name. A name alone
/// makes a container only `style()` queries and a name-only query can pick.
pub(crate) fn declared(style: &StyleDesc) -> Option<Declared> {
    let kind = style
        .container_type
        .or(style.container.as_ref().map(|shorthand| shorthand.kind))
        .unwrap_or_default();
    let names = style
        .container_name
        .clone()
        .or_else(|| style.container.as_ref().map(|shorthand| shorthand.names.clone()))
        .unwrap_or_default();
    (kind != ContainerType::Normal || !names.0.is_empty()).then_some(Declared { names, kind })
}

/// Whether an element can be a query container, and if it can, whether it can be size contained.
/// CSS lets any element be one, and makes size containment apply to the principal box of a block, a
/// flex or grid item, an atomic inline, a table caption and a replaced element; not to an inline box
/// that is not atomic, a table, a table-internal box or an element with no box. The ones that are not
/// built as an element of their own are the paragraph's to scope (`renderer::build_atomics`) and the
/// ones that are spliced into their parent's (`display: contents`) travel with `contents_scopes`.
pub(crate) fn can_be_sized(element: &RetainedElement) -> Option<bool> {
    match element.element_type.as_str() {
        "virtual-list" => Some(true),
        host if crate::text::inline::is_host_type(host) => Some(
            !crate::layout::contents::is_display_contents(element) && crate::layout::table::can_be_container(element),
        ),
        _ => None,
    }
}

// ── Rules ─────────────────────────────────────────────────────────────────

/// The `@container` blocks of a style, in written order: a later block wins over an
/// earlier one that sets the same property.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContainerRules(Arc<[ContainerRule]>);

#[derive(Debug, Clone, PartialEq)]
struct ContainerRule {
    source: String,
    query: ContainerQuery,
    style: StyleDesc,
}

impl ContainerRules {
    /// `earlier`'s blocks, then `later`'s: the order a layered style cascades in.
    pub(crate) fn concat(earlier: Option<&Self>, later: Option<&Self>) -> Option<Self> {
        match (earlier, later) {
            (Some(earlier), Some(later)) => Some(Self(earlier.0.iter().chain(later.0.iter()).cloned().collect())),
            (Some(only), None) | (None, Some(only)) => Some(only.clone()),
            (None, None) => None,
        }
    }
}

impl<'de> Deserialize<'de> for ContainerRules {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RulesVisitor;

        impl<'de> Visitor<'de> for RulesVisitor {
            type Value = ContainerRules;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an object of container queries to styles")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ContainerRules, A::Error> {
                let mut rules = Vec::new();
                while let Some((source, style)) = map.next_entry::<String, StyleDesc>()? {
                    let query = ContainerQuery::parse(&source).map_err(de::Error::custom)?;
                    rules.push(ContainerRule { source, query, style });
                }
                Ok(ContainerRules(rules.into()))
            }
        }

        deserializer.deserialize_map(RulesVisitor)
    }
}

impl Serialize for ContainerRules {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for rule in self.0.iter() {
            map.serialize_entry(&rule.source, &rule.style)?;
        }
        map.end()
    }
}

/// `style` with every `@container` block whose query matches folded in, in written
/// order, or `None` when no block matches (the style is then used as it is). A block is
/// laid over the style the way a universal style is laid under an element's own, so its
/// shorthands reset the longhands they cover and its state keys (`hover`, ...) merge
/// into the style's own. The result has no `@container` blocks left.
pub(crate) fn fold(style: &StyleDesc, containers: &dyn Containers) -> Option<StyleDesc> {
    let rules = style.container_rules.as_ref()?;
    let mut folded: Option<StyleDesc> = None;
    for rule in rules.0.iter() {
        if rule.query.matches(containers) {
            let below = folded.as_ref().unwrap_or(style);
            folded = Some(crate::universal_style::layer_under(below, &rule.style));
        }
    }
    folded.map(|mut folded| {
        folded.container_rules = None;
        folded
    })
}

/// Records what the folded `declared` style of an element with `@container` blocks says about
/// its `display` and its `order`, which its parent builds its children by (`display: none` takes
/// no part in the parent's layout, and the paragraph planner makes a box of it, a word on a line
/// or an atomic inline by it; a flex or grid container sorts its items by `order`). Returns
/// whether that is not what the parent assumed, so the frame has to be built again.
pub(crate) fn settle(element: &RetainedElement, declared: Option<&StyleDesc>) -> bool {
    let assumed_display = element.display().map(std::borrow::Cow::into_owned);
    let folded_display = declared.and_then(|style| style.display.clone());
    let assumed_order = element.order();
    let folded_order = declared.and_then(|style| style.order);
    let changed = assumed_display != folded_display || assumed_order != folded_order;
    *element.container_display.borrow_mut() = Some(folded_display);
    *element.container_order.borrow_mut() = Some(folded_order);
    changed
}

/// [`settle`] for the children of `parent` that its own build may not visit, since
/// nothing else would find that their blocks show them or turn them into another kind of box: a
/// child with no box now, and an inline box of the paragraph (a `<span>`, an element a block made
/// `inline`), which is not built as an element. The inline boxes' children are settled too,
/// since the planner reads their display to tell what the box around them is. `containers` is
/// what they are in.
pub(crate) fn settle_children(
    tree: &RetainedTree,
    parent: &RetainedElement,
    containers: &dyn Containers,
    window: &mut gpui::Window,
) {
    if !tree.has_container_rules() {
        return;
    }
    let mut changed = false;
    settle_among(tree, parent, containers, &mut changed);
    if changed {
        window.request_redraw_before_present();
    }
}

fn settle_among(tree: &RetainedTree, parent: &RetainedElement, containers: &dyn Containers, changed: &mut bool) {
    for child_id in crate::layout::contents::flatten(tree, &parent.children).iter() {
        let Some(child) = tree.elements.get(child_id) else { continue };
        if let Some(style) = child.style.as_deref().filter(|style| style.container_rules.is_some()) {
            let folded = fold(style, containers);
            *changed |= settle(child, folded.as_ref().or(Some(style)));
        }
        if crate::text::inline::is_inline_wrapper(child, parent) {
            settle_among(tree, child, containers, changed);
        }
    }
}

// ── Sizes ─────────────────────────────────────────────────────────────────

/// A container's content box in px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Size {
    pub width: f64,
    pub height: f64,
}

/// What the last layout produced for each container, by element id. Shared by the
/// view, which reads it while it builds, and the prepaint listeners, which write it.
#[derive(Clone, Default)]
pub(crate) struct ContainerStore(Rc<RefCell<rustc_hash::FxHashMap<u64, Size>>>);

impl ContainerStore {
    pub(crate) fn get(&self, id: u64) -> Option<Size> {
        self.0.borrow().get(&id).copied()
    }

    /// What the size of container `id` adds to the key of the element, which builds the scope
    /// its children are in from it.
    pub(crate) fn key(&self, id: u64) -> u64 {
        let mut fp = crate::retain::Fingerprint::new();
        match self.get(id) {
            Some(size) => {
                fp.f64(size.width);
                fp.f64(size.height);
            }
            None => fp.word(u64::MAX),
        }
        fp.finish()
    }

    fn record(&self, id: u64, size: Size) {
        self.0.borrow_mut().insert(id, size);
    }

    /// Drops the containers `keep` no longer names.
    pub(crate) fn retain(&self, mut keep: impl FnMut(u64) -> bool) {
        let mut sizes = self.0.borrow_mut();
        if !sizes.is_empty() {
            sizes.retain(|id, _| keep(*id));
        }
    }

    #[cfg(test)]
    pub(crate) fn set(&self, id: u64, width: f64, height: f64) {
        self.record(id, Size { width, height });
    }
}

/// A query container above the element being built, as one build saw it.
pub(crate) struct ContainerScope {
    pub id: u64,
    names: ContainerNames,
    kind: ContainerType,
    /// Whether the element is size contained, which is what gives it a content box that size
    /// features and `cq` units read. An element with a `container-type` that cannot be size
    /// contained (an inline box that is not atomic, a `display: contents` element, a table or
    /// one of its internal boxes) is still the container a query finds by its type and name, but
    /// answers no size feature, and a `cq` unit looks past it.
    sized: bool,
    /// The content box the build assumed: the last one layout recorded, `None` before
    /// the first layout and for a container that is not sized.
    size: Option<Size>,
    /// The container's own context, which `em` in its queries is read in.
    lengths: LengthContext,
    /// What the container computes its custom properties to, which `style()` reads.
    custom: CustomProperties,
    parent: Option<Arc<ContainerScope>>,
    /// The axes something in this pass read, as [`Axes`] bits.
    reads: AtomicU8,
}

const WIDTH: u8 = 1;
const HEIGHT: u8 = 2;

impl ContainerScope {
    /// This container and the ones above it, as the build below sees them, for the key of a
    /// retained subtree. The size is the one the build assumed, so a container that laid out
    /// differently changes the key of what it holds.
    pub(crate) fn fingerprint(&self, fp: &mut crate::retain::Fingerprint) {
        let mut scope = Some(self);
        while let Some(container) = scope {
            fp.word(container.id);
            fp.hash(&container.kind);
            for name in container.names.0.iter() {
                fp.str(name);
            }
            fp.word(u64::MAX);
            match container.size {
                Some(size) => {
                    fp.f64(size.width);
                    fp.f64(size.height);
                }
                None => fp.word(u64::MAX),
            }
            fp.bits(&[container.sized]);
            container.lengths.fingerprint(fp);
            container.custom.fingerprint(fp);
            scope = container.parent.as_deref();
        }
    }

    pub(crate) fn new(
        id: u64,
        declared: Declared,
        size: Option<Size>,
        lengths: LengthContext,
        custom: CustomProperties,
        parent: Option<Arc<ContainerScope>>,
    ) -> Self {
        Self {
            id,
            sized: declared.kind != ContainerType::Normal,
            names: declared.names,
            kind: declared.kind,
            size,
            lengths,
            custom,
            parent,
            reads: AtomicU8::new(0),
        }
    }

    /// A container that is not size contained: see [`Self::sized`].
    pub(crate) fn without_size(
        id: u64,
        declared: Declared,
        lengths: LengthContext,
        custom: CustomProperties,
        parent: Option<Arc<ContainerScope>>,
    ) -> Self {
        Self { sized: false, ..Self::new(id, declared, None, lengths, custom, parent) }
    }

    pub(crate) fn is_sized(&self) -> bool {
        self.sized
    }

    /// Whether the size of this container is known. The pass is provisional until it is.
    pub(crate) fn is_known(&self) -> bool {
        (self.size.is_some() || !self.is_sized()) && self.parent.as_deref().is_none_or(Self::is_known)
    }

    /// The context of the descendants of this container: `cqw` and `cqh` are fractions of
    /// its content box, for the axes it contains.
    pub(crate) fn descendant_lengths(&self, lengths: LengthContext) -> LengthContext {
        let mut lengths = lengths;
        if !self.is_sized() {
            return lengths;
        }
        if self.kind.axes().width {
            lengths = lengths.with_container_width(self.size.map(|size| size.width));
        }
        if self.kind.axes().height {
            lengths = lengths.with_container_height(self.size.map(|size| size.height));
        }
        lengths
    }

    /// Records that something built from the nearest container that contains `axes`, from
    /// this one up, depends on those axes of it.
    pub(crate) fn note_reads_from(&self, axes: Axes) {
        if axes.width {
            self.nearest_sized(Axes::WIDTH).inspect(|scope| scope.note_read(Axes::WIDTH));
        }
        if axes.height {
            self.nearest_sized(Axes::HEIGHT).inspect(|scope| scope.note_read(Axes::HEIGHT));
        }
    }

    /// The container a `cq` unit reading `axes` is a fraction of: one that is size contained.
    fn nearest_sized(&self, axes: Axes) -> Option<&Self> {
        let mut scope = Some(self);
        while let Some(candidate) = scope {
            if candidate.is_sized() && candidate.kind.contains(axes) {
                return Some(candidate);
            }
            scope = candidate.parent.as_deref();
        }
        None
    }

    /// The container that answers a query: the nearest with `name` whose type contains the axes
    /// the query reads. With no axis and no name there is none: the query is about the element
    /// above.
    fn nearest(&self, name: Option<&str>, axes: Axes) -> Option<&Self> {
        if name.is_none() && axes == Axes::NONE {
            return None;
        }
        let mut scope = Some(self);
        while let Some(candidate) = scope {
            if candidate.kind.contains(axes) && name.is_none_or(|name| candidate.names.contains(name)) {
                return Some(candidate);
            }
            scope = candidate.parent.as_deref();
        }
        None
    }

    fn note_read(&self, axes: Axes) {
        let bits = u8::from(axes.width) * WIDTH | u8::from(axes.height) * HEIGHT;
        if bits != 0 {
            self.reads.fetch_or(bits, Ordering::Relaxed);
            crate::tape::note_build(crate::tape::BuildEffect::ContainerRead(self.id, axes));
        }
    }

    /// Repeats a read that the build of a subtree made of the container `id`, for a frame that
    /// replays the subtree instead of building it. Without it the container would not know that
    /// something is built from its size, and a layout that moves only the container would not
    /// ask for another build. A container that is not among these is inside the subtree.
    pub(crate) fn repeat_read(&self, id: u64, axes: Axes) {
        let mut scope = Some(self);
        while let Some(container) = scope {
            if container.id == id {
                container.note_read(axes);
                return;
            }
            scope = container.parent.as_deref();
        }
    }

    /// Whether `observed`, the content box this layout produced, is not what the build
    /// assumed for an axis something read.
    pub(crate) fn assumption_failed(&self, observed: Size) -> bool {
        let reads = self.reads.load(Ordering::Relaxed);
        if reads == 0 {
            return false;
        }
        let Some(assumed) = self.size else { return true };
        // Sizes are snapped to sixty-fourths of a px, so unequal means a real change.
        (reads & WIDTH != 0 && assumed.width != observed.width) || (reads & HEIGHT != 0 && assumed.height != observed.height)
    }
}

/// The containers above an element, nearest first, as [`Containers`], and what the element's
/// parent computes to: the container a query with nothing but `style()` and no name asks.
pub(crate) struct Chain<'a> {
    pub scopes: Option<&'a ContainerScope>,
    pub parent_custom: &'a CustomProperties,
    pub parent_lengths: LengthContext,
}

impl Containers for Chain<'_> {
    fn container(&self, name: Option<&str>, needs: Needs) -> Option<Measured> {
        let Some(scope) = self.scopes.and_then(|scopes| scopes.nearest(name, needs.axes)) else {
            return (name.is_none() && needs.axes == Axes::NONE && needs.style).then(|| {
                crate::css::custom_properties::reads::note();
                Measured {
                    width: None,
                    height: None,
                    lengths: self.parent_lengths,
                    custom: self.parent_custom.clone(),
                }
            });
        };
        scope.note_read(needs.axes);
        let contained = scope.kind.axes();
        Some(Measured {
            width: scope.size.filter(|_| contained.width).map(|size| size.width),
            height: scope.size.filter(|_| contained.height).map(|size| size.height),
            lengths: scope.lengths,
            custom: scope.custom.clone(),
        })
    }
}

/// The most times one draw builds the frame before it presents: the first build, which reads the
/// sizes the last layout recorded, and one more for each level of containers whose size changed
/// the style of the containers below it.
const MAX_PASSES: usize = 4;

/// Lets the window build a frame again before presenting it. Call it when the window opens, with
/// `events::enable_dom_hit_testing`.
pub(crate) fn allow_redraw_passes(window: &mut gpui::Window) {
    window.set_max_draw_passes(MAX_PASSES);
}

/// The content box of a container laid out in `bounds`, snapped to sixty-fourths of a px
/// as layout units are.
pub(crate) fn content_box(
    bounds: gpui::Bounds<gpui::Pixels>,
    layout_id: gpui::LayoutId,
    window: &mut gpui::Window,
) -> Size {
    let insets = window.layout_insets(layout_id);
    let gutter = window.layout_scrollbar_size(layout_id);
    let snap = |pixels: gpui::Pixels| (f64::from(f32::from(pixels).max(0.0)) * 64.0).round() / 64.0;
    Size {
        width: snap(
            bounds.size.width
                - insets.padding.left
                - insets.padding.right
                - insets.border.left
                - insets.border.right
                - gutter.width,
        ),
        height: snap(
            bounds.size.height
                - insets.padding.top
                - insets.padding.bottom
                - insets.border.top
                - insets.border.bottom
                - gutter.height,
        ),
    }
}

/// Size containment on the axes a container of type `kind` contains.
pub(crate) fn contain_size<E: gpui::Styled>(mut element: E, kind: ContainerType) -> E {
    let contained = kind.axes();
    element.style().contain_inline_size = Some(contained.width);
    element.style().contain_block_size = Some(contained.height);
    element
}

/// Size containment for an element that is a container only for that: a replaced element
/// (`<img>`, `<svg>`) has nothing inside it to query. A browser sizes it as it sizes a contained
/// block, without its natural size on the contained axes. An `<input>` is not touched, since the
/// browser ignores it there.
pub(crate) fn contain_declared<E: gpui::Styled>(element: E, style: Option<&StyleDesc>) -> E {
    match style.and_then(declared) {
        Some(declared) if declared.kind != ContainerType::Normal => {
            let mut element = contain_size(element, declared.kind);
            // A replaced element is not stretched to the width of a block around it: with nothing to
            // size it by on the contained axis it is as wide as its padding and border.
            element.style().item_is_replaced = Some(true);
            element
        }
        _ => element,
    }
}

/// Makes `element` the container `scope` describes: its size on the contained axes no longer
/// depends on its content, and it reports its content box when layout has produced it, asking for
/// the frame to be built again when something was built from another one.
pub(crate) fn contain<E: gpui::InteractiveElement + gpui::Styled>(
    mut element: E,
    scope: Arc<ContainerScope>,
    store: ContainerStore,
) -> E {
    debug_assert!(scope.is_sized(), "a container that is not size contained has no box to report");
    element = contain_size(element, scope.kind);
    element.on_prepainted_layout(move |bounds, layout_id, window, _| {
        let observed = content_box(bounds, layout_id, window);
        store.record(scope.id, observed);
        if scope.assumption_failed(observed) {
            window.request_redraw_before_present();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap_or_else(|error| panic!("{json}: {error}"))
    }

    fn lengths() -> LengthContext {
        LengthContext::initial(1000.0, 800.0)
    }

    fn chain(scopes: Option<&ContainerScope>) -> Chain<'_> {
        static NONE: std::sync::LazyLock<CustomProperties> = std::sync::LazyLock::new(CustomProperties::default);
        Chain { scopes, parent_custom: &NONE, parent_lengths: lengths() }
    }

    fn scope(
        id: u64,
        names: &str,
        kind: ContainerType,
        size: Option<(f64, f64)>,
        parent: Option<Arc<ContainerScope>>,
    ) -> Arc<ContainerScope> {
        Arc::new(ContainerScope::new(
            id,
            Declared { names: ContainerNames::parse(names).unwrap(), kind },
            size.map(|(width, height)| Size { width, height }),
            lengths(),
            CustomProperties::default(),
            parent,
        ))
    }

    #[test]
    fn the_declaration_reads_the_longhands_and_the_shorthand() {
        assert_eq!(declared(&style("{}")), None);
        assert_eq!(declared(&style(r#"{"containerType":"normal"}"#)), None);
        let inline = declared(&style(r#"{"containerType":"inline-size","containerName":"a b"}"#)).unwrap();
        assert_eq!(inline.kind, ContainerType::InlineSize);
        assert!(inline.names.contains("a") && inline.names.contains("b") && !inline.names.contains("c"));
        let shorthand = declared(&style(r#"{"container":"card / size"}"#)).unwrap();
        assert_eq!(shorthand.kind, ContainerType::Size);
        assert!(shorthand.names.contains("card"));
        // A shorthand without a type is `normal`: a name only `style()` and a name-only query find.
        let named = declared(&style(r#"{"container":"card"}"#)).unwrap();
        assert_eq!(named.kind, ContainerType::Normal);
        assert!(named.names.contains("card"));
        assert_eq!(declared(&style(r#"{"container":"none"}"#)), None);
        // The longhands win over the shorthand.
        let mixed = declared(&style(r#"{"container":"card / size","containerType":"inline-size","containerName":"other"}"#)).unwrap();
        assert_eq!(mixed.kind, ContainerType::InlineSize);
        assert!(mixed.names.contains("other") && !mixed.names.contains("card"));
        assert!(declared(&style(r#"{"containerType":"size","containerName":"none"}"#)).unwrap().names.0.is_empty());
    }

    #[test]
    fn a_bad_declaration_fails_the_style() {
        for json in [
            r#"{"containerType":"block-size"}"#,
            r#"{"containerType":1}"#,
            r#"{"containerName":"a and b"}"#,
            r#"{"containerName":"none a"}"#,
            r#"{"containerName":""}"#,
            r#"{"containerName":"4x"}"#,
            r#"{"container":"/ size"}"#,
            r#"{"container":"a / wide"}"#,
            r#"{"@container":{"(min-width":{}}}"#,
            r#"{"@container":{"(min-width: 1px) or":{}}}"#,
            r#"{"@container":[]}"#,
        ] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
    }

    #[test]
    fn the_blocks_keep_their_written_order_and_round_trip() {
        let style = style(
            r##"{"@container":{"(min-width: 700px)":{"color":"#111111"},"(min-width: 300px)":{"color":"#222222"},"side (width > 1px)":{}}}"##,
        );
        let rules = style.container_rules.as_ref().unwrap();
        let sources: Vec<&str> = rules.0.iter().map(|rule| rule.source.as_str()).collect();
        assert_eq!(sources, ["(min-width: 700px)", "(min-width: 300px)", "side (width > 1px)"]);
        let json = serde_json::to_string(&style).unwrap();
        let back: StyleDesc = serde_json::from_str(&json).unwrap();
        assert_eq!(back, style);
        assert!(json.contains(r#""@container":{"(min-width: 700px)""#), "{json}");
    }

    #[test]
    fn a_later_matching_block_wins_and_a_block_that_does_not_match_changes_nothing() {
        let own = style(
            r##"{"color":"#000001","@container":{
                "(min-width: 700px)":{"color":"#000002","opacity":0.5},
                "(min-width: 300px)":{"color":"#000003"},
                "(min-width: 900px)":{"color":"#000004"}}}"##,
        );
        let wide = scope(1, "none", ContainerType::InlineSize, Some((800.0, 0.0)), None);
        let folded = fold(&own, &chain(Some(&wide))).unwrap();
        assert_eq!(folded.color.as_deref(), Some("#000003"));
        assert_eq!(folded.opacity, Some(0.5));
        assert!(folded.container_rules.is_none());
        let narrow = scope(1, "none", ContainerType::InlineSize, Some((100.0, 0.0)), None);
        assert!(fold(&own, &chain(Some(&narrow))).is_none());
        assert!(fold(&own, &chain(None)).is_none(), "no container above: nothing matches");
        let unmeasured = scope(1, "none", ContainerType::InlineSize, None, None);
        assert!(fold(&own, &chain(Some(&unmeasured))).is_none(), "an unmeasured container matches nothing");
    }

    #[test]
    fn a_block_resets_the_longhands_of_its_shorthand_and_merges_state_keys() {
        let own = style(
            r##"{"paddingTop":4,"padding":2,"hover":{"color":"#000001"},
                 "@container":{"(min-width: 1px)":{"padding":10,"hover":{"opacity":0.5}}}}"##,
        );
        let container = scope(1, "none", ContainerType::Size, Some((10.0, 10.0)), None);
        let folded = fold(&own, &chain(Some(&container))).unwrap();
        assert_eq!(folded.padding, style(r#"{"padding":10}"#).padding);
        assert_eq!(folded.padding_top, None);
        let hover = folded.hover.as_deref().unwrap();
        assert_eq!(hover.color.as_deref(), Some("#000001"), "the style's own hover is kept");
        assert_eq!(hover.opacity, Some(0.5), "and the block's merges into it");
    }

    #[test]
    fn a_named_query_reaches_past_the_nearer_container() {
        let outer = scope(1, "outer", ContainerType::InlineSize, Some((900.0, 0.0)), None);
        let inner = scope(2, "none", ContainerType::InlineSize, Some((100.0, 0.0)), Some(outer));
        let own = style(
            r##"{"@container":{"outer (min-width: 800px)":{"color":"#000001"},"(min-width: 800px)":{"opacity":0.5}}}"##,
        );
        let folded = fold(&own, &chain(Some(&inner))).unwrap();
        assert_eq!(folded.color.as_deref(), Some("#000001"));
        assert_eq!(folded.opacity, None, "the unnamed query asks the nearer, narrow one");
    }

    #[test]
    fn a_block_axis_feature_skips_an_inline_size_container() {
        let outer = scope(1, "none", ContainerType::Size, Some((900.0, 700.0)), None);
        let inner = scope(2, "none", ContainerType::InlineSize, Some((100.0, 0.0)), Some(outer));
        let own = style(r##"{"@container":{"(min-height: 600px)":{"color":"#000001"},"(max-width: 100px)":{"opacity":0.5}}}"##);
        let folded = fold(&own, &chain(Some(&inner))).unwrap();
        assert_eq!(folded.color.as_deref(), Some("#000001"));
        assert_eq!(folded.opacity, Some(0.5));
    }

    #[test]
    fn only_the_axes_a_query_read_make_the_assumption_fail() {
        let container = scope(1, "none", ContainerType::Size, Some((400.0, 300.0)), None);
        let own = style(r#"{"@container":{"(min-width: 100px)":{}}}"#);
        assert!(!container.assumption_failed(Size { width: 1.0, height: 1.0 }), "nothing read yet");
        fold(&own, &chain(Some(&container)));
        assert!(!container.assumption_failed(Size { width: 400.0, height: 999.0 }), "only the width was read");
        assert!(container.assumption_failed(Size { width: 401.0, height: 300.0 }));
        let height = style(r#"{"@container":{"(min-height: 100px)":{}}}"#);
        fold(&height, &chain(Some(&container)));
        assert!(container.assumption_failed(Size { width: 400.0, height: 301.0 }));
    }

    #[test]
    fn a_container_that_was_unmeasured_when_read_fails_at_its_first_layout() {
        let container = scope(1, "none", ContainerType::InlineSize, None, None);
        fold(&style(r#"{"@container":{"(min-width: 100px)":{}}}"#), &chain(Some(&container)));
        assert!(container.assumption_failed(Size { width: 0.0, height: 0.0 }));
        assert!(!container.is_known());
    }

    #[test]
    fn container_units_read_the_nearest_container_that_contains_their_axis() {
        let outer = scope(1, "none", ContainerType::Size, Some((900.0, 700.0)), None);
        let inner = scope(2, "none", ContainerType::InlineSize, Some((100.0, 0.0)), Some(outer.clone()));
        inner.note_reads_from(Axes::BOTH);
        assert!(inner.assumption_failed(Size { width: 101.0, height: 0.0 }));
        assert!(outer.assumption_failed(Size { width: 900.0, height: 701.0 }), "cqh is the outer one's height");
        assert!(!outer.assumption_failed(Size { width: 901.0, height: 700.0 }), "and its width was not read");
        let lengths = inner.descendant_lengths(outer.descendant_lengths(lengths()));
        assert_eq!((lengths.container_width, lengths.container_height), (Some(100.0), Some(700.0)));
    }

    fn custom(entries: &[(&str, &str)]) -> CustomProperties {
        CustomProperties::declared(entries.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())))
            .unwrap_or_default()
    }

    fn scope_with(
        id: u64,
        names: &str,
        kind: ContainerType,
        sized: bool,
        size: Option<(f64, f64)>,
        custom: CustomProperties,
        parent: Option<Arc<ContainerScope>>,
    ) -> Arc<ContainerScope> {
        let declared = Declared { names: ContainerNames::parse(names).unwrap(), kind };
        Arc::new(if sized {
            ContainerScope::new(id, declared, size.map(|(width, height)| Size { width, height }), lengths(), custom, parent)
        } else {
            ContainerScope::without_size(id, declared, lengths(), custom, parent)
        })
    }

    #[test]
    fn a_container_that_cannot_be_size_contained_is_found_and_answers_no_size() {
        let outer = scope(1, "none", ContainerType::InlineSize, Some((900.0, 0.0)), None);
        let span = scope_with(2, "none", ContainerType::InlineSize, false, None, CustomProperties::default(), Some(outer));
        let own = style(
            r##"{"@container":{"(min-width: 100px)":{"color":"#000001"},"not (min-width: 100px)":{"color":"#000002"},"(min-height: 1px)":{"opacity":0.5}}}"##,
        );
        assert!(fold(&own, &chain(Some(&span))).is_none(), "the lookup stops at it: its width is unknown, not the outer one's");
        assert!(span.is_known(), "nothing to wait for");
    }

    #[test]
    fn a_container_that_cannot_be_size_contained_is_skipped_by_a_type_that_does_not_fit_and_by_units() {
        let outer = scope(1, "none", ContainerType::Size, Some((900.0, 700.0)), None);
        let span = scope_with(2, "none", ContainerType::InlineSize, false, None, CustomProperties::default(), Some(outer.clone()));
        let own = style(r##"{"@container":{"(min-height: 600px)":{"color":"#000001"}}}"##);
        assert_eq!(fold(&own, &chain(Some(&span))).unwrap().color.as_deref(), Some("#000001"), "height skips an inline-size container");
        span.note_reads_from(Axes::BOTH);
        assert!(outer.assumption_failed(Size { width: 1.0, height: 700.0 }), "a unit reads the one that is sized");
        let lengths = span.descendant_lengths(outer.descendant_lengths(lengths()));
        assert_eq!((lengths.container_width, lengths.container_height), (Some(900.0), Some(700.0)));
    }

    #[test]
    fn a_name_picks_a_container_without_a_type_for_a_name_only_query_and_style() {
        let named = scope_with(1, "card", ContainerType::Normal, false, None, custom(&[("--x", "1")]), None);
        let near = scope_with(2, "none", ContainerType::InlineSize, true, Some((100.0, 0.0)), CustomProperties::default(), Some(named));
        let own = style(
            r##"{"@container":{"card":{"color":"#000001"},"card style(--x: 1)":{"opacity":0.5},"card (min-width: 1px)":{"width":7},"style(--x: 1)":{"height":9}}}"##,
        );
        let folded = fold(&own, &chain(Some(&near))).unwrap();
        assert_eq!(folded.color.as_deref(), Some("#000001"));
        assert_eq!(folded.opacity, Some(0.5));
        assert_eq!(folded.width, None, "a container with no type has no size to answer a size feature");
        assert_eq!(folded.height, None, "an unnamed style() asks the parent, which has no --x here");
    }

    #[test]
    fn an_unnamed_style_query_asks_the_parent_and_a_mixed_one_the_size_container() {
        let size = scope_with(1, "none", ContainerType::InlineSize, true, Some((100.0, 0.0)), custom(&[("--x", "outer")]), None);
        let parent = custom(&[("--x", "parent")]);
        let chain_of = |scopes| Chain { scopes, parent_custom: &parent, parent_lengths: lengths() };
        let own = style(
            r##"{"@container":{"style(--x: parent)":{"color":"#000001"},"(min-width: 1px) and style(--x: outer)":{"opacity":0.5},"(min-width: 1px) and style(--x: parent)":{"width":7}}}"##,
        );
        let folded = fold(&own, &chain_of(Some(&size))).unwrap();
        assert_eq!(folded.color.as_deref(), Some("#000001"));
        assert_eq!(folded.opacity, Some(0.5), "the size container answers both");
        assert_eq!(folded.width, None);
        let folded = fold(&own, &chain_of(None)).unwrap();
        assert_eq!(folded.color.as_deref(), Some("#000001"), "with no container at all the parent still answers");
        assert_eq!(folded.opacity, None, "and a size feature has no container");
    }
}
