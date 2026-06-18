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

#[cfg(test)]
mod tests {
    use super::*;

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
