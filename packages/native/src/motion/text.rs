//! Numbers as Chrome writes them in the values `getKeyframes()` and `getTiming()` read.

/// Which effect a text is read for: the keyframes of an animation hold the values as they were written,
/// the endpoints of a transition hold them as they interpolate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Text {
    Keyframe,
    Transition,
}

/// A number with the digits that tell it from its neighbours and no more, as JavaScript writes it: an
/// easing function keeps the whole double (`0.123456789`), and a very small or large one takes an
/// exponent (`1e-7`).
pub(crate) fn shortest(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    // `{:e}` gives the shortest digits that read back as the same double.
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("a float in scientific notation has an exponent");
    let digits: String = mantissa.chars().filter(|digit| *digit != '.').collect();
    let exponent: i32 = exponent.parse().expect("the exponent of a float is a number");
    let sign = if value < 0.0 { "-" } else { "" };
    let count = digits.len() as i32;
    let body = if (0..21).contains(&exponent) {
        if count <= exponent + 1 {
            format!("{digits}{}", "0".repeat((exponent + 1 - count) as usize))
        } else {
            format!("{}.{}", &digits[..(exponent + 1) as usize], &digits[(exponent + 1) as usize..])
        }
    } else if (-6..0).contains(&exponent) {
        format!("0.{}{digits}", "0".repeat((-exponent - 1) as usize))
    } else {
        let tail = if count > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        format!("{}{tail}e{}{}", &digits[..1], if exponent < 0 { "-" } else { "+" }, exponent.abs())
    };
    format!("{sign}{body}")
}

/// A CSS number to six significant digits with no trailing zeros, the precision Chrome serialises
/// lengths, angles, opacities and colour components to.
pub(crate) fn six_digits(value: f64) -> String {
    if value == 0.0 || !value.is_finite() {
        return "0".to_string();
    }
    let scientific = format!("{:.5e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("a float in scientific notation has an exponent");
    let digits: String = mantissa.chars().filter(|digit| *digit != '.').collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let exponent: i32 = exponent.parse().expect("the exponent of a float is a number");
    let sign = if value < 0.0 { "-" } else { "" };
    let count = digits.len() as i32;
    let body = if (0..21).contains(&exponent) {
        if count <= exponent + 1 {
            format!("{digits}{}", "0".repeat((exponent + 1 - count) as usize))
        } else {
            format!("{}.{}", &digits[..(exponent + 1) as usize], &digits[(exponent + 1) as usize..])
        }
    } else if (-6..0).contains(&exponent) {
        format!("0.{}{digits}", "0".repeat((-exponent - 1) as usize))
    } else {
        let tail = if count > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        format!("{}{tail}e{}{}", &digits[..1], if exponent < 0 { "-" } else { "+" }, exponent.abs())
    };
    format!("{sign}{body}")
}

/// `value` px at six digits.
pub(crate) fn pixels(value: f64) -> String {
    format!("{}px", six_digits(value))
}

/// A length of `px` and a `fraction` of its basis as Chrome writes it: `10px`, `50%`, or `calc()`
/// with the percentage first.
pub(crate) fn length(px: f64, fraction: f64) -> String {
    let percent = fraction * 100.0;
    match (px == 0.0, percent == 0.0) {
        (true, true) => "0px".to_string(),
        (_, true) => pixels(px),
        (true, _) => format!("{}%", six_digits(percent)),
        _ => {
            let sign = if px < 0.0 { '-' } else { '+' };
            format!("calc({}% {sign} {})", six_digits(percent), pixels(px.abs()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shortest_form_is_javascripts() {
        assert_eq!(shortest(0.0), "0");
        assert_eq!(shortest(1.0), "1");
        assert_eq!(shortest(0.25), "0.25");
        assert_eq!(shortest(-0.5), "-0.5");
        assert_eq!(shortest(0.123456789), "0.123456789");
        assert_eq!(shortest(1e-7), "1e-7");
        assert_eq!(shortest(0.000001), "0.000001");
        assert_eq!(shortest(12345.0), "12345");
        assert_eq!(shortest(1.5e21), "1.5e+21");
    }

    #[test]
    fn six_digits_drop_the_zeros_and_keep_the_magnitude() {
        assert_eq!(six_digits(0.0), "0");
        assert_eq!(six_digits(57.29577951308232), "57.2958");
        assert_eq!(six_digits(2.123456789), "2.12346");
        assert_eq!(six_digits(0.333333333), "0.333333");
        assert_eq!(six_digits(100.0), "100");
        assert_eq!(six_digits(-3.5), "-3.5");
        assert_eq!(six_digits(0.0000362098), "0.0000362098");
        assert_eq!(six_digits(-6.853946e-9), "-6.85395e-9");
        assert_eq!(six_digits(1234567.0), "1234570");
        assert_eq!(pixels(14.79687), "14.7969px");
        assert_eq!(length(0.0, 0.0), "0px");
        assert_eq!(length(10.0, 0.0), "10px");
        assert_eq!(length(0.0, 0.5), "50%");
        assert_eq!(length(10.0, 0.05), "calc(5% + 10px)");
        assert_eq!(length(-20.0, 1.0), "calc(100% - 20px)");
    }
}
