//! Which properties a shorthand covers, and the cascade order between them.
//!
//! In CSS a shorthand and a longhand it covers are two declarations of the same property, and the
//! one written later wins. The style parsers rank them instead (a longhand beats a logical pair
//! like `paddingBlock`, which beats `padding`), which is right whenever the longhand is written
//! last. This table makes the rank give CSS's answer the rest of the time, in two places:
//!
//! - [`deserialize_ordered`] runs as a style object is parsed, and drops the longhands a shorthand
//!   covers that were written before it.
//! - [`shadow`] runs where one style layers over another (a state style over its base, an
//!   element's own style over the universal one), and drops what the upper layer's shorthands
//!   cover from the lower one.
//!
//! Motion maps follow written order themselves, as they hold their own property set.

use std::borrow::Cow;
use std::sync::LazyLock;

use rustc_hash::FxHashMap;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, Visitor};

use crate::css::custom_properties::{self, CustomProperties};
use crate::events::state_styles::{split_border_all_sides, without_invalid_values, AllSides};
use crate::style::StyleDesc;

type KeyId = usize;

struct Key {
    /// The field's name, in snake case.
    name: &'static str,
    is_set: fn(&StyleDesc) -> bool,
    clear: fn(&mut StyleDesc),
}

macro_rules! key {
    ($field:ident) => {
        Key {
            name: stringify!($field),
            is_set: |style| style.$field.is_some(),
            clear: |style| style.$field = None,
        }
    };
}

/// Each shorthand and the properties it covers directly. A covered property may be a shorthand
/// itself, and what that one covers is covered too.
macro_rules! covers {
    ($($shorthand:ident => [$($covered:ident),+ $(,)?]),+ $(,)?) => {
        fn direct() -> Vec<(Key, Vec<Key>)> {
            vec![$((key!($shorthand), vec![$(key!($covered)),+])),+]
        }
    };
}

covers! {
    padding => [padding_block, padding_inline, padding_top, padding_right, padding_bottom, padding_left],
    padding_block => [padding_top, padding_bottom],
    padding_inline => [padding_left, padding_right],
    margin => [margin_block, margin_inline, margin_top, margin_right, margin_bottom, margin_left],
    margin_block => [margin_top, margin_bottom],
    margin_inline => [margin_left, margin_right],
    inset => [inset_block, inset_inline, top, right, bottom, left],
    inset_block => [top, bottom],
    inset_inline => [left, right],
    border => [
        border_block, border_inline, border_top, border_right, border_bottom, border_left,
        border_width, border_color, border_style,
    ],
    border_block => [border_top, border_bottom],
    border_inline => [border_left, border_right],
    border_top => [border_top_width, border_top_color, border_top_style],
    border_right => [border_right_width, border_right_color, border_right_style],
    border_bottom => [border_bottom_width, border_bottom_color, border_bottom_style],
    border_left => [border_left_width, border_left_color, border_left_style],
    border_width => [border_top_width, border_right_width, border_bottom_width, border_left_width],
    border_color => [border_top_color, border_right_color, border_bottom_color, border_left_color],
    border_style => [border_top_style, border_right_style, border_bottom_style, border_left_style],
    border_radius => [
        border_top_left_radius, border_top_right_radius, border_bottom_left_radius, border_bottom_right_radius,
    ],
    outline => [outline_width, outline_style, outline_color],
    flex => [flex_grow, flex_shrink, flex_basis],
    flex_flow => [flex_direction, flex_wrap],
    place_items => [align_items, justify_items],
    place_content => [align_content, justify_content],
    place_self => [align_self, justify_self],
    gap => [row_gap, column_gap],
    overflow => [overflow_x, overflow_y],
    overscroll_behavior => [overscroll_behavior_x, overscroll_behavior_y],
    container => [container_type, container_name],
    grid_area => [grid_column, grid_row],
    grid_column => [grid_column_start, grid_column_end],
    grid_row => [grid_row_start, grid_row_end],
    transition => [
        transition_property, transition_duration, transition_timing_function, transition_delay,
        transition_behavior,
    ],
    animation => [
        animation_name, animation_duration, animation_timing_function, animation_delay,
        animation_iteration_count, animation_direction, animation_fill_mode, animation_play_state,
    ],
    text_decoration => [
        text_decoration_line, text_decoration_style, text_decoration_color, text_decoration_thickness,
    ],
    text_box => [text_box_trim, text_box_edge],
    text_wrap => [text_wrap_mode, text_wrap_style],
    mask => [
        mask_image, mask_size, mask_position, mask_repeat, mask_origin, mask_clip, mask_mode,
        mask_composite,
    ],
    background => [
        background_color, background_image, background_size, background_position, background_repeat,
        background_origin, background_clip,
    ],
}

