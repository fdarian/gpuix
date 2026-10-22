/// Parse any color accepted by csscolorparser 0.8.3 into GPUI's sRGB paint type.
/// Out-of-gamut channels are hard-clipped because GPUI paints sRGB Rgba/Hsla.
pub(crate) fn parse_color_rgba(value: &str) -> Option<gpui::Rgba> {
    let parsed = csscolorparser::parse(value).ok()?.clamp();
    Some(gpui::Rgba {
        r: parsed.r,
        g: parsed.g,
        b: parsed.b,
        a: parsed.a,
    })
}

/// A colour before gamut clipping, so `color-mix()` can interpolate a Display P3
/// red as the out-of-range sRGB it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Srgba {
    pub rgb: [f64; 3],
    pub alpha: f64,
}

impl Srgba {
    fn clip(self) -> gpui::Rgba {
        let channel = |value: f64| value.clamp(0.0, 1.0) as f32;
        gpui::Rgba {
            r: channel(self.rgb[0]),
            g: channel(self.rgb[1]),
            b: channel(self.rgb[2]),
            a: channel(self.alpha),
        }
    }
}

impl From<gpui::Rgba> for Srgba {
    fn from(color: gpui::Rgba) -> Self {
        Self {
            rgb: [color.r as f64, color.g as f64, color.b as f64],
            alpha: color.a as f64,
        }
    }
}

/// The light or dark side a `light-dark()` picks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Scheme {
    #[default]
    Light,
    Dark,
}

impl Scheme {
    /// CSS `color-scheme`. `None` for a value that names no scheme, which CSS
    /// treats as invalid. `"light dark"` (either order) follows `appearance`,
    /// like a page that supports both and is shown to a user who prefers one.
    pub(crate) fn parse(value: &str, appearance: Scheme) -> Option<Scheme> {
        let (mut light, mut dark, mut normal) = (false, false, false);
        for token in value.split_whitespace() {
            match token.to_ascii_lowercase().as_str() {
                "normal" => normal = true,
                "light" => light = true,
                "dark" => dark = true,
                // `only` forbids the UA from overriding the page's choice, which
                // changes nothing for `light-dark()`. Any other identifier is a
                // custom scheme name, which CSS ignores.
                _ => {}
            }
        }
        match (light, dark, normal) {
            (true, true, _) => Some(appearance),
            (true, false, _) => Some(Scheme::Light),
            (false, true, _) => Some(Scheme::Dark),
            (false, false, true) => Some(Scheme::Light),
            (false, false, false) => None,
        }
    }
}

/// What a colour needs from the element it is on: `currentColor`, which side
/// of `light-dark()` to take, and the window appearance that `color-scheme:
/// light dark` follows.
///
/// A context describes one element. `descend` makes the context its children
/// inherit; `for_state` makes the context a `hover`/`active` refinement of the
/// same element paints with.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ColorContext {
    /// This element's computed `color`. Read through [`ColorContext::current`], which says so to the
    /// retained element being built.
    current: gpui::Rgba,
    /// This element's computed `color-scheme`.
    pub scheme: Scheme,
    /// The window's appearance.
    pub appearance: Scheme,
    /// The parent's computed `color`, which is what `color: currentColor` means.
    inherited: gpui::Rgba,
    /// The text colour this element sets on gpui, if any: its own `color` when that
    /// resolved, or the new default text colour when its `colorScheme` changed it.
    declared: Option<gpui::Rgba>,
    /// No element up to this one declared a `color`, so the text colour is still
    /// the initial `CanvasText`, which follows `colorScheme`.
    canvas_text: bool,
    /// Where `current` comes from, see [`reads`].
    current_origin: u64,
    /// Where `inherited` comes from.
    inherited_origin: u64,
}

/// Contexts are equal when they resolve every colour alike: where a colour comes from is how a
/// retained element finds out what it read, not what it resolves to.
impl PartialEq for ColorContext {
    fn eq(&self, other: &Self) -> bool {
        self.current == other.current
            && self.scheme == other.scheme
            && self.appearance == other.appearance
            && self.inherited == other.inherited
            && self.declared == other.declared
            && self.canvas_text == other.canvas_text
    }
}

