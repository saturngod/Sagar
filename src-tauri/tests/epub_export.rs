//! End-to-end export tests: save translations through the persistence layer,
//! export, then validate the output and re-import it.

use std::io::Read as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use sagar_lib::epub;
use sagar_lib::error::AppError;
use sagar_lib::services::{self, export::ExportRequest};
use sagar_lib::storage::translations::SaveInput;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

// ---------------------------------------------------------------- helpers

fn tempdir() -> PathBuf {
    tempfile::tempdir().expect("create temp dir").keep()
}

fn write_epub(dir: &Path, file_name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join(file_name);
    let file = std::fs::File::create(&path).expect("create epub file");
    let mut zip = ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut first = true;
    for (name, contents) in files {
        if name.ends_with('/') {
            zip.add_directory(*name, deflated).expect("directory entry");
            continue;
        }
        let options = if first && *name == "mimetype" {
            stored
        } else {
            deflated
        };
        zip.start_file(*name, options).expect("start entry");
        zip.write_all(contents as &[u8]).expect("write entry");
        first = false;
    }
    zip.finish().expect("finish epub");
    path
}

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
 <rootfiles>
  <rootfile full-path="OEBPS/content.opf" media-type="application/oebp+package+xml"/>
 </rootfiles>
</container>"#;

fn chapter_doc(title: &str, body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
 <head><title>{title}</title></head>
 <body>
  <h1 epub:type="title">{title}</h1>
  {body}
 </body>
</html>"#
    )
}

/// Two-chapter book with a cover, an inline image, an internal link and a
/// footnote-style anchor — the preservation regression fixture.
fn rich_book_files() -> Vec<(String, Vec<u8>)> {
    let ch1 = chapter_doc(
        "Introduction",
        "<p>River <em>softly</em>.</p>\
         <p>See <a href=\"ch2.xhtml#note-1\">note 1</a>.</p>\
         <p><img src=\"images/river.png\" alt=\"River\"/> by the bank.</p>",
    );
    let ch2 = chapter_doc(
        "Ten",
        "<p id=\"note-1\">Footnote text.</p><ul><li>Item one</li></ul>",
    );
    let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
 <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
  <dc:identifier id="uid">urn:uuid:export-test</dc:identifier>
  <dc:title>Riverside Tales</dc:title>
  <dc:creator>A. Author</dc:creator>
  <dc:language>en</dc:language>
 </metadata>
 <manifest>
  <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
  <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  <item id="c2" href="ch2.xhtml" media-type="application/xhtml+xml"/>
  <item id="img-river" href="images/river.png" media-type="image/png"/>
  <item id="cover-img" href="images/cover.png" media-type="image/png" properties="cover-image"/>
 </manifest>
 <spine><itemref idref="c1"/><itemref idref="c2"/></spine>
</package>"#;
    let nav = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
 <body><nav epub:type="toc"><ol>
  <li><a href="ch1.xhtml">Introduction</a></li>
  <li><a href="ch2.xhtml">Ten</a></li>
 </ol></nav></body>
</html>"#;
    vec![
        ("mimetype".to_string(), b"application/epub+zip".to_vec()),
        (
            "META-INF/container.xml".to_string(),
            CONTAINER.as_bytes().to_vec(),
        ),
        ("OEBPS/content.opf".to_string(), opf.as_bytes().to_vec()),
        ("OEBPS/nav.xhtml".to_string(), nav.as_bytes().to_vec()),
        ("OEBPS/ch1.xhtml".to_string(), ch1.into_bytes()),
        ("OEBPS/ch2.xhtml".to_string(), ch2.into_bytes()),
        (
            "OEBPS/images/river.png".to_string(),
            vec![0x89, b'P', b'N', b'G', 7, 7],
        ),
        (
            "OEBPS/images/cover.png".to_string(),
            vec![0x89, b'P', b'N', b'G', 9, 9],
        ),
    ]
}

struct Imported {
    data_dir: PathBuf,
    project_id: String,
    chapter_ids: Vec<String>,
}

fn import_rich_book(data_dir: &Path, source: &Path) -> Imported {
    let result = services::import::import_epub(data_dir, source, false).expect("import");
    Imported {
        data_dir: data_dir.to_path_buf(),
        project_id: result.project.id,
        chapter_ids: result.chapters.iter().map(|c| c.id.clone()).collect(),
    }
}

/// Saves a translation for the block at `block_index` of `chapter_index`.
fn save_block(book: &Imported, chapter_index: usize, block_index: i64, text: &str, status: &str) {
    let mut conn =
        sagar_lib::storage::open_database(&book.data_dir.join("sagar.db")).expect("open db");
    let block_id: String = conn
        .query_row(
            "SELECT b.id FROM blocks b WHERE b.chapter_id = ?1 AND b.block_index = ?2",
            rusqlite::params![book.chapter_ids[chapter_index], block_index],
            |row| row.get(0),
        )
        .expect("block exists");
    let revision: i64 = conn
        .query_row(
            "SELECT revision FROM translations WHERE block_id = ?1",
            rusqlite::params![block_id],
            |row| row.get(0),
        )
        .expect("translation exists");
    sagar_lib::storage::translations::save_translation(
        &mut conn,
        SaveInput {
            block_id,
            target_text: Some(text.to_string()),
            status: status.to_string(),
            expected_revision: revision,
        },
    )
    .expect("save");
}

// ------------------------------------------------------------------ tests

#[test]
fn export_preserves_trailing_resources_when_zip_has_directory_entries() {
    let dir = tempdir();
    let data_dir = tempdir();
    let mut files = rich_book_files();
    files.insert(1, ("META-INF/".into(), Vec::new()));
    files.insert(3, ("OEBPS/".into(), Vec::new()));
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "directories.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let destination = dir.join("export.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export with directories");
    epub::export::validate_output(&destination).expect("valid export");
    let mut output =
        zip::ZipArchive::new(std::fs::File::open(&destination).expect("output")).expect("zip");
    for (name, bytes) in &files {
        if name.ends_with('/') || name.ends_with(".opf") {
            continue;
        }
        let mut actual = Vec::new();
        output
            .by_name(name)
            .expect("resource retained")
            .read_to_end(&mut actual)
            .expect("read");
        assert_eq!(&actual, bytes, "resource {name}");
    }
    assert_eq!(std::fs::read(source).expect("source unchanged"), original);
}

