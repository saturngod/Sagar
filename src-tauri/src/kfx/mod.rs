//! EPUB → KFX (Kindle Format 10) conversion, ported from Yay's Swift
//! converter. Converts reflowable XHTML, CSS subset, images (JPEG/PNG/GIF),
//! embedded fonts, links and navigation. Fixed layout, SVG, MathML, tables
//! as native layout and DRM are not supported.

mod css;
mod dom;
mod image;
mod ion;
mod layout;
mod model;
mod reader;
mod style;
mod util;
mod writer;

use std::path::Path;

use crate::domain::ImportWarning;
use crate::error::AppError;

/// Converts an EPUB file to KFX bytes. Warnings describe content that was
/// omitted or simplified (unsupported images, missing stylesheets).
pub fn convert_epub(epub: &Path) -> Result<(Vec<u8>, Vec<ImportWarning>), AppError> {
    let (book, mut warnings) = reader::read_epub(epub)?;
    let (bytes, writer_warnings) = writer::encode(book, "Sagar", env!("CARGO_PKG_VERSION"));
    warnings.extend(writer_warnings);
    Ok((bytes, warnings))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;

    fn build_epub(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("book.epub");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("create"));
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let files: [(&str, &str); 7] = [
            ("mimetype", "application/epub+zip"),
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            ),
            (
                "OEBPS/content.opf",
                r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="uid">urn:uuid:12345678-1234-1234-1234-123456789abc</dc:identifier><dc:title>မြန်မာ Test</dc:title><dc:language>my</dc:language><dc:creator>Author</dc:creator></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c1" href="text/c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="text/c2.xhtml" media-type="application/xhtml+xml"/><item id="css" href="style.css" media-type="text/css"/><item id="img" href="img/a.svg" media-type="image/svg+xml"/></manifest><spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#,
            ),
            (
                "OEBPS/nav.xhtml",
                r##"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="text/c1.xhtml">One</a></li><li><a href="text/c2.xhtml#n1">Two</a></li></ol></nav></body></html>"##,
            ),
            (
                "OEBPS/style.css",
                "p { text-indent: 1em; } .note { color: #333 }",
            ),
            (
                "OEBPS/text/c1.xhtml",
                r##"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="../style.css"/></head><body><h1>ခေါင်းစဉ်</h1><p>Hello&nbsp;<strong>world</strong> <a href="c2.xhtml#n1">note</a></p><img src="../img/a.svg"/></body></html>"##,
            ),
            (
                "OEBPS/text/c2.xhtml",
                r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p id="n1" class="note">Second chapter.</p><ul><li>A</li><li>B</li></ul></body></html>"#,
            ),
        ];
        for (name, bytes) in files {
            zip.start_file(name, stored).expect("start");
            zip.write_all(bytes.as_bytes()).expect("write");
        }
        zip.start_file("OEBPS/img/a.svg", stored).expect("start");
        zip.write_all(b"<svg xmlns='http://www.w3.org/2000/svg'/>")
            .expect("write");
        zip.finish().expect("finish");
        path
    }

    #[test]
    fn converts_epub_to_kfx_container() {
        let dir = tempfile::tempdir().expect("tempdir");
        let epub = build_epub(dir.path());
        let (bytes, warnings) = convert_epub(&epub).expect("convert");
        assert_eq!(&bytes[..4], b"CONT");
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 2);
        let header_length = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]) as usize;
        let payload = &bytes[header_length..];
        assert!(payload.starts_with(b"ENTY"));
        // Text, including Burmese, is stored as UTF-8 content strings.
        let haystack = String::from_utf8_lossy(&bytes);
        assert!(haystack.contains("ခေါင်းစဉ်"));
        assert!(haystack.contains("Hello\u{a0}world note"));
        assert!(haystack.contains("Second chapter."));
        assert!(haystack.contains("kfxgen_payload_sha1"));
        // The SVG is reported, not silently dropped.
        assert!(warnings.iter().any(|w| w.code == "kfx_unsupported_image"));
        // Entity table offsets cover the whole payload.
        let info_offset = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize;
        assert!(info_offset > 18 && info_offset < header_length);
    }

    #[test]
    fn conversion_is_deterministic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let epub = build_epub(dir.path());
        let first = convert_epub(&epub).expect("convert").0;
        let second = convert_epub(&epub).expect("convert").0;
        assert_eq!(first, second);
    }

    /// Developer check against Yay's Swift converter: writes `<name>.rs.kfx`
    /// for every EPUB in `KFX_COMPARE_DIR`, using the reference creator strings.
    #[test]
    #[ignore]
    fn write_reference_comparison_outputs() {
        let Ok(dir) = std::env::var("KFX_COMPARE_DIR") else {
            return;
        };
        for entry in std::fs::read_dir(&dir).expect("dir").flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("epub") {
                continue;
            }
            match reader::read_epub(&path) {
                Ok((book, _)) => {
                    let (bytes, warnings) = writer::encode(book, "YayExport", "1.0.0");
                    std::fs::write(path.with_extension("rs.kfx"), bytes).expect("write");
                    println!("{}: {} warnings", path.display(), warnings.len());
                }
                Err(error) => println!("{}: {error}", path.display()),
            }
        }
    }
}
