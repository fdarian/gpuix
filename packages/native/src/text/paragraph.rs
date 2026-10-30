//! Painting a paragraph whose inline boxes style their own runs.
//!
//! [`super::inline`] decides what a paragraph is. This turns each text node's
//! chain of inline boxes into the properties of its run, and lays the string out
//! with them.
//!
//! The runs are built at layout time, on top of `window.text_style()`: a run a
//! box does not restyle keeps the container's colour, weight and family, which
//! only gpui knows once the element is in the tree.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use gpui::{
    px, AtomicInline, DefiniteLength, ExtraDecoration, FontWeight, Hsla, InlineBox, InlineContent, OverlineStyle,
    Pixels, SharedString, StrikethroughStyle, StyledText, TextInset, TextLayout, TextRun, UnderlineStyle,
};

use crate::color::ColorContext;
use crate::css::font_features::FontFeatureState;
use crate::css::length::LengthContext;
use crate::css::text_decoration::{self, is_patterned};
use crate::css::vertical_align::ResolvedVerticalAlign;
use crate::css::text_underline_offset::TextUnderlineOffset;
use crate::events::state_styles::{folded, PointerStates};
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;

use crate::motion::InlineStyles;

use super::inline::Paragraph;

/// What one run changes about the text style it inherits.
///
/// A box's run inherits from its parent box as in CSS, so the entry for a
/// nested box already holds everything the boxes around it declared. A box's
/// background and border are not here: they are painted around its line
/// fragments by [`super::inline_decoration`], so that its padding is inside them.
///
/// `line_height` is what the innermost box that declared one gives, resolved
/// where it was declared as `line-height` is inherited: a multiplier stays a
/// multiplier, so a descendant with another size gets its own multiple, and a
/// length is px. `font_size` is in px.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunStyle {
    pub len: usize,
    pub font_size: Option<f32>,
    pub line_height: Option<gpui::LineHeight>,
    pub color: Option<Hsla>,
    pub weight: Option<FontWeight>,
    pub font_style: Option<gpui::FontStyle>,
    pub family: Option<SharedString>,
    pub features: Option<gpui::FontFeatures>,
    pub variations: Option<gpui::FontVariations>,
    pub font_smoothing: Option<gpui::FontSmoothing>,
    /// The decoration lines of a run keep the colour, style and thickness of the
    /// box that declared them, so a descendant that changes `color` does not
    /// recolour them. The innermost box's line of a kind is the field, the lines
    /// of the same kind from the boxes around it are `extra_decorations`: as in
    /// CSS, nested declarations accumulate.
    pub underline: Option<UnderlineStyle>,
    pub strikethrough: Option<StrikethroughStyle>,
    pub overline: Option<OverlineStyle>,
    pub extra_decorations: Vec<ExtraDecoration>,
    /// The placeholder of an atomic inline. CSS propagates neither a decoration
    /// nor the text of a box to an atomic inline, so it is a run of its own with
    /// the lines of none of the boxes around it.
    pub atomic: bool,
    /// The placeholder of an empty inline box that takes space along the line
    /// ([`super::empty_box`]). It is shaped as no text, so it keeps the box's font and line height and
    /// none of its decorations: CSS draws a line under text, and there is none.
    pub placeholder: bool,
    /// The [`OpacityGroup`] the run is painted in: the id of the innermost box around it whose
    /// `opacity` is below one, or `0`.
    pub group: u64,
    /// The text node the run is, as the index of its piece. A text node is a run of its own even
    /// when its neighbour is styled the same, because Chrome measures every one as an item and
    /// ceils its width on its own. Not part of the style: a pattern of the paragraph's own
    /// decoration starts over in each, see [`InlineText::runs`].
    pub piece: Option<usize>,
}

impl RunStyle {

    /// `decorator` is the box that declared the decoration, and `fragment` the text node the
    /// run is, which a patterned line starts its pattern over with.
    fn add_decoration(
        &mut self,
        decoration: &text_decoration::TextDecoration,
        color: Hsla,
        lengths: &LengthContext,
        decorator: u64,
        fragment: u32,
    ) {
        let fragment = if decoration.is_patterned() { fragment } else { 0 };
        if decoration.underline {
            let underline = UnderlineStyle {
                decorator: Some(decorator),
                fragment,
                ..decoration.underline_style(color, lengths)
            };
            if let Some(outer) = self.underline.replace(underline) {
                self.extra_decorations.push(ExtraDecoration::Underline(outer));
            }
        }
        if decoration.line_through {
            let strikethrough = StrikethroughStyle {
                decorator: Some(decorator),
                fragment,
                ..decoration.strikethrough_style(color, lengths)
            };
            if let Some(outer) = self.strikethrough.replace(strikethrough) {
                self.extra_decorations
                    .push(ExtraDecoration::Strikethrough(outer));
            }
        }
        if decoration.overline {
            let overline = OverlineStyle {
                fragment,
                ..decoration.overline_style(color, lengths)
            };
            if let Some(outer) = self.overline.replace(overline) {
                self.extra_decorations.push(ExtraDecoration::Overline(outer));
            }
        }
    }

    /// Whether the text style of a box has another font or line height than the one it inherits
    /// from the container, so that its strut is another one.
    fn changes_strut(&self) -> bool {
        self.font_size.is_some()
            || self.line_height.is_some()
            || self.weight.is_some()
            || self.font_style.is_some()
            || self.family.is_some()
    }

    fn inherits_everything(&self) -> bool {
        !self.atomic
            && Self {
                len: 0,
                piece: None,
                ..self.clone()
            } == Self::default()
    }
}

/// An inline box of a paragraph as the line layout needs to know it: where it is
/// and how it sits on its line. See [`listed_boxes`].
#[derive(Clone, Debug, PartialEq)]
pub struct BoxStyle {
    pub id: u64,
    pub range: Range<usize>,
    /// The box around it that has a box of its own.
    pub parent: Option<u64>,
    /// What the box changes about the text style it inherits, with everything the
    /// boxes around it declared: the style of its own text.
    pub style: RunStyle,
    pub align: ResolvedVerticalAlign,
    /// Whether a line may break in the box's text, as the box or the boxes around it
    /// say; `None` when none of them does and the container's mode stands.
    pub wraps: Option<bool>,
    /// Whether the box itself says so. Such a box is listed to the line layout whatever
    /// its alignment is, because a box that does not wrap takes the break before its
    /// first space away from the word before it, whether it changes the mode or not.
    pub declares_wrap: bool,
    /// Whether the box declares an underline, which is drawn at the baseline of the box.
    pub decorates: bool,
    /// Whether the box declares a strike-through, which is drawn by the ascent of its font.
    pub strikes: bool,
    /// The box holds nothing but its placeholder ([`RunStyle::placeholder`]). It is always listed
    /// to the line layout: its strut and its content area are the box's.
    pub empty: bool,
}

/// The runs of `paragraph`, covering its text exactly, one per text node. Empty when no box
/// restyles anything and the text is one node, so the caller can paint it as plain text.
///
/// `colors`, `lengths`, `font_state` and `underline_offset` are the container's, as its
/// children inherit them. `states` says which states apply to a box, so that its `hover`, `active`,
/// `focusWithin` and `focusVisible` styles are folded into the run it paints.
/// `motion` holds the boxes whose colours a transition or an animation moves.
#[cfg_attr(not(test), allow(dead_code))]
pub fn run_styles(
    tree: &RetainedTree,
    paragraph: &Paragraph,
    colors: &ColorContext,
    lengths: &LengthContext,
    font_state: &FontFeatureState,
    underline_offset: TextUnderlineOffset,
    states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    motion: &InlineStyles,
) -> Vec<RunStyle> {
    styles(
        tree,
        paragraph,
        colors,
        lengths,
        font_state,
        underline_offset,
        states,
        motion,
    )
    .runs
}

/// An inline box with an `opacity` below one. CSS composites it as a unit: everything it
/// paints (its background, border and outline, its glyphs and decoration lines, and what its
/// descendants paint) is drawn opaque into a layer that is then faded. It is painted after the
/// text around it, as a stacking context is.
#[derive(Clone, Debug, PartialEq)]
pub struct OpacityGroup {
    /// The id of the box, which is also what [`RunStyle::group`] and the decorations name.
    pub id: u64,
    /// The group around it, `0` for none.
    pub parent: u64,
    pub opacity: f32,
}