#[test]
fn translated_fixture_exports_and_reimports_with_assets_intact() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "rich.epub", &files);
    let source_bytes = std::fs::read(&source).expect("read source");

    let book = import_rich_book(&data_dir, &source);

    // Chapter 0: h1(loc 0), p(loc 1), p-with-link(loc 2), p-with-image(loc 3).
    save_block(&book, 0, 0, "နိဒါန်း", "reviewed");
    save_block(&book, 0, 1, "မြစ် ပျော့ပျောင်းစွာ", "reviewed");
    // The link and image paragraphs stay untranslated so the fixture also
    // proves untranslated blocks keep their original markup byte-for-byte.
    // Chapter 1: leave everything untranslated (draft mode).

    let destination = dir.join("out/translated.epub");
    let outcome = services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id.clone(),
            destination: destination.clone(),
            mode: "draft".into(),
            title: Some("မြစ်ဇာတ်ကြောင်း".into()),
            language: Some("my".into()),
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export succeeds");
    assert_eq!(outcome.blocks_translated, 2);
    assert!(destination.exists(), "output written to requested path");
    let _ = outcome;

    // Source untouched; output valid.
    assert_eq!(std::fs::read(&source).expect("source after"), source_bytes);
    let warnings = epub::export::validate_output(&destination).expect("output valid");
    assert!(
        warnings.iter().all(|w| w.code != "chapter_patch_failed"),
        "{warnings:?}"
    );

    // OCF: mimetype first, stored, exact content.
    let output = std::fs::File::open(&destination).expect("open output");
    let mut zip = zip::ZipArchive::new(output).expect("zip");
    let first_name = zip
        .by_index(0)
        .expect("first")
        .name()
        .expect("readable name")
        .to_string();
    assert_eq!(first_name, "mimetype");
    let mut first = Vec::new();
    zip.by_index(0)
        .expect("first")
        .read_to_end(&mut first)
        .expect("read");
    assert_eq!(first, b"application/epub+zip");

    // Chapter 1 keeps the patched fragments and its assets/links.
    let mut ch1 = Vec::new();
    zip.by_name("OEBPS/ch1.xhtml")
        .expect("chapter entry")
        .read_to_end(&mut ch1)
        .expect("read chapter");
    let ch1 = String::from_utf8(ch1).expect("utf8");
    assert!(ch1.contains("<h1 epub:type=\"title\">နိဒါန်း</h1>"), "{ch1}");
    assert!(ch1.contains("မြစ် ပျော့ပျောင်းစွာ"), "{ch1}");
    assert!(
        ch1.contains("<p>See <a href=\"ch2.xhtml#note-1\">note 1</a>.</p>"),
        "untranslated link paragraph preserved verbatim: {ch1}"
    );
    assert!(
        ch1.contains("<p><img src=\"images/river.png\" alt=\"River\"/> by the bank.</p>"),
        "untranslated image paragraph preserved verbatim: {ch1}"
    );
    assert!(
        !ch1.contains("River <em>softly"),
        "translated block replaced: {ch1}"
    );
    assert!(!ch1.contains("<em>"), "no source markup leaks: {ch1}");

    // Chapter 2 untouched (untranslated draft content keeps the original).
    let mut ch2 = Vec::new();
    zip.by_name("OEBPS/ch2.xhtml")
        .expect("chapter entry")
        .read_to_end(&mut ch2)
        .expect("read chapter");
    assert!(
        String::from_utf8(ch2)
            .expect("utf8")
            .contains("Footnote text."),
        "untranslated chapter keeps original"
    );

    // Byte-identical resources.
    let mut river_out = Vec::new();
    zip.by_name("OEBPS/images/river.png")
        .expect("image entry")
        .read_to_end(&mut river_out)
        .expect("read image");
    assert_eq!(river_out, vec![0x89, b'P', b'N', b'G', 7, 7]);

    // Metadata updated.
    let mut opf = Vec::new();
    zip.by_name("OEBPS/content.opf")
        .expect("opf entry")
        .read_to_end(&mut opf)
        .expect("read opf");
    let opf = String::from_utf8(opf).expect("utf8");
    assert!(opf.contains("<dc:title>မြစ်ဇာတ်ကြောင်း</dc:title>"), "{opf}");
    assert!(opf.contains("<dc:language>my</dc:language>"), "{opf}");
    assert!(opf.contains("<dc:creator>A. Author</dc:creator>"), "{opf}");

    // Nav label follows the translated reviewed heading.
    let mut nav = Vec::new();
    zip.by_name("OEBPS/nav.xhtml")
        .expect("nav entry")
        .read_to_end(&mut nav)
        .expect("read nav");
    let nav = String::from_utf8(nav).expect("utf8");
    assert!(nav.contains(">နိဒါန်း</a>"), "nav label translated: {nav}");
    assert!(
        nav.contains(">Ten</a>"),
        "untranslated nav label kept: {nav}"
    );

    // Re-import the output: spine order, chapters and title round-trip.
    let reimported = services::import::import_epub(&data_dir, &destination, true)
        .expect("reimport exported file");
    assert_eq!(reimported.project.title, "မြစ်ဇာတ်ကြောင်း");
    assert_eq!(reimported.project.source_language.as_deref(), Some("my"));
    let hrefs: Vec<&str> = reimported
        .chapters
        .iter()
        .map(|c| c.resource_href.as_str())
        .collect();
    assert_eq!(hrefs, vec!["OEBPS/ch1.xhtml", "OEBPS/ch2.xhtml"]);
    let titles: Vec<&str> = reimported
        .chapters
        .iter()
        .map(|c| c.title.as_str())
        .collect();
    assert_eq!(titles, vec!["နိဒါန်း", "Ten"]);
}

#[test]
fn completed_export_is_blocked_while_required_blocks_are_pending() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "rich.epub", &files);
    let book = import_rich_book(&data_dir, &source);
    save_block(&book, 0, 0, "done", "reviewed");

    let error: AppError = services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id.clone(),
            destination: dir.join("out.epub"),
            mode: "completed".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect_err("completed export must be blocked");
    assert_eq!(error.code, "export_blocked");
    assert!(error.details.is_some(), "review counts accompany the block");
}

#[test]
fn export_never_overwrites_its_own_snapshot() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "rich.epub", &files);
    let book = import_rich_book(&data_dir, &source);
    save_block(&book, 0, 0, "done", "reviewed");
    save_block(&book, 0, 1, "done", "reviewed");
    save_block(&book, 0, 2, "done", "reviewed");
    save_block(&book, 0, 3, "done", "reviewed");
    save_block(&book, 1, 0, "done", "reviewed");
    save_block(&book, 1, 1, "done", "reviewed");
    save_block(&book, 1, 2, "done", "reviewed");

    let snapshot = data_dir.join(format!("projects/{}/source.epub", book.project_id));
    let error: AppError = services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id.clone(),
            destination: snapshot.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect_err("snapshot overwrite rejected");
    assert_eq!(error.code, "destination_unsafe");
    assert!(snapshot.exists(), "snapshot untouched");
}

#[test]
fn review_export_reports_counts_before_any_saves() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "rich.epub", &files);
    let book = import_rich_book(&data_dir, &source);

    let review = services::export::review_export(&data_dir, &book.project_id).expect("review");
    assert_eq!(review.pending_total, 2); // ch1: 4 blocks, ch2: 3 blocks
    assert_eq!(review.reviewed_total, 0);
    assert!(!review.complete);
    assert_eq!(review.chapters.len(), 2);
}

