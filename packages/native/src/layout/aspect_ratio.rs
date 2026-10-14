use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// CSS `aspect-ratio`: width divided by height. A number, or a string such as
/// `"16 / 9"`, `"1.5"` or `"auto 4 / 3"`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AspectRatio {
    /// `auto`, or a degenerate ratio, which CSS also treats as `auto`.
    Auto,
    Ratio(f64),
}

impl AspectRatio {
    /// The ratio to lay out with, if the style asks for one.
    pub fn ratio(self) -> Option<f64> {
        match self {
            Self::Ratio(ratio) => Some(ratio),
            Self::Auto => None,
        }
    }

    fn from_numbers(width: f64, height: f64) -> Self {
        let ratio = width / height;
        if ratio.is_finite() && ratio > 0.0 {
            Self::Ratio(ratio)
        } else {
            Self::Auto
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        let invalid = || format!("invalid aspect ratio: {value:?}");
        let mut rest = value.trim();
        // `auto <ratio>` only differs from `<ratio>` for boxes with an intrinsic
        // ratio, and the intrinsic ratio is the fallback here anyway.
        if let Some(stripped) = rest.strip_prefix("auto") {
            rest = stripped.trim_start();
            if rest.is_empty() {
                return Ok(Self::Auto);
            }
        }
        let number = |text: &str| text.trim().parse::<f64>().map_err(|_| invalid());
        let (width, height) = match rest.split_once('/') {
            Some((width, height)) => (number(width)?, number(height)?),
            None => (number(rest)?, 1.0),
        };
        if width < 0.0 || height < 0.0 {
            return Err(invalid());
        }
        Ok(Self::from_numbers(width, height))
    }
}

impl Serialize for AspectRatio {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Ratio(ratio) => serializer.serialize_f64(*ratio),
        }
    }
}

impl<'de> Deserialize<'de> for AspectRatio {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AspectRatioVisitor;

        impl<'de> Visitor<'de> for AspectRatioVisitor {
            type Value = AspectRatio;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a number or a string like '16 / 9'")
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(AspectRatio::from_numbers(v, 1.0))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(AspectRatio::from_numbers(v as f64, 1.0))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(AspectRatio::from_numbers(v as f64, 1.0))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                AspectRatio::parse(v).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(AspectRatioVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(value: &str) -> AspectRatio {
        AspectRatio::parse(value).unwrap()
    }

    #[test]
    fn parses_css_ratios() {
        assert_eq!(parse("16 / 9"), AspectRatio::Ratio(16.0 / 9.0));
        assert_eq!(parse("16/9"), AspectRatio::Ratio(16.0 / 9.0));
        assert_eq!(parse("1"), AspectRatio::Ratio(1.0));
        assert_eq!(parse("1.5"), AspectRatio::Ratio(1.5));
        assert_eq!(parse("auto 4 / 3"), AspectRatio::Ratio(4.0 / 3.0));
        assert_eq!(parse("auto"), AspectRatio::Auto);
    }

    #[test]
    fn a_degenerate_ratio_is_auto() {
        assert_eq!(parse("0 / 1"), AspectRatio::Auto);
        assert_eq!(parse("1 / 0"), AspectRatio::Auto);
    }

    #[test]
    fn rejects_garbage() {
        assert!(AspectRatio::parse("wide").is_err());
        assert!(AspectRatio::parse("16 / x").is_err());
        assert!(AspectRatio::parse("-1").is_err());
        assert!(AspectRatio::parse("").is_err());
    }

    #[test]
    fn deserializes_numbers_and_strings() {
        let number: AspectRatio = serde_json::from_str("1.5").unwrap();
        let string: AspectRatio = serde_json::from_str("\"3 / 2\"").unwrap();
        assert_eq!(number, string);
    }
}
