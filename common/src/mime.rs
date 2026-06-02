/// MIME type support whitelists for worker image processing.
///
/// Workers check the picture's `mime_type` field before attempting EXIF extraction
/// or thumbnail generation to avoid feeding unsupported formats to native libraries.

/// MIME types that GExiv2 (rexiv2) can read and write EXIF metadata for.
pub const MIME_TYPES_EXIF: &[&str] = &[
    "image/jpeg",
    "image/jpg", // non-standard alias, common in the wild
    "image/png",
    "image/tiff",
    "image/tif", // non-standard alias
    "image/webp",
    "image/heic",
    "image/heif",
    "image/avif",
    "image/bmp",
    "image/x-bmp",
    // Common camera raw formats (GExiv2 reads EXIF from many raw formats)
    "image/x-nikon-nef",
    "image/x-canon-cr2",
    "image/x-canon-cr3",
    "image/x-sony-arw",
    "image/x-fuji-raf",
    "image/x-adobe-dng",
    "image/x-panasonic-rw2",
];

/// MIME types that ImageMagick can decode and from which WebP thumbnails can be generated.
pub const MIME_TYPES_THUMBNAIL: &[&str] = &[
    "image/jpeg",
    "image/jpg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/tiff",
    "image/tif",
    "image/bmp",
    "image/x-bmp",
    "image/heic",
    "image/heif",
    "image/avif",
    "image/ico",
    "image/x-icon",
    "image/pnm",
    "image/x-portable-anymap",
];

/// Returns `true` when GExiv2 supports EXIF extraction for this MIME type.
pub fn supports_exif(mime_type: &str) -> bool {
    let lower = mime_type.to_lowercase();
    MIME_TYPES_EXIF.contains(&lower.as_str())
}

/// Returns `true` when ImageMagick can decode this MIME type for thumbnail generation.
pub fn supports_thumbnail(mime_type: &str) -> bool {
    let lower = mime_type.to_lowercase();
    MIME_TYPES_THUMBNAIL.contains(&lower.as_str())
}