#[test]
fn rich_translation_images_survive_save_export_and_reopen() {
    use base64::Engine;
    let dir = tempdir();
    let data_dir = tempdir();
    let mut files = rich_book_files();
    let original_png = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aIu0AAAAASUVORK5CYII=").expect("png");
    files
        .iter_mut()
        .find(|(name, _)| name == "OEBPS/images/river.png")
        .expect("asset")
        .1 = original_png;

    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "rich.epub", &refs);
    let before = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let content =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("chapter");
    assert!(content.preview_html.contains("<img"));
    assert_eq!(content.images.len(), 1);
    assert!(content.images[0].src.starts_with("data:image/png;base64,"));
    assert!(content.source_document.contains("<em>softly</em>"));
    let png = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aIu0AAAAASUVORK5CYII=").expect("png");
    let src = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    );
    let html = format!(
        "<strong>မြန်မာ &amp; English</strong><br/><em>Translation</em><img src=\"{src}\" alt=\"New image\"/>"
    );
    let mut conn = services::workspace::open_db(&data_dir).expect("db");
    let linked = &content.blocks[2];
    let error = sagar_lib::storage::translations::save_rich_translation(
        &mut conn,
        linked.id.clone(),
        Some("Lost link".into()),
        "draft".into(),
        0,
    )
    .expect_err("links must survive");
    assert_eq!(error.code, "missing_source_link");
    sagar_lib::storage::translations::save_rich_translation(
        &mut conn,
        linked.id.clone(),
        Some("<a href=\"ch2.xhtml#note-1\">Translated note</a>".into()),
        "draft".into(),
        0,
    )
    .expect("preserved link");
    let block = &content.blocks[1];
    sagar_lib::storage::translations::save_rich_translation(
        &mut conn,
        block.id.clone(),
        Some(html.clone()),
        "draft".into(),
        0,
    )
    .expect("rich save");
    let reloaded =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("reload");
    assert_eq!(
        reloaded.blocks[1].target_html.as_deref(),
        Some(html.as_str())
    );
    assert!(
        sagar_lib::storage::translations::save_rich_translation(
            &mut conn,
            block.id.clone(),
            Some("<script>bad</script>".into()),
            "reviewed".into(),
            1
        )
        .is_err()
    );
    assert!(
        sagar_lib::storage::translations::save_rich_translation(
            &mut conn,
            block.id.clone(),
            Some("stale".into()),
            "draft".into(),
            0
        )
        .is_err()
    );
    let destination = dir.join("translated.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id.clone(),
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export");
    epub::export::validate_output(&destination).expect("valid output");
    assert_eq!(std::fs::read(source).expect("source"), before);
    let file = std::fs::File::open(&destination).expect("output");
    let mut archive = zip::ZipArchive::new(file).expect("zip");
    assert_eq!(
        archive.by_index(0).expect("first").name().expect("name"),
        "mimetype"
    );
    let mut chapter = String::new();
    archive
        .by_name("OEBPS/ch1.xhtml")
        .expect("chapter")
        .read_to_string(&mut chapter)
        .expect("read");
    assert!(chapter.contains("<strong>မြန်မာ &amp; English</strong>"));
    assert!(!chapter.contains("data:image"));
    let doc = roxmltree::Document::parse(&chapter).expect("xml");
    let inserted = doc
        .descendants()
        .filter(|n| n.has_tag_name(("http://www.w3.org/1999/xhtml", "img")))
        .find(|n| n.attribute("alt") == Some("New image"))
        .expect("image");
    let path = format!("OEBPS/{}", inserted.attribute("src").expect("src"));
    let mut bytes = Vec::new();
    archive
        .by_name(&path)
        .expect("image asset")
        .read_to_end(&mut bytes)
        .expect("read image");
    assert_eq!(bytes, png);
    let mut opf = String::new();
    archive
        .by_name("OEBPS/content.opf")
        .expect("opf")
        .read_to_string(&mut opf)
        .expect("read");
    assert!(opf.contains("sagar-images/"));
    epub::inspect(&destination).expect("reopen exported epub");
}

#[test]
fn empty_image_chapter_accepts_free_document_and_exports_it() {
    use base64::Engine;
    let dir = tempdir();
    let data_dir = tempdir();
    let mut files = rich_book_files();
    let png=base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aIu0AAAAASUVORK5CYII=").expect("png");
    files
        .iter_mut()
        .find(|(n, _)| n == "OEBPS/images/river.png")
        .expect("image")
        .1 = png.clone();
    files
        .iter_mut()
        .find(|(n, _)| n == "OEBPS/ch1.xhtml")
        .expect("chapter")
        .1 = chapter_doc(
        "Cover",
        "<figure id=\"cover-anchor\"><img src=\"images/river.png\" alt=\"Cover\"/></figure>",
    )
    .into_bytes();
    // chapter_doc adds a heading; remove it to reproduce an image-only chapter.
    let entry = files
        .iter_mut()
        .find(|(n, _)| n == "OEBPS/ch1.xhtml")
        .expect("chapter");
    entry.1 = String::from_utf8(entry.1.clone())
        .expect("utf8")
        .replace("<h1 epub:type=\"title\">Cover</h1>", "")
        .into_bytes();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let chapter =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("load");
    assert!(chapter.blocks.is_empty());
    assert!(chapter.document.html.is_none());
    assert_eq!(chapter.document.status, "pending");
    assert_eq!(chapter.images.len(), 1);
    let html = format!(
        "<h2>My own title</h2><p>မြန်မာ <strong>text</strong></p><p>Another freely written paragraph.</p><ul><li><p>A new list item</p></li></ul><img src=\"{}\" alt=\"Illustration\"/>",
        chapter.images[0].src
    );
    let saved = services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(html.clone()),
        "draft",
        0,
    )
    .expect("save");
    assert_eq!(saved.revision, 1);
    let loaded =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("restart");
    assert_eq!(loaded.document.html.as_deref(), Some(html.as_str()));
    assert_eq!(
        services::workspace::save_chapter_document(
            &data_dir,
            &book.chapter_ids[0],
            Some("<p>stale</p>".into()),
            "draft",
            0
        )
        .expect_err("stale")
        .code,
        "revision_conflict"
    );
    assert!(
        services::workspace::save_chapter_document(
            &data_dir,
            &book.chapter_ids[0],
            Some("<script>bad</script>".into()),
            "draft",
            1
        )
        .is_err()
    );
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(html.clone()),
        "skipped",
        1,
    )
    .expect("skip");
    let skipped =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("skipped");
    assert_eq!(skipped.document.html.as_deref(), Some(html.as_str()));
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(html.clone()),
        "reviewed",
        2,
    )
    .expect("review");
    services::workspace::save_chapter_document(&data_dir, &book.chapter_ids[1], None, "skipped", 0)
        .expect("keep other chapter");
    let review =
        services::export::review_export(&data_dir, &book.project_id).expect("review report");
    assert!(review.complete);
    assert_eq!(review.reviewed_total, 1);
    assert_eq!(review.skipped_total, 1);
    let destination = dir.join("translated.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id.clone(),
            destination: destination.clone(),
            mode: "completed".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export");
    let file = std::fs::File::open(&destination).expect("output");
    let mut zip = zip::ZipArchive::new(file).expect("archive");
    let mut output = String::new();
    zip.by_name("OEBPS/ch1.xhtml")
        .expect("chapter")
        .read_to_string(&mut output)
        .expect("read");
    assert!(output.contains("<h2>My own title</h2>"));
    assert!(output.contains("<ul><li><p>A new list item</p></li></ul>"));
    assert!(output.contains("id=\"cover-anchor\""));
    assert!(!output.contains("data:image"));
    assert!(!output.contains("<figure"));
    let mut second = Vec::new();
    zip.by_name("OEBPS/ch2.xhtml")
        .expect("second")
        .read_to_end(&mut second)
        .expect("read");
    assert_eq!(
        second,
        files
            .iter()
            .find(|(n, _)| n == "OEBPS/ch2.xhtml")
            .expect("original")
            .1
    );
    assert_eq!(std::fs::read(source).expect("source after"), original);
    epub::inspect(&destination).expect("reopen");
}

#[test]
fn legacy_drafts_load_without_erasing_original_block_records() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let book = import_rich_book(&data_dir, &source);
    save_block(&book, 0, 1, "Existing translation", "draft");
    let loaded =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("chapter");
    assert_eq!(
        loaded.document.html.as_deref(),
        Some("<p>Existing translation</p>")
    );
    assert!(!loaded.document.authored);
    assert_eq!(
        loaded.blocks[1].target_plain_text.as_deref(),
        Some("Existing translation")
    );
}

