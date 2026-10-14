//! The grid properties of `StyleDesc`, parsed when the style is deserialized.
//!
//! Each value keeps the text it came from, so serializing a style (the
//! automation tree dump) shows what the app wrote.

use std::sync::Arc;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use super::grid;

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

/// `gridTemplateColumns` / `gridTemplateRows`: a CSS track list, or a count `n`
/// meaning `repeat(n, minmax(0, 1fr))`.
#[derive(Debug, Clone, PartialEq)]
pub enum GridTemplateValue {
    Count(u16),
    Tracks {
        source: String,
        tracks: Arc<gpui::GridTemplateTracks>,
    },
}

impl Serialize for GridTemplateValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Count(count) => serializer.serialize_u16(*count),
            Self::Tracks { source, .. } => serializer.serialize_str(source),
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
                Input::Text(text) => Ok(Self::Tracks {
                    source: text.to_string(),
                    tracks: Arc::new(grid::parse_template(text)?),
                }),
            },
        )
    }
}

/// `gridAutoColumns` / `gridAutoRows`.
#[derive(Debug, Clone, PartialEq)]
pub struct GridAutoTracks {
    source: String,
    pub tracks: Arc<Vec<gpui::GridTrack>>,
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
                let tracks = Arc::new(grid::parse_auto_tracks(&source)?);
                Ok(Self { source, tracks })
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
