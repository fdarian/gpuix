//! The CSS `url()` token: `url(a.png)`, `url("a.png")`, `url( 'a.png' )`.
//!
//! The splitters in `syntax.rs` treat a url token as opaque, because a quoted
//! address can hold the `)`, `,` and whitespace they would split at.

use std::borrow::Cow;

#[derive(Debug, PartialEq)]
pub(crate) enum Parsed<'a> {
    NotUrl,
    /// Starts like a url but is not one: unclosed, or something after the quote.
    Invalid,
    /// The address, unquoted and unescaped. `url()` is the empty address.
    Url(Cow<'a, str>),
}

pub(crate) fn parse(item: &str) -> Parsed<'_> {
    let item = item.trim();
    let Some(body) = strip_url_open(item) else {
        return Parsed::NotUrl;
    };
    match scan(body) {
        Some(Scanned { address, end }) if body[end..].trim().is_empty() => {
            Parsed::Url(unescape(address))
        }
        _ => Parsed::Invalid,
    }
}

/// Where the url token starting at byte `start` of `value` ends, if there is one:
/// the index after its closing parenthesis. A `url(` inside an identifier
/// (`my-url(`) is not a token.
pub(crate) fn token_end(value: &str, start: usize) -> Option<usize> {
    let before = value[..start].chars().next_back();
    if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    let body = strip_url_open(&value[start..])?;
    let opened = value.len() - body.len();
    scan(body).map(|scanned| opened + scanned.end)
}

fn strip_url_open(text: &str) -> Option<&str> {
    let head = text.get(..4)?;
    head.eq_ignore_ascii_case("url(").then(|| &text[4..])
}

struct Scanned<'a> {
    /// The address as written, between its quotes if it has them.
    address: &'a str,
    /// The index after the closing parenthesis.
    end: usize,
}

/// Reads the address and the `)` after it from the text following `url(`.
fn scan(body: &str) -> Option<Scanned<'_>> {
    let trimmed = body.trim_start();
    let skipped = body.len() - trimmed.len();
    let quote = trimmed.chars().next().filter(|c| matches!(c, '"' | '\''));
    if let Some(quote) = quote {
        let content = &trimmed[1..];
        let close = closing_quote(content, quote)?;
        let after = &content[close + 1..];
        let after_trimmed = after.trim_start();
        if !after_trimmed.starts_with(')') {
            return None;
        }
        let end = body.len() - after_trimmed.len() + 1;
        return Some(Scanned { address: &content[..close], end });
    }
    let close = closing_paren(trimmed)?;
    Some(Scanned { address: trimmed[..close].trim_end(), end: skipped + close + 1 })
}

/// The index of the first `quote` that is not escaped. A raw newline ends the
/// string unclosed, as in CSS.
fn closing_quote(text: &str, quote: char) -> Option<usize> {
    let mut chars = text.char_indices();
    while let Some((index, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '\n' => return None,
            c if c == quote => return Some(index),
            _ => {}
        }
    }
    None
}

fn closing_paren(text: &str) -> Option<usize> {
    let mut chars = text.char_indices();
    while let Some((index, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            ')' => return Some(index),
            _ => {}
        }
    }
    None
}

/// CSS escapes: `\` and a hex number (up to six digits, one space after), or `\`
/// and any other character, which stands for itself. `\` and a newline is
/// nothing, inside a string.
fn unescape(address: &str) -> Cow<'_, str> {
    if !address.contains('\\') {
        return Cow::Borrowed(address);
    }
    let mut out = String::with_capacity(address.len());
    let mut chars = address.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            None => out.push('\u{FFFD}'),
            Some('\n') => {
                chars.next();
            }
            Some(next) if next.is_ascii_hexdigit() => {
                let mut value = 0u32;
                let mut digits = 0;
                while digits < 6 {
                    let Some(digit) = chars.peek().and_then(|c| c.to_digit(16)) else {
                        break;
                    };
                    value = value * 16 + digit;
                    chars.next();
                    digits += 1;
                }
                if chars.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                    chars.next();
                }
                out.push(char::from_u32(value).filter(|c| *c != '\0').unwrap_or('\u{FFFD}'));
            }
            Some(next) => {
                out.push(next);
                chars.next();
            }
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(item: &str) -> Option<String> {
        match parse(item) {
            Parsed::Url(address) => Some(address.into_owned()),
            _ => None,
        }
    }

    #[test]
    fn the_three_quoting_forms() {
        assert_eq!(url("url(a.png)").as_deref(), Some("a.png"));
        assert_eq!(url("url(\"a.png\")").as_deref(), Some("a.png"));
        assert_eq!(url("url('a.png')").as_deref(), Some("a.png"));
        assert_eq!(url("URL( a.png )").as_deref(), Some("a.png"));
        assert_eq!(url("url(  \"a.png\"  )").as_deref(), Some("a.png"));
        assert_eq!(url("url()").as_deref(), Some(""));
    }

    #[test]
    fn a_quoted_address_may_hold_parentheses_commas_and_spaces() {
        assert_eq!(url("url(\"a)b, c.png\")").as_deref(), Some("a)b, c.png"));
        assert_eq!(url("url('a (1).png')").as_deref(), Some("a (1).png"));
    }

    #[test]
    fn escapes_are_resolved() {
        assert_eq!(url(r#"url("a\"b.png")"#).as_deref(), Some("a\"b.png"));
        assert_eq!(url(r"url('a\'b.png')").as_deref(), Some("a'b.png"));
        assert_eq!(url(r"url(a\)b.png)").as_deref(), Some("a)b.png"));
        assert_eq!(url(r#"url("a\29 b")"#).as_deref(), Some("a)b"));
        assert_eq!(url(r#"url("\\")"#).as_deref(), Some("\\"));
    }

    #[test]
    fn data_urls_with_commas_are_one_address() {
        assert_eq!(
            url("url(data:image/svg+xml,%3Csvg%2F%3E)").as_deref(),
            Some("data:image/svg+xml,%3Csvg%2F%3E")
        );
    }

    #[test]
    fn what_is_not_a_url_or_is_a_broken_one() {
        assert_eq!(parse("linear-gradient(red, blue)"), Parsed::NotUrl);
        assert_eq!(parse("none"), Parsed::NotUrl);
        assert_eq!(parse("image-set(url(a.png) 1x)"), Parsed::NotUrl);
        assert_eq!(parse("url(a.png"), Parsed::Invalid);
        assert_eq!(parse("url(\"a.png)"), Parsed::Invalid);
        assert_eq!(parse("url(\"a.png\" x)"), Parsed::Invalid);
        assert_eq!(parse("url(a.png) x"), Parsed::Invalid);
    }

    #[test]
    fn token_end_skips_a_whole_url() {
        let text = "x url(\"a)b\") y";
        assert_eq!(token_end(text, 2), Some(text.len() - 2));
        assert_eq!(token_end("my-url(a)", 3), None);
        assert_eq!(token_end("url(unclosed", 0), None);
    }
}