#[test]
fn draft_export_includes_reviewed_and_unreviewed_chapter_documents() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let book = import_rich_book(&data_dir, &source);
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some("<p>Reviewed translation</p>".into()),
        "reviewed",
        0,
    )
    .expect("reviewed chapter");
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[1],
        Some("<p>Unreviewed draft translation</p>".into()),
        "draft",
        0,
    )
    .expect("draft chapter");
    let destination = dir.join("draft.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("draft export");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(destination).expect("output")).expect("archive");
    for (path, translation) in [
        ("OEBPS/ch1.xhtml", "Reviewed translation"),
        ("OEBPS/ch2.xhtml", "Unreviewed draft translation"),
    ] {
        let mut html = String::new();
        archive
            .by_name(path)
            .expect("chapter")
            .read_to_string(&mut html)
            .expect("read");
        assert!(
            html.contains(translation),
            "draft must include {translation}"
        );
    }
}

#[test]
fn book_semantics_and_table_sections_save_export_and_reopen() {
    let dir = tempdir();
    let data_dir = dir.join("data");
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let html = include_str!("../../tests/fixtures/editor/book-compatibility.xhtml").trim();
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(html.into()),
        "draft",
        0,
    )
    .expect("save");
    let loaded =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("reload");
    assert_eq!(loaded.document.html.as_deref(), Some(html));
    let destination = dir.join("compatible.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&destination).expect("file")).expect("zip");
    let mut output = String::new();
    archive
        .by_name("OEBPS/ch1.xhtml")
        .expect("chapter")
        .read_to_string(&mut output)
        .expect("read");
    assert!(output.contains(html));
    epub::inspect(&destination).expect("reopen exported EPUB");
    assert_eq!(std::fs::read(source).expect("source after"), original);
}

#[test]
fn chapter_heading_group_and_parent_save_export_and_reopen() {
    let dir = tempdir();
    let data_dir = dir.join("data");
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let html = include_str!("../../tests/fixtures/editor/chapter-heading-group.xhtml").trim();
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(html.into()),
        "draft",
        0,
    )
    .expect("save");
    let loaded =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("reload");
    let saved = loaded.document.html.as_deref().expect("saved HTML");
    let document = roxmltree::Document::parse(saved).expect("saved XHTML");
    assert_eq!(
        document.root_element().attribute("data-parent"),
        Some("part-1")
    );
    assert_eq!(
        document.root_element().attribute("role"),
        Some("doc-chapter")
    );
    assert!(
        document
            .descendants()
            .any(|n| n.has_tag_name("hgroup") && n.attribute("class") == Some("has-h3"))
    );
    assert!(document.descendants().any(|n| n.has_tag_name("abbr")
        && n.text() == Some("Mr.")
        && n.attribute(("http://www.idpf.org/2007/ops", "type")) == Some("z3998:name-title")));
    let destination = dir.join("compatible.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&destination).expect("file")).expect("zip");
    let mut output = String::new();
    archive
        .by_name("OEBPS/ch1.xhtml")
        .expect("chapter")
        .read_to_string(&mut output)
        .expect("read");
    assert!(output.contains(saved));
    epub::inspect(&destination).expect("reopen exported EPUB");
    assert_eq!(std::fs::read(source).expect("source after"), original);
}

#[test]
fn borderless_table_styles_save_export_and_reopen() {
    let dir = tempdir();
    let data_dir = dir.join("data");
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let html = include_str!("../../tests/fixtures/editor/borderless-table.xhtml").trim();
    let (validated, _) = epub::rich_text::validate_document(html).expect("validate styled table");
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(html.into()),
        "draft",
        0,
    )
    .expect("save styled table");
    let loaded =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("reload styled table");
    assert_eq!(loaded.document.html.as_deref(), Some(validated.as_str()));
    let destination = dir.join("borderless.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export styled table");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&destination).expect("file")).expect("zip");
    let mut output = String::new();
    archive
        .by_name("OEBPS/ch1.xhtml")
        .expect("chapter")
        .read_to_string(&mut output)
        .expect("read");
    assert!(output.contains(&validated));
    assert!(!output.contains("data-book-borderless"));
    epub::inspect(&destination).expect("reopen exported EPUB");
    assert_eq!(std::fs::read(source).expect("unchanged source"), original);
}

#[test]
fn unsupported_chapter_html_with_empty_link_saves_reloads_and_exports() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let html = "<h1>ခေါင်းစဉ်</h1><p><font color=\"red\">မြန်မာ</font><a href=\"\">လင့်</a></p><center><p>Centered</p></center>";
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(html.into()),
        "draft",
        0,
    )
    .expect("unsupported HTML is kept");
    let loaded =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("reload");
    assert_eq!(loaded.document.html.as_deref(), Some(html));

    let destination = dir.join("permissive.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export permissive HTML");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&destination).expect("file")).expect("zip");
    let mut output = String::new();
    archive
        .by_name("OEBPS/ch1.xhtml")
        .expect("chapter")
        .read_to_string(&mut output)
        .expect("read");
    assert!(
        output.contains("<font color=\"red\">မြန်မာ</font>"),
        "{output}"
    );
    assert!(output.contains("<a href=\"\">လင့်</a>"), "{output}");
    assert!(output.contains("<center>"), "{output}");
    epub::inspect(&destination).expect("reopen");
    assert_eq!(std::fs::read(&source).expect("unchanged source"), original);
}

#[test]
fn malformed_or_invalid_image_chapter_exports_original_with_warning() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);

    let malformed = "<p>မြန်မာ <em>unclosed";
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(malformed.into()),
        "draft",
        0,
    )
    .expect("malformed HTML is kept in the draft");
    let loaded =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("reload");
    assert_eq!(loaded.document.html.as_deref(), Some(malformed));

    let destination = dir.join("malformed.epub");
    let outcome = services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id.clone(),
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export falls back instead of aborting");
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| warning.code == "chapter_translation_unusable"),
        "{:?}",
        outcome.warnings
    );
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&destination).expect("file")).expect("zip");
    let mut output = String::new();
    archive
        .by_name("OEBPS/ch1.xhtml")
        .expect("chapter")
        .read_to_string(&mut output)
        .expect("read");
    assert!(output.contains("River <em>softly</em>."), "{output}");
    assert!(!output.contains("unclosed"), "{output}");
    epub::inspect(&destination).expect("reopen");

    let invalid_image = "<p>Text<img src=\"data:image/png;base64,notbase64\" alt=\"Bad\"/></p>";
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(invalid_image.into()),
        "draft",
        1,
    )
    .expect("invalid image data is kept in the draft");
    let destination = dir.join("invalid-image.epub");
    let outcome = services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("invalid image falls back instead of aborting");
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| warning.code == "chapter_translation_unusable"),
        "{:?}",
        outcome.warnings
    );
    epub::inspect(&destination).expect("reopen invalid-image fallback");
    assert_eq!(std::fs::read(&source).expect("unchanged source"), original);
}

#[test]
fn authored_tables_save_reload_export_and_reopen() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let empty =
        "<table><tbody><tr><td colspan=\"1\" rowspan=\"1\"><p></p></td></tr></tbody></table>";
    let saved = services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(empty.into()),
        "draft",
        0,
    )
    .expect("save empty table");
    assert_eq!(saved.status, "draft");
    let loaded =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("reload");
    assert_eq!(loaded.document.html.as_deref(), Some(empty));
    let html = "<h1>Table chapter</h1><table><tbody><tr><th><p>Key</p></th><th><p>Translation</p></th></tr><tr><td><p>A</p></td><td><p>မြန်မာ &amp; text</p></td></tr></tbody></table>";
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(html.into()),
        "reviewed",
        1,
    )
    .expect("review table");
    let destination = dir.join("table.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&destination).expect("file")).expect("zip");
    let mut output = String::new();
    archive
        .by_name("OEBPS/ch1.xhtml")
        .expect("chapter")
        .read_to_string(&mut output)
        .expect("read");
    assert!(output.contains(html));
    epub::inspect(&destination).expect("reopen");
    assert_eq!(std::fs::read(source).expect("source after"), original);
}