impl ColorContext {
    /// Everything a colour is resolved from but the colours themselves, for the key of a retained
    /// subtree: which side `light-dark()` takes does not depend on what the subtree reads.
    pub(crate) fn fingerprint(&self, fp: &mut crate::retain::Fingerprint) {
        fp.bits(&[
            self.scheme == Scheme::Dark,
            self.appearance == Scheme::Dark,
            self.declared.is_some(),
            self.canvas_text,
        ]);
    }

    /// The colour the elements below inherit, for the key of a retained subtree that read the colour
    /// it inherits (see [`reads`]). The colour of the parent and the one `declared` sets on gpui are
    /// the context's own: no element below reads them, each has `current` of its parent for its
    /// `inherited`.
    pub(crate) fn fingerprint_colors(&self, fp: &mut crate::retain::Fingerprint) {
        fp.f32(self.current.r);
        fp.f32(self.current.g);
        fp.f32(self.current.b);
        fp.f32(self.current.a);
    }

    /// This element's computed `color`. A build that reads it reads the colour the element inherits
    /// unless an element between declared its own.
    pub(crate) fn current(&self) -> gpui::Rgba {
        self.note_read();
        self.current
    }

    /// Notes that what is built from this context depends on `current` without asking for it: text
    /// takes its colour from gpui's text style, which `declared` of the nearest element above sets.
    pub(crate) fn note_read(&self) {
        reads::note(self.current_origin);
    }

    /// The text colour this element sets on gpui, which is not a read: it sets it.
    pub(crate) fn declared(&self) -> Option<gpui::Rgba> {
        self.declared
    }

    #[cfg(test)]
    pub(crate) fn with_declared(mut self, declared: gpui::Rgba) -> Self {
        self.declared = Some(declared);
        self
    }

    /// The same context with `color` another colour, which is no read of the first.
    pub(crate) fn with_current(mut self, current: gpui::Rgba) -> Self {
        self.current = current;
        self
    }
}

/// Which retained elements read the colour they inherit.
///
/// A colour a style declares gives the elements below it a colour of their own: a change of the
/// `color` of the page is no change to a card whose text sets its own, and the card is skipped.
/// For that the build says what it read. A context carries where its colour comes from: the
/// element being built when a `color` was declared, and for an element that declares none the
/// place its parent's colour came from. Reading the colour notes that place, and an element that
/// is built reads the colour it inherits when something in it noted a place above it. Retained
/// elements are numbered as they are entered, so "above" is "numbered less", and a build that
/// runs later (gpui asks for the subtree of a skipped element when it cannot replay it) is below
/// every place a build before it numbered.
pub(crate) mod reads {
    use std::cell::Cell;

    thread_local! {
        static NEXT: Cell<u64> = const { Cell::new(1) };
        /// The retained element being built, 0 outside one.
        static OPEN: Cell<u64> = const { Cell::new(0) };
        /// The least place noted while the open element was built, with what is below it.
        static LOWEST: Cell<u64> = const { Cell::new(u64::MAX) };
    }

    /// A retained element being built: closing it says what it read.
    #[must_use]
    pub(crate) struct Frame {
        number: u64,
        outer: u64,
        outer_lowest: u64,
    }

    pub(crate) fn open() -> Frame {
        let number = NEXT.get();
        NEXT.set(number + 1);
        Frame {
            number,
            outer: OPEN.replace(number),
            outer_lowest: LOWEST.replace(u64::MAX),
        }
    }

    impl Frame {
        /// Whether the element read the colour it inherits. What it read goes on to the element
        /// around it, which reads what is above that.
        pub(crate) fn close(self) -> bool {
            let lowest = LOWEST.get();
            OPEN.set(self.outer);
            LOWEST.set(self.outer_lowest.min(lowest));
            lowest < self.number
        }
    }

    /// The place a colour declared by the element being built comes from.
    pub(super) fn here() -> u64 {
        OPEN.get()
    }

    pub(super) fn note(origin: u64) {
        LOWEST.set(LOWEST.get().min(origin));
    }
}

/// CSS `CanvasText`: the initial `color`, black on a light scheme and white on a dark one.
fn canvas_text(scheme: Scheme) -> gpui::Rgba {
    let value = match scheme {
        Scheme::Light => 0.0,
        Scheme::Dark => 1.0,
    };
    gpui::Rgba { r: value, g: value, b: value, a: 1.0 }
}