/// What [`styles`] reads off the boxes of a paragraph.
pub struct Styles {
    /// See [`run_styles`]. A paragraph with an atomic inline always has runs: the
    /// placeholder is a run of its own.
    pub runs: Vec<RunStyle>,
    /// The boxes whose `opacity` is below one, outermost first.
    pub groups: Vec<OpacityGroup>,
    /// The boxes with a box of their own, outermost first.
    pub boxes: Vec<BoxStyle>,
}

/// [`run_styles`], with the style and alignment of each inline box too.
pub fn styles(
    tree: &RetainedTree,
    paragraph: &Paragraph,
    colors: &ColorContext,
    lengths: &LengthContext,
    font_state: &FontFeatureState,
    underline_offset: TextUnderlineOffset,
    states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    motion: &InlineStyles,
) -> Styles {
    let mut runs: Vec<RunStyle> = Vec::with_capacity(paragraph.pieces.len());
    let mut boxes: Vec<BoxStyle> = Vec::new();
    let mut groups: Vec<OpacityGroup> = Vec::new();
    for (piece_ix, piece) in paragraph.pieces.iter().enumerate() {
        let mut style = RunStyle {
            len: piece.range.len(),
            piece: Some(piece_ix),
            ..RunStyle::default()
        };
        let mut colors = *colors;
        let mut lengths = *lengths;
        let mut font_state = font_state.clone();
        let mut underline_offset = underline_offset;
        let mut group = 0;
        let mut parent = None;
        let mut wraps = None;
        for id in &piece.boxes {
            let (mut decorates, mut strikes) = (false, false);
            let Some(base) = motion.declared_by(tree, *id) else {
                // A box with no style inherits everything, and is still a box.
                note_box(&mut boxes, paragraph, *id, parent, &style, ResolvedVerticalAlign::Baseline, wraps, (false, false, false));
                parent = Some(*id);
                continue;
            };
            let declared = &*motion.declared(*id, base, states(*id, base));
            colors = colors.descend(declared);
            // A `display: contents` box has a font size too, and its children
            // inherit it.
            lengths = lengths.descend(declared.font_size.as_ref(), false);
            underline_offset = underline_offset.inherit(declared.text_underline_offset.as_ref(), &lengths);
            // It has no box, so what it declares reaches the text only as far as
            // it is inherited.
            let has_box = declared.display.as_deref() != Some("contents");
            if has_box {
                let opacity = box_opacity(declared);
                if opacity < 1.0 {
                    if !groups.iter().any(|known| known.id == *id) {
                        groups.push(OpacityGroup { id: *id, parent: group, opacity });
                    }
                    group = *id;
                }
            }
            if declared.font_size.is_some() {
                style.font_size = Some(lengths.font_size as f32);
            }
            if let Some(line_height) = declared.line_height.as_ref() {
                style.line_height = Some(line_height.text_style_value(&lengths));
            }
            if let Some(color) = colors.declared() {
                style.color = Some(color.into());
            }
            if let Some(weight) = declared.font_weight.as_ref() {
                style.weight = Some(crate::renderer::parse_font_weight(weight));
            }
            if let Some(font_style) = declared
                .font_style
                .as_deref()
                .and_then(crate::renderer::parse_font_style)
            {
                style.font_style = Some(font_style);
            }
            if let Some(family) = declared.font_family.as_ref() {
                style.family = Some(SharedString::from(family.clone()));
            }
            let (next, features) = font_state.descend(
                declared.font_variant_numeric.as_deref(),
                declared.font_feature_settings.as_deref(),
            );
            font_state = next;
            if features.is_some() {
                style.features = features;
            }
            if let Some(settings) = &declared.font_variation_settings {
                style.variations = Some(settings.variations());
            }
            if let Some(smoothing) = declared.webkit_font_smoothing {
                style.font_smoothing = Some(smoothing.to_gpui());
            }
            // Inherited by what is inside whether the box has a box or not.
            let declared_wraps = crate::css::text_wrap::declared_wraps(declared);
            wraps = declared_wraps.or(wraps);
            // CSS draws a decoration over the whole box and a descendant cannot
            // take it off, so `none` on a box adds nothing and removes nothing.
            if has_box {
                if let Some(mut decoration) = text_decoration::of(declared) {
                    decoration.underline_offset = underline_offset;
                    let color = decoration.resolve_color(&colors);
                    style.add_decoration(&decoration, color, &lengths, *id, piece_ix as u32 + 1);
                    decorates = decoration.underline;
                    strikes = decoration.line_through;
                }
                let align = declared
                    .vertical_align
                    .as_ref()
                    .map_or(ResolvedVerticalAlign::Baseline, |align| align.resolve(&lengths));
                note_box(&mut boxes, paragraph, *id, parent, &style, align, wraps, (declared_wraps.is_some(), decorates, strikes));
                parent = Some(*id);
            }
        }
        style.group = group;
        if piece.placeholder {
            style.placeholder = true;
            style.underline = None;
            style.strikethrough = None;
            style.overline = None;
            style.extra_decorations.clear();
            if let Some(known) = piece.boxes.last().and_then(|last| boxes.iter_mut().find(|known| known.id == *last)) {
                known.empty = true;
            }
        }
        if piece.atomic.is_some() {
            style = RunStyle {
                len: piece.range.len(),
                atomic: true,
                piece: Some(piece_ix),
                ..RunStyle::default()
            };
        }
        runs.push(style);
    }
    // A box that only changes its alignment still needs the runs: the line layout
    // splits them where the box starts and ends. So do two text nodes, which are two items.
    if paragraph.pieces.len() <= 1
        && runs.iter().all(RunStyle::inherits_everything)
        && boxes
            .iter()
            .all(|style| style.align == ResolvedVerticalAlign::Baseline && !style.declares_wrap)
    {
        runs.clear();
    }
    Styles { runs, groups, boxes }
}

/// Records the first sight of the box `id`, whose text has `style`.
fn note_box(
    boxes: &mut Vec<BoxStyle>,
    paragraph: &Paragraph,
    id: u64,
    parent: Option<u64>,
    style: &RunStyle,
    align: ResolvedVerticalAlign,
    wraps: Option<bool>,
    (declares_wrap, decorates, strikes): (bool, bool, bool),
) {
    if boxes.iter().any(|known| known.id == id) {
        return;
    }
    if let Some((_, range)) = paragraph.boxes.iter().find(|(box_id, _)| *box_id == id) {
        boxes.push(BoxStyle {
            id,
            range: range.clone(),
            parent,
            style: RunStyle { len: 0, ..style.clone() },
            align,
            wraps,
            declares_wrap,
            decorates,
            strikes,
            empty: false,
        });
    }
}

/// The boxes of a paragraph the line layout is told about, outermost first: those
/// aligned to something other than the baseline, and those around a box that is,
/// or around an atomic inline that is (`shifted`), since the alignment of a box
/// is relative to the font of the box it is in. A box around an atomic inline
/// whose font or line height differs from the container's is listed too, for its
/// strut: it is in the line whatever the box holds. A baseline-aligned box with
/// nothing shifted in it and no strut of its own has nothing to say.
pub fn listed_boxes(
    paragraph: &Paragraph,
    boxes: &[BoxStyle],
    shifted: &HashSet<u64>,
) -> Vec<BoxStyle> {
    let by_id: HashMap<u64, &BoxStyle> = boxes.iter().map(|style| (style.id, style)).collect();
    let struts_around = |atomic: &&super::inline::Piece| {
        atomic
            .boxes
            .iter()
            .filter_map(|id| by_id.get(id))
            .any(|style| style.style.changes_strut())
    };
    // Where some box wraps, every box is listed: the ones that do not wrap are the line
    // layout's to tell from the text around them, and a plain box is one of them where
    // the text does not wrap.
    let mixed_wrap = boxes.iter().any(|style| style.wraps == Some(true));
    // An underline is drawn at the baseline of its box, which only differs from that of its
    // row where a box is aligned to something else.
    let aligned = boxes.iter().any(|style| style.align != ResolvedVerticalAlign::Baseline);
    // A strike-through goes by the ascent of the font of the box that declared it, which
    // the line layout reads off the box, so a box in another font than the text it is over is
    // listed.
    let fonts_differ = boxes.iter().any(|style| style.style.changes_strut());
    let needs_listing = |style: &BoxStyle| {
        mixed_wrap
            || style.empty
            || style.align != ResolvedVerticalAlign::Baseline
            || style.declares_wrap
            || (aligned && style.decorates)
            || (fonts_differ && style.strikes)
    };
    if shifted.is_empty()
        && !boxes.iter().any(needs_listing)
        && !paragraph.atomics().map(|(_, piece)| piece).any(|piece| struts_around(&piece))
    {
        return Vec::new();
    }
    let mut needed: HashSet<u64> = HashSet::new();
    let mut need_chain = |mut id: Option<u64>| {
        while let Some(current) = id {
            if !needed.insert(current) {
                break;
            }
            id = by_id.get(&current).and_then(|style| style.parent);
        }
    };
    for style in boxes.iter().filter(|style| needs_listing(style)) {
        need_chain(Some(style.id));
    }
    for (atomic, piece) in paragraph.atomics() {
        if shifted.contains(&atomic) {
            need_chain(piece.boxes.iter().rev().find(|id| by_id.contains_key(id)).copied());
        }
        for id in piece.boxes.iter().filter(|id| by_id.get(id).is_some_and(|style| style.style.changes_strut())) {
            need_chain(Some(*id));
        }
    }
    paragraph
        .boxes
        .iter()
        .filter_map(|(id, _)| by_id.get(id).filter(|_| needed.contains(id)))
        .map(|style| (*style).clone())
        .collect()
}