#[test]
fn draft_export_without_originals_omits_untranslated_chapters() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let book = import_rich_book(&data_dir, &source);
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some("<h1>Translated intro</h1><p>Translated body</p>".into()),
        "reviewed",
        0,
    )
    .expect("translate first chapter");
    // Second chapter stays pending; a skipped chapter must also be omitted.
    // (Skipped is covered by the pending path: no saved translation.)

    let destination = dir.join("translated-only.epub");
    let outcome = services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id.clone(),
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: false,
            format: "epub".into(),
        },
    )
    .expect("filtered draft export");
    assert_eq!(outcome.chapters_exported, 1);
    assert!(
        outcome
            .warnings
            .iter()
            .any(|w| w.code == "excluded_untranslated"),
        "warnings report omission: {:?}",
        outcome.warnings
    );
    epub::export::validate_output(&destination).expect("filtered output valid");

    let file = std::fs::File::open(&destination).expect("output");
    let mut zip = zip::ZipArchive::new(file).expect("zip");
    assert!(
        zip.by_name("OEBPS/ch2.xhtml").is_err(),
        "untranslated chapter file must be omitted"
    );
    let mut ch1 = String::new();
    zip.by_name("OEBPS/ch1.xhtml")
        .expect("translated chapter kept")
        .read_to_string(&mut ch1)
        .expect("read");
    assert!(ch1.contains("Translated body"), "{ch1}");

    let mut opf = String::new();
    zip.by_name("OEBPS/content.opf")
        .expect("opf")
        .read_to_string(&mut opf)
        .expect("read opf");
    assert!(opf.contains("ch1.xhtml"), "{opf}");
    assert!(
        !opf.contains("ch2.xhtml"),
        "OPF must not reference omitted file: {opf}"
    );

    let mut nav = String::new();
    zip.by_name("OEBPS/nav.xhtml")
        .expect("nav")
        .read_to_string(&mut nav)
        .expect("read nav");
    assert!(
        !nav.contains(">Ten</a>"),
        "TOC omits untranslated entry: {nav}"
    );

    // Reimporting the filtered draft yields only the translated chapter.
    let reimported =
        services::import::import_epub(&data_dir, &destination, true).expect("reimport");
    let hrefs: Vec<&str> = reimported
        .chapters
        .iter()
        .map(|c| c.resource_href.as_str())
        .collect();
    assert_eq!(hrefs, vec!["OEBPS/ch1.xhtml"]);
}

#[test]
fn draft_export_without_originals_requires_a_translation() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let book = import_rich_book(&data_dir, &source);

    let error: AppError = services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: dir.join("empty.epub"),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: false,
            format: "epub".into(),
        },
    )
    .expect_err("empty filtered export must fail");
    assert_eq!(error.code, "nothing_to_export");
}

#[test]
fn book_settings_persist_instructions_and_export_metadata_without_changing_source() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "book-settings.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let mut settings = services::book_settings::get(&data_dir, &book.project_id).expect("settings");
    assert_eq!(
        settings.batch_delay_secs, 10,
        "new books pause 10 seconds by default"
    );
    settings.batch_delay_secs = 45;
    settings.title = "မြန်မာစာ & Stories".into();
    settings.author = "မောင်မောင် & Co".into();
    settings.publisher = "Local <Press>".into();
    settings.description = "စာအုပ် description & details".into();
    settings.instructions = "Use ဖန်ရှန်း for Fan Xian.".into();
    services::book_settings::save(&data_dir, &book.project_id, settings.clone()).expect("save");
    let loaded = services::book_settings::get(&data_dir, &book.project_id).expect("reopen");
    assert_eq!(loaded.title, settings.title);
    assert_eq!(loaded.instructions, settings.instructions);
    assert_eq!(loaded.batch_delay_secs, 45);
    let mut too_long = settings.clone();
    too_long.batch_delay_secs = 3601;
    assert!(services::book_settings::save(&data_dir, &book.project_id, too_long).is_err());
    let conn = services::workspace::open_db(&data_dir).expect("db");
    let ai_source =
        sagar_lib::storage::ai_translate::translation_source(&conn, &book.chapter_ids[0])
            .expect("AI source");
    assert_eq!(ai_source.instructions, settings.instructions);
    let other = services::import::import_epub(&data_dir, &source, true).expect("separate book");
    assert!(
        services::book_settings::get(&data_dir, &other.project.id)
            .expect("other")
            .instructions
            .is_empty()
    );
    let mut invalid = settings.clone();
    invalid.target_language = "my MM!".into();
    invalid.title = "Should not persist".into();
    assert!(services::book_settings::save(&data_dir, &book.project_id, invalid).is_err());
    assert_eq!(
        services::book_settings::get(&data_dir, &book.project_id)
            .expect("unchanged")
            .title,
        settings.title
    );
    save_block(&book, 0, 0, "မြန်မာ", "draft");
    let destination = dir.join("with-metadata.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id.clone(),
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export saved settings");
    let mut zip =
        zip::ZipArchive::new(std::fs::File::open(&destination).expect("output")).expect("zip");
    let mut opf = String::new();
    zip.by_name("OEBPS/content.opf")
        .expect("opf")
        .read_to_string(&mut opf)
        .expect("read");
    let doc = roxmltree::Document::parse(&opf).expect("XML");
    for (name, expected) in [
        ("title", settings.title.as_str()),
        ("creator", settings.author.as_str()),
        ("publisher", settings.publisher.as_str()),
        ("description", settings.description.as_str()),
    ] {
        assert_eq!(
            doc.descendants()
                .find(|n| n.has_tag_name(("http://purl.org/dc/elements/1.1/", name)))
                .and_then(|n| n.text()),
            Some(expected)
        );
    }
    assert!(!opf.contains("Use ဖန်ရှန်း"));
    let reopened = services::import::import_epub(&data_dir, &destination, true).expect("reimport");
    assert_eq!(reopened.project.title, settings.title);
    assert_eq!(
        reopened.project.author.as_deref(),
        Some(settings.author.as_str())
    );
    assert_eq!(std::fs::read(&source).expect("source intact"), original);
    let snapshot =
        services::workspace::snapshot_path(&data_dir, &book.project_id).expect("snapshot");
    assert_eq!(std::fs::read(snapshot).expect("snapshot intact"), original);
}

#[test]
fn export_rejects_original_path_even_when_original_has_changed() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let book = import_rich_book(&data_dir, &source);
    std::fs::write(&source, b"changed original").unwrap();
    let error = services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: source.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect_err("original path protected");
    assert_eq!(error.code, "destination_unsafe");
    assert_eq!(std::fs::read(source).unwrap(), b"changed original");
}