impl ColorContext {
    /// The initial `color-scheme` is `normal`, which is light, and the initial
    /// `color` is `CanvasText`, so black. GPUI's default text colour is black too.
    pub(crate) fn root(appearance: Scheme) -> Self {
        let black = canvas_text(Scheme::Light);
        Self {
            current: black,
            scheme: Scheme::Light,
            appearance,
            inherited: black,
            declared: None,
            canvas_text: true,
            current_origin: 0,
            inherited_origin: 0,
        }
    }

    /// The context of an element styled by `style`, whose parent has `self`.
    pub(crate) fn descend(&self, style: &crate::style::StyleDesc) -> Self {
        Self::resolve_style(*self, (self.current, self.current_origin), self.current_origin, style)
    }

    /// The context of a state refinement (`hover`, `active`, `focusVisible`) of
    /// the element that has `self`. Its `currentColor` is still the parent's
    /// colour, as in CSS, and a state that sets no `color` keeps the base one.
    pub(crate) fn for_state(&self, style: &crate::style::StyleDesc) -> Self {
        Self::resolve_style(*self, (self.inherited, self.inherited_origin), self.current_origin, style)
    }

    /// The context an element's own `color` resolves in: its `colorScheme`, with
    /// `currentColor` meaning the parent's colour. Motion animates `color` in it.
    pub(crate) fn for_color_property(&self, style: &crate::style::StyleDesc) -> Self {
        Self::color_property_context(*self, (self.current, self.current_origin), style)
    }

    fn color_property_context(
        base: Self,
        (parent_color, parent_origin): (gpui::Rgba, u64),
        style: &crate::style::StyleDesc,
    ) -> Self {
        let scheme = style
            .color_scheme
            .as_deref()
            .and_then(|value| Scheme::parse(value, base.appearance))
            .unwrap_or(base.scheme);
        Self {
            scheme,
            inherited: parent_color,
            declared: None,
            current: parent_color,
            current_origin: parent_origin,
            inherited_origin: parent_origin,
            ..base
        }
    }

    /// `parent` is the colour `currentColor` means, with where it comes from. A style that sets no
    /// `color` keeps `base.current`.
    fn resolve_style(
        base: Self,
        parent: (gpui::Rgba, u64),
        kept_origin: u64,
        style: &crate::style::StyleDesc,
    ) -> Self {
        let kept_color = base.current;
        let mut context = Self::color_property_context(base, parent, style);
        let scheme = context.scheme;
        context.declared = style
            .color
            .as_deref()
            .and_then(|value| resolve_color(value, &context));
        context.canvas_text = base.canvas_text && context.declared.is_none();
        // A colour set here is read as this element's own by the ones below it, not as the one its
        // parent hands down. Canvas text is set by nobody: gpui's text style has it from the window.
        context.current_origin = kept_origin;
        context.current = match context.declared {
            Some(color) => {
                context.current_origin = reads::here();
                color
            }
            None if context.canvas_text => {
                let color = canvas_text(scheme);
                // gpui's text colour is black until told otherwise.
                if scheme != base.scheme {
                    context.declared = Some(color);
                    context.current_origin = reads::here();
                }
                color
            }
            None => kept_color,
        };
        context
    }

    pub(crate) fn resolve(&self, value: &str) -> Option<gpui::Rgba> {
        resolve_color(value, self)
    }

    /// Whether every colour resolves the same in both. `resolve_unclamped` reads
    /// `current` and `scheme` and nothing else: keep this in step with it, because
    /// the parse caches treat contexts that agree here as one.
    pub(crate) fn resolves_like(&self, other: &Self) -> bool {
        self.current == other.current && self.scheme == other.scheme
    }
}

/// Resolve any CSS colour for an element: everything `parse_color_rgba` takes,
/// plus `currentColor`, `light-dark()`, `color-mix()` and `color()`, nested
/// freely. The result is clipped to sRGB. `None` for an invalid value, which the
/// caller ignores as it always has.
pub(crate) fn resolve_color(value: &str, context: &ColorContext) -> Option<gpui::Rgba> {
    if !is_contextual(value) {
        return parse_color_rgba(value);
    }
    resolve_unclamped(value, context).map(Srgba::clip)
}

