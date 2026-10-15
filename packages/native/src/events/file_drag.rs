//! The files of an OS file drag, described the way the DOM describes a `File`.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::element_tree::DroppedFile;

/// Describes each path. A path that is not valid UTF-8 or that cannot be
/// stat'ed (deleted mid-drag, unreadable) is left out and logged:
/// `to_string_lossy` would invent a path that does not exist on disk.
pub fn describe(paths: &[PathBuf]) -> Vec<DroppedFile> {
    paths
        .iter()
        .filter_map(|path| match describe_one(path) {
            Ok(file) => Some(file),
            Err(reason) => {
                log::warn!("File drag: skipping {}: {reason}", path.display());
                None
            }
        })
        .collect()
}

fn describe_one(path: &Path) -> Result<DroppedFile, String> {
    let text = path.to_str().ok_or("the path is not valid UTF-8")?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("the path has no file name")?;
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    let modified = metadata.modified().map_err(|error| error.to_string())?;
    let last_modified = match modified.duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_millis() as f64,
        Err(before) => -(before.duration().as_millis() as f64),
    };
    Ok(DroppedFile {
        path: text.to_string(),
        name: name.to_string(),
        size: metadata.len() as f64,
        mime_type: mime_type(path).to_string(),
        last_modified,
    })
}

/// The MIME type a browser reports for the extension, or `""`. A directory has
/// none: its extension, if it has one, says nothing about its content.
fn mime_type(path: &Path) -> &'static str {
    if path.is_dir() {
        return "";
    }
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return "";
    };
    match extension.to_ascii_lowercase().as_str() {
        "txt" | "text" | "log" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "md" | "markdown" => "text/markdown",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "tif" | "tiff" => "image/tiff",
        "heic" => "image/heic",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_described_by_its_metadata() {
        let dir = std::env::temp_dir().join("gpuix-file-drag-describe");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Notes.PNG");
        std::fs::write(&path, b"12345").unwrap();

        let files = describe(&[path.clone()]);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, path.to_str().unwrap());
        assert_eq!(files[0].name, "Notes.PNG");
        assert_eq!(files[0].size, 5.0);
        assert_eq!(files[0].mime_type, "image/png");
        assert!(files[0].last_modified > 1.0e12);
    }

    #[test]
    fn an_unknown_extension_and_a_directory_have_no_type() {
        let dir = std::env::temp_dir().join("gpuix-file-drag-types.d");
        std::fs::create_dir_all(&dir).unwrap();
        let unknown = dir.join("data.zzz");
        std::fs::write(&unknown, b"").unwrap();

        let files = describe(&[unknown, dir.clone()]);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].mime_type, "");
        assert_eq!(files[1].mime_type, "");
    }

    #[test]
    fn a_missing_file_is_left_out() {
        let missing = std::env::temp_dir().join("gpuix-file-drag-missing-file.txt");
        std::fs::remove_file(&missing).ok();
        assert!(describe(&[missing]).is_empty());
    }
}