#[test]
fn legacy_original_hash_is_protected_and_filtered_links_block_export() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let book = import_rich_book(&data_dir, &source);
    let conn = sagar_lib::storage::open_database(&data_dir.join("sagar.db")).unwrap();
    conn.execute(
        "UPDATE projects SET original_source_path=NULL WHERE id=?1",
        [&book.project_id],
    )
    .unwrap();
    save_block(&book, 0, 0, "Translated", "reviewed");
    for (destination, expected) in [
        (source.clone(), "destination_unsafe"),
        (dir.join("filtered.epub"), "export_broken_reference"),
    ] {
        let error = services::export::export_epub(
            &data_dir,
            ExportRequest {
                project_id: book.project_id.clone(),
                destination: destination.clone(),
                mode: "draft".into(),
                title: None,
                language: None,
                include_original: false,
                format: "epub".into(),
            },
        )
        .expect_err("unsafe export rejected");
        assert_eq!(error.code, expected);
        if destination != source {
            assert!(!destination.exists());
        }
    }
    // A failed validation must preserve an existing output file too.
    let destination = dir.join("existing.epub");
    std::fs::write(&destination, b"previous output").unwrap();
    assert!(
        services::export::export_epub(
            &data_dir,
            ExportRequest {
                project_id: book.project_id,
                destination: destination.clone(),
                mode: "draft".into(),
                title: None,
                language: None,
                include_original: false,
                format: "epub".into(),
            }
        )
        .is_err()
    );
    assert_eq!(std::fs::read(destination).unwrap(), b"previous output");
}

#[test]
fn authored_heading_updates_nested_fragment_navigation() {
    let dir = tempdir();
    let data_dir = tempdir();
    let mut files = rich_book_files();
    let nav = files
        .iter_mut()
        .find(|(name, _)| name == "OEBPS/nav.xhtml")
        .unwrap();
    nav.1 = String::from_utf8(nav.1.clone())
        .unwrap()
        .replace("ch1.xhtml", "../ch1.xhtml#heading")
        .replace("ch2.xhtml", "../ch2.xhtml")
        .into_bytes();
    nav.0 = "OEBPS/navigation/nav.xhtml".into();
    let opf = files
        .iter_mut()
        .find(|(name, _)| name == "OEBPS/content.opf")
        .unwrap();
    opf.1 = String::from_utf8(opf.1.clone())
        .unwrap()
        .replace("href=\"nav.xhtml\"", "href=\"navigation/nav.xhtml\"")
        .into_bytes();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let book = import_rich_book(&data_dir, &source);
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some("<h1 id=\"heading\">မြန်မာ <em>ခေါင်းစဉ်</em></h1><p>Text</p>".into()),
        "reviewed",
        0,
    )
    .unwrap();
    let destination = dir.join("out.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(destination).unwrap()).unwrap();
    let mut nav = String::new();
    zip.by_name("OEBPS/navigation/nav.xhtml")
        .unwrap()
        .read_to_string(&mut nav)
        .unwrap();
    assert!(nav.contains(">မြန်မာ ခေါင်းစဉ်</a>"), "{nav}");
    assert!(nav.contains("../ch1.xhtml#heading"));
}

#[test]
fn validator_rejects_missing_assets_invalid_navigation_and_missing_anchors() {
    for case in 0..3 {
        let dir = tempdir();
        let mut files = rich_book_files();
        match case {
            0 => files.retain(|(name, _)| name != "OEBPS/images/river.png"),
            1 => {
                files
                    .iter_mut()
                    .find(|(name, _)| name == "OEBPS/nav.xhtml")
                    .unwrap()
                    .1 = b"<html>".to_vec()
            }
            _ => {
                let chapter = files
                    .iter_mut()
                    .find(|(name, _)| name == "OEBPS/ch1.xhtml")
                    .unwrap();
                chapter.1 = String::from_utf8(chapter.1.clone())
                    .unwrap()
                    .replace("#note-1", "#missing")
                    .into_bytes();
            }
        }
        let refs: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        let path = write_epub(&dir, "invalid.epub", &refs);
        assert!(epub::export::validate_output(&path).is_err(), "case {case}");
    }
}

#[test]
fn custom_cover_persists_exports_restores_and_keeps_source_immutable() {
    use base64::Engine;
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jvJkAAAAASUVORK5CYII=";
    let png = image;
    for has_cover in [true, false] {
        let image = if has_cover {
            "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7"
        } else {
            png
        };
        let expected_mime = if has_cover { "image/gif" } else { "image/png" };
        let dir = tempdir();
        let data_dir = tempdir();
        let mut files = rich_book_files();
        let opf = files
            .iter_mut()
            .find(|(n, _)| n == "OEBPS/content.opf")
            .unwrap();
        let mut text = String::from_utf8(opf.1.clone()).unwrap();
        if has_cover {
            text = text.replace(
                "</metadata>",
                "<meta name=\"cover\" content=\"cover-img\"/></metadata>",
            );
        } else {
            text = text.replace("  <item id=\"cover-img\" href=\"images/cover.png\" media-type=\"image/png\" properties=\"cover-image\"/>", "");
        }
        opf.1 = text.into_bytes();
        if has_cover {
            let chapter = files
                .iter_mut()
                .find(|(n, _)| n == "OEBPS/ch1.xhtml")
                .unwrap();
            chapter.1 = String::from_utf8(chapter.1.clone())
                .unwrap()
                .replace("images/river.png", "images/cover.png")
                .replace("</body>", "<object data=\"images/cover.png\" type=\"image/png\"/><svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><image xlink:href=\"images/cover.png\"/></svg></body>")
                .into_bytes();
        }
        let refs: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        let source = write_epub(&dir, "source.epub", &refs);
        let original = std::fs::read(&source).unwrap();
        let book = import_rich_book(&data_dir, &source);
        let mut settings = services::book_settings::get(&data_dir, &book.project_id).unwrap();
        assert!(settings.custom_cover.is_none());
        settings.custom_cover = Some(image.into());
        services::book_settings::save(&data_dir, &book.project_id, settings.clone()).unwrap();
        assert_eq!(
            services::book_settings::get(&data_dir, &book.project_id)
                .unwrap()
                .custom_cover
                .as_deref(),
            Some(image)
        );
        let cover = services::projects::get_cover(&data_dir, &book.project_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            format!("data:{};base64,{}", cover.media_type, cover.base64),
            image
        );
        let destination = dir.join("custom.epub");
        let export = |destination: PathBuf| {
            services::export::export_epub(
                &data_dir,
                ExportRequest {
                    project_id: book.project_id.clone(),
                    destination,
                    mode: "draft".into(),
                    title: None,
                    language: None,
                    include_original: true,
                    format: "epub".into(),
                },
            )
        };
        export(destination.clone()).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&destination).unwrap()).unwrap();
        let mut opf = String::new();
        zip.by_name("OEBPS/content.opf")
            .unwrap()
            .read_to_string(&mut opf)
            .unwrap();
        let doc = roxmltree::Document::parse(&opf).unwrap();
        let covers: Vec<_> = doc
            .descendants()
            .filter(|n| {
                n.attribute("properties")
                    .is_some_and(|p| p.split_whitespace().any(|p| p == "cover-image"))
            })
            .collect();
        assert_eq!(covers.len(), 1);
        let href = covers[0].attribute("href").unwrap();
        assert_eq!(covers[0].attribute("media-type"), Some(expected_mime));
        let mut bytes = Vec::new();
        zip.by_name(&format!("OEBPS/{href}"))
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(
            bytes,
            base64::engine::general_purpose::STANDARD
                .decode(image.split_once(',').unwrap().1)
                .unwrap()
        );
        if has_cover {
            let mut chapter = String::new();
            zip.by_name("OEBPS/ch1.xhtml")
                .unwrap()
                .read_to_string(&mut chapter)
                .unwrap();
            assert!(chapter.contains(href));
            let chapter_doc = roxmltree::Document::parse(&chapter).unwrap();
            assert!(chapter_doc.descendants().any(|node| {
                node.has_tag_name(("http://www.w3.org/1999/xhtml", "object"))
                    && node.attribute("data") == Some(href)
                    && node.attribute("type") == Some(expected_mime)
            }));
            assert!(chapter_doc.descendants().any(|node| {
                node.has_tag_name(("http://www.w3.org/2000/svg", "image"))
                    && node.attribute(("http://www.w3.org/1999/xlink", "href")) == Some(href)
            }));
            assert!(
                doc.descendants()
                    .any(|n| n.attribute("name") == Some("cover")
                        && n.attribute("content") == covers[0].attribute("id"))
            );
        }
        let mut invalid = settings.clone();
        invalid.custom_cover = Some("data:image/png;base64,bm90IGFuIGltYWdl".into());
        assert!(services::book_settings::save(&data_dir, &book.project_id, invalid).is_err());
        assert_eq!(
            services::book_settings::get(&data_dir, &book.project_id)
                .unwrap()
                .custom_cover,
            settings.custom_cover
        );
        settings.custom_cover = None;
        services::book_settings::save(&data_dir, &book.project_id, settings).unwrap();
        let restored = dir.join("restored.epub");
        export(restored.clone()).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(restored).unwrap()).unwrap();
        let mut restored_opf = String::new();
        zip.by_name("OEBPS/content.opf")
            .unwrap()
            .read_to_string(&mut restored_opf)
            .unwrap();
        assert!(!restored_opf.contains("sagar-cover-"));
        assert_eq!(std::fs::read(&source).unwrap(), original);
        epub::inspect(&destination).unwrap();
    }
}

