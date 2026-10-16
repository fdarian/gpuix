//! The grid properties of `StyleDesc`, parsed when the style is deserialized.
//!
//! Each value keeps the text it came from, so serializing a style (the
//! automation tree dump) shows what the app wrote.

use std::sync::Arc;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use super::grid;
use crate::css::length::LengthContext;

enum Input<'a> {
    Number(f64),
    Text(&'a str),
}

fn deserialize_input<'de, D, T>(
    deserializer: D,
    expecting: &'static str,
    build: impl Fn(Input) -> Result<T, String>,
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
{
    struct InputVisitor<F>(&'static str, F);

    impl<'de, T, F: Fn(Input) -> Result<T, String>> Visitor<'de> for InputVisitor<F> {
        type Value = T;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str(self.0)
        }

        fn visit_f64<E: de::Error>(self, v: f64) -> Result<T, E> {
            (self.1)(Input::Number(v)).map_err(E::custom)
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<T, E> {
            (self.1)(Input::Number(v as f64)).map_err(E::custom)
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<T, E> {
            (self.1)(Input::Number(v as f64)).map_err(E::custom)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<T, E> {
            (self.1)(Input::Text(v)).map_err(E::custom)
        }
    }

    deserializer.deserialize_any(InputVisitor(expecting, build))
}

fn number_as_line(number: f64) -> Result<gpui::GridPlacement, String> {
    if number.fract() != 0.0 || number == 0.0 || number.abs() > i16::MAX as f64 {
        return Err(format!("invalid grid line {number}"));
    }
    Ok(gpui::GridPlacement::Line(number as i16))
}

/// The context a track list is checked in when it is deserialized. Whether a
/// list is valid never depends on the context, only the px of its relative
/// lengths do.
fn validation_context() -> LengthContext {
    LengthContext::initial(0.0, 0.0)
}

const REPARSE: &str = "a track list that parsed once parses in every context";

/// `gridTemplateColumns` / `gridTemplateRows`: a CSS track list, or a count `n`
/// meaning `repeat(n, minmax(0, 1fr))`.
#[derive(Debug, Clone, PartialEq)]
pub enum GridTemplateValue {
    Count(u16),
    Tracks(GridTemplateList),
}

/// A track list and the text it was parsed from.
#[derive(Debug, Clone, PartialEq)]
pub struct GridTemplateList {
    source: String,
    /// Parsed in [`validation_context`], so only final when `relative` is false.
    tracks: Arc<gpui::GridTemplateTracks>,
    relative: bool,
}

impl GridTemplateList {
    /// The tracks with their `em`, `rem` and viewport lengths resolved in
    /// `lengths`, the context of the element the list belongs to.
    pub(crate) fn resolve(&self, lengths: &LengthContext) -> gpui::GridTemplateTracks {
        if !self.relative {
            return self.tracks.as_ref().clone();
        }
        grid::parse_template(&self.source, lengths).expect(REPARSE).value
    }
}

impl Serialize for GridTemplateValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Count(count) => serializer.serialize_u16(*count),
            Self::Tracks(list) => serializer.serialize_str(&list.source),
        }
    }
}

impl<'de> Deserialize<'de> for GridTemplateValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_input(
            deserializer,
            "a column count or a CSS track list such as 'repeat(3, 1fr)'",
            |input| match input {
                // Existing behaviour: a count is clamped, not rejected.
                Input::Number(count) => Ok(Self::Count(count.round().clamp(1.0, 64.0) as u16)),
                Input::Text(text) => {
                    let parsed = grid::parse_template(text, &validation_context())?;
                    Ok(Self::Tracks(GridTemplateList {
                        source: text.to_string(),
                        tracks: Arc::new(parsed.value),
                        relative: parsed.relative,
                    }))
                }
            },
        )
    }
}

/// `gridAutoColumns` / `gridAutoRows`.
#[derive(Debug, Clone, PartialEq)]
pub struct GridAutoTracks {
    source: String,
    /// Parsed in [`validation_context`], so only final when `relative` is false.
    tracks: Arc<Vec<gpui::GridTrack>>,
    relative: bool,
}

impl GridAutoTracks {
    /// See [`GridTemplateList::resolve`].
    pub(crate) fn resolve(&self, lengths: &LengthContext) -> Vec<gpui::GridTrack> {
        if !self.relative {
            return self.tracks.as_ref().clone();
        }
        grid::parse_auto_tracks(&self.source, lengths).expect(REPARSE).value
    }
}

impl Serialize for GridAutoTracks {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.source)
    }
}

impl<'de> Deserialize<'de> for GridAutoTracks {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_input(
            deserializer,
            "a pixel size or a CSS track size list",
            |input| {
                let source = match input {
                    Input::Number(pixels) => format!("{pixels}px"),
                    Input::Text(text) => text.to_string(),
                };
                let parsed = grid::parse_auto_tracks(&source, &validation_context())?;
                Ok(Self {
                    source,
                    tracks: Arc::new(parsed.value),
                    relative: parsed.relative,
                })
            },
        )
    }
}

/// `gridColumnStart` and its three siblings.
#[derive(Debug, Clone, PartialEq)]
pub struct GridLineValue {
    source: String,
    pub line: gpui::GridPlacement,
}

impl Serialize for GridLineValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.source)
    }
}