struct Covers {
    keys: Vec<Key>,
    /// By the key's name in the style object (camel case).
    ids: FxHashMap<String, KeyId>,
    /// Every property a key covers, through shorthands it covers as well. Empty for a longhand.
    covered: Vec<Vec<KeyId>>,
    border_width: KeyId,
    border_color: KeyId,
    border_style: KeyId,
}

static COVERS: LazyLock<Covers> = LazyLock::new(Covers::build);

fn camel_case(snake: &str) -> String {
    let mut camel = String::with_capacity(snake.len());
    let mut upper = false;
    for character in snake.chars() {
        if character == '_' {
            upper = true;
        } else if upper {
            camel.push(character.to_ascii_uppercase());
            upper = false;
        } else {
            camel.push(character);
        }
    }
    camel
}

impl Covers {
    fn build() -> Self {
        let mut keys: Vec<Key> = Vec::new();
        let mut ids: FxHashMap<String, KeyId> = FxHashMap::default();
        let mut id_of = |key: Key, keys: &mut Vec<Key>| -> KeyId {
            *ids.entry(camel_case(key.name)).or_insert_with(|| {
                keys.push(key);
                keys.len() - 1
            })
        };
        let mut direct_covers: Vec<(KeyId, Vec<KeyId>)> = Vec::new();
        for (shorthand, covered) in direct() {
            let shorthand = id_of(shorthand, &mut keys);
            let covered = covered.into_iter().map(|key| id_of(key, &mut keys)).collect();
            direct_covers.push((shorthand, covered));
        }
        let mut covered: Vec<Vec<KeyId>> = vec![Vec::new(); keys.len()];
        for (shorthand, direct) in &direct_covers {
            covered[*shorthand] = direct.clone();
        }
        // Close over nested shorthands. The table is a few dozen rows, so repeating until nothing
        // is added is simpler than ordering them.
        loop {
            let mut grew = false;
            for shorthand in 0..keys.len() {
                for target in covered[shorthand].clone() {
                    for deeper in covered[target].clone() {
                        if !covered[shorthand].contains(&deeper) {
                            covered[shorthand].push(deeper);
                            grew = true;
                        }
                    }
                }
            }
            if !grew {
                break;
            }
        }
        let id = |name: &str| *ids.get(name).unwrap_or_else(|| panic!("{name} is in the covers table"));
        Self {
            border_width: id("borderWidth"),
            border_color: id("borderColor"),
            border_style: id("borderStyle"),
            keys,
            ids,
            covered,
        }
    }

    fn is_shorthand(&self, key: KeyId) -> bool {
        !self.covered[key].is_empty()
    }

    /// The sides a border shorthand sets, in top, right, bottom, left order.
    fn border_sides(&self, key: KeyId) -> Option<[bool; 4]> {
        Some(match self.keys[key].name {
            "border_top" => [true, false, false, false],
            "border_right" => [false, true, false, false],
            "border_bottom" => [false, false, true, false],
            "border_left" => [false, false, false, true],
            "border_block" => [true, false, true, false],
            "border_inline" => [false, true, false, true],
            _ => return None,
        })
    }

    /// `style` was parsed from keys written in `written` order: drop every property a shorthand
    /// covers that was written before it. A border value for all sides written before a
    /// shorthand for some sides is pushed down to the other sides, as in
    /// [`drop_shadowed_longhands`](crate::events::state_styles).
    fn normalise(&self, style: &mut StyleDesc, written: &[KeyId]) {
        if !written.iter().skip(1).any(|&key| self.is_shorthand(key)) {
            return;
        }
        // CSS drops an invalid declaration whole, so one that is invalid covers nothing.
        let invalid: Vec<KeyId> = {
            let valid = without_invalid_values(style);
            written
                .iter()
                .copied()
                .filter(|&key| self.is_shorthand(key) && (self.keys[key].is_set)(style) && !(self.keys[key].is_set)(&valid))
                .collect()
        };
        for (index, &key) in written.iter().enumerate() {
            if !self.is_shorthand(key) || !(self.keys[key].is_set)(style) || invalid.contains(&key) {
                continue;
            }
            let later = &written[index + 1..];
            if let Some(sides) = self.border_sides(key) {
                split_border_all_sides(
                    style,
                    sides,
                    AllSides {
                        width: !later.contains(&self.border_width),
                        color: !later.contains(&self.border_color),
                        style: !later.contains(&self.border_style),
                    },
                );
            }
            for &target in &self.covered[key] {
                if !later.contains(&target) {
                    (self.keys[target].clear)(style);
                }
            }
        }
    }

