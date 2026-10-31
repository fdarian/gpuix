# Text

How text is laid out and drawn: inline runs and spans, atomic inlines, white space, line boxes, wrapping and line breaking, text-box trim, decorations, fonts, glyph positions, and the keys that let a text keep its measure between frames. The goal throughout is Chrome's numbers: boxes, baselines and line breaks that match headless Chrome on macOS, checked against oracles.

## Inline text and spans

`<span>` and a nested `<text>` are inline runs in one paragraph (`src/text/inline.rs`). Each has its own colour, background, weight, family, `fontStyle`, decoration, `fontVariantNumeric` and `fontFeatureSettings`. A span's `fontSize` and `lineHeight` size its own run, and its `em` is its own font size. A row is as tall as its CSS line box (see [Line height and line boxes](#line-height-and-line-boxes)). Paint, decorations, hit testing, selection and highlight rows, and a span's content area use each row's own baseline and height. Sibling text nodes in a block `div` flow on one line, and `getPaintedText()` returns the whole paragraph.

Spans with listeners, `hover`/`active`/`groupHover`/`groupActive` styles, `cursor`, `tabIndex`, and `focusVisible`/`focusWithin` stay inline and are targets of their own events: see [events-and-input.md](events-and-input.md#inline-span-listeners-cursor-and-focus). `motion.span` animates what an inline box shows: see [motion.md](motion.md#motionspan). The `opacity` of an inline box is a compositing group: see [paint-and-transforms.md](paint-and-transforms.md#effect-groups).

### Inline boxes

Margin, border and padding along the line are a `TextInset` side channel in gpui (`StyledText::with_insets`): later glyphs shift as after a tab, and wrapping and hit mapping count the inset. `backgroundColor`, borders, radii and vertical padding paint per line fragment with `box-decoration-break: slice` semantics (`inline_decoration.rs`, `inline_insets.rs`). Padding, border and margin, along the line and vertical, take any CSS length, resolved through the span's own `LengthContext`.

### Empty inline boxes

A container whose children are only inline boxes with nothing in them (`<span></span>`, nested or with whitespace around) plans a `Child::EmptyLine` (`text/inline.rs`): a block of no size, as Blink lays out an empty line box for an inline box with no content (`inline_layout_algorithm.cc`, `SetIsEmptyLineBox`). It has no height and no baseline, but it is a child. A table cell that holds only empty spans therefore has a child and no baseline, and takes the bottom of its content box for its baseline as in Blink ([layout.md](layout.md#baselines)); with no child it took no part in aligning its row. The line is planned when an in-flow, non-`contents`, non-atomic inline box was entered and nothing else is left; the renderer builds a childless `div` for it, and text, search, accessibility, selection and motion do not see it. An empty span directly under a `tr` or `tbody` is wrapped in an anonymous cell, as any inline child is.

An empty span whose style declares padding, border or margin along the line is not an empty line (Blink's `line_breaker.cc`: an open tag that is not an empty item makes a line box; vertical padding alone is empty). `empty_box::takes_space` decides it from the span's own style at plan time, the same lengths `inline_insets::space_of` reads (zero is zero in every unit; a percentage is not zero), and `same_paragraphs` compares it, so a style change re-plans. The builder then puts one U+FFFC in the paragraph, in the span (`Piece::placeholder`, the span last in `boxes`), where the span would have its text: it is `RunStyle::placeholder` (the span's font and line height, none of its decoration lines), the span's `BoxStyle::empty` lists it to the line layout always, and the paragraph is not `plain`. The white space around it collapses as if it was not there (the tail is not touched), and `Builder::pop` does not trim a space across it. Copy, find and accessibility skip the character like an atomic inline's (`selection.rs` cuts at it, so a copy puts no line break there since `Paragraph::has_placeholder` gives the paragraph a `line`).

The line layout knows it as `InlineContent::Empty { font, font_size }` (zed `text_system/inline.rs`): the box takes the place of the U+FFFC like an atomic inline, with no advance, ascent or descent of its own, and no element. It is the box's own strut (`struts_around` starts at the box itself, so the row has the font and line height of the span, `vertical-align` included, and the container's strut as every line with an atomic has), and `InlineRun::empty` carries the ascent and descent of the font for `content_extent`, which is what the box's border box is as tall as. Its glyph carries the span's `TextInset`s, so the margin, border and padding come out of the existing inset model: the border box, `getBoundingClientRect`, the painted background and border, hit testing and `cursor` hitboxes all read `inline_bounds::border_box_rows` over the placeholder's bytes, and a fragment of no width (a percentage padding in a shrink-to-fit box) is still a fragment (`range_rows`). `TextLayout::objects` leaves the placeholder out because there is no element to place, and so `position_for_index` is the position before its leading inset, like any glyph's.

Breaking: the placeholder is a place no line breaks at. `LineBreaks::new_skipping` runs the rules over the text without the placeholders, and a break opportunity before a placeholder moves to after it (Blink moves it past the open and close tags), so a box after a space stays on the line the space ends, and `AAAA<box>AAAA` has no break. The space before a box that ends a line is trailing white space, which Blink removes whatever non-text item follows it: `GlyphPositions::collapse_spaces_before_boxes` takes its width off, and the box starts where the space did. A negative total margin pulls the next text, and `inline_bounds::inside` clamps a width only after both edges are off.

### Selection and find follow document order

Hit testing follows paint order, but selection and find follow document order. A press, a drag and a double or triple click map the pointer into each text's space, so selection works through a transform ([paint-and-transforms.md](paint-and-transforms.md#geometry)). zed lifts a positioned element, so its text registers after the siblings that follow it, and the registry is in paint order. Each run therefore carries a `DocPos` (child indices through the retained tree, then `sub`; flex `order` and `display: contents` do not move it, as in the DOM), and `resolve_drag` sorts by it, so the range, the wash and the copied text read in the tree's order. Find numbers matches in the same order: runs announce their matches in prepaint and the first wash of a frame numbers them all. The press and the drag head still land on the topmost text under the pointer.

The text of an atomic inline sorts at the byte of the box's placeholder in the paragraph around it. The paragraph is cut there for the drag, and copy puts no line break between the text of the line and the box's inline content.

## Atomic inlines

`inline-block`, `inline-flex`, `inline-grid` and `inline-table` sit on a line of text. They are a placeholder in gpui text: one U+FFFC run in the paragraph whose advance is the box's margin-box width and whose ascent and descent come from the box's baseline (zed `text_system/inline.rs`; `vertical-align` acts on the line box as in CSS 2.1 10.8). A flex or grid item, or an absolute box, blockifies them as CSS does. `display: "table"` and the `table-*` values lay out as tables.

The box is a real element owned by `StyledText` (`elements/atomic_inline.rs`). It is measured before shaping (min and max-content, shrink-to-fit against the line), laid out again at the used width, then prepainted and painted at the placeholder's position, so events, hit testing, focus, bounds and observers work on it as on any block.

The text keeps a private layout engine per paragraph and each box is a layout root in it. This is needed because the text measures the boxes while the window's engine is taken: the window is in the middle of laying itself out. Two consequences: a box that fits costs two subtree layouts per frame, and an absolute descendant resolves against the atomic inline. A box with a `min-width` or `max-width` in `%` is laid out with the width of the paragraph as the parent size its percentages are of (taffy `compute_root_layout_with_parent_size`), measured again per paragraph width.

gpuix side: `layout/display.rs` (the inline-level values, blockified where CSS blockifies them), `text/atomic.rs`, the planner in `text/inline.rs`, `css/vertical_align.rs`.

An atomic inline is placed from the unsnapped origin of its paragraph in both axes and keeps the fraction in every box it reports (`getBoundingClientRect`, `onLayout`, baselines). It is painted at the device pixel its place rounds half up to, as Chrome does (`floor(position + 0.5)`, #80), and so is what is inside it. Its keys in the measure cache are in [Text measure keys](#text-measure-keys-and-the-shape-store); it is excluded from retention, see [retention.md](retention.md#eligibility-and-exclusions).

## White space and truncation

`whiteSpace` is inherited and processed as CSS Text 3 §4: `normal`, `nowrap`, `pre`, `pre-wrap`, `pre-line` and `break-spaces`, collapsing across spans and stripping at paragraph and line edges (`src/text/white_space.rs`). The paragraph builder applies it, so paint, search and copy share the text; adjacent text leaves of an `anchored` or `virtual-list` parent go through the same builder.

- A tab stays a tab: gpui lays it out on tab stops (`TextStyle::tab_size`, from `tabSize`).
- Spaces at a soft wrap hang (`hang_trailing_spaces`), and `break-spaces` breaks after every space (`break_spaces`).
- `wordBreak` and `overflowWrap` map to `WordBreak` and `OverflowWrap`.

All four are `TextStyle` fields carried by `TextLayoutOptions` into the layout cache key and `LineLayout::compute_wrap_boundaries`. gpui's defaults leave Zed's text as it was (no tab stops, spaces stay on the line, a word that does not fit is broken), and gpuix sets the CSS ones on the root view.

`textOverflow` with `lineClamp` or a width cuts where the text lays out (`WindowTextSystem::truncate_end`, `text_system/line_truncation.rs`). It keeps the layout's own rows, so tabs, hanging spaces, `break-spaces`, `wordBreak` and `overflowWrap` apply.

## Wrapping and line breaking

### `text-wrap`

`textWrap` (shorthand), `textWrapMode` and `textWrapStyle` are inherited. The mode is the longhand of `whiteSpace`'s wrap component and wins over it on the same element (`nowrap` beats `pre-wrap`; `whiteSpace: "pre"` with `textWrapMode: "wrap"` wraps and hangs like `pre-wrap`). `balance` and `pretty` choose the breaks (zed `text_system/text_wrap.rs`) among the same break opportunities with greedy wrapping's own line measure, so the line count and the width the text reports to layout are greedy's, and `textAlign` aligns within it. `stable` is `auto`.

The style is a block container's alone, as in Chrome; on a span it does nothing. The mode is also per inline box: a `nowrap` span is not broken, a `wrap` span in a `nowrap` block is. A span's `whiteSpace` counts too, and `textWrapMode` on the same span wins over it.

### Where a line may break

The places a line may break are Blink's `LazyLineBreakIterator` (zed `text_system/line_break.rs`):

- A fast pair table for two characters in `!`..`ÿ`: `FillAscii` for ASCII, Chrome's measured table for the rest.
- The Unicode line breaking algorithm over the whole text for every other pair, from ICU4X with the Unicode 17 data (`unstable` feature), `Normal` strictness and its dictionaries for Thai, Lao, Khmer and Burmese. The release addon grows by 1.9 MB for it.
- `keep-all` is the same places minus two letters or numbers in a row.
- `break-all` is the same plus Blink's `kBreakAllLineBreakClassTable` (`breaks_after_break_all`): a break between the UAX #14 classes the table lists, asked before the fast table's refusal, never before a space or a `BA` character but `|`. gpui's own `may_break_between` broke after any letter that was not punctuation.
- `/` is not a break and `?` is, as in Chrome.
- A soft hyphen is a place to break, and the row that breaks there ends with a hyphen ([Soft hyphen](#soft-hyphen)).
- A `-` that starts a line does not break from a digit after it (`-20`), except under `break-all`: `LazyLineBreakIterator` looks for a letter or digit before the hyphen on the line.

The min-content width follows these places. Where a line breaks at the edge of a span follows Chromium's rules for spaces and for the places between letters. They were read off its behaviour and not its source, and are checked for a word, a space and a word each in a box of its own, for `break-all` and for CJK. `line-break-oracle` and `hyphen-break-oracle` check all of it against Chrome, each case in a box of its own.

### The fit test

Where a line breaks was measured against Chrome on 596 texts (Helvetica Neue, Menlo, Times, Helvetica, Inter Variable with Fluid Functionalism's text styles; the UAX #14, punctuation, `break-all`, `keep-all`, `break-word` and soft hyphen cases) at about 65,500 widths per scale (every width from the min-content width to the natural one, the widths at which one line is within three 1/64 of fitting, and fixed widths), at 1x and 2x. The places were already Blink's for `normal` and `keep-all`; what differed was the fit test.

gpuix used to fit a line when its float width was at most the box plus 1/1024. Blink fits it when the sum over the items of the line of `ceil` to 1/64 of a device pixel of each piece's float width from where it starts on the line (`ShapeResultView::SnappedWidth`) is at most the box cut to 1/64 of a device pixel plus one 1/64 (`LineBreaker::AvailableWidthToFit`, `LayoutUnit::AddEpsilon`). Lines within a hair of fitting differed: 2167 of about 10,400 edge widths, and 33 (1x) and 18 (2x) of 28,153 real ones.

zed: `SnapMarks::fit_units` (the pieces and their kern shares as the width has them, except that the first glyph of a line has no share, because Blink shapes the start of a line again with nothing before it) against `util::fit_layout_units`. It is used in `compute_wrap_boundaries` for greedy wrapping, the min-content pass (a width of nothing) and the overflow-wrap cut, and in `text_wrap.rs` for `balance` and `pretty`, which keep their algorithms. The width a line is reported to have (`line_widths`, min-content, `width`) is `SnapMarks::width` ([Text positions](#text-positions)), so the text measure is unchanged.

A soft hyphen counts the width of the hyphen Blink draws there in the fit ([Soft hyphen](#soft-hyphen)).

Layouts that differ from Chrome, out of those of a font Chrome and gpuix measure alike, before and after (1x; 2x is the same to 1 %):

| Set | Differing before | Differing after |
|---|---|---|
| all | 3589 of 52,294 (6.9 %) | 262 (0.5 %) |
| `normal` text, other fonts | 578 of 23,803 | 8 |
| Inter | 1623 of 16,816 | 9 |
| soft hyphens | 73 of 552 | 0 |
| `break-all` | 956 of 4226 | 105 |
| `break-word` and `anywhere` | 125 of 2128 | 65 |

At 2x, all went from 3478 of 52,340 to 269. The two sections below (integer sums, the end of a line shaped again) took the 262 (1x) and 269 (2x) of the non-fallback fonts down to 37 and 35, and Inter's 9 to 0 at 1x. What is left is in [deviations.md](deviations.md#text).

### Blink adds integers

Blink does not add exact advances. Skia hands HarfBuzz the advance of a glyph as an f32 of device pixels, Blink cuts that toward zero to 1/65536 (`SkiaScalarToHarfBuzzPosition`), HarfBuzz adds a kern as a whole number of the same, and `InlineLayoutUnit` adds the integers. A line of 40 to 100 glyphs is 1e-4 to 6e-4 px under the exact sum that CoreText's positions give, and the fit test sits on 1/64 boundaries, so that decided the nine Inter 550/18 edge layouts per scale that broke a word early.

`SnapMarks::measure` carries that sum before every glyph. `width` and `fit_units` take a piece as the f32 of the difference of two sums, ceiled to a unit (`ShapeResult::SnappedWidth`), moved by what tab stops and insets did to the glyph since it was shaped. The advance of a glyph is its own, from `PlatformTextSystem::glyph_advance` (an f64 at the size and scale; `CTFontGetAdvancesForGlyphs` in `gpui_macos`, where `advance` gave an f32 of font units rounded twice), taken as an f32 and cut. The kern is what is left of the distance to the next glyph: a whole number of font units when it is within the noise of the positions of one, nothing below it, and the legacy `kern` shares take the same kern. The noise floor is 2e-4 px: the positions are f32, and CoreText shapes the first font run at the next f32 size up (`next_up`) to keep ligatures from forming, which moves every position by about that much. Paint positions and the widths the layout reports are unchanged.

### The end of a line is shaped again

`ShapingLineBreaker::ShapeLine` shapes the last glyphs of a line again with nothing after them where the boundary is not safe to break at, which is a kerned pair, so the line loses that kern. `well-kno|wn` is 56.67 in `well-known` and 56.94 shaped alone, in a box of 56.89: the line is as wide as its end shaped alone, and it must have room for that. A cut word (`overflow-wrap`), `break-all` and a place to break before a hyphen all end a line between letters.

`SnapMarks::end_reshapes` is the data, built only for a line that is wider than its box: for each kerned boundary the loss, from the advance of the glyph in the text against its advance shaped on its own (the full kern of a GPOS font, `kern >> 1` for a font with a `kern` table). `fits` is the test, used by the greedy walk (`break_candidate` goes back over the break opportunities, and a word with none over its glyphs, for a cut word and `break-all` alike) and by `balance` and `pretty`, which keep their algorithms and use the same rule. Blink's room check (`line_breaker.cc` 569) measures from the ceiled position of the last safe boundary, which is the start of the run of kerned pairs the line ends in, not of the last pair: a line that has no room for its end shaped again goes back to the opportunity before. The width a line is reported to have has the loss taken off. A min-content width is measured from the positions of the glyphs and does not reshape (`TextLayoutOptions::min_content`). Pairs of kana and ideographs are reshaped like Latin ones: Hiragino kerns `テス` through GPOS as CoreText does, and Chrome breaks between them with `テ` shaped alone, so the line is as wide as that.

### Soft hyphen

A row that breaks at U+00AD ends with a hyphen, as Blink adds a generated fragment after the text of such a row, shaped with the style at the break: U+2010 if the font has it, else `-`, in the colour and font of the run before the break. It counts for the line (`LineBreaker::BreakText`: a hyphen that does not fit sends the line back to the last place that fits a hyphen narrower), in the width of the row, in its alignment and in the rects of the element that holds the soft hyphen. It is not in the text.

zed: `SnapMarks` keeps the glyph of the hyphen with its advance (`Hyphen`); `WrappedLineLayout::hyphen_of_row` knows the rows that end with one and adds its snapped width to `line_widths`. `paint_line` paints the glyph at the end of the row and extends the decorations and the background of the row under it, and `aligned_origin_x` takes the width, so `row_offset` and the selection that follows it place the row with the hyphen. `index_for_position` and `position_for_index` are unchanged: the hyphen has no index, so the copied text, search, the caret and hit testing do not see it, and the selection wash stops where the text does. `TextLayout::hyphen_width_at_break` tells an embedder how wide the hyphen is at the end of a row.

In the rects it is a Range rect of its own after the row's, and inside the rects of the span that holds the soft hyphen, as in Chrome (`getBoundingClientRect` of an element around it is that much wider on that row; `range_rows` does this for the bounds, hit testing, background and outline of a span). A soft hyphen in the middle of a row paints nothing: CoreText shapes U+00AD to no glyph.

### A row wider than its box

A row that does not fit its box starts at the start edge of the box's direction whatever its `text-align` (CSS Text 3 §7.1), so a centred or right-aligned row sticks out at the end and not on the left; in a `direction: rtl` box its right edge is at the box's and it sticks out on the left. A row that only overflows by the spaces that hang at its end is aligned by its text, as Chrome does. zed: `aligned_origin_x`. gpui text has no direction of its own, so `TextStyle.direction` is inherited like the CSS property (`Inherited::apply` writes it where it differs from the parent's) and the alignment the wrapped-line paint, hit and atomic-origin functions take is a `TextAlignment`, a `TextAlign` and the `Direction` of the box. That is all `direction` does to a line: there is no bidi, `ShapedLine` and `LineLayout` paint left to right, so the glyphs of an rtl row stay in logical order from the left.

## Text box trim

`textBoxTrim`, `textBoxEdge` and `textBox` cut the space above the first line and below the last of a block container down to the cap height, x-height or baseline (zed `text_system/text_box.rs`). The trim is not inherited: the renderer hands it down block children to the first and last in-flow one and into the paragraph.

### Cap height and x-height (#85)

The cap height and x-height of a font are the OS/2 table's (version 2 or later, nonzero, below two ems) of its default instance. That is how Skia's `SkScalerContext_Mac::generateFontMetrics` takes them and how Chrome measures `ex` and the `ex` and `cap` edges of `text-box-edge`. `CTFontGetCapHeight` and `CTFontGetXHeight` are not used: Inter Variable's x-height is 1118 of 2048 units in OS/2 and 1056 in CoreText (1097 at `wght` 700), so an `ex`-trimmed label was 0.36 px short at 12px. zed: `os2_heights` in `gpui_macos/src/text_system.rs`, read through font-kit's `load_font_table`. The cap height of Inter is 1490 either way: the 0.704 em of FF's `tabs-subtle` check is the system font, because `createCheck` renders a bare view without `fontFamily: 'Inter Variable'`. gpuix has no `ex` unit.

### Baseline of a trimmed label (#44, #66)

A cap-trimmed label centred in a pill used to sit a pixel off Chrome. Taffy snapped the top of the box to a device pixel, and the glyph baseline was placed from that snapped top and rounded again, so a label whose top fell at 9.27 and whose baseline was 18.73 below it painted at 18 where Chrome rounds once, to 19. A text now keeps how far the unsnapped top of its box lies below the snapped one, and its lines, their positions and the boxes placed in them start from the unsnapped top. A relocation by whole pixels leaves the phase as it is. The baseline of a run and the origins of atomic inlines start where the lines do, above the bounds by what `text-box-trim` cut.

The same label inside an `inline-block`, `inline-flex` or `inline-grid` sat 5.33 px too high in a 16px/24px Menlo block (the cap top at 0; Chrome has it at 5.328125, on the strut's baseline). `first_baseline` and `last_baseline` of a text counted from the top of the untrimmed lines, and an atomic's ascent (and a flex or grid item's baseline) is read from them, so it was `trim_top` too big. They count from the trimmed box now, and `text/atomic.rs` reads the unsnapped tops that glyphs are placed from. An atomic inline keeps the 5.328125 Chrome keeps, and the box of a `<span>` in a flex or grid item at a fractional top is measured once from the unsnapped origin ([Text in fractions of a pixel](#text-in-fractions-of-a-pixel-41)). The half-leading of a line is shared out in device pixels (baseline 17.5 at scale 2, as Chrome has it), and the trimmed baseline is rounded to the layout unit its box is in, so an atomic around a cap-trimmed label sits on Chrome's baseline. Baselines of flex and grid items are in [layout.md](layout.md#baselines).

## Line height and line boxes

`lineHeight` is CSS `line-height`. A number is a multiplier of the element's font size, a string is a length, and `"normal"` (also the default of an element that sets none) is the height of the font and of the fonts the line falls back to. `TextStyle.line_height` is a `gpui::LineHeight`, `Normal` or a `DefiniteLength`, no longer a `DefiniteLength` that defaults to `phi`.

Line boxes are Chrome's on macOS. `FontMetrics::line_box(font_size, scale_factor)` rounds a face's ascent, descent and line gap in device pixels at the used size and widens the ascent of the families Times, Helvetica and Courier (the `widens_ascent` field, set by `gpui_macos` from the face's family name; the 15 % is of the device-pixel content area). `normal` is their sum, and the half-leading above the text is floored to a device pixel (`extent_in_line`). A line height is laid out in sixty-fourths of a device pixel instead of snapped to the device pixel: a number is cut, a length or percentage rounded. Blink zooms the font to the display before it measures it, so Menlo 20px is 24px of content at scale 1 and 23px at scale 2, and a 24px line around Menlo 16px has its baseline at 17px and 17.5px.

The scale is the window's. `WindowTextSystem` holds it (set with the window's own scale, so a window that changes display shapes again), `PlatformTextSystem::layout_line` takes it, and a `LineLayout` records the scale it was shaped for, which its rows and every reader without a window use. It is part of the line layout cache key, the shape store's and the text element's measure key.

The strut is the font of the element (`TextLayoutOptions::strut_font`), not the largest font among the runs. A run whose rounded ascent or descent differs from the strut's, a weight change included, is a box of its own. The model was fitted to `packages/react/src/__tests__/fixtures/line-box-oracle.json` (1080 boxes), which `line-box.test.tsx` asserts.

Runs have their own size: `TextRun` has `font_size` and `line_height` (`Option`s; `None` is the line's) for per-run `fontSize` in inline text. Sizes go through CoreText only: the macOS shaper builds a font per run, and the other shapers still shape every run at the line size. `FontRun` and `ShapedRun` carry the size, `TextLayout` gains `content_extent` and `row_for_index`, wrapped rows take the height of their CSS line box, and paint, hit testing and start and middle truncation follow each row and run.

Under `normal` a line is as tall as the line boxes of every font used on it, as in Chrome: a row of Han, kana, Hangul or emoji in Helvetica Neue 13px was 3, 5, 1 and 6 px short when only the font asked for counted. Blink unites, per line, the content area of the box's font with that of every font that has a glyph on the line, each with its line gap halved and floored to a device pixel (`InlineBoxState::AccumulateUsedFonts`, `CalculateLeadingSpace`). `TextRun` and `FontRun` carry `normal_line_height` (`TextStyle::to_run` sets it from `LineHeight::Normal`, its own or inherited; it is in the measure hash and the `FontRun` merge). The macOS shaper gives a run that has glyphs in another font a `ShapedRun::used_font` (the extent of that font's line box), and `LineLayout::line_rows` unites it with the run's own extent in each row the font has a glyph in. The ascent and descent of the run stay those of the font asked for, so decorations and the box of a span keep the primary font's. Apple Color Emoji reports its ascent and descent per strike, so its box comes from the `CTFont` at the device size and not from the font units (they gave 39 px at 24px where Chrome has 32). A number or a length is unaffected, and a span with another font size in a `normal` line was already a box of its own.

## Text decoration

A `TextRun` carries one underline, strikethrough and overline plus extra lines of each kind (`extra_decorations`, `TextStyle::refine_nested`), so nested boxes accumulate as CSS draws them.

- **Position.** An underline is drawn at the baseline of its decorating box, a line-through by two thirds of the ascent of the decorating box's font from the top of the text it crosses, and an overline up from the top of that text, as Chrome does (oracle `text-decoration-lines-oracle`). The decorating box is only known to the line layout when it is listed (`paragraph::listed_boxes`: one aligned off the baseline, or a strike-through in another font than a box's inside it). A line is drawn at the baseline of the box that declared it, wherever the boxes inside it are raised to.
- **Underline offset.** `textUnderlineOffset` (inherited) sets the distance from the baseline, and the default is Chrome's: the line's top edge is `max(1, ceil(thickness / 2))` under the baseline.
- **Thickness.** `auto` thickness, and `from-font` when the font has none, is a tenth of the font size, not 1px. A thickness that is a length rounds half up to a device pixel, at least one. `text-decoration-thickness` takes `em`, `rem`, the viewport units, `calc()` and a `%` of 1em, resolved at the element that declares it. `from-font` reads the run's font.
- **Dotted and dashed.** A line is fitted to the line it is on as Blink's `StrokeData` does (`DecorationGeometry::fitted`: the gaps stretch so the last mark ends on the line's end, and a thin dotted line is square marks), and starts over with every text fragment (`UnderlineStyle::fragment`, set per text node by `text/paragraph.rs`). A decoration the paragraph's own block declares restarts per text node only in a paragraph where some box restyles something: one in which no box does is plain text, one fragment.
- **Animation.** `textUnderlineOffset` and `textDecorationThickness` animate, from the offset an element inherits when it leaves it unset (an unset thickness is `auto`, which is not inherited), and through a negative thickness, which is valid and paints one device pixel as zero does. All of it is checked against Chrome.

## Fonts

### Variations

`fontVariationSettings` is a comma list of `"tag" number`, inherited, on elements, spans (per run) and inputs, like `fontFeatureSettings`. On a variable font `fontWeight` sets `wght` as Chrome's font matching does, so 650 is its own weight and 600 is 600; a `wght` in the settings wins. Runs of one family with different variations keep their own face (zed `Font` and `TextStyle` carry the axes). macOS only.

### Features

Font features need a font that has them, as in a browser. The macOS system font has `smcp`, `frac`, `tnum` and `pnum` but no `zero` or `onum`, so those paint like the default there; Avenir Next and Baskerville have `smcp` and `onum`.

### Advances of a fallback font

Chrome and CoreText pick the same fallback fonts (PingFang SC, Hiragino, Apple SD Gothic Neo, Apple Color Emoji) but advance a font with a `trak` table differently. Without an `sbix` table (PingFang, the UI font) HarfBuzz tracks the advances at the CSS size (`harfbuzz_face.cc`, ptem); with one (Apple Color Emoji) Skia measures `SkCTFontCreateExactCopy` at the device size with the tracking off. CoreText does either only for a copy that has the optical size attribute (and `NSCTFontUnscaledTrackingAttribute` 0), never for the font a `CTLine` cascades to. So a 13px Han glyph is 13.0 in CoreText and 13.26 in Chrome, a 13px emoji 17 and 16, and at scale 2 Menlo 16px has an emoji of 21 and Chrome 16.

zed `gpui_macos` `layout_line` moves the glyphs of every `CTRun` whose font has a `trak` table (`AdvanceSource`, read once per font from its table tags) by the difference between the advance in such a copy (`advance_font`, cached by face, size and scale) and the advance CoreText used: a kern CoreText applied stays, the later glyphs shift and the width of the line gets the shift. This holds for the cascade and for a font asked for by name alike (PingFang SC and the UI font differ the same). `PlatformTextSystem::glyph_advance` takes the scale factor and gives the advance the line was shaped with, so the integer sums agree. An emoji is rasterised from the same device-size copy with no scale (and `raster_bounds` for that size): Apple Color Emoji draws 16 wide at 13px and 24 at 26px, so the glyph drawn at 13px and scaled by 2 was a third wider than its advance and overlapped its neighbour at scale 2.

### Fallback fonts follow Blink's script runs

CoreText cascades the characters a font lacks in stretches, so in `日本語のテキストです。` the runs were PingFang (`日本語`), Hiragino (`のテキストです`) and PingFang (`。`): the full stop after kana was PingFang's glyph with no `す。` kern pair. Chrome cuts the text in script runs (`ScriptRunIterator`: Common and Inherited join the run around them, CJK punctuation stays in a Han, Hiragana or Bopomofo run and starts the next otherwise, a closing bracket takes the script of its opening one), asks the system for one font for the characters of a run that the font lacks (`FontFallbackIterator`: the hint is the first of them with a script of its own), and gives that font every character it covers; the rest is asked for again with its next hint.

`gpui_macos/src/script_runs.rs` is the port of the iterator (`icu_properties` Script and Script_Extensions, the paired brackets, the East Asian width) and `font_fallback.rs` assigns the fonts, independent of CoreText (marks, joiners and variation selectors follow the font of the character before them; control and invisible characters are left to the shaper). `layout_line` sets the font that `CTFontCreateForString` gives as the explicit font attribute of those characters before the `CTLine`, cached by face, size and hint. CJK punctuation is therefore set in the font of its script run, with that font's kerning.

### CJK punctuation: `text-spacing-trim`

Chrome's initial `text-spacing-trim` is `normal`, and gpuix always does it (there is no property): the empty half of fullwidth punctuation is cut where two meet. `。「` is 6.5 px narrower at 13px, a `「` after a `」`, `・` or another `「` is cut on its left, a `。` or `」` before a closing bracket on its right. `han_kerning.rs` holds Blink's pair rules (`HanKerning`: `ShouldKern`, `ShouldKernLast`, the character classes from category, block and East Asian width, and the font's own class for the comma, full stop, colon, semicolon and quotes from the bounds of ten probe glyphs). `layout_line` runs them after the glyph loop on every stretch of one font that has `halt` in GSUB or GPOS (Hiragino Sans and Mincho; PingFang has none, so nothing is trimmed in text that falls to it), measures what the feature does to a glyph from a one-character line with and without `halt` in a copy of the font, narrows the advance of the glyph, moves the glyphs after it and the width of the line.

`ShapedGlyph::halt` (`Halt`, 8 bytes a glyph) says what happened: `Open` (trimmed for what is before it), `Close` (for what follows) and `CloseAtLineEnd` (a closing bracket that is whole in the text and that Blink trims when it is the last glyph of a row and that makes the row fit). An `Open` glyph is painted `trim` before the start of its box (`Halt::paint_offset`); the caret, hit test and selection keep the logical start. Where a line breaks next to a trimmed glyph Blink shapes the part between the safe boundaries again, alone: a row that starts with a trimmed `「` starts it whole, with the width of the glyph shaped alone in the font of its run (`lone_character`), and a closing bracket that overflows is trimmed to fit if that makes the row fit (`WrapComputation::end_trims`). Those boundaries are unsafe to break at in `end_reshapes` ([the end of a line](#the-end-of-a-line-is-shaped-again)); a min-content layout keeps the shaped positions.

### Smoothing

`WebkitFontSmoothing` (`auto`, `antialiased`, `subpixel-antialiased`, which is `auto`) is CSS `-webkit-font-smoothing` as Chrome has it on macOS, and the first vendor-prefixed style key. gpuix used to draw every glyph with CoreGraphics smoothing on, 9-17 % heavier than Chrome's `antialiased` for dark text and 22-42 % for light (#67). It is inherited like `fontWeight`, through the zed `TextStyle` (`font_smoothing`, in the refinement `apply_styles` writes), on spans (`RunStyle`), inputs (the editor's text style) and through `display: contents` (`inheritable_fields!`), and it can be set in a state style. It is not animatable.

In zed the value rides on the `TextRun` and the `DecorationRun` with the colour to `Window::paint_glyph`, and is part of `RenderGlyphParams` (equality, hash), so one glyph drawn both ways has two atlas tiles. Under a general transform the glyph is rasterized through the matrix and the value means what it means untransformed ([paint-and-transforms.md](paint-and-transforms.md#gpui)); `RenderGlyphParams` also holds the quantized 2x2 then, so a glyph drawn at two matrices has two tiles.

- `Antialiased` is `set_should_smooth_fonts(false)` for every colour.
- `Auto` is Skia's recipe for smoothed text (`SkScalerContext_mac_ct.cpp`): CoreGraphics draws the glyph black on white with smoothing whatever the text colour, its gamma of 2.0 is taken out of each pixel (`sk_pow2_table`), and the coverage goes through Skia's mask gamma table (`SkTMaskGamma_build_correcting_lut`, sRGB device, contrast 0) for the colour's luminance, with each channel halved and cut to 3 bits, as `onFilterRec` does for "some" smoothing. The bucket (`RenderGlyphParams::luminance_bucket`, `glyph_luminance_bucket_for_color`) replaces the grey fill per luminance level the rasteriser had (`dilation`).
- The system's own `AppleFontSmoothing = 0` still turns it off for `Auto` (the plain coverage, as for `antialiased`). The fork does not read or write that preference to get the effect: it is a user setting, not the page's.

## Text in fractions of a pixel (#41)

Chrome lays text out in 1/64 of a pixel and snaps when it paints; gpui used to measure to the next whole pixel, so a box as wide as its text was up to a pixel wider than Chrome's and what followed it later, and a text-box-trimmed height was rounded up to whole pixels. Now:

- A measured width is the advance of the text rounded up to 1/64 of a device pixel (Blink's `ShapeResult::SnappedWidth`, `LayoutUnit::FromFloatCeil`: 1/128 px at scale 2, the window's).
- A trimmed height is the font metric to the nearest 1/64 of one.
- A number line height is cut to 1/64 and a length or percentage rounded to it (`LineHeight::to_laid_out_pixels`). A percentage is rounded like a length, as Chrome does, because it computes to a length.
- The leaf in taffy no longer rounds a measured size up to a device pixel, and forced and atomic-inline widths are not snapped.
- Wrapping and truncation tolerate the float a measured width comes back as: the start and middle ellipsis use the tolerance the end ellipsis and wrapping have, where they used to cut a text by the floor of its width or not at all.
- Positions snap to device pixels when painted, and not in `getBoundingClientRect` ([layout.md](layout.md#the-164-px-unit)). `Window::layout_unsnapped_bounds` reads a box before it is snapped, which a moved replay needs.
- A span's box is the rows of its paragraph measured from where the glyphs are painted, moved by `TextLayout::unsnapped_offset`.

## Glyph positions

### Half-pixel glyph origin (#84)

A glyph is painted at its origin rounded half up on both axes, as Skia rounds a glyph position (`floor(position + 1 / (2 * steps))`), and not half down. A text line whose baseline lands on a half device pixel (a label at y 20.5 at scale 1, at 20.25 or 20.75 at scale 2) used to be one row above Chrome's. zed: `split_glyph_origin` (x in quarters, y whole), and the pixel `paint_emoji` sits in uses `round_half_up`. The mono svg sprite stays half down.

### Text positions

Blink measures every text width from the cumulative advances of an item (a text node), counted from the item's start and ceiled to 1/64 of a device pixel: an item is `ceil(P_end)` wide, a piece of one (a min-content word, the part of an item on a wrapped line) `ceil(P_end) - ceil(P_start)`, a line the sum of its pieces, and an element the widest line.

zed: `SnapMarks::width` (`gpui/src/text_system/line_layout/snap.rs`) is that model, over the glyph x of a `LineLayout`, the glyph each item starts at (the font runs the line was shaped as, and the `TextRun`s `shape_text_inline` passes `layout_wrapped_line` as `item_starts`) and the kerns. `WrappedLineLayout` keeps the width of each line (`line_widths`, min-content) and of the whole text (`width`). Alignment still reads the CoreText positions. Wrap decisions and the reported widths sum Blink's integer advances ([Blink adds integers](#blink-adds-integers)) and take the width of the end of a line shaped again ([the fit test](#the-fit-test)). The `ceil_to_layout_unit` of the element's size stays, for tab stops and insets that are not whole units.

A font with no GPOS table (`FontMetrics::legacy_kerning`, found with font-kit's `load_font_table`) kerns through a `kern`/`kerx` table, which HarfBuzz splits: `k >> 1` goes into the advance before the pair and `k - (k >> 1)` into the glyph after it (and its offset, so it is drawn where CoreText draws it). A glyph after a kern is measured `k - (k >> 1)` earlier than CoreText has it, which makes a word after a kerned space (`a To` in Helvetica) and one before it narrower in the measure only. This fits the 51 DOM rows of the first sweep and the 152 kerned rows of `text-measure-oracle` exactly, and only with the split at both edges of a word.

**Text nodes.** An item is a text node, not a font run: `Quick brown ` + `fox jumps` is 8601/64 of a pixel as two nodes in Chrome and 8600/64 as one (`Hello {name}` in React is two). `paragraph::styles` keeps a run per text node (it used to merge neighbours of one style, which only saved runs; a paragraph of one node with no restyling is still plain). `shape_text_inline` passes the byte where each `TextRun` after the first of a line starts, and the cache key of a wrapped line holds them. Shaping is unchanged: runs of one font are one font run, so ligatures and kerning across the edge of two nodes stay. A kern pair across the edge is split between the two items as one inside a node is (Blink's split applies within the shaped run): the item before ends `k - (k >> 1)` earlier and the one after is that much wider, so `SnapMarks` measures every glyph, an item edge included, without the share of the kern before it and from the same measure of its item's start.

## Text measure keys and the shape store

A text whose measure depends on more than its string used to be measured and shaped again every frame, which costs about 18 µs a text, and a page keeps hidden copies and `opacity: 0` ghosts of every label. A text now carries a measure key, and with an equal key the layout engine keeps the sizes taffy cached for it ([layout.md](layout.md#the-layout-engine-persists-across-frames) has the engine side: keyed measured leaves and what the key must hash).

What the key holds for each element kind:

- **`StyledText`**: text, runs with their fonts and variation axes, font size, line height, white-space, overflow, line clamp, layout options such as the wrap style, `text-box-trim` and `text-box-edge`, inline boxes, the insets key, the atomic inlines' digest, font generation, and the font of the style when a trim cuts the box by its cap height or x-height. `img`, `list` and `uniform_list` are keyed too.
- **`<input>`**: its shaped text, runs, sizes, rows, font generation and entity (`TextEditorState::measure_key`).
- **gpui's `TextLayoutOptions`, `TextRun` and `InlineBox`** hash all their fields, so `strut_font`, `nowrap`, `TextRun.group`, `UnderlineStyle.decorator` and `.fragment`, and `InlineBox.wraps` and `.key` are in the key. `LineHeight::Normal` is the line height measured from the text style's font, and `normal_line_height` of the runs says whether the fonts they fall back to count.
- **A padded span** gives the text's key the insets that depend on the width (`InsetSpec::text_key`, `StyledText::with_insets_for_width(key, ..)`). What `Insets::text` reads is the measure key's share of the insets, and each consumer still reads the edges for the width the layout kept. The sampled insets of a span that moves are in it too.
- **Gestures**: a span or element gesture is a pointer delta, with a motion's controller named through `gesture_controller`.
- **Atomic inline boxes** (an `inline-grid` label, an `inline-block`): their private layout engine persists on the text's layout node (`AtomicCarry`), and the frame asks it for the same nodes. The text's key holds what the boxes asked for (`AtomicInlines::digest`) and the width they were measured at, so a min-content probe is not answered with the size of a max-content one. A text whose boxes are made anew each frame (a keyless request, a measured leaf as a box's root) has no key and is measured every frame, and a text with atomic inlines is keyed only while its boxes persist. A text with atomic boxes is still built every frame (~43 µs each), because an atomic inline is excluded from retention.

The digest of a verified paragraph (the retention verifier) includes the insets and the decorations.

A 200-text page with padded spans went from 4.73 to 0.28 ms, and an inline-grid page from 11.3 to 8.6 ms.

Two causes of needless measuring were fixed in the key and the cache around it (#46):

- **A colour that re-measured the page.** The measure key of a text hashed every field of its runs, so an inherited `color` that changed measured and shaped every text without a colour of its own, about five measures a text a frame. The key now holds each run's length, font, font size, line height and whether the decoration changes there. When the key hits and the paint hash differs, the carried lines are decorated again (`redecorate_lines`, which shares their layouts) and painted with the new colours. A text that truncates still hashes its runs, because the lines it cut cannot be decorated from them.
- **A line layout cache that forgot.** A frame in which nothing consulted the cache still swapped its generations, so a few frames of replay (nothing looked a layout up) dropped every label's layout and the next frame that measured shaped them all again: a spike of 60 shapes, about 15 ms. The swap now waits for a frame that used the cache.

The line layout cache counts what it did (`getLayoutCounters().shapes`, `wrappedLayouts`, `lineLayoutHits`, `shapeCacheHits`). Behind its two frame generations the window keeps a store of what it shaped (`shape_store.rs`, 8 MiB estimated, least recently used out, emptied when fonts are added), so a page mounted again after other frames finds its lines (`shapeCacheHits`) and shapes nothing.

## Breaking changes

- `display: "inline-block"`, `"inline-flex"` and `"inline-grid"` are atomic inlines that sit on a line of text, where they were ignored and left the box a block. `display: "table"` and the `table-*` values, ignored before, lay out as tables.
- Sibling text nodes in a block `div` flow on one line instead of stacking, and `getPaintedText()` returns the whole paragraph. A nested `<text>` with listeners or state styles stays a block.
- Selection, the copied text, the highlighted range and find's match numbers follow document order, not paint order. A `position: relative` (lifted) element no longer reorders them. Hit testing still follows paint order.
- `lineHeight` is CSS `line-height`: a number is a multiplier of the font size (it was px).
- The default underline sits at the baseline plus `max(1, ceil(thickness / 2))`, and `auto` thickness is a tenth of the font size (it was 1px), so underlines are lower and thicker on big text than they were.
- `fontWeight` on a variable font renders the exact `wght`: 600 on the macOS system font is 600, no longer the 590 semibold instance.
- `textWrapMode` wins over the wrap component of `whiteSpace` on the same element.
- Text is laid out in fractions of a pixel ([#41](#text-in-fractions-of-a-pixel-41)): boxes as wide as their text are no longer up to a pixel wider than Chrome's.
- A row that breaks at a soft hyphen paints a hyphen and is as wide as it, and a row wider than its box starts at the start edge whatever its `textAlign` (it was centred or right-aligned into a negative offset).
- Text in a fallback font (CJK, emoji) has Chrome's advances, so it is wider or narrower than it was by a fraction of a pixel per glyph, CJK punctuation is trimmed where two marks meet, and under `lineHeight: "normal"` a line with a glyph in a fallback font is as tall as that font's line box.

## Upstreaming

- The line box model changes `TextStyle`, `TextLayoutOptions` (it is `Hash` and in the cache key), `FontMetrics` and the platform shapers' ascent and descent, so Zed's editor sees it too. The rest of the Zed workspace is not updated for `LineHeight`.
- Per-run `font_size` and `line_height` change the text path Zed's editor uses (`text_system`, `line_layout`, `line_wrapper`, `line.rs`, `elements/text.rs`), which makes it the least upstreamable change here.
- Font smoothing adds a field to `TextRun`, `DecorationRun` and `RenderGlyphParams`.

## Tests

- Line boxes: `line-box.test.tsx` against `line-box-oracle.json` (1080 boxes).
- Text measure: gpui `chrome_layout_units` (cases measured in Chrome 155, equal to 1/64); `text-layout-units.test.tsx` (what an app reads back with the system fonts); `text-measure.test.tsx` (the rows of headless Chrome at scale 1 and 2, exact min-content and multi-run rows, kerned `w`, `mc` and `wrap` rows of Helvetica and Times at 13 and 16px) with `text-measure-oracle` (its `nodes` rows are two to four text nodes, an edge between words, inside a word and around a kerned space: 984 per scale).
- Baseline of a trimmed label: a gpui test with the metrics of Inter, a 13px label in a 28px pill on row 19 and a 12px label in a 24px pill on row 16, as painted, and a label at the top of a box where it was.
- Glyph origin: `glyph-origin.test.tsx` holds the first and last ink rows of 68 labels, tops at every 1/16 from 20 to 21, to headless Chrome's at scale 1 and 2 (`fixtures/glyph-origin-oracle.*`).
- Cap height and x-height: `text-box-metrics.test.tsx` holds 240 trims per scale against headless Chrome's (`fixtures/x-height-oracle.*`), in five instances of `inter-subset.ttf` that Chrome measures alike.
- Line breaking: `line-break-oracle`, `hyphen-break-oracle`, and `line-fit-oracle`, which pins 865 (1x) and 851 (2x) rows of texts and widths of the fit-test sweep with the lines Chrome puts them in (edge widths where the old rule and Blink's differ, real widths, rows both agreed on, kerned Helvetica and Times, Inter, `break-all`, cut words, soft hyphens; 104 `d-reshape` cases of cut words and `break-all` in kerned Inter, Helvetica, Times and unkerned Menlo and of `-` before a digit, taken from a sweep of 8142 widths per scale; the 200 edge widths per scale of the four Inter 550/18 texts), in `inter-text-subset.ttf` (Inter Variable cut to the characters of the texts). The rows gpuix still lays out differently are left out (Inter in a box one or two letters wide). `line-fit.test.tsx`.
- Soft hyphen: `soft-hyphen.test.tsx` against `soft-hyphen-oracle` (Chrome at 1x and 2x: 114 texts of Inter, Helvetica and Menlo, left, centre and right aligned; the x and width of each row, the box of the span, min-content, the ink of the hyphen at the end of each hyphenated row and none after the others, its colour and underline, a soft hyphen mid-row painting nothing), and the rows of a row wider than its box (a word wider than a 90px box, a word that fits only without its hanging space, in Menlo 14px, `start`, `end`, `left`, `right` and `center`, and 10 `rtl` cases per scale: where every row starts and where the rows with no hanging space end). gpui `a_row_that_breaks_at_a_soft_hyphen_is_painted_with_a_hyphen_that_is_not_text`, `test_a_hyphen_is_part_of_the_aligned_width`, `test_a_row_wider_than_its_box_starts_at_the_right_edge_in_a_right_to_left_box`.
- Integer sums and the end of a line: gpui `snap::tests` (an advance cut to a 65536th before it is summed, as an f32, a kern added whole, a position a hair off is noise) and the reshape tests (room check, run of kerned pairs, cut word, `break-all`, the candidate walk, `-20` at a line start); gpui_macos kerned Helvetica and Times rows and min-content against Chrome.
- Fallback advances: `fallback-advance.test.tsx` against `fallback-advance-oracle` (693 cases per scale, at 1x and 2x: the end of every grapheme cluster within 1/64 of a device pixel of Chrome, in the fallback of Helvetica Neue and Menlo, PingFang SC, Hiragino Sans and Apple SD Gothic Neo named, the system font for what must not change, and three kana texts with punctuation); gpui_macos `test_a_font_with_a_trak_table_is_advanced_as_chrome_advances_it`, `test_the_glyphs_after_a_fallback_glyph_move_with_its_advance`.
- CJK spacing: `cjk-spacing.test.tsx` against `cjk-spacing-oracle` (27 texts of Japanese, Chinese and Korean in Helvetica Neue and Menlo at 12, 14 and 17px: the width of the Range to the end of every grapheme cluster; the caret and hit test of every trimmed opening bracket by `caretPositionFromPoint` points in and after its box; the rows, starts and first cluster of 360 box widths per scale, also with Hiragino Sans named; the kana texts of `テス` pairs at every box width from 50 to 300px), cluster-exact at 1x and 2x but for the prefix before the second glyph of a Latin kern pair (`LEGACY_KERN_SPLIT`, as in line-fit). gpui `script_runs` (Blink's `script_run_iterator_test`), `font_fallback`, `han_kerning`, `trimmed_line`, gpui_macos `test_punctuation_after_kana_is_in_the_font_of_the_kana`, `test_an_opening_bracket_after_a_full_stop_is_trimmed`, `test_a_closing_bracket_before_another_is_trimmed`, `test_nothing_is_trimmed_in_a_font_without_halt`.
- `line-height: normal`: `line-height-normal.test.tsx` against `line-height-normal-oracle` (324 cases per scale, 1x and 2x: Helvetica Neue and Menlo, 9 sizes from 10 to 24px, Latin, Han, kana, Hangul, emoji and mixed text, three-line texts, spans of 1.5em in Latin and Han, and explicit line heights of 24px and 1.5; the height of the block and per line the baseline, the top and height of a span), all within 1/64 px. gpui `test_a_row_is_as_tall_as_the_line_box_of_every_font_used_in_it`, `test_the_line_box_of_a_used_font_leaves_the_content_area_of_its_run_alone`, gpui_macos `test_a_fallback_font_under_line_height_normal_has_a_line_box_of_its_own`. A test that builds its text in a font that lacks some letters (`GpuixTestSquare`) gets the line box of the fallback font under `normal`, as in Chrome; `content-visibility-auto.test.tsx` gives its text a `lineHeight` so its boxes keep the size the oracle measured.
- Decoration lines: `text-decoration-lines-oracle`.
- Empty inline boxes: `table-baseline-cell.test.tsx` (the rows of an empty span, see [layout.md](layout.md#tests)), `empty-inline-box.test.tsx` against `empty-inline-box-oracle` (455 rows per scale at 1x and 2x: a span with padding, a border, a margin, in both and on one side, a negative margin, a percentage padding, a background, a larger font and a smaller line height, `vertical-align: top`, alone, before, between and after text and spaces, in a padded div, centered and right-aligned, under `nowrap` and `pre`, after and between blocks, in an inline-block, a flex item and a table cell, 8 widths of a wrapping line with the box after a space, before one and inside a word, with a larger font, two boxes together; nested and side by side), `empty-inline-box-behaviour.test.tsx` (paint, click, bounds, copy, a style change), `text/inline.rs` unit tests, gpui `test_a_line_breaks_around_an_empty_box_where_it_would_without_it` and `test_the_space_before_a_box_that_ends_a_line_is_removed`, and the table, inline, line-box, baseline, text-measure, find, search and accessibility tests.
- Measure keys: `shape-cache.test.tsx` (a page mounted again shapes nothing), `text-idle-measures.test.tsx` (an idle and an animating frame cost no measure and no shaping, retention on and off).

Oracles are listed by topic in [testing.md](testing.md#chrome-oracles).

Deviations from Chrome in this area: [deviations.md](deviations.md#text).