impl<'de> Deserialize<'de> for GridLineValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_input(deserializer, "a grid line such as 2, 'span 2' or 'name'", |input| {
            Ok(match input {
                Input::Number(number) => Self {
                    source: number.to_string(),
                    line: number_as_line(number)?,
                },
                Input::Text(text) => Self {
                    source: text.to_string(),
                    line: grid::parse_line(text)?,
                },
            })
        })
    }
}

/// `gridColumn` / `gridRow`: `start` or `start / end`.
#[derive(Debug, Clone, PartialEq)]
pub struct GridLinePairValue {
    source: String,
    pub start: gpui::GridPlacement,
    pub end: gpui::GridPlacement,
}

impl Serialize for GridLinePairValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.source)
    }
}

impl<'de> Deserialize<'de> for GridLinePairValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_input(
            deserializer,
            "a grid line pair such as '1 / 3', 'span 2' or 2",
            |input| {
                Ok(match input {
                    Input::Number(number) => Self {
                        source: number.to_string(),
                        start: number_as_line(number)?,
                        end: gpui::GridPlacement::Auto,
                    },
                    Input::Text(text) => {
                        let (start, end) = grid::parse_line_pair(text)?;
                        Self {
                            source: text.to_string(),
                            start,
                            end,
                        }
                    }
                })
            },
        )
    }
}

/// `gridArea`.
#[derive(Debug, Clone, PartialEq)]
pub struct GridAreaValue {
    source: String,
    pub row: (gpui::GridPlacement, gpui::GridPlacement),
    pub column: (gpui::GridPlacement, gpui::GridPlacement),
}

impl Serialize for GridAreaValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.source)
    }
}

impl<'de> Deserialize<'de> for GridAreaValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_input(
            deserializer,
            "a grid area name or 'row-start / column-start / row-end / column-end'",
            |input| {
                let source = match input {
                    Input::Number(number) => number.to_string(),
                    Input::Text(text) => text.to_string(),
                };
                let [row, column] = grid::parse_area(&source)?;
                Ok(Self {
                    source,
                    row,
                    column,
                })
            },
        )
    }
}

/// `gridTemplateAreas`.
#[derive(Debug, Clone, PartialEq)]
pub struct GridTemplateAreasValue {
    source: String,
    pub areas: Arc<gpui::GridTemplateAreas>,
}

impl Serialize for GridTemplateAreasValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.source)
    }
}

impl<'de> Deserialize<'de> for GridTemplateAreasValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_input(
            deserializer,
            "quoted rows of area names such as '\"a a\" \"b c\"'",
            |input| match input {
                Input::Number(_) => Err("grid-template-areas needs quoted rows".to_string()),
                Input::Text(text) => Ok(Self {
                    source: text.to_string(),
                    areas: Arc::new(grid::parse_template_areas(text)?),
                }),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(text: &str) -> GridTemplateValue {
        serde_json::from_value(serde_json::json!(text)).unwrap()
    }

    fn list(text: &str) -> GridTemplateList {
        match template(text) {
            GridTemplateValue::Tracks(list) => list,
            GridTemplateValue::Count(_) => panic!("{text} is a track list"),
        }
    }

    fn first_track(tracks: &gpui::GridTemplateTracks) -> gpui::GridTrackSize {
        match &tracks.entries[0] {
            gpui::GridTemplateEntry::Track(track) => track.min,
            entry => panic!("expected a track, got {entry:?}"),
        }
    }

    #[test]
    fn a_relative_list_resolves_in_each_context_it_is_applied_in() {
        let list = list("10em 1fr");
        let small = LengthContext::initial(100.0, 100.0);
        assert_eq!(first_track(&list.resolve(&small)), gpui::GridTrackSize::Length(160.0));
        assert_eq!(
            first_track(&list.resolve(&LengthContext::for_tests())),
            gpui::GridTrackSize::Length(200.0)
        );
    }

    #[test]
    fn an_absolute_list_is_the_same_in_every_context() {
        let list = list("10px 1fr");
        assert!(!list.relative);
        assert_eq!(
            list.resolve(&LengthContext::for_tests()),
            list.resolve(&LengthContext::initial(1.0, 1.0))
        );
    }

    #[test]
    fn an_invalid_list_fails_to_deserialize() {
        assert!(serde_json::from_value::<GridTemplateValue>(serde_json::json!("-1em")).is_err());
        assert!(serde_json::from_value::<GridAutoTracks>(serde_json::json!("calc(1em")).is_err());
        assert!(serde_json::from_value::<GridTemplateValue>(serde_json::json!("fit-content(calc(10% + 1em))")).is_err());
    }

    #[test]
    fn the_source_is_kept_for_the_automation_dump() {
        assert_eq!(
            serde_json::to_value(template("repeat(2, 5em)")).unwrap(),
            serde_json::json!("repeat(2, 5em)")
        );
        let auto: GridAutoTracks = serde_json::from_value(serde_json::json!("calc(10vw)")).unwrap();
        assert_eq!(serde_json::to_value(&auto).unwrap(), serde_json::json!("calc(10vw)"));
    }

    #[test]
    fn an_auto_track_number_is_px() {
        let auto: GridAutoTracks = serde_json::from_value(serde_json::json!(40)).unwrap();
        assert!(!auto.relative);
        assert_eq!(
            auto.resolve(&LengthContext::for_tests()),
            vec![gpui::GridTrack {
                min: gpui::GridTrackSize::Length(40.0),
                max: gpui::GridTrackSize::Length(40.0)
            }]
        );
    }
}