#[test]
fn svg_cover_with_html_doctype_loads_and_exports_unchanged() {
    let dir = tempdir();
    let data_dir = tempdir();
    let mut files = rich_book_files();
    let cover = r#"<!DOCTYPE html><html xmlns="http://www.w3.org/1999/xhtml"><head><title>Cover</title></head><body><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 800 1104"><image width="800" height="1104" xlink:href="images/cover.png"/></svg></body></html>"#;
    files
        .iter_mut()
        .find(|(n, _)| n == "OEBPS/ch1.xhtml")
        .expect("chapter")
        .1 = cover.as_bytes().to_vec();
    // Fixture PNGs are stub bytes without a real image signature; use a
    // genuine 1px PNG so the picker mime check accepts the SVG reference.
    use base64::Engine as _;
    let png = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aIu0AAAAASUVORK5CYII=")
        .expect("png");
    files
        .iter_mut()
        .find(|(n, _)| n == "OEBPS/images/cover.png")
        .expect("cover image")
        .1 = png;
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "directories.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let chapter =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("load SVG cover");
    assert!(chapter.preview_html.contains("<svg"));
    assert!(chapter.preview_html.contains("/OEBPS/images/cover.png"));
    assert_eq!(chapter.images.len(), 1);
    // Pasting the original ebookmaker cover markup into the translation must
    // validate (the reported bug): save it as the chapter document here and
    // check the exported book below keeps the SVG image reference intact.
    let translation = "<div class=\"x-ebookmaker-cover\"><svg xmlns=\"http://www.w3.org/2000/svg\" height=\"100%\" preserveAspectRatio=\"xMidYMid meet\" version=\"1.1\" viewBox=\"0 0 800 1104\" width=\"100%\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><image width=\"800\" height=\"1104\" xlink:href=\"images/cover.png\"/></svg></div>";
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(translation.into()),
        "reviewed",
        0,
    )
    .expect("save translated cover");
    services::workspace::save_chapter_document(&data_dir, &book.chapter_ids[1], None, "skipped", 0)
        .expect("keep other chapter");
    let destination = dir.join("export.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export with directories");
    epub::export::validate_output(&destination).expect("valid export");
    let mut output =
        zip::ZipArchive::new(std::fs::File::open(&destination).expect("output")).expect("zip");
    let mut exported_cover = String::new();
    output
        .by_name("OEBPS/ch1.xhtml")
        .expect("translated cover")
        .read_to_string(&mut exported_cover)
        .expect("read");
    assert!(
        exported_cover.contains("x-ebookmaker-cover"),
        "{exported_cover}"
    );
    assert!(exported_cover.contains("<svg"), "{exported_cover}");
    assert!(
        exported_cover.contains("images/cover.png"),
        "{exported_cover}"
    );
    assert!(!exported_cover.contains("data:image"), "{exported_cover}");
    for (name, bytes) in &files {
        if name.ends_with('/') || name.ends_with(".opf") || name == "OEBPS/ch1.xhtml" {
            continue;
        }
        let mut actual = Vec::new();
        output
            .by_name(name)
            .expect("resource retained")
            .read_to_end(&mut actual)
            .expect("read");
        assert_eq!(&actual, bytes, "resource {name}");
    }
    assert_eq!(std::fs::read(source).expect("source unchanged"), original);
    epub::inspect(&destination).expect("reopen exported epub");
}

#[test]
fn xhtml11_doctype_imports_editable_content_and_survives_export() {
    let dir = tempdir();
    let data = tempdir();
    let mut files = rich_book_files();
    let declaration = r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">"#;
    let chapter = format!(
        "{declaration}<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Legacy</title></head><body><p id=\"text\">မြန်မာ <em>text</em> &#x2019;</p></body></html>"
    );
    files
        .iter_mut()
        .find(|(n, _)| n == "OEBPS/ch1.xhtml")
        .expect("chapter")
        .1 = chapter.as_bytes().to_vec();
    let refs: Vec<_> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "legacy.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let result = services::import::import_epub(&data, &source, false).expect("import");
    assert!(!result.warnings.iter().any(|w| w.code == "chapter_invalid"));
    let loaded =
        services::workspace::get_chapter(&data, &result.project.id, &result.chapters[0].id)
            .expect("load chapter");
    assert!(loaded.preview_html.contains("မြန်မာ"));
    let parsed = epub::inspect(&source).expect("inspect");
    assert!(
        parsed.chapters[0]
            .blocks
            .iter()
            .any(|b| b.supported && b.is_translatable)
    );
    let destination = dir.join("output.epub");
    services::export::export_epub(
        &data,
        ExportRequest {
            project_id: result.project.id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export");
    epub::export::validate_output(&destination).expect("validate");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(destination).expect("output")).expect("archive");
    let mut output = String::new();
    archive
        .by_name("OEBPS/ch1.xhtml")
        .expect("chapter")
        .read_to_string(&mut output)
        .expect("read");
    assert_eq!(output, chapter);
    assert_eq!(std::fs::read(source).expect("source"), original);
}

#[test]
fn aligned_div_save_export_and_reopen() {
    let dir = tempdir();
    let data_dir = dir.join("data");
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let html = include_str!("../../tests/fixtures/editor/aligned-div.xhtml").trim();
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(html.into()),
        "draft",
        0,
    )
    .expect("save");
    let loaded =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("reload");
    let saved = loaded.document.html.as_deref().expect("saved HTML");
    let document = roxmltree::Document::parse(saved).expect("saved XHTML");
    assert_eq!(document.root_element().attribute("align"), Some("left"));
    assert!(saved.contains("&lt; အခန်းအမှားတင်ပြရန် &gt;"));
    let destination = dir.join("compatible.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&destination).expect("file")).expect("zip");
    let mut output = String::new();
    archive
        .by_name("OEBPS/ch1.xhtml")
        .expect("chapter")
        .read_to_string(&mut output)
        .expect("read");
    assert!(output.contains(saved));
    epub::inspect(&destination).expect("reopen exported EPUB");
    assert_eq!(std::fs::read(source).expect("source after"), original);
}

#[test]
fn heading_group_and_time_save_export_and_reopen() {
    let dir = tempdir();
    let data_dir = dir.join("data");
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let original = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let source_html = include_str!("../../tests/fixtures/editor/selection-time.xhtml");
    let units = epub::selection::collect(source_html).expect("selection");
    assert!(units.iter().all(|unit| unit.unavailable.is_none()));
    let assembled = epub::selection::assemble(
        &units
            .into_iter()
            .map(|unit| {
                let html = unit.html.clone();
                (unit, html)
            })
            .collect::<Vec<_>>(),
    );
    let html = assembled.as_str();
    services::workspace::save_chapter_document(
        &data_dir,
        &book.chapter_ids[0],
        Some(html.into()),
        "draft",
        0,
    )
    .expect("save");
    let loaded =
        services::workspace::get_chapter(&data_dir, &book.project_id, &book.chapter_ids[0])
            .expect("reload");
    assert_eq!(loaded.document.html.as_deref(), Some(html));
    let destination = dir.join("compatible.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id,
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&destination).expect("file")).expect("zip");
    let mut output = String::new();
    archive
        .by_name("OEBPS/ch1.xhtml")
        .expect("chapter")
        .read_to_string(&mut output)
        .expect("read");
    assert!(output.contains(html));
    epub::inspect(&destination).expect("reopen exported EPUB");
    assert_eq!(std::fs::read(source).expect("source after"), original);
}

