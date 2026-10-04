//! Removes `exif:GPS*` properties from an XMP packet (attribute and element forms).

use regex::bytes::Regex;
use std::sync::LazyLock;

static ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\s+exif:GPS[A-Za-z]+\s*=\s*("[^"]*"|'[^']*')"#).expect("valid regex")
});
static EMPTY_ELEMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<exif:GPS[A-Za-z]+[^>]*/>").expect("valid regex"));
static ELEMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<exif:GPS[A-Za-z]+[^>]*>.*?</exif:GPS[A-Za-z]+>").expect("valid regex")
});

pub fn strip(segment: &[u8]) -> Vec<u8> {
    let step = ATTRIBUTE.replace_all(segment, &b""[..]);
    let step = EMPTY_ELEMENT.replace_all(&step, &b""[..]);
    ELEMENT.replace_all(&step, &b""[..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_attribute_and_element_forms() {
        let xmp = br#"<rdf:Description exif:GPSLatitude="40,26.7N" exif:ExposureTime="1/60" exif:GPSLongitude='3,42W'>
<exif:GPSAltitude>650/1</exif:GPSAltitude><exif:GPSVersionID/><exif:FNumber>4</exif:FNumber>"#;
        let out = String::from_utf8(strip(xmp)).unwrap();
        assert!(!out.contains("GPS"), "{out}");
        assert!(out.contains("ExposureTime") && out.contains("FNumber"));
    }
}
