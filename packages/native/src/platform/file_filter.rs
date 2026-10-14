//! The HTML `accept` attribute syntax, as file picker filters.
use gpui::PathPromptFilter;

/// Parses `".png,.jpg,image/*,application/pdf"`.
///
/// Tokens are case-insensitive and empty ones are skipped, as in a browser. A
/// token that is neither `.ext` nor `type/subtype` is an error: a typo that
/// silently widened the picker would accept files the app did not ask for.
/// `*/*` accepts everything, so it leaves no filter.
pub(crate) fn parse_accept(accept: &str) -> Result<Vec<PathPromptFilter>, String> {
    let mut filters = Vec::new();
    for token in accept.split(',') {
        let token = token.trim().to_ascii_lowercase();
        if token.is_empty() {
            continue;
        }
        if let Some(extension) = token.strip_prefix('.') {
            if extension.is_empty() || extension.contains(['/', '.', ' ']) {
                return Err(format!("Invalid accept extension \"{token}\""));
            }
            filters.push(PathPromptFilter::Extension(extension.to_string().into()));
            continue;
        }
        match token.split_once('/') {
            Some(("*", "*")) => return Ok(Vec::new()),
            Some((kind, subtype))
                if !kind.is_empty()
                    && !subtype.is_empty()
                    && !kind.contains(['*', ' '])
                    && !subtype.contains('/')
                    && (subtype == "*" || !subtype.contains('*')) =>
            {
                filters.push(PathPromptFilter::MimeType(token.into()));
            }
            _ => return Err(format!("Invalid accept token \"{token}\"")),
        }
    }
    Ok(filters)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extension(name: &str) -> PathPromptFilter {
        PathPromptFilter::Extension(name.to_string().into())
    }

    fn mime(name: &str) -> PathPromptFilter {
        PathPromptFilter::MimeType(name.to_string().into())
    }

    #[test]
    fn parses_extensions_and_mime_types() {
        assert_eq!(
            parse_accept(".PNG, .jpg ,image/*,application/pdf").unwrap(),
            vec![
                extension("png"),
                extension("jpg"),
                mime("image/*"),
                mime("application/pdf")
            ]
        );
    }

    #[test]
    fn empty_tokens_are_skipped() {
        assert_eq!(parse_accept(".png,, ").unwrap(), vec![extension("png")]);
        assert_eq!(parse_accept("").unwrap(), Vec::new());
    }

    #[test]
    fn accept_all_leaves_no_filter() {
        assert_eq!(parse_accept(".png,*/*").unwrap(), Vec::new());
    }

    #[test]
    fn rejects_tokens_that_are_neither_extension_nor_mime_type() {
        for bad in ["png", ".", "image/", "/png", "*/png", "image/png/x", ".tar.gz", "a b/c"] {
            assert!(parse_accept(bad).is_err(), "{bad} should be rejected");
        }
    }
}