    fn shadow(&self, base: &mut Cow<'_, StyleDesc>, over: &StyleDesc) {
        for (key, covered) in self.covered.iter().enumerate() {
            if covered.is_empty() || !(self.keys[key].is_set)(over) {
                continue;
            }
            for &target in covered {
                // Each clear copies the base the first time it finds something to clear.
                if (self.keys[target].is_set)(base) {
                    (self.keys[target].clear)(base.to_mut());
                }
            }
        }
    }
}

/// A shorthand in `over` resets what it covers in `base`, as a later CSS declaration does.
pub(crate) fn shadow(base: &mut Cow<'_, StyleDesc>, over: &StyleDesc) {
    COVERS.shadow(base, over);
}

/// Parses a style object, then drops the longhands a shorthand covers that were written before it,
/// and collects its `--name` keys, which no field names, as the custom properties.
pub(crate) fn deserialize_ordered<'de, D: Deserializer<'de>>(deserializer: D) -> Result<StyleDesc, D::Error> {
    let mut recorder = Recorder::default();
    let mut style = StyleDesc::deserialize(RecordKeys { inner: deserializer, recorder: &mut recorder })?;
    COVERS.normalise(&mut style, &recorder.written);
    style.custom_properties = CustomProperties::declared(recorder.customs);
    Ok(style)
}

#[derive(Default)]
struct Recorder {
    /// The table keys read, in order.
    written: Vec<KeyId>,
    customs: Vec<(String, String)>,
    /// The custom property whose value comes next.
    pending: Option<String>,
}

impl Recorder {
    fn key(&mut self, name: &str) {
        self.pending = custom_properties::is_name(name).then(|| name.to_owned());
        if let Some(&key) = COVERS.ids.get(name) {
            self.written.push(key);
        }
    }
}

/// Hands the derived `StyleDesc` deserialiser its input untouched, and notes which table keys it
/// reads and in what order: one pass over the input, with no tree built to look at it.
struct RecordKeys<'a, D> {
    inner: D,
    recorder: &'a mut Recorder,
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for RecordKeys<'_, D> {
    type Error = D::Error;

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_struct(name, fields, RecordMap { visitor, recorder: self.recorder })
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(visitor)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map enum identifier ignored_any
    }
}

struct RecordMap<'a, V> {
    visitor: V,
    recorder: &'a mut Recorder,
}

impl<'de, V: Visitor<'de>> Visitor<'de> for RecordMap<'_, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.visitor.expecting(formatter)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        self.visitor.visit_map(RecordedMap { map, recorder: self.recorder })
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        self.visitor.visit_seq(seq)
    }
}

struct RecordedMap<'a, A> {
    map: A,
    recorder: &'a mut Recorder,
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for RecordedMap<'_, A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        self.map.next_key_seed(KeySeed { seed, recorder: &mut *self.recorder })
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Self::Error> {
        match self.recorder.pending.take() {
            Some(name) => self.map.next_value_seed(CaptureSeed { seed, name, customs: &mut self.recorder.customs }),
            None => self.map.next_value_seed(seed),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        self.map.size_hint()
    }
}

struct KeySeed<'a, S> {
    seed: S,
    recorder: &'a mut Recorder,
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for KeySeed<'_, S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.seed.deserialize(RecordKey { inner: deserializer, recorder: self.recorder })
    }
}

struct RecordKey<'a, D> {
    inner: D,
    recorder: &'a mut Recorder,
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for RecordKey<'_, D> {
    type Error = D::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(RecordName { visitor, recorder: self.recorder })
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_identifier(RecordName { visitor, recorder: self.recorder })
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map struct enum ignored_any
    }
}

struct RecordName<'a, V> {
    visitor: V,
    recorder: &'a mut Recorder,
}