/// True for the forms `parse_color_rgba` cannot resolve alone.
pub(crate) fn is_contextual(value: &str) -> bool {
    let value = value.trim();
    let starts = |prefix: &str| {
        value
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    };
    value.eq_ignore_ascii_case("currentcolor")
        || starts("light-dark(")
        || starts("color-mix(")
        || starts("color(")
}

/// Whether CSS Color 4 calls the syntax of a colour that resolved legacy: hex,
/// a name (`transparent` included), `rgb()`, `rgba()`, `hsl()` or `hsla()`. Two
/// legacy colours interpolate in sRGB, any other pair in Oklab. `light-dark()`
/// has the syntax of the side it picks. `currentColor` and `color-mix()` count
/// as modern: the context keeps the resolved colour, not how it was written.
pub(crate) fn is_legacy_syntax(value: &str, context: &ColorContext) -> bool {
    use crate::css::syntax::{function_args, split_commas};

    let value = value.trim();
    if let Some(args) = function_args(value, "light-dark") {
        let sides = split_commas(args);
        let picked = match context.scheme {
            Scheme::Light => sides.first(),
            Scheme::Dark => sides.get(1),
        };
        return picked.is_some_and(|side| is_legacy_syntax(side, context));
    }
    if is_contextual(value) {
        return false;
    }
    let Some(open) = value.find('(') else {
        return true;
    };
    let name = &value[..open];
    let relative = value[open + 1..]
        .trim_start()
        .get(..5)
        .is_some_and(|head| head.eq_ignore_ascii_case("from "));
    !relative
        && ["rgb", "rgba", "hsl", "hsla"]
            .iter()
            .any(|legacy| name.eq_ignore_ascii_case(legacy))
}

/// Same as `resolve_color` but keeps out-of-gamut channels, for the operands of
/// `color-mix()`.
pub(crate) fn resolve_unclamped(value: &str, context: &ColorContext) -> Option<Srgba> {
    use crate::css::syntax::{function_args, split_commas};

    let value = value.trim();
    if value.eq_ignore_ascii_case("currentcolor") {
        return Some(context.current().into());
    }
    if let Some(args) = function_args(value, "light-dark") {
        let [light, dark] = split_commas(args).as_slice().try_into().ok()?;
        // Both sides must be valid, as CSS rejects the declaration otherwise.
        let (light, dark) = (
            resolve_unclamped(light, context)?,
            resolve_unclamped(dark, context)?,
        );
        return Some(match context.scheme {
            Scheme::Light => light,
            Scheme::Dark => dark,
        });
    }
    if let Some(args) = function_args(value, "color-mix") {
        return crate::css::color_mix::parse(args, context);
    }
    if let Some(args) = function_args(value, "color") {
        return crate::css::color_function::parse(args);
    }
    let parsed = csscolorparser::parse(value).ok()?;
    Some(Srgba {
        rgb: [parsed.r as f64, parsed.g as f64, parsed.b as f64],
        alpha: parsed.a as f64,
    })
}

/// Compatibility helper kept at the gpuix-native crate root.
pub fn parse_color(value: &str) -> Option<(f32, f32, f32, f32)> {
    parse_color_rgba(value).map(|color| (color.r, color.g, color.b, color.a))
}