/// The `opacity` of a box, which is a compositing group ([`OpacityGroup`]) below one.
pub(crate) fn box_opacity(style: &StyleDesc) -> f32 {
    style.opacity.map_or(1.0, |opacity| opacity.clamp(0.0, 1.0) as f32)
}

pub(crate) fn fade(color: Hsla, opacity: f32) -> Hsla {
    Hsla { a: color.a * opacity, ..color }
}

/// The `LengthContext` of every box of `paragraph`: the container's, descended
/// along the chain of boxes around the box, so that a span's `em` is its own
/// font size. A box without a style has the lengths of the box around it. A box is declared the same way around every piece of text it
/// holds, so the first piece that holds it decides.
pub fn box_lengths(
    tree: &RetainedTree,
    paragraph: &Paragraph,
    lengths: &LengthContext,
    states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    motion: &InlineStyles,
) -> HashMap<u64, LengthContext> {
    let mut of_box = HashMap::with_capacity(paragraph.boxes.len());
    for piece in &paragraph.pieces {
        let mut lengths = *lengths;
        for id in &piece.boxes {
            let Some(base) = motion.declared_by(tree, *id) else {
                of_box.entry(*id).or_insert(lengths);
                continue;
            };
            lengths = lengths.descend(motion.declared(*id, base, states(*id, base)).font_size.as_ref(), false);
            of_box.entry(*id).or_insert(lengths);
        }
    }
    of_box
}

/// What turns the line height of a run into the pixels its line box is laid out with.
pub struct RunHeights<'a> {
    /// The device pixels to a pixel of the window: a line height is laid out in 1/64 of one, as
    /// `TextLayout::layout` does the line's (`gpui::LineHeight::to_laid_out_pixels`).
    pub scale_factor: f32,
    /// The height `line-height: normal` gives text of a font and size.
    pub normal: &'a dyn Fn(&gpui::Font, Pixels) -> Pixels,
}

/// The line height a run contributes to its line box, or `None` when it takes
/// the line's. A run with its own size has one even when no box declared a line
/// height, because the line height it inherits is a multiple of that size, and
/// so does a run in another font when the line height is `normal`, which is the
/// height of the font.
fn run_line_height(
    style: &RunStyle,
    run_font: &gpui::Font,
    run_font_size: Option<Pixels>,
    base: &gpui::TextStyle,
    rem_size: Pixels,
    heights: &RunHeights,
) -> Option<Pixels> {
    let line_height = style.line_height.unwrap_or(base.line_height);
    let own = style.line_height.is_some()
        || run_font_size.is_some()
        || (line_height == gpui::LineHeight::Normal && *run_font != base.font());
    if !own {
        return None;
    }
    let font_size = run_font_size.unwrap_or_else(|| base.font_size.to_pixels(rem_size));
    Some(line_height.to_laid_out_pixels(font_size.into(), rem_size, heights.scale_factor, || {
        (heights.normal)(run_font, font_size)
    }))
}

/// `StyledText` whose runs are `styles` laid over the inherited text style.
pub struct InlineText {
    styled: StyledText,
    styles: Vec<RunStyle>,
    /// The boxes the line layout aligns, see [`listed_boxes`].
    boxes: Vec<BoxStyle>,
    /// The atomic inlines. `parent` of each is an index of `boxes`.
    atomics: Vec<AtomicInline>,
}

impl InlineText {
    pub fn new(text: SharedString, styles: Vec<RunStyle>) -> Self {
        assert_eq!(
            styles.iter().map(|style| style.len).sum::<usize>(),
            text.len(),
            "inline runs must cover the paragraph exactly"
        );
        Self {
            styled: StyledText::new(text),
            styles,
            boxes: Vec::new(),
            atomics: Vec::new(),
        }
    }

    /// Aligns `boxes` on their lines and puts `atomics` in the text.
    pub fn with_boxes(mut self, boxes: Vec<BoxStyle>, atomics: Vec<AtomicInline>) -> Self {
        self.boxes = boxes;
        self.atomics = atomics;
        self
    }

    /// Lays the text out with the space `insets` put around ranges of it at the
    /// width the text is laid out in, see [`super::inline_insets`].
    pub fn with_insets_for_width(
        mut self,
        key: u64,
        insets: impl Fn(Option<gpui::Pixels>) -> Vec<TextInset> + 'static,
    ) -> Self {
        self.styled = self.styled.with_insets_for_width(key, insets);
        self
    }

    /// See [`StyledText::with_text_box_trim`].
    pub fn with_text_box_trim(mut self, trim: gpui::TextBoxTrim) -> Self {
        self.styled = self.styled.with_text_box_trim(trim);
        self
    }

    pub fn layout(&self) -> &TextLayout {
        self.styled.layout()
    }

    fn run(
        style: &RunStyle,
        base: &gpui::TextStyle,
        rem_size: Pixels,
        heights: &RunHeights,
    ) -> TextRun {
        let base_font_size = base.font_size.to_pixels(rem_size);
        let mut run = base.to_run(style.len);
        run.group = style.group;
        if style.atomic {
            run.background_color = None;
            run.underline = None;
            run.strikethrough = None;
            run.overline = None;
            run.extra_decorations.clear();
            return run;
        }
        run.font_size = style
            .font_size
            .map(px)
            .filter(|size| (f32::from(*size) - f32::from(base_font_size)).abs() > 1e-3);
        if let Some(color) = style.color {
            run.color = color;
        }
        if let Some(weight) = style.weight {
            run.font.weight = weight;
        }
        if let Some(font_style) = style.font_style {
            run.font.style = font_style;
        }
        if let Some(family) = &style.family {
            run.font.family = family.clone();
        }
        if let Some(features) = &style.features {
            run.font.features = features.clone();
        }
        if let Some(variations) = &style.variations {
            run.font.variations = variations.clone();
        }
        if let Some(smoothing) = style.font_smoothing {
            run.font_smoothing = smoothing;
        }
        run.line_height = run_line_height(style, &run.font, run.font_size, base, rem_size, heights);
        run.normal_line_height =
            style.line_height.unwrap_or(base.line_height) == gpui::LineHeight::Normal;
        // The lines the text inherits from the boxes around the paragraph
        // stay under the ones the run's own boxes declare.
        if let Some(underline) = style.underline {
            if let Some(outer) = run.underline.replace(underline) {
                run.extra_decorations.push(ExtraDecoration::Underline(outer));
            }
        }
        if let Some(strikethrough) = style.strikethrough {
            if let Some(outer) = run.strikethrough.replace(strikethrough) {
                run.extra_decorations
                    .push(ExtraDecoration::Strikethrough(outer));
            }
        }
        if let Some(overline) = style.overline {
            if let Some(outer) = run.overline.replace(overline) {
                run.extra_decorations.push(ExtraDecoration::Overline(outer));
            }
        }
        run.extra_decorations
            .extend(style.extra_decorations.iter().copied());
        run
    }