impl<'de, V: Visitor<'de>> Visitor<'de> for RecordName<'_, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.visitor.expecting(formatter)
    }

    fn visit_str<E: de::Error>(self, name: &str) -> Result<Self::Value, E> {
        self.recorder.key(name);
        self.visitor.visit_str(name)
    }

    fn visit_borrowed_str<E: de::Error>(self, name: &'de str) -> Result<Self::Value, E> {
        self.recorder.key(name);
        self.visitor.visit_borrowed_str(name)
    }

    fn visit_string<E: de::Error>(self, name: String) -> Result<Self::Value, E> {
        self.recorder.key(&name);
        self.visitor.visit_string(name)
    }

    fn visit_u64<E: de::Error>(self, index: u64) -> Result<Self::Value, E> {
        self.visitor.visit_u64(index)
    }

    fn visit_bytes<E: de::Error>(self, name: &[u8]) -> Result<Self::Value, E> {
        self.visitor.visit_bytes(name)
    }

    fn visit_borrowed_bytes<E: de::Error>(self, name: &'de [u8]) -> Result<Self::Value, E> {
        self.visitor.visit_borrowed_bytes(name)
    }

    fn visit_byte_buf<E: de::Error>(self, name: Vec<u8>) -> Result<Self::Value, E> {
        self.visitor.visit_byte_buf(name)
    }
}

/// A custom property's value: hands the derived deserialiser the value as the ignored one it
/// expects for a key it has no field for, and keeps its text.
struct CaptureSeed<'a, S> {
    seed: S,
    name: String,
    customs: &'a mut Vec<(String, String)>,
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for CaptureSeed<'_, S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.seed.deserialize(CaptureValue { inner: deserializer, name: self.name, customs: self.customs })
    }
}

struct CaptureValue<'a, D> {
    inner: D,
    name: String,
    customs: &'a mut Vec<(String, String)>,
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for CaptureValue<'_, D> {
    type Error = D::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(CaptureVisitor { visitor, name: self.name, customs: self.customs })
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_any(visitor)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier
    }
}

struct CaptureVisitor<'a, V> {
    visitor: V,
    name: String,
    customs: &'a mut Vec<(String, String)>,
}

impl<V> CaptureVisitor<'_, V> {
    fn keep(&mut self, text: String) {
        self.customs.push((std::mem::take(&mut self.name), text));
    }
}