/// Compatibility helper kept at the gpuix-native crate root.
pub fn parse_color_hex(value: &str) -> Option<u32> {
    parse_color_rgba(value).map(u32::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_same(input: &str, expected: &str) {
        let actual = parse_color(input).unwrap_or_else(|| panic!("did not parse {input}"));
        let expected = parse_color(expected).unwrap();
        for (actual, expected) in [actual.0, actual.1, actual.2, actual.3]
            .into_iter()
            .zip([expected.0, expected.1, expected.2, expected.3])
        {
            assert!((actual - expected).abs() <= 1.0 / 255.0, "{input}");
        }
    }

    #[test]
    fn parses_every_absolute_function_family() {
        let cases = [
            ("#f00f", "#ff0000ff"),
            ("ff0000ff", "#ff0000ff"),
            ("rebeccapurple", "#663399"),
            ("transparent", "#00000000"),
            ("rgb(255 0 0)", "#ff0000"),
            ("rgba(255, 0, 0, 1)", "#ff0000"),
            ("hsl(0 100% 50%)", "#ff0000"),
            ("hsla(0, 100%, 50%, 1)", "#ff0000"),
            ("hwb(0 0% 0%)", "#ff0000"),
            ("hwba(0, 0%, 0%, 1)", "#ff0000"),
            ("hsv(0 100% 100%)", "#ff0000"),
            ("hsva(0, 100%, 100%, 1)", "#ff0000"),
            ("lab(100% 0 0)", "#ffffff"),
            ("lch(100% 0 0)", "#ffffff"),
            ("oklab(0.62796 0.22486 0.12585)", "#ff0000"),
            ("oklch(0.62796 0.25768 29.23388)", "#ff0000"),
            ("rgb(none none none / none)", "#00000000"),
        ];

        for (input, expected) in cases {
            assert_same(input, expected);
        }
    }

    #[test]
    fn legacy_syntax_is_hex_names_rgb_and_hsl() {
        let context = ColorContext::root(Scheme::Light);
        for value in [
            "#f00", "#ff0000", "#ff000080", "red", "RebeccaPurple", "transparent",
            "rgb(255 0 0)", "rgba(255, 0, 0, 0.5)", "RGB(255 0 0 / 50%)",
            "hsl(0 100% 50%)", "hsla(0, 100%, 50%, 1)",
        ] {
            assert!(is_legacy_syntax(value, &context), "{value}");
        }
        for value in [
            "hwb(0 0% 0%)", "lab(54% 80 70)", "lch(54% 100 40)", "oklab(0.6 0.2 0.1)",
            "oklch(0.6 0.2 30)", "color(srgb 1 0 0)", "color-mix(in srgb, red, blue)",
            "currentColor", "rgb(from red r g b)", "hsl(from #f00 h s l)",
        ] {
            assert!(!is_legacy_syntax(value, &context), "{value}");
        }
    }

    #[test]
    fn light_dark_is_legacy_when_the_picked_side_is() {
        let mut context = ColorContext::root(Scheme::Light);
        assert!(is_legacy_syntax("light-dark(red, oklch(0.6 0.2 30))", &context));
        assert!(!is_legacy_syntax("light-dark(oklch(0.6 0.2 30), red)", &context));
        context.scheme = Scheme::Dark;
        assert!(!is_legacy_syntax("light-dark(red, oklch(0.6 0.2 30))", &context));
    }

    #[test]
    fn parses_alpha_in_every_function_family() {
        let cases = [
            "rgb(0 0 0 / 50%)",
            "rgba(0, 0, 0, 0.5)",
            "hsl(0 0% 0% / 50%)",
            "hsla(0, 0%, 0%, 0.5)",
            "hwb(0 0% 100% / 50%)",
            "hwba(0, 0%, 100%, 0.5)",
            "hsv(0 0% 0% / 50%)",
            "hsva(0, 0%, 0%, 0.5)",
            "lab(0% 0 0 / 50%)",
            "lch(0% 0 0 / 50%)",
            "oklab(0 0 0 / 50%)",
            "oklch(0 0 0 / 50%)",
        ];

        for input in cases {
            let (_, _, _, alpha) =
                parse_color(input).unwrap_or_else(|| panic!("did not parse {input}"));
            assert!((alpha - 0.5).abs() < f32::EPSILON, "{input}");
        }
    }

    #[test]
    fn parses_every_relative_function_family() {
        let cases = [
            ("rgb(from #bad455 b r g / alpha)", "#55bad4"),
            ("hsl(from #bad455 h s l / alpha)", "#bad455"),
            ("hwb(from #bad455 h w b / alpha)", "#bad455"),
            ("hsv(from #bad455 h s v / alpha)", "#bad455"),
            ("lab(from #bad455 l a b / alpha)", "#bad455"),
            ("lch(from #bad455 l c h / alpha)", "#bad455"),
            ("oklab(from #bad455 calc(l * 0.7) a b)", "#708500"),
            (
                "oklch(from #bad455 calc(l - 0.15) calc(c * 0.7) h)",
                "#8fa150",
            ),
        ];

        for (input, expected) in cases {
            assert_same(input, expected);
        }

        for input in [
            "lab(from #bad455 l a b / calc(alpha / 2))",
            "lch(from #bad455 l c h / calc(alpha * 0.5))",
        ] {
            let (_, _, _, alpha) = parse_color(input).unwrap();
            assert!((alpha - 0.5).abs() < f32::EPSILON, "{input}");
        }
    }

    #[test]
    fn rejects_values_outside_the_parser_contract() {
        for input in [
            "",
            "reddish",
            "#gg0000",
            "hsl(nope)",
            "color(display-p3 1 0 0)",
        ] {
            assert_eq!(parse_color_hex(input), None, "{input}");
        }
    }

    #[test]
    fn clips_wide_gamut_output_before_gpui() {
        let color = parse_color_rgba("oklch(90% 0.4 40)").expect("valid OKLCH");
        for channel in [color.r, color.g, color.b, color.a] {
            assert!((0.0..=1.0).contains(&channel));
        }
    }

    fn rgba(r: f32, g: f32, b: f32) -> gpui::Rgba {
        gpui::Rgba { r, g, b, a: 1.0 }
    }

    fn style(color: Option<&str>, scheme: Option<&str>) -> crate::style::StyleDesc {
        crate::style::StyleDesc {
            color: color.map(str::to_owned),
            color_scheme: scheme.map(str::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn plain_colours_resolve_exactly_like_the_context_free_parser() {
        let context = ColorContext::root(Scheme::Light);
        for input in ["#336699", "rebeccapurple", "oklch(90% 0.4 40)", "rgb(300 0 0)", "nope"] {
            assert_eq!(resolve_color(input, &context), parse_color_rgba(input), "{input}");
        }
    }

    #[test]
    fn current_color_is_the_computed_color_and_inherits() {
        let root = ColorContext::root(Scheme::Light);
        assert_eq!(root.resolve("CurrentColor"), Some(rgba(0.0, 0.0, 0.0)));
        let red = root.descend(&style(Some("#ff0000"), None));
        assert_eq!(red.resolve("currentColor"), Some(rgba(1.0, 0.0, 0.0)));
        // A child that sets nothing inherits it, and did not declare its own.
        let child = red.descend(&style(None, None));
        assert_eq!(child.current, rgba(1.0, 0.0, 0.0));
        assert_eq!(child.declared, None);
    }

    #[test]
    fn a_color_of_current_color_means_the_parents_color() {
        let red = ColorContext::root(Scheme::Light).descend(&style(Some("#ff0000"), None));
        let child = red.descend(&style(Some("currentColor"), None));
        assert_eq!(child.declared, Some(rgba(1.0, 0.0, 0.0)));
        let mixed = red.descend(&style(Some("color-mix(in srgb, currentColor, blue)"), None));
        assert!((mixed.current.r - 0.5).abs() < 1e-6 && (mixed.current.b - 0.5).abs() < 1e-6);
    }

    #[test]
    fn an_invalid_color_is_ignored_and_keeps_the_inherited_one() {
        let red = ColorContext::root(Scheme::Light).descend(&style(Some("#ff0000"), None));
        let child = red.descend(&style(Some("light-dark(red)"), None));
        assert_eq!(child.declared, None);
        assert_eq!(child.current, rgba(1.0, 0.0, 0.0));
    }

    #[test]
    fn light_dark_follows_the_scheme_and_inherits_it() {
        let root = ColorContext::root(Scheme::Dark);
        let value = "light-dark(#ff0000, #0000ff)";
        // `normal` is light even in a dark window.
        assert_eq!(root.resolve(value), Some(rgba(1.0, 0.0, 0.0)));
        let dark = root.descend(&style(None, Some("dark")));
        assert_eq!(dark.resolve(value), Some(rgba(0.0, 0.0, 1.0)));
        let inherited = dark.descend(&style(None, None));
        assert_eq!(inherited.resolve(value), Some(rgba(0.0, 0.0, 1.0)));
        let overridden = dark.descend(&style(None, Some("only light")));
        assert_eq!(overridden.resolve(value), Some(rgba(1.0, 0.0, 0.0)));
        // An element's color is resolved under its own scheme.
        let both = root.descend(&style(Some(value), Some("dark")));
        assert_eq!(both.current, rgba(0.0, 0.0, 1.0));
    }

    #[test]
    fn the_default_text_colour_follows_the_scheme_until_a_color_is_declared() {
        let root = ColorContext::root(Scheme::Dark);
        assert_eq!(root.current, rgba(0.0, 0.0, 0.0));
        let dark = root.descend(&style(None, Some("dark")));
        assert_eq!(dark.current, rgba(1.0, 1.0, 1.0));
        assert_eq!(dark.declared, Some(rgba(1.0, 1.0, 1.0)));
        // Inherited without being re-declared.
        let child = dark.descend(&style(None, None));
        assert_eq!((child.current, child.declared), (rgba(1.0, 1.0, 1.0), None));
        // A light island inside it flips back.
        let light = dark.descend(&style(None, Some("light")));
        assert_eq!((light.current, light.declared), (rgba(0.0, 0.0, 0.0), Some(rgba(0.0, 0.0, 0.0))));
        // `light dark` follows the window, and `normal` stays black.
        assert_eq!(root.descend(&style(None, Some("light dark"))).current, rgba(1.0, 1.0, 1.0));
        assert_eq!(ColorContext::root(Scheme::Light).descend(&style(None, Some("light dark"))).current, rgba(0.0, 0.0, 0.0));
        assert_eq!(root.descend(&style(None, Some("normal"))).declared, None);
    }

    #[test]
    fn a_declared_color_stops_following_the_scheme() {
        let root = ColorContext::root(Scheme::Dark);
        let red = root.descend(&style(Some("#ff0000"), None));
        let dark_child = red.descend(&style(None, Some("dark")));
        assert_eq!((dark_child.current, dark_child.declared), (rgba(1.0, 0.0, 0.0), None));
        // Same element: the color wins over the scheme's default.
        let both = root.descend(&style(Some("#ff0000"), Some("dark")));
        assert_eq!(both.declared, Some(rgba(1.0, 0.0, 0.0)));
    }

    #[test]
    fn color_scheme_values() {
        for (value, appearance, expected) in [
            ("normal", Scheme::Dark, Some(Scheme::Light)),
            ("light", Scheme::Dark, Some(Scheme::Light)),
            ("dark", Scheme::Light, Some(Scheme::Dark)),
            ("only dark", Scheme::Light, Some(Scheme::Dark)),
            ("light dark", Scheme::Dark, Some(Scheme::Dark)),
            ("dark light", Scheme::Light, Some(Scheme::Light)),
            ("light dark", Scheme::Light, Some(Scheme::Light)),
            ("Dark foo", Scheme::Light, Some(Scheme::Dark)),
            ("", Scheme::Light, None),
            ("sepia", Scheme::Light, None),
        ] {
            assert_eq!(Scheme::parse(value, appearance), expected, "{value}");
        }
    }

    #[test]
    fn a_state_keeps_the_base_color_unless_it_sets_one() {
        let base = ColorContext::root(Scheme::Light).descend(&style(Some("#ff0000"), None));
        assert_eq!(base.for_state(&style(None, None)).current, rgba(1.0, 0.0, 0.0));
        let hovered = base.for_state(&style(Some("#0000ff"), None));
        assert_eq!(hovered.current, rgba(0.0, 0.0, 1.0));
        // currentColor inside the state's own color is still the parent's.
        let parent_black = base.for_state(&style(Some("currentColor"), None));
        assert_eq!(parent_black.current, rgba(0.0, 0.0, 0.0));
    }

    #[test]
    fn contextual_forms_are_detected_case_insensitively() {
        for input in ["currentColor", "CURRENTCOLOR", " color-mix(in srgb, red, blue)", "Light-Dark(red, blue)", "COLOR(srgb 1 0 0)"] {
            assert!(is_contextual(input), "{input}");
        }
        assert!(!is_contextual("rgb(0 0 0)"));
        assert!(!is_contextual("colorful"));
    }

    #[test]
    fn compatibility_helpers_share_the_same_result() {
        let rgba = parse_color_rgba("oklch(0.62796 0.25768 29.23388 / 50%)").unwrap();
        assert_eq!(
            parse_color("oklch(0.62796 0.25768 29.23388 / 50%)"),
            Some((rgba.r, rgba.g, rgba.b, rgba.a))
        );
        assert_eq!(
            parse_color_hex("oklch(0.62796 0.25768 29.23388 / 50%)"),
            Some(u32::from(rgba))
        );
    }

    /// What an element at each depth of `declare` (the `color` it declares, if any) reads, when the
    /// innermost reads the colour it has. Outermost first. The page declares its own.
    fn reads_of(declare: &[Option<&str>]) -> Vec<bool> {
        let mut context = ColorContext::root(Scheme::Light);
        let mut frames = Vec::new();
        for (depth, color) in std::iter::once(&Some("#223344")).chain(declare).enumerate() {
            frames.push(reads::open());
            context = context.descend(&style(*color, None));
            let _ = depth;
        }
        context.current();
        let mut closed: Vec<bool> = frames.into_iter().rev().map(reads::Frame::close).collect();
        closed.reverse();
        closed.remove(0);
        closed
    }

    #[test]
    fn an_element_reads_the_colour_it_inherits_when_the_one_it_has_is_not_declared_below_it() {
        assert_eq!(reads_of(&[None, None, None]), [true, true, true]);
    }

    #[test]
    fn a_colour_declared_cuts_the_read_for_what_is_above_it() {
        assert_eq!(reads_of(&[None, Some("#ff0000"), None, None]), [false, false, true, true]);
        assert_eq!(reads_of(&[Some("#ff0000"), None]), [false, true]);
        assert_eq!(reads_of(&[Some("#ff0000")]), [false]);
        assert_eq!(reads_of(&[None, Some("#ff0000"), Some("#00ff00")]), [false, false, false]);
    }

    #[test]
    fn a_colour_declared_from_the_one_inherited_reads_it_and_does_not_cut() {
        for declared in ["currentColor", "color-mix(in srgb, currentColor 50%, white)", "light-dark(currentColor, red)"] {
            assert_eq!(reads_of(&[None, Some(declared)]), [true, true], "{declared}");
            assert_eq!(reads_of(&[None, Some("#ff0000"), Some(declared)]), [false, false, true], "{declared}");
        }
    }

    #[test]
    fn canvas_text_is_read_by_every_element_it_reaches_and_set_by_a_scheme_that_changes_it() {
        let page = reads::open();
        let context = ColorContext::root(Scheme::Light).descend(&style(None, None));
        let card = reads::open();
        context.current();
        assert!(card.close() && page.close(), "the text colour of a page that declares none is the window's");
        let page = reads::open();
        let context = ColorContext::root(Scheme::Light).descend(&style(None, Some("dark")));
        let card = reads::open();
        context.current();
        assert!(card.close(), "the card reads what the page set");
        assert!(!page.close(), "the page set it");
    }

    #[test]
    fn a_colour_that_names_no_colour_of_the_context_reads_none() {
        let context = ColorContext::root(Scheme::Light);
        let frame = reads::open();
        for value in ["#ff0000", "rgb(0 0 0)", "light-dark(red, blue)", "color-mix(in srgb, red, blue)"] {
            context.resolve(value);
        }
        assert!(!frame.close());
    }

    #[test]
    fn a_state_reads_the_colour_of_its_parent_for_its_own_currentcolor() {
        let page = ColorContext::root(Scheme::Light).descend(&style(Some("#223344"), None));
        let frame = reads::open();
        let own = page.descend(&style(Some("#ff0000"), None));
        own.for_state(&style(None, None)).current();
        assert!(!frame.close(), "a state that sets none keeps the colour the element declared");
        let frame = reads::open();
        let own = page.descend(&style(None, None));
        own.for_state(&style(Some("currentColor"), None)).current();
        assert!(frame.close(), "currentColor in a state is the colour of the parent");
    }

    #[test]
    fn the_key_of_a_context_names_the_colour_below_and_not_the_one_beside_it() {
        let key = |context: &ColorContext| {
            let mut fp = crate::retain::Fingerprint::new();
            context.fingerprint_colors(&mut fp);
            fp.finish()
        };
        let page = ColorContext::root(Scheme::Light).descend(&style(Some("#ff0000"), None));
        let child = page.descend(&style(Some("#00ff00"), None));
        let other = ColorContext::root(Scheme::Light).descend(&style(Some("#0000ff"), None)).descend(&style(Some("#00ff00"), None));
        assert_eq!(key(&child), key(&other));
        assert_ne!(key(&page), key(&child));
    }
}
