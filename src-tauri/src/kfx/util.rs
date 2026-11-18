//! Small helpers shared by the KFX converter: SHA-1, path resolution,
//! percent decoding and CSS number parsing.

use super::model::LinkTarget;

/// SHA-1 is required by the KFX identifiers and IDPF font deobfuscation;
/// it is not used for security.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut message = data.to_vec();
    let bit_length = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());
    let mut h: [u32; 5] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];
    for chunk in message.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5a827999),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (index, value) in h.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

pub fn sha1_hex(data: &[u8]) -> String {
    sha1(data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Strict percent decoding; `None` when the escape sequence or UTF-8 is invalid.
pub fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = value.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Normalizes an archive path: forward slashes, no `.`/empty segments,
/// `..` clamped at the root.
pub fn normalize_path(path: &str) -> String {
    let replaced = path.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for part in replaced.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Resolves an href relative to the document that contains it.
pub fn resolve_epub_path(href: &str, document: &str) -> String {
    let path = href.split('#').next().unwrap_or("");
    let decoded = percent_decode(path).unwrap_or_else(|| path.to_string());
    if decoded.is_empty() {
        return document.to_string();
    }
    let base = document
        .rfind('/')
        .map(|index| &document[..index])
        .unwrap_or("");
    if decoded.starts_with('/') {
        normalize_path(&decoded)
    } else {
        normalize_path(&format!("{base}/{decoded}"))
    }
}

/// Leading numeric prefix of a CSS value (`"1.5em"` → `1.5`).
pub fn css_number(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    let prefix: String = trimmed
        .chars()
        .take_while(|c| "0123456789.-+".contains(*c))
        .collect();
    prefix.parse::<f64>().ok()
}

pub fn unquoted(value: &str) -> String {
    value
        .trim_matches(|c: char| matches!(c, ' ' | '\t' | '\r' | '\n' | '"' | '\''))
        .to_string()
}

/// Rust's shortest round-trip float formatting, used for stored computed CSS.
pub fn format_number(value: f64) -> String {
    format!("{value}")
}

impl LinkTarget {
    /// Supported link destinations; other URI schemes stay plain text.
    pub fn from_href(href: &str, document: &str) -> Option<Self> {
        let href = href.trim();
        if let Some(rest) = href.strip_prefix("//") {
            return Some(LinkTarget::External(format!("https://{rest}")));
        }
        if let Some(scheme) = uri_scheme(href) {
            let scheme = scheme.to_ascii_lowercase();
            if matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel") {
                return Some(LinkTarget::External(href.to_string()));
            }
            return None;
        }
        let (file_part, fragment) = match href.split_once('#') {
            Some((file, fragment)) => (file, Some(fragment)),
            None => (href, None),
        };
        let file = file_part.split('?').next().unwrap_or("");
        Some(LinkTarget::Location {
            path: resolve_epub_path(file, document),
            fragment: fragment.map(|f| percent_decode(f).unwrap_or_else(|| f.to_string())),
        })
    }
}

fn uri_scheme(href: &str) -> Option<&str> {
    let colon = href.find(':')?;
    let scheme = &href[..colon];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    chars
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        .then_some(scheme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_known_vectors() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn resolves_relative_paths_and_links() {
        assert_eq!(
            resolve_epub_path("../img/a%20b.png", "OEBPS/text/c1.xhtml"),
            "OEBPS/img/a b.png"
        );
        assert_eq!(resolve_epub_path("#x", "OEBPS/c1.xhtml"), "OEBPS/c1.xhtml");
        assert_eq!(
            LinkTarget::from_href("c2.xhtml?x=1#n%201", "OEBPS/c1.xhtml"),
            Some(LinkTarget::Location {
                path: "OEBPS/c2.xhtml".into(),
                fragment: Some("n 1".into())
            })
        );
        assert_eq!(
            LinkTarget::from_href("javascript:alert(1)", "a.xhtml"),
            None
        );
        assert_eq!(
            LinkTarget::from_href("//example.com", "a.xhtml"),
            Some(LinkTarget::External("https://example.com".into()))
        );
    }
}
