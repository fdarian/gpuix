//! Tokenising helpers shared by the CSS value parsers.

/// The text between the parentheses of `name( ... )`, when `value` is exactly
/// one such function call. The name is matched case-insensitively and no space
/// may precede the `(`, as in CSS.
pub(crate) fn function_args<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let value = value.trim();
    let head = value.get(..name.len())?;
    if !head.eq_ignore_ascii_case(name) {
        return None;
    }
    let rest = value[name.len()..].strip_prefix('(')?;
    let mut depth = 1usize;
    for (index, character) in rest.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    // The call must close at the very end: `f(a) g(b)` is not one call.
                    return (index + 1 == rest.len()).then(|| &rest[..index]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits at commas that are not inside parentheses or a `url()`. Empty pieces
/// are kept so the caller can reject `a,,b`.
pub(crate) fn split_commas(value: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut url_end = 0;
    for (index, character) in value.char_indices() {
        if index < url_end {
            continue;
        }
        if let Some(end) = url_token_end(value, index, character) {
            url_end = end;
            continue;
        }
        match character {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                pieces.push(value[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    pieces.push(value[start..].trim());
    pieces
}

/// Splits at whitespace that is not inside parentheses or a `url()`, so
/// `rgb(0 0 0) 30%` is two tokens.
pub(crate) fn split_whitespace(value: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut depth = 0i32;
    let mut start: Option<usize> = None;
    let mut url_end = 0;
    for (index, character) in value.char_indices() {
        if index < url_end {
            continue;
        }
        if let Some(end) = url_token_end(value, index, character) {
            url_end = end;
            start.get_or_insert(index);
            continue;
        }
        match character {
            '(' => {
                depth += 1;
                start.get_or_insert(index);
            }
            ')' => depth -= 1,
            character if character.is_whitespace() && depth == 0 => {
                if let Some(begin) = start.take() {
                    tokens.push(&value[begin..index]);
                }
            }
            _ => {
                start.get_or_insert(index);
            }
        }
    }
    if let Some(begin) = start {
        tokens.push(&value[begin..]);
    }
    tokens
}

/// The end of the `url()` token that starts at `index`, whose first character is
/// `character`. A quoted address can hold the parentheses, commas and spaces the
/// splitters look for.
fn url_token_end(value: &str, index: usize, character: char) -> Option<usize> {
    if !matches!(character, 'u' | 'U') {
        return None;
    }
    super::url::token_end(value, index)
}

/// Whitespace-separated tokens with every `/` outside parentheses a token of
/// its own, so `10px/5px` and `10px / 5px` split alike while `calc(1px / 2)`
/// stays whole.
pub(crate) fn split_slash_tokens(value: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut depth = 0i32;
    let mut start: Option<usize> = None;
    for (index, character) in value.char_indices() {
        let separator = character.is_whitespace() || character == '/';
        match character {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && separator {
            if let Some(begin) = start.take() {
                tokens.push(&value[begin..index]);
            }
            if character == '/' {
                tokens.push(&value[index..index + 1]);
            }
        } else {
            start.get_or_insert(index);
        }
    }
    if let Some(begin) = start {
        tokens.push(&value[begin..]);
    }
    tokens
}

/// One to four values in the order `margin` uses: all, vertical / horizontal,
/// top / horizontal / bottom, or each side clockwise from the top.
pub(crate) fn expand_sides<T: Clone>(values: &[T]) -> Option<[T; 4]> {
    Some(match values {
        [all] => [all.clone(), all.clone(), all.clone(), all.clone()],
        [vertical, horizontal] => [vertical.clone(), horizontal.clone(), vertical.clone(), horizontal.clone()],
        [top, horizontal, bottom] => [top.clone(), horizontal.clone(), bottom.clone(), horizontal.clone()],
        [top, right, bottom, left] => [top.clone(), right.clone(), bottom.clone(), left.clone()],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_args_requires_one_whole_call() {
        assert_eq!(function_args("Color-Mix(in srgb, a, b)", "color-mix"), Some("in srgb, a, b"));
        assert_eq!(function_args("f(g(1)) ", "f"), Some("g(1)"));
        assert_eq!(function_args("f(a) g(b)", "f"), None);
        assert_eq!(function_args("f (a)", "f"), None);
        assert_eq!(function_args("f(a", "f"), None);
        assert_eq!(function_args("other(a)", "f"), None);
    }

    #[test]
    fn splits_respect_parentheses() {
        assert_eq!(split_commas("a, rgb(1, 2, 3) ,b"), vec!["a", "rgb(1, 2, 3)", "b"]);
        assert_eq!(split_commas("a,,b"), vec!["a", "", "b"]);
        assert_eq!(split_whitespace("  rgb(0 0 0)  30% "), vec!["rgb(0 0 0)", "30%"]);
    }

    #[test]
    fn a_url_is_one_token_whatever_its_address_holds() {
        assert_eq!(
            split_commas("url(\"a,b)c.png\") no-repeat, url(d.png)"),
            vec!["url(\"a,b)c.png\") no-repeat", "url(d.png)"]
        );
        assert_eq!(
            split_whitespace("url( 'a b)c.png' ) center url(data:image/svg+xml,%3Csvg%2F%3E)"),
            vec!["url( 'a b)c.png' )", "center", "url(data:image/svg+xml,%3Csvg%2F%3E)"]
        );
        // `url(` inside a longer identifier, or never closed, is nothing special.
        assert_eq!(split_commas("my-url(a, b), c"), vec!["my-url(a, b)", "c"]);
        assert_eq!(split_commas("url(a, b"), vec!["url(a, b"]);
    }

    #[test]
    fn slashes_are_tokens_outside_parentheses_only() {
        assert_eq!(split_slash_tokens("10px/5px"), vec!["10px", "/", "5px"]);
        assert_eq!(split_slash_tokens(" 1px  2px / 3px "), vec!["1px", "2px", "/", "3px"]);
        assert_eq!(split_slash_tokens("calc(1px / 2) / 3px"), vec!["calc(1px / 2)", "/", "3px"]);
    }

    #[test]
    fn sides_expand_like_margin() {
        assert_eq!(expand_sides(&[1]), Some([1, 1, 1, 1]));
        assert_eq!(expand_sides(&[1, 2]), Some([1, 2, 1, 2]));
        assert_eq!(expand_sides(&[1, 2, 3]), Some([1, 2, 3, 2]));
        assert_eq!(expand_sides(&[1, 2, 3, 4]), Some([1, 2, 3, 4]));
        assert_eq!(expand_sides::<i32>(&[]), None);
        assert_eq!(expand_sides(&[1, 2, 3, 4, 5]), None);
    }
}