    /// The runs of the text over `base`, one per text node. Where a line the paragraph's own
    /// block declares has a pattern (dotted, dashed or wavy), each has its own fragment of the
    /// line, which Chrome draws with the pattern started over.
    fn runs(&self, base: &gpui::TextStyle, rem_size: Pixels, heights: &RunHeights) -> Vec<TextRun> {
        let patterned = base.underline.is_some_and(|line| is_patterned(line.style))
            || base.strikethrough.is_some_and(|line| is_patterned(line.style))
            || base.overline.is_some_and(|line| is_patterned(line.style));
        self.styles
            .iter()
            .map(|style| {
                let piece = match style.piece {
                    Some(piece) if patterned && !style.atomic => piece,
                    _ => return Self::run(style, base, rem_size, heights),
                };
                let mut base = base.clone();
                let fragment = piece as u32 + 1;
                for line in base.underline.iter_mut().filter(|line| is_patterned(line.style)) {
                    line.fragment = fragment;
                }
                for line in base.strikethrough.iter_mut().filter(|line| is_patterned(line.style)) {
                    line.fragment = fragment;
                }
                for line in base.overline.iter_mut().filter(|line| is_patterned(line.style)) {
                    line.fragment = fragment;
                }
                Self::run(style, &base, rem_size, heights)
            })
            .collect()
    }

    /// The boxes as the line layout takes them: the font of a box is that of its
    /// own text, and its line height the one its runs have.
    fn inline_boxes(
        &self,
        base: &gpui::TextStyle,
        rem_size: Pixels,
        heights: &RunHeights,
    ) -> Vec<InlineBox> {
        let base_font_size = base.font_size.to_pixels(rem_size);
        self.boxes
            .iter()
            .map(|style| {
                let run = Self::run(&style.style, base, rem_size, heights);
                InlineBox {
                    range: style.range.clone(),
                    parent: style
                        .parent
                        .and_then(|parent| self.boxes.iter().position(|listed| listed.id == parent)),
                    align: vertical_align(style.align),
                    line_height: run.line_height,
                    wraps: style.wraps,
                    key: Some(style.id),
                    content: if style.empty {
                        InlineContent::Empty {
                            font: run.font,
                            font_size: run.font_size.unwrap_or(base_font_size),
                        }
                    } else {
                        InlineContent::Text {
                            font: run.font,
                            font_size: run.font_size.unwrap_or(base_font_size),
                        }
                    },
                }
            })
            .collect()
    }
}

/// How far from the baseline the underlines of `runs` are put and how thick they are, in px.
pub fn line_reach(runs: &[RunStyle]) -> f32 {
    let reach = |line: &UnderlineStyle| line.offset.map_or(0.0, |offset| f32::from(offset).abs()) + f32::from(line.thickness);
    runs.iter()
        .flat_map(|run| {
            run.underline.iter().chain(run.extra_decorations.iter().filter_map(|extra| match extra {
                ExtraDecoration::Underline(line) => Some(line),
                _ => None,
            }))
        })
        .map(reach)
        .fold(0.0, f32::max)
}

/// `align` as the line layout takes it.
pub fn vertical_align(align: ResolvedVerticalAlign) -> gpui::VerticalAlign {
    match align {
        ResolvedVerticalAlign::Baseline => gpui::VerticalAlign::Baseline,
        ResolvedVerticalAlign::Sub => gpui::VerticalAlign::Sub,
        ResolvedVerticalAlign::Super => gpui::VerticalAlign::Super,
        ResolvedVerticalAlign::TextTop => gpui::VerticalAlign::TextTop,
        ResolvedVerticalAlign::TextBottom => gpui::VerticalAlign::TextBottom,
        ResolvedVerticalAlign::Middle => gpui::VerticalAlign::Middle,
        ResolvedVerticalAlign::Top => gpui::VerticalAlign::Top,
        ResolvedVerticalAlign::Bottom => gpui::VerticalAlign::Bottom,
        ResolvedVerticalAlign::Offset { px: length, line_height_fraction } => gpui::VerticalAlign::Offset {
            length: px(length as f32),
            line_height_fraction: line_height_fraction as f32,
        },
    }
}

impl gpui::IntoElement for InlineText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for InlineText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, ()) {
        let rem_size = window.rem_size();
        let text_style = window.text_style();
        let text_system = window.text_system().clone();
        let scale_factor = window.scale_factor();
        let heights = RunHeights {
            scale_factor,
            normal: &|font, font_size| {
                text_system
                    .line_box(text_system.resolve_font(font), font_size, scale_factor)
                    .normal_line_height()
            },
        };
        let runs = self.runs(&text_style, rem_size, &heights);
        let boxes = self.inline_boxes(&text_style, rem_size, &heights);
        // `with_runs` takes the element by value. The placeholder is dropped
        // at once; the layout cell that callers cloned lives in `styled`.
        let styled = std::mem::replace(&mut self.styled, StyledText::new(""));
        self.styled = styled
            .with_runs(runs)
            .with_inline_boxes(boxes)
            .with_atomic_inlines(std::mem::take(&mut self.atomics));
        gpui::Element::request_layout(&mut self.styled, id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        state: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        gpui::Element::prepaint(
            &mut self.styled,
            id,
            inspector_id,
            bounds,
            state,
            window,
            cx,
        )
    }

    fn paint(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        request_layout: &mut (),
        prepaint: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        gpui::Element::paint(
            &mut self.styled,
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        )
    }

    fn prepaint_culled(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        state: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        gpui::Element::prepaint_culled(
            &mut self.styled,
            id,
            inspector_id,
            bounds,
            state,
            window,
            cx,
        )
    }

    fn paint_culled(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        request_layout: &mut (),
        prepaint: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        gpui::Element::paint_culled(
            &mut self.styled,
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Scheme;
    use crate::text::inline::plan_children;
    use crate::text::inline::Child;
    use gpui::TextDecorationStyle;
    use std::sync::Arc;

    fn tree_with(styles: &[(u64, serde_json::Value)]) -> RetainedTree {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "text".to_string());
        tree.append_child(1, 2);
        tree.set_text(2, "a ".to_string());
        tree.create_element(3, "span".to_string());
        tree.append_child(1, 3);
        tree.create_element(4, "text".to_string());
        tree.append_child(3, 4);
        tree.set_text(4, "bold".to_string());
        tree.create_element(5, "span".to_string());
        tree.append_child(3, 5);
        tree.create_element(6, "text".to_string());
        tree.append_child(5, 6);
        tree.set_text(6, "red".to_string());
        tree.create_element(7, "text".to_string());
        tree.append_child(1, 7);
        tree.set_text(7, " end".to_string());
        for (id, style) in styles {
            let style = serde_json::from_value(style.clone()).expect("a style");
            tree.set_style(*id, Arc::new(style));
        }
        tree
    }

    fn runs_of(tree: &RetainedTree) -> Vec<RunStyle> {
        runs_with(tree, &|_, _| PointerStates::default())
    }

    fn runs_with(
        tree: &RetainedTree,
        states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    ) -> Vec<RunStyle> {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        run_styles(
            tree,
            paragraph,
            &ColorContext::root(Scheme::Light),
            &container_lengths(),
            &FontFeatureState::default(),
                Default::default(),
            states,
            &InlineStyles::default(),
        )
    }

    /// Heights of a window of scale 1 whose font has no metrics of its own: a `normal` line is a
    /// quarter taller than its text.
    fn exact_heights() -> RunHeights<'static> {
        RunHeights {
            scale_factor: 1.0,
            normal: &|_, font_size| font_size * 1.25,
        }
    }

    /// A container with `fontSize: 10`, which is the root's too.
    fn container_lengths() -> LengthContext {
        LengthContext::initial(800.0, 600.0)
            .descend(Some(&crate::css::length::LinearLength::px(10.0)), true)
    }

    fn sizes(tree: &RetainedTree) -> Vec<Option<f32>> {
        runs_of(tree).iter().map(|run| run.font_size).collect()
    }

    /// The runs gpui gets for `tree`'s paragraph in a container of 10px text
    /// whose line height is `container_line_height`.
    fn text_runs(tree: &RetainedTree, container_line_height: gpui::LineHeight) -> Vec<TextRun> {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        let base = gpui::TextStyle {
            font_size: px(10.0).into(),
            line_height: container_line_height,
            ..Default::default()
        };
        let styles = run_styles(
            tree,
            paragraph,
            &ColorContext::root(Scheme::Light),
            &container_lengths(),
            &FontFeatureState::default(),
                Default::default(),
            &|_, _| PointerStates::default(),
            &InlineStyles::default(),
        );
        InlineText::new(paragraph.text.clone().into(), styles).runs(&base, px(16.0), &exact_heights())
    }

