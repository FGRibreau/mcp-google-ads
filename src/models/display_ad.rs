//! Uploaded display ad formats.
//!
//! Limits follow Google's "Uploaded display ads specifications"
//! (support.google.com/google-ads/answer/1722096): an image ad is a GIF, JPG or
//! PNG of at most 150 KB; an HTML5 ad is a ZIP of at most 600 KB holding no more
//! than 40 files. A kilobyte is taken as 1024 bytes.

api_enum! {
    /// Kind of uploaded display ad.
    pub enum DisplayAdType {
        /// HTML5 bundle (ZIP) as a `MEDIA_BUNDLE` asset, served through
        /// `display_upload_ad` with product type `HTML5_UPLOAD_AD`.
        Html5 = "HTML5",
        /// Single image (JPG, PNG or GIF) as an `IMAGE` asset, served through
        /// `image_ad`.
        Image = "IMAGE",
    }
}

impl DisplayAdType {
    /// Largest file Google accepts for this kind of ad, in bytes.
    pub fn max_file_bytes(&self) -> u64 {
        match self {
            DisplayAdType::Html5 => 600 * 1024,
            DisplayAdType::Image => 150 * 1024,
        }
    }

    /// File extensions accepted for this kind of ad (lowercase, without dot).
    pub fn allowed_extensions(&self) -> &'static [&'static str] {
        match self {
            DisplayAdType::Html5 => &["zip"],
            DisplayAdType::Image => &["jpg", "jpeg", "png", "gif"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_match_the_published_specs() {
        assert_eq!(DisplayAdType::Html5.max_file_bytes(), 614_400);
        assert_eq!(DisplayAdType::Image.max_file_bytes(), 153_600);
        assert_eq!(DisplayAdType::Html5.allowed_extensions(), &["zip"]);
        assert!(DisplayAdType::Image.allowed_extensions().contains(&"gif"));
    }

    #[test]
    fn round_trips_and_rejects_unknown() {
        for t in DisplayAdType::ALL {
            let wire = serde_json::to_string(t).unwrap();
            assert_eq!(wire, format!("\"{}\"", t.as_api_str()));
        }
        assert!(serde_json::from_str::<DisplayAdType>("\"VIDEO\"").is_err());
        assert!(serde_json::from_str::<DisplayAdType>("\"html5\"").is_err());
    }
}
