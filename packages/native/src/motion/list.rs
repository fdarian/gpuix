//! A motion value that holds a multi-value CSS string: `"0.5em 0.75em"`,
//! `"10px / 20px"`, `"red blue"`. Each component is a length (a number, a
//! percentage or a `calc()` of the two) or a colour, and they blend one by one,
//! as framer-motion blends the numbers and colours of a complex string.

use super::value::{align_in, AnimValue, Aligned, Interpolation, Layout};
use crate::css::length::LinearLength;
use crate::css::syntax::{expand_sides, split_slash_tokens};

/// The components of a multi-value string, with where its `/` stands (the
/// number of components before it) for a `borderRadius`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ListValue {
    /// Lengths and colours: [`AnimValue::Number`], [`AnimValue::Percent`],
    /// [`AnimValue::Calc`], [`AnimValue::Relative`] or [`AnimValue::Color`].
    pub(super) items: Vec<AnimValue>,
    pub(super) slash: Option<usize>,
}

impl ListValue {
    /// `text` as components, each read by `item`, which gives `None` for a token
    /// that is no length or colour. Only a `borderRadius` has a `/`, so one is
    /// taken between lengths only.
    pub(super) fn parse(
        text: &str,
        mut item: impl FnMut(&str) -> Result<Option<AnimValue>, String>,
    ) -> Result<Self, String> {
        let mut items = Vec::new();
        let mut slash = None;
        for token in split_slash_tokens(text) {
            if token == "/" {
                if slash.is_some() || items.is_empty() {
                    return Err(format!("a stray / in \"{text}\""));
                }
                slash = Some(items.len());
                continue;
            }
            items.push(item(token)?.ok_or_else(|| format!("\"{token}\" is neither a CSS length nor a colour"))?);
        }
        if slash == Some(items.len()) {
            return Err(format!("nothing after the / in \"{text}\""));
        }
        let list = Self { items, slash };
        if list.slash.is_some() && list.items.iter().any(|item| item.as_terms().is_none()) {
            return Err(format!("only lengths can stand on either side of the / in \"{text}\""));
        }
        Ok(list)
    }

    pub(super) fn horizontal(&self) -> &[AnimValue] {
        &self.items[..self.slash.unwrap_or(self.items.len())]
    }

    pub(super) fn vertical(&self) -> Option<&[AnimValue]> {
        self.slash.map(|slash| &self.items[slash..])
    }

    /// The string the value reads back as: px, percentages and `calc()`s for
    /// lengths, `rgba()` for colours.
    pub(super) fn to_css(&self) -> Result<String, String> {
        let mut text = String::new();
        for (index, item) in self.items.iter().enumerate() {
            if index > 0 {
                text.push(' ');
            }
            if self.slash == Some(index) {
                text.push_str("/ ");
            }
            match item {
                AnimValue::Color(color) => text.push_str(&color.to_css()),
                item => {
                    let [px, percent, em, rem] = item.as_terms().ok_or_else(|| format!("a list cannot hold {item:?}"))?;
                    text.push_str(&LinearLength { px, percent, em, rem, ..Default::default() }.to_string());
                }
            }
        }
        Ok(text)
    }
}

/// A value as the components of a list: a lone length or colour is a list of one.
fn components(value: &AnimValue) -> Option<(&[AnimValue], Option<&[AnimValue]>)> {
    match value {
        AnimValue::List(list) => Some((list.horizontal(), list.vertical())),
        AnimValue::Number(_)
        | AnimValue::Percent(_)
        | AnimValue::Calc { .. }
        | AnimValue::Relative { .. }
        | AnimValue::Color(_) => {
            Some((std::slice::from_ref(value), None))
        }
        _ => None,
    }
}

/// `items` as `count` components by the rule a one to four value shorthand fills
/// its missing sides with, which is also how a `gap` of one value is two. `None`
/// where that rule says nothing: either side has more than four.
fn expand(items: &[AnimValue], count: usize) -> Option<Vec<AnimValue>> {
    if items.len() == count {
        return Some(items.to_vec());
    }
    if count > 4 {
        return None;
    }
    let mut sides = expand_sides(items)?.to_vec();
    sides.truncate(count);
    Some(sides)
}

/// Several values, at least one a list, in one channel layout. A value with
/// fewer components than another is first filled out by the shorthand rule
/// (`"8px"` against `"8px 12px"` is `"8px 8px"`), because the motion value does
/// not know which style property, if any, it will be bound to, and every
/// shorthand it can bind to fills a short value out the same way. Values whose
/// component counts that cannot reconcile jump at the halfway point, as
/// framer-motion jumps between complex strings of different shape.
pub(super) fn align(
    name: &str,
    values: &[&AnimValue],
    interpolation: Interpolation,
) -> Result<Aligned, String> {
    let mixed = |value: &AnimValue| {
        format!("motion {name} cannot animate a list with {value:?}: mixed units are not supported")
    };
    let parts = values
        .iter()
        .map(|value| components(value).ok_or_else(|| mixed(value)))
        .collect::<Result<Vec<_>, _>>()?;

    let has_slash = parts.iter().any(|(_, vertical)| vertical.is_some());
    // A radius with no `/` has the same vertical radii as horizontal.
    let horizontal_count = parts.iter().map(|(horizontal, _)| horizontal.len()).max().unwrap_or(0);
    let vertical_count = parts
        .iter()
        .map(|(horizontal, vertical)| vertical.unwrap_or(horizontal).len())
        .max()
        .unwrap_or(0);

    let expanded: Option<Vec<Vec<AnimValue>>> = parts
        .iter()
        .map(|(horizontal, vertical)| {
            let mut items = expand(horizontal, horizontal_count)?;
            if has_slash {
                items.extend(expand(vertical.unwrap_or(horizontal), vertical_count)?);
            }
            Some(items)
        })
        .collect();
    let Some(expanded) = expanded else {
        return Ok(Aligned {
            layout: Layout::DiscreteList(values.iter().map(|value| (*value).clone()).collect()),
            frames: (0..values.len()).map(|index| vec![index as f64]).collect(),
        });
    };

    let mut layouts = Vec::new();
    let mut frames = vec![Vec::new(); values.len()];
    for position in 0..expanded[0].len() {
        let column: Vec<&AnimValue> = expanded.iter().map(|items| &items[position]).collect();
        let aligned = align_in(name, &column, interpolation)?;
        layouts.push((aligned.layout, aligned.frames[0].len()));
        for (frame, channels) in frames.iter_mut().zip(aligned.frames) {
            frame.extend(channels);
        }
    }
    Ok(Aligned {
        layout: Layout::List { parts: layouts, slash: has_slash.then_some(horizontal_count) },
        frames,
    })
}