    fn line_heights(runs: &[TextRun]) -> Vec<Option<f32>> {
        runs.iter()
            .map(|run| run.line_height.map(f32::from))
            .collect()
    }

    #[test]
    fn a_span_font_size_is_its_runs_size_and_a_nested_span_inherits_it() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": 20}))]);
        assert_eq!(sizes(&tree), vec![None, Some(20.0), Some(20.0), None]);
    }

    #[test]
    fn a_nested_font_size_in_em_is_relative_to_the_span_around_it() {
        let tree = tree_with(&[
            (3, serde_json::json!({"fontSize": 20})),
            (5, serde_json::json!({"fontSize": "1.5em"})),
        ]);
        assert_eq!(sizes(&tree), vec![None, Some(20.0), Some(30.0), None]);
    }

    #[test]
    fn a_font_size_in_rem_is_the_roots() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": "3rem"}))]);
        assert_eq!(sizes(&tree), vec![None, Some(30.0), Some(30.0), None]);
    }

    #[test]
    fn unsized_text_has_no_font_size() {
        let tree = tree_with(&[(3, serde_json::json!({"fontWeight": 700}))]);
        assert_eq!(sizes(&tree), vec![None; 4]);
    }

    #[test]
    fn a_display_contents_box_passes_its_font_size_to_its_children() {
        let tree = tree_with(&[
            (3, serde_json::json!({"fontWeight": 700})),
            (5, serde_json::json!({"display": "contents", "fontSize": 20})),
        ]);
        assert_eq!(sizes(&tree), vec![None, None, Some(20.0), None]);
        let nested = tree_with(&[
            (3, serde_json::json!({"display": "contents", "fontSize": 20})),
            (5, serde_json::json!({"fontSize": "2em"})),
        ]);
        assert_eq!(sizes(&nested), vec![None, Some(20.0), Some(40.0), None]);
    }

    #[test]
    fn a_line_height_number_is_a_multiple_of_the_runs_own_size() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": 20}))]);
        let runs = text_runs(&tree, gpui::relative(1.5).into());
        assert_eq!(line_heights(&runs), vec![None, Some(30.0), Some(30.0), None]);
        assert_eq!(runs[1].font_size, Some(px(20.0)));
        assert_eq!(runs[0].font_size, None);
    }

    #[test]
    fn a_line_height_length_of_the_container_is_inherited_as_px() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": 20}))]);
        let runs = text_runs(&tree, px(24.0).into());
        assert_eq!(line_heights(&runs), vec![None, Some(24.0), Some(24.0), None]);
    }

    #[test]
    fn a_spans_own_line_height_is_resolved_where_it_is_declared() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": 20, "lineHeight": 2}))]);
        let runs = text_runs(&tree, gpui::relative(1.5).into());
        assert_eq!(line_heights(&runs), vec![None, Some(40.0), Some(40.0), None]);

        let tree = tree_with(&[(3, serde_json::json!({"fontSize": 20, "lineHeight": "1.5em"}))]);
        let runs = text_runs(&tree, gpui::relative(1.0).into());
        assert_eq!(line_heights(&runs), vec![None, Some(30.0), Some(30.0), None]);

        let tree = tree_with(&[
            (3, serde_json::json!({"lineHeight": "1.5em"})),
            (5, serde_json::json!({"fontSize": 30})),
        ]);
        let runs = text_runs(&tree, gpui::relative(1.0).into());
        assert_eq!(
            line_heights(&runs),
            vec![None, Some(15.0), Some(15.0), None],
            "a length is px by the time the nested span sizes its text"
        );
    }

    #[test]
    fn a_nested_span_resizes_a_multiplier_it_inherits() {
        let tree = tree_with(&[
            (3, serde_json::json!({"lineHeight": 2})),
            (5, serde_json::json!({"fontSize": 30})),
        ]);
        let runs = text_runs(&tree, gpui::relative(1.0).into());
        assert_eq!(line_heights(&runs), vec![None, Some(20.0), Some(60.0), None]);
    }

    #[test]
    fn a_span_with_only_a_line_height_has_one_and_keeps_the_size() {
        let tree = tree_with(&[(3, serde_json::json!({"lineHeight": 3}))]);
        let runs = text_runs(&tree, gpui::relative(1.5).into());
        assert_eq!(line_heights(&runs), vec![None, Some(30.0), Some(30.0), None]);
        assert_eq!(runs[1].font_size, None);
    }

    #[test]
    fn a_span_that_sets_nothing_about_size_takes_the_lines() {
        let tree = tree_with(&[(3, serde_json::json!({"fontWeight": 700}))]);
        let runs = text_runs(&tree, gpui::relative(1.5).into());
        assert_eq!(line_heights(&runs), vec![None; 4]);
        assert!(runs.iter().all(|run| run.font_size.is_none()));
    }

    #[test]
    fn under_a_normal_line_height_a_span_in_another_font_has_the_height_of_that_font() {
        // The fake `normal` of `exact_heights` is a quarter more than the size: 12.5px at 10px.
        let tree = tree_with(&[(3, serde_json::json!({"fontWeight": 700}))]);
        let runs = text_runs(&tree, gpui::LineHeight::Normal);
        assert_eq!(line_heights(&runs), vec![None, Some(12.5), Some(12.5), None]);

        let tree = tree_with(&[(3, serde_json::json!({"fontFamily": "Menlo"}))]);
        let runs = text_runs(&tree, gpui::LineHeight::Normal);
        assert_eq!(line_heights(&runs), vec![None, Some(12.5), Some(12.5), None]);

        let tree = tree_with(&[(3, serde_json::json!({"fontSize": 20}))]);
        let runs = text_runs(&tree, gpui::LineHeight::Normal);
        assert_eq!(line_heights(&runs), vec![None, Some(25.0), Some(25.0), None]);
    }

    #[test]
    fn under_a_normal_line_height_a_span_in_the_font_of_the_container_takes_the_lines() {
        let tree = tree_with(&[(3, serde_json::json!({"color": "#ff0000"}))]);
        let runs = text_runs(&tree, gpui::LineHeight::Normal);
        assert_eq!(line_heights(&runs), vec![None; 4]);
    }

    #[test]
    fn a_span_that_declares_normal_has_the_height_of_its_font_under_a_length() {
        let tree = tree_with(&[(3, serde_json::json!({"lineHeight": "normal"}))]);
        let runs = text_runs(&tree, gpui::relative(2.0).into());
        assert_eq!(line_heights(&runs), vec![None, Some(12.5), Some(12.5), None]);
    }

    #[test]
    fn a_font_size_equal_to_the_containers_is_no_size_of_its_own() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": "1em", "fontWeight": 700}))]);
        let runs = text_runs(&tree, gpui::relative(1.5).into());
        assert!(runs.iter().all(|run| run.font_size.is_none()));
        assert_eq!(line_heights(&runs), vec![None; 4]);
    }

    #[test]
    fn boxes_get_the_lengths_of_their_own_font_size() {
        let tree = tree_with(&[
            (3, serde_json::json!({"fontSize": 20})),
            (5, serde_json::json!({"fontSize": "1.5em"})),
        ]);
        let plan = plan_children(&tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        let lengths = box_lengths(&tree, paragraph, &container_lengths(), &|_, _| PointerStates::default(), &InlineStyles::default());
        assert_eq!(lengths[&3].font_size, 20.0);
        assert_eq!(lengths[&5].font_size, 30.0);
    }

    #[test]
    fn a_hovered_box_paints_its_hover_style_and_its_children_inherit_it() {
        let tree = tree_with(&[
            (
                3,
                serde_json::json!({
                    "color": "#ff0000",
                    "hover": {"color": "#0000ff", "fontWeight": 700}
                }),
            ),
            (5, serde_json::json!({"fontStyle": "italic"})),
        ]);
        let rest = runs_of(&tree);
        let hovered = runs_with(&tree, &|id, _| PointerStates {
            hovered: id == 3,
            ..Default::default()
        });
        assert_eq!(rest[1].weight, None);
        assert_ne!(hovered[1].color, rest[1].color);
        assert_eq!(hovered[1].weight, Some(FontWeight(700.0)));
        assert_eq!(
            hovered[2].color, hovered[1].color,
            "the span nested in it inherits the hover colour"
        );
        assert_eq!(hovered[0], rest[0], "the text before it does not change");
    }

    #[test]
    fn unstyled_boxes_leave_every_text_node_a_run_that_inherits_everything() {
        let runs = runs_of(&tree_with(&[]));
        assert!(runs.iter().all(RunStyle::inherits_everything));
        assert_eq!(runs.iter().map(|run| run.len).collect::<Vec<_>>(), vec![2, 4, 3, 4]);
    }

    #[test]
    fn a_nested_box_inherits_from_its_parent_box() {
        let tree = tree_with(&[
            (
                3,
                serde_json::json!({"color": "#ff0000", "fontWeight": 700}),
            ),
            (5, serde_json::json!({"fontStyle": "italic"})),
        ]);
        let runs = runs_of(&tree);
        let lens: Vec<_> = runs.iter().map(|run| run.len).collect();
        assert_eq!(lens, vec![2, 4, 3, 4]);
        assert_eq!(
            runs[0],
            RunStyle {
                len: 2,
                piece: runs[0].piece,
                ..RunStyle::default()
            }
        );
        assert_eq!(runs[1].weight, Some(FontWeight(700.0)));
        assert_eq!(runs[1].font_style, None);
        assert_eq!(runs[2].weight, Some(FontWeight(700.0)));
        assert_eq!(runs[2].color, runs[1].color);
        assert_eq!(runs[2].font_style, Some(gpui::FontStyle::Italic));
        assert_eq!(
            runs[3],
            RunStyle {
                len: 4,
                piece: runs[3].piece,
                ..RunStyle::default()
            }
        );
    }

    #[test]
    fn a_child_colour_wins_over_its_parents() {
        let tree = tree_with(&[
            (3, serde_json::json!({"color": "#ff0000"})),
            (5, serde_json::json!({"color": "#0000ff"})),
        ]);
        let runs = runs_of(&tree);
        assert_ne!(runs[1].color, runs[2].color);
    }

    #[test]
    fn a_background_is_not_part_of_the_run() {
        let tree = tree_with(&[(
            5,
            serde_json::json!({"backgroundColor": "#ffff00", "borderTopWidth": 2, "borderTopStyle": "solid", "paddingLeft": 4}),
        )]);
        assert!(
            runs_of(&tree).iter().all(RunStyle::inherits_everything),
            "the box paints it around its fragments"
        );
    }

    #[test]
    fn adjacent_pieces_with_one_style_are_runs_of_their_own() {
        let tree = tree_with(&[(3, serde_json::json!({"color": "#ff0000"}))]);
        let runs = runs_of(&tree);
        let lens: Vec<_> = runs.iter().map(|run| run.len).collect();
        assert_eq!(lens, vec![2, 4, 3, 4], "Chrome ceils the width of every text node on its own");
        assert_eq!(runs[1].color, runs[2].color, "bold and red are styled the same");
        let pieces: Vec<_> = runs.iter().map(|run| run.piece).collect();
        assert_eq!(pieces, vec![Some(0), Some(1), Some(2), Some(3)]);
    }

    #[test]
    fn decorations_accumulate_and_none_removes_nothing() {
        let tree = tree_with(&[
            (3, serde_json::json!({"textDecoration": "underline"})),
            (5, serde_json::json!({"textDecoration": "none"})),
        ]);
        let runs = runs_of(&tree);
        let lens: Vec<_> = runs.iter().map(|run| run.len).collect();
        assert_eq!(lens, vec![2, 4, 3, 4]);
        assert!(runs[1].underline.is_some());
        assert!(runs[2].underline.is_some());
        assert!(runs[3].underline.is_none());
    }

    #[test]
    fn a_line_keeps_the_colour_of_the_box_that_declared_it() {
        let tree = tree_with(&[
            (3, serde_json::json!({"color": "#ff0000", "textDecoration": "underline"})),
            (5, serde_json::json!({"color": "#0000ff"})),
        ]);
        let runs = runs_of(&tree);
        let red = runs[1].underline.expect("underlined").color;
        assert_eq!(runs[1].color, red);
        assert_eq!(runs[2].underline.expect("inherited").color, red);
        assert_ne!(runs[2].color, red);
    }

    #[test]
    fn lines_have_their_own_style_and_colour() {
        let tree = tree_with(&[
            (3, serde_json::json!({"textDecoration": "underline wavy #00ff00"})),
            (5, serde_json::json!({"textDecorationLine": "line-through", "textDecorationColor": "#0000ff"})),
        ]);
        let runs = runs_of(&tree);
        let nested = runs.iter().find(|run| run.strikethrough.is_some()).expect("a run");
        let underline = nested.underline.expect("the parent's underline");
        assert_eq!(underline.style, TextDecorationStyle::Wavy);
        assert_ne!(nested.strikethrough.unwrap().color, underline.color);
        assert_eq!(
            nested.strikethrough.unwrap().style,
            TextDecorationStyle::Solid,
            "a nested declaration of one line brings its own style"
        );
    }

    #[test]
    fn one_declaration_gives_every_line_its_style_colour_and_thickness() {
        let tree = tree_with(&[(
            3,
            serde_json::json!({"textDecoration": "underline overline line-through dotted 3px #00ff00"}),
        )]);
        let run = runs_of(&tree).remove(1);
        let underline = run.underline.expect("underline");
        let strikethrough = run.strikethrough.expect("line-through");
        let overline = run.overline.expect("overline");
        assert_eq!(underline.style, TextDecorationStyle::Dotted);
        assert_eq!(strikethrough.style, TextDecorationStyle::Dotted);
        assert_eq!(overline.style, TextDecorationStyle::Dotted);
        assert_eq!(underline.thickness, gpui::px(3.0));
        assert_eq!(strikethrough.thickness, gpui::px(3.0));
        assert_eq!(overline.thickness, gpui::px(3.0));
        assert!(underline.color.is_some());
        assert_eq!(underline.color, strikethrough.color);
        assert_eq!(underline.color, overline.color);
        assert_eq!(underline.thickness_kind, gpui::DecorationThickness::Length);
    }

    #[test]
    fn a_thickness_in_em_is_the_declaring_spans_own_font_size() {
        let tree = tree_with(&[
            (3, serde_json::json!({"fontSize": 20, "textDecoration": "underline 0.1em"})),
            (5, serde_json::json!({"fontSize": 40})),
        ]);
        let runs = runs_of(&tree);
        assert_eq!(runs[1].underline.unwrap().thickness, gpui::px(2.0));
        assert_eq!(
            runs[2].underline.unwrap().thickness,
            gpui::px(2.0),
            "a descendant with a bigger font keeps the thickness resolved at the declaring span"
        );
    }

    #[test]
    fn a_thickness_in_percent_and_rem_in_a_span() {
        let tree = tree_with(&[
            (3, serde_json::json!({"fontSize": 30, "textDecorationLine": "underline overline", "textDecorationThickness": "10%"})),
            (5, serde_json::json!({"textDecoration": "line-through 0.5rem"})),
        ]);
        let runs = runs_of(&tree);
        assert_eq!(runs[1].underline.unwrap().thickness, gpui::px(3.0));
        assert_eq!(runs[1].overline.unwrap().thickness, gpui::px(3.0));
        assert_eq!(
            runs[2].strikethrough.unwrap().thickness,
            gpui::px(5.0),
            "the container's 10px is the root's"
        );
    }

    #[test]
    fn the_longhands_set_the_style_and_thickness_of_the_lines() {
        let tree = tree_with(&[(
            3,
            serde_json::json!({
                "textDecorationLine": "overline",
                "textDecorationStyle": "double",
                "textDecorationThickness": "from-font",
            }),
        )]);
        let run = runs_of(&tree).remove(1);
        let overline = run.overline.expect("overline");
        assert_eq!(overline.style, TextDecorationStyle::Double);
        assert_eq!(overline.thickness_kind, gpui::DecorationThickness::FromFont);
        assert_eq!(overline.thickness, gpui::px(1.0), "the fallback of a font with no metric: a tenth of the 10px font");
        assert_eq!(run.underline, None);
    }

    #[test]
    fn a_nested_box_that_declares_a_line_again_adds_it_to_the_outer_one() {
        let tree = tree_with(&[
            (3, serde_json::json!({"textDecoration": "underline wavy 2px #ff0000"})),
            (5, serde_json::json!({"textDecoration": "underline dashed #0000ff"})),
        ]);
        let runs = runs_of(&tree);
        let outer = runs[1].underline.expect("outer");
        assert!(runs[1].extra_decorations.is_empty());
        let inner = runs[2].underline.expect("inner");
        assert_eq!((inner.style, inner.thickness), (TextDecorationStyle::Dashed, gpui::px(1.0)));
        assert_ne!(inner.color, outer.color);
        // A line with a pattern starts over with every text node, so this one is another fragment.
        let ExtraDecoration::Underline(carried) = runs[2].extra_decorations[0] else {
            panic!("an underline")
        };
        assert_ne!(carried.fragment, outer.fragment);
        assert_eq!(
            runs[2].extra_decorations,
            vec![ExtraDecoration::Underline(UnderlineStyle { fragment: carried.fragment, ..outer })],
            "the outer underline is still there, with its own style, colour and thickness"
        );
        assert_eq!((outer.style, outer.thickness), (TextDecorationStyle::Wavy, gpui::px(2.0)));
    }

    #[test]
    fn lines_of_each_kind_accumulate_on_their_own() {
        let tree = tree_with(&[
            (3, serde_json::json!({"textDecoration": "underline line-through"})),
            (5, serde_json::json!({"textDecoration": "line-through double"})),
        ]);
        let run = runs_of(&tree).remove(2);
        assert_eq!(run.strikethrough.expect("inner").style, TextDecorationStyle::Double);
        assert!(run.underline.is_some());
        assert!(matches!(run.extra_decorations[..], [ExtraDecoration::Strikethrough(_)]));
    }

    #[test]
    fn a_run_keeps_the_lines_it_inherits_beneath_its_own() {
        let outer = gpui::UnderlineStyle {
            thickness: gpui::px(3.0),
            ..Default::default()
        };
        let base = gpui::TextStyle {
            underline: Some(outer),
            ..Default::default()
        };
        let tree = tree_with(&[(3, serde_json::json!({"textDecoration": "underline dotted"}))]);
        let styles = runs_of(&tree);
        let runs = InlineText::new("a boldred end".into(), styles).runs(&base, px(16.0), &exact_heights());
        assert_eq!(runs[0].underline, Some(outer), "text outside the box keeps its underline");
        assert!(runs[0].extra_decorations.is_empty());
        let inner = runs[1].underline.expect("the box's underline");
        assert_eq!(inner.style, TextDecorationStyle::Dotted);
        assert_eq!(runs[1].extra_decorations, vec![ExtraDecoration::Underline(outer)]);
    }

    #[test]
    fn a_box_with_an_opacity_is_a_group_that_holds_the_boxes_in_it() {
        let tree = tree_with(&[
            (3, serde_json::json!({"opacity": 0.5, "color": "#ff0000"})),
            (5, serde_json::json!({"opacity": 0.25})),
        ]);
        let styles = styles_of(&tree);
        assert_eq!(
            styles.groups,
            vec![
                OpacityGroup { id: 3, parent: 0, opacity: 0.5 },
                OpacityGroup { id: 5, parent: 3, opacity: 0.25 },
            ]
        );
        let groups: Vec<_> = styles.runs.iter().map(|run| run.group).collect();
        assert_eq!(groups, vec![0, 3, 5, 0], "the text outside the boxes is in no group");
        let outer = gpui::Rgba::from(styles.runs[1].color.expect("the colour of the box"));
        assert_eq!((outer.r, outer.a), (1.0, 1.0), "a group is faded as a whole, not its colours");
    }

    #[test]
    fn a_box_that_is_not_faded_is_in_the_group_around_it() {
        let tree = tree_with(&[(3, serde_json::json!({"opacity": 0.5})), (5, serde_json::json!({"color": "#ff0000"}))]);
        let groups: Vec<_> = runs_of(&tree).iter().map(|run| run.group).collect();
        assert_eq!(groups, vec![0, 3, 3, 0]);
    }

    #[test]
    fn a_line_goes_in_the_group_of_the_text_it_is_drawn_over_unfaded() {
        let tree = tree_with(&[
            (3, serde_json::json!({"opacity": 0.5, "textDecoration": "underline #ff0000"})),
            (5, serde_json::json!({"opacity": 0.5})),
        ]);
        let run = &runs_of(&tree)[2];
        let line = gpui::Rgba::from(run.underline.expect("the line of the outer box").color.expect("a colour"));
        assert_eq!(line.a, 1.0);
        assert_eq!(run.group, 5);
    }

    #[test]
    fn a_display_contents_box_has_no_opacity_to_give() {
        let tree = tree_with(&[(3, serde_json::json!({"opacity": 0.5, "display": "contents"}))]);
        assert!(runs_of(&tree).iter().all(|run| run.color.is_none()));
    }

    #[test]
    fn a_display_contents_box_passes_inherited_style_on_and_paints_nothing_of_its_own() {
        let tree = tree_with(&[
            (
                5,
                serde_json::json!({
                    "display": "contents",
                    "color": "#ff0000",
                    "fontWeight": 700,
                    "backgroundColor": "#ffff00",
                    "textDecoration": "underline"
                }),
            ),
        ]);
        let runs = runs_of(&tree);
        assert_eq!(runs[2].weight, Some(FontWeight(700.0)));
        assert!(runs[2].color.is_some());
        assert_eq!(runs[2].underline, None);
    }

    #[test]
    fn a_direct_display_contents_child_passes_its_style_to_the_text() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "div".to_string());
        tree.append_child(1, 2);
        tree.create_element(3, "text".to_string());
        tree.append_child(2, 3);
        tree.set_text(3, "red".to_string());
        let style = serde_json::json!({"display": "contents", "color": "#ff0000", "fontWeight": 700});
        tree.set_style(2, Arc::new(serde_json::from_value(style).expect("a style")));
        let runs = runs_of(&tree);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].weight, Some(FontWeight(700.0)));
        assert!(runs[0].color.is_some());
    }

    #[test]
    fn font_style_is_inherited_and_a_child_can_reset_it() {
        let tree = tree_with(&[
            (3, serde_json::json!({"fontStyle": "italic"})),
            (5, serde_json::json!({"fontStyle": "normal"})),
        ]);
        let runs = runs_of(&tree);
        assert_eq!(runs[1].font_style, Some(gpui::FontStyle::Italic));
        assert_eq!(runs[2].font_style, Some(gpui::FontStyle::Normal));
    }

    #[test]
    fn a_focused_box_paints_its_focus_styles_and_an_ancestor_only_its_focus_within() {
        use crate::focus::FocusStyles;

        let ring = |color: &str| {
            serde_json::json!({
                "color": color,
                "focusVisible": {"color": "#ff0000"},
                "focusWithin": {"fontWeight": 700}
            })
        };
        let tree = tree_with(&[(3, ring("#000000")), (5, ring("#00ff00"))]);
        let rest = runs_of(&tree);
        let focus = FocusStyles {
            visible: Some(5),
            within: Arc::from([5, 3, 1]),
            ..Default::default()
        };
        let focused = runs_with(&tree, &|id, style| {
            focus.states_of(id, style, PointerStates::default())
        });

        assert_eq!(focused[1].weight, Some(FontWeight(700.0)), "the box around it is focus-within");
        assert_eq!(focused[1].color, rest[1].color, "but not focus-visible");
        assert_eq!(focused[2].weight, Some(FontWeight(700.0)));
        assert_ne!(focused[2].color, rest[2].color);
        assert_eq!(focused[0], rest[0]);
        assert_eq!(focused[3], rest[3], "the text after them is untouched");
    }

    #[test]
    fn focus_beats_hover_on_the_same_field() {
        let tree = tree_with(&[(
            3,
            serde_json::json!({
                "hover": {"color": "#0000ff"},
                "focusVisible": {"color": "#ff0000"}
            }),
        )]);
        let both = runs_with(&tree, &|_, _| PointerStates {
            hovered: true,
            focus_visible: true,
            ..Default::default()
        });
        let focus_only = runs_with(&tree, &|_, _| PointerStates {
            focus_visible: true,
            ..Default::default()
        });
        assert_eq!(both[1].color, focus_only[1].color);
    }

    /// "a " + span(3, "bold" + span(5, "red")) + atomic(8) + " end"
    fn tree_with_atomic(styles: &[(u64, serde_json::Value)]) -> RetainedTree {
        let mut tree = tree_with(styles);
        tree.create_element(8, "div".to_string());
        tree.append_child(3, 8);
        tree.set_style(
            8,
            Arc::new(serde_json::from_value(serde_json::json!({"display": "inline-block"})).expect("a style")),
        );
        tree
    }

    fn styles_of(tree: &RetainedTree) -> Styles {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        styles(
            tree,
            paragraph,
            &ColorContext::root(Scheme::Light),
            &container_lengths(),
            &FontFeatureState::default(),
            Default::default(),
            &|_, _| PointerStates::default(),
            &InlineStyles::default(),
        )
    }

    #[test]
    fn an_atomic_placeholder_is_a_run_of_its_own_with_none_of_the_boxes_lines() {
        let tree = tree_with_atomic(&[(3, serde_json::json!({"textDecoration": "underline", "color": "#ff0000"}))]);
        let runs = styles_of(&tree).runs;
        let atomic: Vec<_> = runs.iter().filter(|run| run.atomic).collect();
        assert_eq!(atomic.len(), 1);
        assert_eq!(atomic[0].len, 3, "one U+FFFC");
        assert_eq!(atomic[0].underline, None);
        assert_eq!(atomic[0].color, None);
        assert!(runs.iter().any(|run| run.underline.is_some()), "the span's text is underlined");

        let base = gpui::TextStyle {
            underline: Some(gpui::UnderlineStyle::default()),
            background_color: Some(gpui::red()),
            ..Default::default()
        };
        let run = InlineText::run(atomic[0], &base, px(16.0), &exact_heights());
        assert_eq!((run.underline, run.background_color), (None, None), "not even the container's");
        assert!(run.extra_decorations.is_empty());
    }

    #[test]
    fn a_paragraph_with_an_atomic_always_has_runs() {
        assert!(!styles_of(&tree_with_atomic(&[])).runs.is_empty());
        assert!(!styles_of(&tree_with(&[])).runs.is_empty(), "two text nodes are two runs");
    }

    #[test]
    fn a_paragraph_of_one_text_node_that_nothing_restyles_is_plain() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "text".to_string());
        tree.append_child(1, 2);
        tree.set_text(2, "alone".to_string());
        assert!(runs_of(&tree).is_empty());
    }

    #[test]
    fn a_box_that_only_moves_on_its_line_keeps_the_runs() {
        let tree = tree_with(&[(3, serde_json::json!({"verticalAlign": "super"}))]);
        let styles = styles_of(&tree);
        assert!(!styles.runs.is_empty());
        assert_eq!(styles.boxes[0].align, ResolvedVerticalAlign::Super);
    }

    #[test]
    fn vertical_align_percentages_are_of_the_boxs_own_line_height_and_lengths_are_its_own() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": 20, "verticalAlign": "2em"})), (5, serde_json::json!({"verticalAlign": "50%"}))]);
        let styles = styles_of(&tree);
        assert_eq!(
            styles.boxes[0].align,
            ResolvedVerticalAlign::Offset { px: 40.0, line_height_fraction: 0.0 }
        );
        assert_eq!(
            styles.boxes[1].align,
            ResolvedVerticalAlign::Offset { px: 0.0, line_height_fraction: 0.5 }
        );
        assert_eq!(styles.boxes[1].parent, Some(3));
        assert_eq!(styles.boxes[1].style.font_size, Some(20.0), "the text of a nested box has its parent's size");
    }

    #[test]
    fn the_boxes_the_line_layout_hears_of_are_the_shifted_ones_and_those_around_them() {
        let paragraph_of = |tree: &RetainedTree| match plan_children(tree, &tree.elements[&1]).remove(0) {
            Child::Paragraph(paragraph) => paragraph,
            Child::Block(_) | Child::AnonymousCell(_) | Child::AnonymousTable(_) | Child::EmptyLine => panic!("a paragraph"),
        };
        let ids = |tree: &RetainedTree, shifted: &[u64]| {
            let paragraph = paragraph_of(tree);
            let styles = styles_of(tree);
            listed_boxes(&paragraph, &styles.boxes, &shifted.iter().copied().collect())
                .iter()
                .map(|style| style.id)
                .collect::<Vec<_>>()
        };
        let plain = tree_with_atomic(&[]);
        assert_eq!(ids(&plain, &[]), Vec::<u64>::new(), "nothing is shifted");
        assert_eq!(ids(&plain, &[8]), vec![3], "the span around a shifted atomic");
        let nested = tree_with(&[(5, serde_json::json!({"verticalAlign": "sub"}))]);
        assert_eq!(ids(&nested, &[]), vec![3, 5], "and the one around a shifted span");
        let own = tree_with(&[(3, serde_json::json!({"verticalAlign": "top"}))]);
        assert_eq!(ids(&own, &[]), vec![3], "a shifted span with nothing shifted inside it");
    }

    #[test]
    fn a_span_around_an_atomic_is_listed_for_its_strut_when_its_font_or_line_height_is_its_own() {
        let paragraph_of = |tree: &RetainedTree| match plan_children(tree, &tree.elements[&1]).remove(0) {
            Child::Paragraph(paragraph) => paragraph,
            Child::Block(_) | Child::AnonymousCell(_) | Child::AnonymousTable(_) | Child::EmptyLine => panic!("a paragraph"),
        };
        let ids = |style: serde_json::Value| {
            let tree = tree_with_atomic(&[(3, style)]);
            let paragraph = paragraph_of(&tree);
            listed_boxes(&paragraph, &styles_of(&tree).boxes, &HashSet::new())
                .iter()
                .map(|style| style.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(serde_json::json!({"lineHeight": "40px"})), vec![3]);
        assert_eq!(ids(serde_json::json!({"fontSize": 30})), vec![3]);
        assert_eq!(ids(serde_json::json!({"fontWeight": 700})), vec![3]);
        assert_eq!(ids(serde_json::json!({"color": "#ff0000"})), Vec::<u64>::new(), "its strut is the container's");
    }

    #[test]
    fn a_box_says_how_its_text_wraps_and_the_boxes_in_it_inherit_it() {
        let tree = tree_with(&[
            (3, serde_json::json!({"textWrapMode": "nowrap"})),
            (5, serde_json::json!({"whiteSpace": "pre-wrap"})),
        ]);
        let styles = styles_of(&tree);
        let wraps = |id: u64| styles.boxes.iter().find(|style| style.id == id).map(|style| style.wraps);
        assert_eq!(wraps(3), Some(Some(false)));
        assert_eq!(wraps(5), Some(Some(true)), "the wrap component of white-space, in a box that does not wrap");
        let inherited = tree_with(&[(3, serde_json::json!({"textWrap": "nowrap"}))]);
        let boxes = styles_of(&inherited).boxes;
        assert_eq!(boxes.iter().map(|style| style.wraps).collect::<Vec<_>>(), vec![Some(false), Some(false)]);
        assert_eq!(
            boxes.iter().map(|style| style.declares_wrap).collect::<Vec<_>>(),
            vec![true, false]
        );
        let plain = styles_of(&tree_with(&[]));
        assert!(plain.boxes.iter().all(|style| style.wraps.is_none() && !style.declares_wrap));
    }

    #[test]
    fn a_box_that_says_how_it_wraps_is_listed_and_where_one_wraps_every_box_is() {
        let ids = |styles: &[(u64, serde_json::Value)]| {
            let tree = tree_with(styles);
            let paragraph = match plan_children(&tree, &tree.elements[&1]).remove(0) {
                Child::Paragraph(paragraph) => paragraph,
                Child::Block(_) | Child::AnonymousCell(_) | Child::AnonymousTable(_) | Child::EmptyLine => panic!("a paragraph"),
            };
            let styles = styles_of(&tree);
            listed_boxes(&paragraph, &styles.boxes, &HashSet::new())
                .iter()
                .map(|style| style.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&[]), Vec::<u64>::new());
        assert_eq!(ids(&[(5, serde_json::json!({"textWrapMode": "nowrap"}))]), vec![3, 5]);
        // The wrap style is the container's alone.
        assert_eq!(ids(&[(5, serde_json::json!({"textWrapStyle": "balance"}))]), Vec::<u64>::new());
        assert_eq!(ids(&[(3, serde_json::json!({"textWrapMode": "wrap"}))]), vec![3, 5]);
    }

    #[test]
    fn runs_cover_the_text_exactly() {
        let tree = tree_with(&[(5, serde_json::json!({"fontWeight": 600}))]);
        assert_eq!(runs_of(&tree).iter().map(|run| run.len).sum::<usize>(), 13);
    }
}
