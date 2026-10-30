//! How a `<webview>` URL is loaded, decided apart from the web engine that loads it.
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub(crate) enum LoadTarget {
    /// A page on disk. The engine is told which directory the page may read, so its relative
    /// scripts, styles and images load: the directory of the file.
    File { url: String, read_access: PathBuf },
    /// Anything else, requested by URL.
    Request { url: String },
}

/// `NSURLErrorUnsupportedURL`, the error code of a URL no engine could load.
pub(crate) const UNSUPPORTED_URL_ERROR: i32 = -1002;

pub(crate) fn classify(url: &str) -> Result<LoadTarget, String> {
    let parsed = url::Url::parse(url).map_err(|error| format!("Invalid URL \"{url}\": {error}"))?;
    if parsed.scheme() != "file" {
        return Ok(LoadTarget::Request { url: parsed.into() });
    }
    let path = parsed
        .to_file_path()
        .map_err(|()| format!("Invalid file URL \"{url}\""))?;
    let read_access = if parsed.path().ends_with('/') {
        path
    } else {
        path.parent()
            .ok_or_else(|| format!("The file URL \"{url}\" has no directory"))?
            .to_path_buf()
    };
    Ok(LoadTarget::File { url: parsed.into(), read_access })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_url_may_read_its_directory() {
        assert_eq!(
            classify("file:///tmp/site/index.html").unwrap(),
            LoadTarget::File {
                url: "file:///tmp/site/index.html".into(),
                read_access: PathBuf::from("/tmp/site"),
            }
        );
    }

    #[test]
    fn a_file_url_of_a_directory_may_read_it() {
        assert_eq!(
            classify("file:///tmp/site/").unwrap(),
            LoadTarget::File { url: "file:///tmp/site/".into(), read_access: PathBuf::from("/tmp/site/") }
        );
    }

    #[test]
    fn an_escaped_file_url_is_a_path() {
        match classify("file:///tmp/a%20b/c.html").unwrap() {
            LoadTarget::File { read_access, .. } => assert_eq!(read_access, PathBuf::from("/tmp/a b")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn other_schemes_are_requested() {
        for url in ["https://example.com/a?b=1", "http://localhost:3000/", "about:blank", "data:text/html,hi"] {
            assert!(matches!(classify(url).unwrap(), LoadTarget::Request { .. }), "{url}");
        }
    }

    #[test]
    fn a_url_without_a_scheme_is_refused() {
        assert!(classify("example.com").is_err());
        assert!(classify("/tmp/a.html").is_err());
        assert!(classify("").is_err());
    }
}
