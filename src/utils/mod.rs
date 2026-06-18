//! Shared utility helpers.

use base64::prelude::*;

/// Resolve an image input into a value usable in an OpenAI-compatible
/// `image_url` content part.
///
/// If `image` is an `http(s)://` URL it is returned unchanged. If it is a path
/// to a local file, the file is read and encoded as a `data:` URI with the
/// guessed MIME type. If it already looks like a `data:` URI it is returned
/// unchanged. Otherwise the input is treated as raw base64 data wrapped in a
/// `data:image/png;base64,` URI.
///
/// # Errors
///
/// Returns an error if a local file path is given but cannot be read.
pub fn resolve_image_input(image: &str) -> crate::error::Result<String> {
    let trimmed = image.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Ok(trimmed.to_string());
    }
    if trimmed.starts_with("data:") {
        return Ok(trimmed.to_string());
    }
    // Treat as a local file path if it exists on disk.
    let path = std::path::Path::new(trimmed);
    if path.is_file() {
        let bytes = std::fs::read(path)?;
        let mime = mime_from_ext(path);
        let b64 = BASE64_STANDARD.encode(&bytes);
        return Ok(format!("data:{mime};base64,{b64}"));
    }
    // Fall back to treating the input as raw base64.
    Ok(format!("data:image/png;base64,{trimmed}"))
}

/// Guess a MIME type from a file extension.
fn mime_from_ext(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "image/png",
    }
}

/// Validate that a size string looks like `WxH` (e.g. `1024x768`).
///
/// # Errors
///
/// Returns an error if the format is invalid.
pub fn validate_size(size: &str) -> crate::error::Result<()> {
    let re = regex_lite::Regex::new(r"^[1-9][0-9]{1,4}x[1-9][0-9]{1,4}$")
        .expect("static regex is valid");
    if re.is_match(size) {
        Ok(())
    } else {
        Err(crate::error::Error::config(format!(
            "invalid size '{size}'. Use a pixel size such as 1024x768."
        )))
    }
}

/// Validate that `num_frames` satisfies the Agnes constraint `<= 441` and
/// `(n - 1) % 8 == 0`.
///
/// # Errors
///
/// Returns an error if the constraint is violated.
pub fn validate_num_frames(num_frames: i64) -> crate::error::Result<()> {
    if !(1..=441).contains(&num_frames) || (num_frames - 1) % 8 != 0 {
        Err(crate::error::Error::config(format!(
            "num_frames must be <= 441 and satisfy 8n + 1 (e.g. 81, 121, 161, 241, 441). Got {num_frames}."
        )))
    } else {
        Ok(())
    }
}

/// Derive a safe filename for a downloaded asset.
///
/// Strategy:
/// 1. Try to extract a basename from the URL path with a recognized image or
///    video extension.
/// 2. If none, use `prefix-{index}` (e.g. `image-1`, `video-0`).
/// 3. The extension comes from `content_type` (e.g. `image/png`) if provided,
///    otherwise from the URL suffix, otherwise falls back to `default_ext`.
///
/// The returned string is a pure filename with no path separators.
#[must_use]
pub fn derive_filename(
    url: &str,
    index: usize,
    content_type: Option<&str>,
    prefix: &str,
    default_ext: &str,
) -> String {
    // 1. Try URL basename with a recognized extension.
    //    Strip query string and fragment first, then take the path basename.
    let url_path = url.split(['?', '#']).next().unwrap_or(url);
    let url_basename = url_path
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(std::string::ToString::to_string);

    let has_recognized_ext = |name: &str| -> bool {
        let lower = name.to_ascii_lowercase();
        [
            ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".mp4", ".webm", ".mov",
        ]
        .iter()
        .any(|e| lower.ends_with(e))
    };

    if let Some(name) = &url_basename {
        if has_recognized_ext(name) {
            return name.clone();
        }
    }

    // 2. Fall back to prefix-index with a derived extension.
    let ext = content_type
        .and_then(ext_from_content_type)
        .or_else(|| url_basename.as_deref().and_then(ext_from_url_suffix))
        .unwrap_or_else(|| default_ext.trim_start_matches('.').to_string());

    format!("{prefix}-{index}.{ext}")
}

/// Map a Content-Type header value to a file extension (without the dot).
fn ext_from_content_type(ct: &str) -> Option<String> {
    let mime = ct.split(';').next()?.trim().to_ascii_lowercase();
    Some(
        match mime.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/bmp" => "bmp",
            "video/mp4" => "mp4",
            "video/webm" => "webm",
            "video/quicktime" => "mov",
            _ => return None,
        }
        .to_string(),
    )
}

/// Try to extract an extension from a URL suffix.
fn ext_from_url_suffix(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let dot_idx = lower.rfind('.')?;
    let ext = &lower[dot_idx + 1..];
    if matches!(
        ext,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "mp4" | "webm" | "mov"
    ) {
        Some(ext.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_filename_from_url_with_ext() {
        let name = derive_filename(
            "https://example.com/path/image.png",
            0,
            None,
            "image",
            "png",
        );
        assert_eq!(name, "image.png");
    }

    #[test]
    fn derive_filename_uses_content_type_when_url_has_no_ext() {
        let name = derive_filename(
            "https://cdn.example.com/abc123",
            2,
            Some("image/jpeg"),
            "image",
            "png",
        );
        assert_eq!(name, "image-2.jpg");
    }

    #[test]
    fn derive_filename_falls_back_to_default_ext() {
        let name = derive_filename("https://cdn.example.com/abc123", 0, None, "video", "mp4");
        assert_eq!(name, "video-0.mp4");
    }

    #[test]
    fn derive_filename_strips_query_string() {
        let name = derive_filename(
            "https://example.com/pic.jpg?token=abc&sig=def",
            0,
            None,
            "image",
            "png",
        );
        assert_eq!(name, "pic.jpg");
    }

    #[test]
    fn derive_filename_recognizes_video_ext() {
        let name = derive_filename("https://example.com/clip.mp4", 0, None, "video", "mp4");
        assert_eq!(name, "clip.mp4");
    }

    #[test]
    fn validate_size_ok() {
        assert!(validate_size("1024x768").is_ok());
        assert!(validate_size("1152x768").is_ok());
    }

    #[test]
    fn validate_size_bad() {
        assert!(validate_size("1024").is_err());
        assert!(validate_size("0x768").is_err());
        assert!(validate_size("1024x").is_err());
    }

    #[test]
    fn validate_num_frames_ok() {
        for n in [81, 121, 161, 241, 441] {
            assert!(validate_num_frames(n).is_ok(), "{n} should be valid");
        }
    }

    #[test]
    fn validate_num_frames_bad() {
        assert!(validate_num_frames(80).is_err());
        assert!(validate_num_frames(500).is_err());
        assert!(validate_num_frames(0).is_err());
    }
}