impl<'de, V: Visitor<'de>> Visitor<'de> for CaptureVisitor<'_, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string or a number")
    }

    fn visit_str<E: de::Error>(mut self, text: &str) -> Result<Self::Value, E> {
        self.keep(text.to_owned());
        self.visitor.visit_str(text)
    }

    fn visit_borrowed_str<E: de::Error>(mut self, text: &'de str) -> Result<Self::Value, E> {
        self.keep(text.to_owned());
        self.visitor.visit_borrowed_str(text)
    }

    fn visit_string<E: de::Error>(mut self, text: String) -> Result<Self::Value, E> {
        self.keep(text.clone());
        self.visitor.visit_string(text)
    }

    fn visit_u64<E: de::Error>(mut self, number: u64) -> Result<Self::Value, E> {
        self.keep(number.to_string());
        self.visitor.visit_u64(number)
    }

    fn visit_i64<E: de::Error>(mut self, number: i64) -> Result<Self::Value, E> {
        self.keep(number.to_string());
        self.visitor.visit_i64(number)
    }

    fn visit_f64<E: de::Error>(mut self, number: f64) -> Result<Self::Value, E> {
        let text = serde_json::Number::from_f64(number)
            .ok_or_else(|| E::custom(format!("custom property {}: {number} is not a finite number", self.name)))?
            .to_string();
        self.keep(text);
        self.visitor.visit_f64(number)
    }

    // null is an unset property, as `undefined` is in a React style.
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.visitor.visit_unit()
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.visitor.visit_unit()
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Err(E::invalid_type(de::Unexpected::Bool(value), &self))
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, _: A) -> Result<Self::Value, A::Error> {
        Err(de::Error::invalid_type(de::Unexpected::Seq, &self))
    }

    fn visit_map<A: MapAccess<'de>>(self, _: A) -> Result<Self::Value, A::Error> {
        Err(de::Error::invalid_type(de::Unexpected::Map, &self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::length::LinearLength;

    fn parse(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap_or_else(|error| panic!("{json}: {error}"))
    }

    fn is_set(style: &StyleDesc, key: &str) -> bool {
        (COVERS.keys[COVERS.ids[key]].is_set)(style)
    }

    /// Each pair is a key that covers and one it covers, with a value for each.
    const PAIRS: &[(&str, &str, &str, &str)] = &[
        ("padding", "8", "paddingLeft", "4"),
        ("padding", "8", "paddingBlock", "4"),
        ("padding", "8", "paddingInline", "4"),
        ("paddingBlock", "8", "paddingTop", "4"),
        ("paddingInline", "8", "paddingRight", "4"),
        ("margin", "8", "marginLeft", "4"),
        ("margin", "8", "marginBlock", "4"),
        ("marginBlock", "8", "marginBottom", "4"),
        ("marginInline", "8", "marginLeft", "4"),
        ("inset", "8", "left", "4"),
        ("inset", "8", "insetInline", "4"),
        ("insetBlock", "8", "top", "4"),
        ("insetInline", "8", "right", "4"),
        ("border", r#""1px solid red""#, "borderTop", r#""2px solid blue""#),
        ("border", r#""1px solid red""#, "borderBlock", r#""2px solid blue""#),
        ("border", r#""1px solid red""#, "borderLeftWidth", "3"),
        ("border", r#""1px solid red""#, "borderWidth", "3"),
        ("border", r#""1px solid red""#, "borderColor", r#""blue""#),
        ("border", r#""1px solid red""#, "borderStyle", r#""dashed""#),
        ("borderBlock", r#""1px solid red""#, "borderTop", r#""2px solid blue""#),
        ("borderBlock", r#""1px solid red""#, "borderBottomColor", r#""blue""#),
        ("borderInline", r#""1px solid red""#, "borderLeftStyle", r#""dashed""#),
        ("borderTop", r#""1px solid red""#, "borderTopWidth", "3"),
        ("borderRight", r#""1px solid red""#, "borderRightColor", r#""blue""#),
        ("borderBottom", r#""1px solid red""#, "borderBottomStyle", r#""dashed""#),
        ("borderLeft", r#""1px solid red""#, "borderLeftWidth", "3"),
        ("borderWidth", "2", "borderTopWidth", "3"),
        ("borderColor", r#""red""#, "borderTopColor", r#""blue""#),
        ("borderStyle", r#""solid""#, "borderTopStyle", r#""dashed""#),
        ("borderRadius", "8", "borderTopLeftRadius", "4"),
        ("outline", r#""2px solid red""#, "outlineColor", r#""blue""#),
        ("flex", "1", "flexGrow", "2"),
        ("flex", "1", "flexBasis", "10"),
        ("flexFlow", r#""column wrap""#, "flexWrap", r#""nowrap""#),
        ("placeItems", r#""center""#, "alignItems", r#""start""#),
        ("placeContent", r#""center""#, "justifyContent", r#""start""#),
        ("placeSelf", r#""center""#, "alignSelf", r#""start""#),
        ("gap", "8", "rowGap", "4"),
        ("overflow", r#""hidden""#, "overflowX", r#""scroll""#),
        ("overscrollBehavior", r#""contain""#, "overscrollBehaviorY", r#""none""#),
        ("container", r#""card / inline-size""#, "containerType", r#""size""#),
        ("gridArea", r#""1 / 2 / 3 / 4""#, "gridColumn", r#""1 / 3""#),
        ("gridArea", r#""1 / 2 / 3 / 4""#, "gridRowEnd", "3"),
        ("gridColumn", r#""1 / 3""#, "gridColumnStart", "2"),
        ("gridRow", r#""1 / 3""#, "gridRowEnd", "2"),
        ("transition", r#""opacity 1s""#, "transitionDuration", r#""2s""#),
        ("animation", r#""spin 1s""#, "animationDuration", r#""2s""#),
        ("textDecoration", r#""underline""#, "textDecorationColor", r#""red""#),
        ("textBox", r#""trim-both cap alphabetic""#, "textBoxTrim", r#""trim-start""#),
        ("textWrap", r#""balance""#, "textWrapMode", r#""nowrap""#),
        ("textWrap", r#""balance""#, "textWrapStyle", r#""pretty""#),
        ("mask", r#""linear-gradient(red, blue)""#, "maskImage", r#""none""#),
        ("background", r#""red""#, "backgroundColor", r#""blue""#),
        ("background", r#""red""#, "backgroundImage", r#""none""#),
    ];

    #[test]
    fn the_key_written_later_wins_between_a_shorthand_and_what_it_covers() {
        for (shorthand, shorthand_value, longhand, longhand_value) in PAIRS {
            let after = parse(&format!("{{ \"{shorthand}\": {shorthand_value}, \"{longhand}\": {longhand_value} }}"));
            assert!(is_set(&after, shorthand), "{shorthand} before {longhand}");
            assert!(is_set(&after, longhand), "{longhand} after {shorthand} stays");

            let before = parse(&format!("{{ \"{longhand}\": {longhand_value}, \"{shorthand}\": {shorthand_value} }}"));
            assert!(is_set(&before, shorthand), "{shorthand} after {longhand}");
            assert!(!is_set(&before, longhand), "{longhand} before {shorthand} is covered");
        }
    }

    #[test]
    fn a_shorthand_covers_the_shorthands_it_covers_and_their_longhands() {
        let style = parse(r#"{ "paddingTop": 1, "paddingBlock": 2, "paddingLeft": 3, "padding": 8 }"#);
        assert_eq!(style.padding_top, None);
        assert_eq!(style.padding_block, None);
        assert_eq!(style.padding_left, None);

        let style = parse(r#"{ "paddingTop": 1, "padding": 8, "paddingBlock": 2 }"#);
        assert_eq!(style.padding_top, None, "the later padding covered it");
        assert!(style.padding_block.is_some());

        let style = parse(r#"{ "borderLeftColor": "red", "borderLeft": "1px solid", "borderTopWidth": 2, "border": "3px solid" }"#);
        assert!(style.border_left_color.is_none() && style.border_left.is_none() && style.border_top_width.is_none());
        assert!(style.border.is_some());
    }

    #[test]
    fn only_what_was_written_before_is_covered() {
        let style = parse(r#"{ "paddingTop": 1, "padding": 8, "paddingLeft": 3 }"#);
        assert_eq!(style.padding_top, None);
        assert_eq!(style.padding_left, Some(LinearLength::px(3.0)));
        assert!(style.padding.is_some());
    }

    #[test]
    fn nested_state_styles_follow_written_order_on_their_own() {
        let style = parse(r#"{ "paddingLeft": 4, "padding": 8, "hover": { "padding": 6, "paddingLeft": 2 } }"#);
        assert_eq!(style.padding_left, None);
        let hover = style.hover.expect("hover");
        assert_eq!(hover.padding_left, Some(LinearLength::px(2.0)));
    }

    #[test]
    fn an_invalid_shorthand_covers_nothing_as_css_drops_the_declaration() {
        // Each: a longhand, an invalid shorthand after it, and the longhand's key.
        for (longhand, shorthand, probe) in [
            (r#""borderTopWidth": 3"#, r#""border": "garbage""#, "borderTopWidth"),
            (r#""borderTopWidth": 3"#, r#""border": "1px solid not-a-colour""#, "borderTopWidth"),
            (r#""borderTopWidth": 3"#, r#""borderBlock": "1px 2px""#, "borderTopWidth"),
            (r#""borderLeftWidth": 3"#, r#""borderLeft": "solid solid""#, "borderLeftWidth"),
            (r#""borderTopColor": "red""#, r#""borderColor": "red bogus""#, "borderTopColor"),
            (r#""borderTopStyle": "solid""#, r#""borderStyle": "wavy""#, "borderTopStyle"),
            (r#""backgroundColor": "red""#, r#""background": "nonsense""#, "backgroundColor"),
            (r#""maskImage": "none""#, r#""mask": "bogus bogus""#, "maskImage"),
            (r#""textDecorationColor": "red""#, r#""textDecoration": "bogus""#, "textDecorationColor"),
            (r#""textBoxTrim": "trim-start""#, r#""textBox": "bogus""#, "textBoxTrim"),
        ] {
            let style = parse(&format!("{{ {longhand}, {shorthand} }}"));
            assert!(is_set(&style, probe), "{shorthand} after {longhand} is invalid and covers nothing");
        }
        let valid = parse(r#"{ "borderTopWidth": 3, "border": "1px solid red" }"#);
        assert!(!is_set(&valid, "borderTopWidth"));
    }

    #[test]
    fn a_typed_shorthand_with_an_invalid_value_fails_the_style_instead() {
        for json in [r#"{ "paddingLeft": 4, "padding": "1px foo" }"#, r#"{ "flexGrow": 1, "flex": "nonsense" }"#] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
    }

    #[test]
    fn a_pure_reorder_without_a_conflict_is_the_same_style() {
        assert_eq!(parse(r#"{ "color": "red", "left": 10 }"#), parse(r#"{ "left": 10, "color": "red" }"#));
    }

    #[test]
    fn a_border_for_all_sides_written_before_a_side_shorthand_keeps_the_other_sides() {
        let style = parse(r#"{ "borderWidth": 2, "borderColor": "red", "borderTop": "5px solid blue" }"#);
        assert_eq!(style.border_width, None);
        assert_eq!(style.border_color, None);
        assert_eq!(style.border_top_width, None, "the side shorthand sets its own");
        for width in [&style.border_right_width, &style.border_bottom_width, &style.border_left_width] {
            assert_eq!(width, &Some(LinearLength::px(2.0)));
        }
        assert_eq!(style.border_left_color.as_deref(), Some("red"));
        assert_eq!(style.border_top_color, None);
    }

    #[test]
    fn a_border_for_all_sides_written_after_a_side_shorthand_stays_whole() {
        let style = parse(r#"{ "borderTop": "5px solid blue", "borderColor": "red" }"#);
        assert_eq!(style.border_color.as_deref(), Some("red"));
        assert!(style.border_left_color.is_none());
    }

    #[test]
    fn a_side_shorthand_only_spreads_the_all_sides_values_written_before_it() {
        let style = parse(r#"{ "borderWidth": 2, "borderTop": "5px solid blue", "borderColor": "red" }"#);
        assert_eq!(style.border_color.as_deref(), Some("red"), "written after: still whole");
        assert_eq!(style.border_width, None);
        assert_eq!(style.border_left_width, Some(LinearLength::px(2.0)));
    }

    #[test]
    fn a_second_side_shorthand_covers_a_width_the_first_spread_to_its_side() {
        let style = parse(r#"{ "borderWidth": 2, "borderTop": "5px solid blue", "borderBottom": "7px solid blue" }"#);
        assert_eq!(style.border_bottom_width, None);
        assert_eq!(style.border_left_width, Some(LinearLength::px(2.0)));
    }

    #[test]
    fn the_table_closes_over_nested_shorthands() {
        let border = COVERS.ids["border"];
        let covered: Vec<&str> = COVERS.covered[border].iter().map(|&key| COVERS.keys[key].name).collect();
        for name in ["border_top_width", "border_left_style", "border_block", "border_top", "border_color"] {
            assert!(covered.contains(&name), "border covers {name}");
        }
        assert!(!covered.contains(&"outline_color"));
    }

    #[test]
    fn custom_properties_are_collected_beside_the_named_fields() {
        let style = parse(r#"{ "--gap": " 8px ", "padding": 4, "--n": 2.5, "--int": 3, "--unset": null, "--empty": "" }"#);
        let custom = style.custom_properties.as_ref().unwrap();
        assert_eq!((custom.get("--gap"), custom.get("--n"), custom.get("--int")), (Some("8px"), Some("2.5"), Some("3")));
        assert_eq!((custom.get("--unset"), custom.get("--empty")), (None, None));
        assert!(style.padding.is_some());
        assert_eq!(parse(r#"{ "padding": 4 }"#).custom_properties, None);
        // A key like a named field's is not one, and an unknown key stays ignored.
        assert_eq!(parse(r#"{ "-x": 1, "--": 1, "unknown": 1 }"#).custom_properties, None);
    }

    #[test]
    fn custom_properties_are_read_in_state_styles_and_container_blocks_and_written_back() {
        let json = r##"{ "--a": "1", "hover": { "--a": "2" }, "@container": { "(min-width: 1px)": { "--b": "3" } } }"##;
        let style = parse(json);
        assert_eq!(style.hover.as_ref().unwrap().custom_properties.as_ref().unwrap().get("--a"), Some("2"));
        let again: StyleDesc = serde_json::from_str(&serde_json::to_string(&style).unwrap()).unwrap();
        assert_eq!(again, style);
        assert_eq!(again.custom_properties.as_ref().unwrap().get("--a"), Some("1"));
    }

    #[test]
    fn a_custom_property_must_be_a_string_or_a_number() {
        for json in [r#"{ "--a": true }"#, r#"{ "--a": [1] }"#, r#"{ "--a": { "b": 1 } }"#] {
            assert!(serde_json::from_str::<StyleDesc>(json).is_err(), "{json}");
        }
    }
}