// ------------------------------------------------------- original / KFX

fn kfx_text(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("kfx output");
    assert_eq!(&bytes[..4], b"CONT", "KFX container signature");
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn original_epub_export_is_a_byte_identical_copy() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let book = import_rich_book(&data_dir, &source);
    save_block(&book, 0, 0, "ဘာသာပြန်", "reviewed");
    let destination = dir.join("original-copy.epub");
    let outcome = services::export::export_original(
        &data_dir,
        services::export::OriginalExportRequest {
            project_id: book.project_id.clone(),
            destination: destination.clone(),
            format: "epub".into(),
        },
    )
    .expect("original export");
    assert_eq!(outcome.mode, "original");
    assert_eq!(outcome.blocks_translated, 0);
    assert_eq!(
        std::fs::read(&destination).expect("copy"),
        std::fs::read(&source).expect("source")
    );
}

#[test]
fn original_export_refuses_to_overwrite_the_source_book() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let before = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);
    let error = services::export::export_original(
        &data_dir,
        services::export::OriginalExportRequest {
            project_id: book.project_id,
            destination: source.clone(),
            format: "kfx".into(),
        },
    )
    .expect_err("source path rejected");
    assert_eq!(error.code, "destination_unsafe");
    assert_eq!(std::fs::read(&source).expect("source"), before);
}

#[test]
fn kfx_exports_contain_original_or_translated_text() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let book = import_rich_book(&data_dir, &source);
    save_block(&book, 0, 0, "မြန်မာဘာသာပြန်ခေါင်းစဉ်", "reviewed");

    let original = dir.join("original.kfx");
    services::export::export_original(
        &data_dir,
        services::export::OriginalExportRequest {
            project_id: book.project_id.clone(),
            destination: original.clone(),
            format: "kfx".into(),
        },
    )
    .expect("original kfx");
    assert!(!kfx_text(&original).contains("မြန်မာဘာသာပြန်ခေါင်းစဉ်"));

    let translated = dir.join("translated.kfx");
    let outcome = services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id.clone(),
            destination: translated.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "kfx".into(),
        },
    )
    .expect("translated kfx");
    assert_eq!(PathBuf::from(&outcome.output_path), translated);
    assert!(kfx_text(&translated).contains("မြန်မာဘာသာပြန်ခေါင်းစဉ်"));
    // No stray temp files are left beside the output.
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn unknown_export_format_is_rejected() {
    let dir = tempdir();
    let data_dir = tempdir();
    let files = rich_book_files();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "source.epub", &refs);
    let book = import_rich_book(&data_dir, &source);
    let error = services::export::export_original(
        &data_dir,
        services::export::OriginalExportRequest {
            project_id: book.project_id,
            destination: dir.join("x.mobi"),
            format: "mobi".into(),
        },
    )
    .expect_err("rejected");
    assert_eq!(error.code, "invalid_export_format");
}

#[test]
fn edited_stylesheet_is_exported_without_touching_the_snapshot() {
    let dir = tempdir();
    let data_dir = tempdir();
    let original_css = b"p { margin: 0; }\n".to_vec();
    let mut files = rich_book_files();
    for (name, bytes) in &mut files {
        if name == "OEBPS/content.opf" {
            let text = String::from_utf8(bytes.clone()).expect("opf text").replace(
                "</manifest>",
                "<item id=\"css\" href=\"styles/book.css\" media-type=\"text/css\"/></manifest>",
            );
            *bytes = text.into_bytes();
        }
    }
    files.push(("OEBPS/styles/book.css".into(), original_css.clone()));
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let source = write_epub(&dir, "styled.epub", &refs);
    let source_bytes = std::fs::read(&source).expect("source");
    let book = import_rich_book(&data_dir, &source);

    let listed = services::stylesheets::list(&data_dir, &book.project_id).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].href, "OEBPS/styles/book.css");
    assert_eq!(listed[0].name, "styles/book.css");
    assert!(!listed[0].modified);

    let href = listed[0].href.clone();
    let read = services::stylesheets::read(&data_dir, &book.project_id, &href).expect("read");
    assert_eq!(read.css.as_bytes(), original_css.as_slice());

    // Unknown paths are rejected rather than read from the archive.
    let unknown = services::stylesheets::read(&data_dir, &book.project_id, "OEBPS/ch1.xhtml");
    assert_eq!(
        unknown.expect_err("not a stylesheet").code,
        "stylesheet_not_found"
    );

    let edited = "p { margin: 0; font-family: \"Noto Sans Myanmar\"; } /* မြန်မာ */\n";
    let saved =
        services::stylesheets::save(&data_dir, &book.project_id, &href, edited).expect("save");
    assert!(saved.modified);
    assert!(services::stylesheets::list(&data_dir, &book.project_id).expect("list")[0].modified);

    let destination = dir.join("styled-export.epub");
    services::export::export_epub(
        &data_dir,
        ExportRequest {
            project_id: book.project_id.clone(),
            destination: destination.clone(),
            mode: "draft".into(),
            title: None,
            language: None,
            include_original: true,
            format: "epub".into(),
        },
    )
    .expect("export");
    epub::export::validate_output(&destination).expect("valid export");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&destination).expect("open export")).expect("zip");
    let mut exported = String::new();
    archive
        .by_name("OEBPS/styles/book.css")
        .expect("css entry")
        .read_to_string(&mut exported)
        .expect("read css");
    assert_eq!(exported, edited);
    assert_eq!(std::fs::read(&source).expect("source"), source_bytes);

    let reset = services::stylesheets::reset(&data_dir, &book.project_id, &href).expect("reset");
    assert!(!reset.modified);
    assert_eq!(reset.css.as_bytes(), original_css.as_slice());
    assert!(!services::stylesheets::list(&data_dir, &book.project_id).expect("list")[0].modified);
}
