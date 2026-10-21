//! Restricted translation fragments. Browser HTML is never trusted.
use crate::epub::preview::escape_html_text;
use crate::error::AppError;
use base64::Engine;

pub const MAX_FRAGMENT: usize = 16 * 1024 * 1024;

pub fn image_bytes(src: &str) -> Result<(&'static str, Vec<u8>), AppError> {
    let invalid = || {
        AppError::new(
            "invalid_image",
            "Choose a PNG, JPEG, GIF, WebP, AVIF or BMP image under 8 MB.",
        )
    };
    let (header, payload) = src.split_once(',').ok_or_else(invalid)?;
    let mime: &'static str = match header {
        "data:image/png;base64" => "image/png",
        "data:image/jpeg;base64" => "image/jpeg",
        "data:image/gif;base64" => "image/gif",
        "data:image/webp;base64" => "image/webp",
        "data:image/avif;base64" => "image/avif",
        "data:image/bmp;base64" => "image/bmp",
        _ => return Err(invalid()),
    };
    if payload.len() > 12 * 1024 * 1024 {
        return Err(invalid());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|_| invalid())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(invalid());
    }
    let signature_ok = match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "image/gif" => bytes.starts_with(b"GIF8"),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP",
        "image/avif" => {
            bytes.len() >= 12
                && bytes[4..8] == *b"ftyp"
                && (bytes[8..12] == *b"avif" || bytes[8..12] == *b"avis")
        }
        "image/bmp" => bytes.starts_with(b"BM"),
        _ => false,
    };
    if !signature_ok {
        return Err(invalid());
    }
    Ok((mime, bytes))
}

/// Validates and canonicalizes inline XHTML, deriving authoritative plain text.
pub fn validate(fragment: &str) -> Result<(String, String), AppError> {
    validate_schema(fragment, false)
}

pub fn validate_document(fragment: &str) -> Result<(String, String), AppError> {
    validate_schema(fragment, true)
}

const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";
const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";
const EPUB_NS: &str = "http://www.idpf.org/2007/ops";

/// Original book structures that travel with the text. Mirrors the read-only
/// preview allowlist so anything the reader shows can also be kept in a
/// translation draft instead of rejected as "unsupported formatting".
fn is_xhtml_document_tag(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
            | "li"
            | "blockquote"
            | "hr"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
            | "caption"
            | "div"
            | "section"
            | "hgroup"
            | "article"
            | "main"
            | "header"
            | "footer"
            | "aside"
            | "figure"
            | "figcaption"
            | "pre"
            | "dl"
            | "dt"
            | "dd"
            | "time"
            | "span"
            | "b"
            | "i"
            | "code"
            | "sup"
            | "sub"
            | "ruby"
            | "rt"
            | "rp"
            | "picture"
            | "source"
            | "small"
            | "abbr"
            | "cite"
            | "dfn"
            | "kbd"
            | "samp"
            | "var"
            | "mark"
    )
}

fn is_xhtml_block(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
            | "li"
            | "blockquote"
            | "hr"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
            | "caption"
            | "div"
            | "section"
            | "hgroup"
            | "article"
            | "main"
            | "header"
            | "footer"
            | "aside"
            | "figure"
            | "figcaption"
            | "pre"
            | "dl"
            | "dt"
            | "dd"
    )
}

fn is_xhtml_inline(tag: &str) -> bool {
    matches!(
        tag,
        "strong"
            | "em"
            | "u"
            | "s"
            | "br"
            | "img"
            | "a"
            | "time"
            | "span"
            | "b"
            | "i"
            | "code"
            | "sup"
            | "sub"
            | "ruby"
            | "rt"
            | "rp"
            | "picture"
            | "source"
    )
}

fn is_svg_tag(tag: &str) -> bool {
    matches!(
        tag,
        "svg"
            | "g"
            | "image"
            | "path"
            | "rect"
            | "circle"
            | "ellipse"
            | "line"
            | "polyline"
            | "polygon"
            | "title"
            | "desc"
            | "text"
            | "tspan"
    )
}

/// Detects a book image mime from magic bytes. Used for editor display
/// (data URLs) so WebP/AVIF/BMP books behave like PNG/JPEG/GIF instead of
/// showing broken images in chapters that contain them.
pub fn image_mime_from_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP" {
        Some("image/webp")
    } else if bytes.len() >= 12
        && bytes[4..8] == *b"ftyp"
        && (bytes[8..12] == *b"avif" || bytes[8..12] == *b"avis")
    {
        Some("image/avif")
    } else if bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        None
    }
}

/// Validates an <img>/<source> srcset value: every candidate URL must stay
/// inside the book (relative) or be a supported data URL. Descriptors
/// (`1x`, `400w`) are ignored. Rejects external/network URLs so drafts can
/// never fetch.
fn validate_srcset(value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(invalid_detail("An image srcset is empty.".to_string()));
    }
    for candidate in value.split(',') {
        let url = candidate.split_whitespace().next().unwrap_or("").trim();
        if url.is_empty() {
            return Err(invalid_detail(
                "An image srcset entry is empty.".to_string(),
            ));
        }
        if url.trim_start().starts_with("data:") {
            image_bytes(url)?;
        } else if !is_relative_resource(url) {
            return Err(invalid_detail(format!(
                "Unsupported image srcset URL \"{url}\": use book-relative images."
            )));
        }
    }
    Ok(())
}

/// Relative resource references that stay inside the book archive.
/// Data URLs are handled separately; anything with a scheme, an absolute
/// path or a network reference is rejected so drafts can never fetch.
fn is_relative_resource(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.contains(':')
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.starts_with("//")
    {
        return false;
    }
    true
}

fn is_safe_external_link(value: &str) -> bool {
    // Book-preserved outbound links. Preview never fetches them (rewritten
    // inert); export keeps the original reference. Active schemes stay
    // rejected so drafts can never execute script or exfiltrate.
    let lower = value.trim_start().to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:"))
        && !lower.starts_with("javascript:")
        && !lower.starts_with("data:")
        && !lower.starts_with("vbscript:")
}

/// Presentational attributes real EPUBs carry on containers/tables/cells/images.
/// Inert strings only; event handlers, URLs and positioning CSS are handled
/// separately. Values are escaped on serialization.
fn is_preserved_layout_attr(tag: &str, name: &str) -> bool {
    matches!(
        (tag, name),
        (
            "table",
            "border" | "cellpadding" | "cellspacing" | "width" | "align"
        ) | (
            "td" | "th",
            "width" | "height" | "align" | "valign" | "scope" | "abbr" | "headers"
        ) | ("img", "width" | "height" | "align")
            | ("div", "align")
            | ("ol" | "ul", "type")
            | ("li", "value")
    )
}

fn invalid_detail(why: String) -> AppError {
    AppError::new(
        "invalid_translation",
        format!("The translation contains unsupported formatting or an invalid image. {why}"),
    )
}

fn validate_schema(fragment: &str, document: bool) -> Result<(String, String), AppError> {
    let invalid = || {
        AppError::new(
            "invalid_translation",
            "The translation contains unsupported formatting or an invalid image.",
        )
    };
    if fragment.len() > MAX_FRAGMENT {
        return Err(invalid());
    }
    // Declare the book namespaces up front so body-inner fragments copied
    // from the original (which may use epub:type or xlink:href while the
    // declaring xmlns lives on <html>) still parse as standalone XML.
    let wrapped = format!(
        "<w xmlns=\"{XHTML_NS}\" xmlns:epub=\"{EPUB_NS}\" xmlns:xlink=\"{XLINK_NS}\">{fragment}</w>"
    );
    let full = if document {
        roxmltree::Document::parse(fragment).ok().filter(|doc| {
            matches!(doc.root_element().tag_name().name(), "html" | "body")
                && doc.root_element().tag_name().namespace() == Some("http://www.w3.org/1999/xhtml")
        })
    } else {
        None
    };
    let doc = match full { Some(doc) => doc, None => roxmltree::Document::parse(&wrapped).map_err(|error| AppError::new("invalid_translation", format!("The HTML is not well-formed XHTML at {}. Check closing tags and escaped & characters.", error.pos())))? };
    let root = if doc
        .root_element()
        .has_tag_name(("http://www.w3.org/1999/xhtml", "html"))
    {
        doc.root_element()
            .children()
            .find(|node| node.has_tag_name(("http://www.w3.org/1999/xhtml", "body")))
            .ok_or_else(|| {
                AppError::new(
                    "invalid_translation",
                    "The pasted XHTML document has no body element.",
                )
            })?
    } else {
        doc.root_element()
    };
    let mut html = String::new();
    let mut plain = String::new();
    fn walk(
        node: roxmltree::Node<'_, '_>,
        html: &mut String,
        plain: &mut String,
        depth: usize,
        document: bool,
    ) -> Result<(), AppError> {
        if depth > 32 {
            return Err(invalid_detail("Content is nested too deeply.".to_string()));
        }
        if node.is_text() {
            let text = node.text().unwrap_or("");
            if document
                && !text.trim().is_empty()
                && node.parent_element().is_some_and(|parent| {
                    matches!(
                        parent.tag_name().name(),
                        "table" | "thead" | "tbody" | "tfoot" | "tr"
                    )
                })
            {
                return Err(AppError::new(
                    "invalid_translation",
                    "Table text must be inside a cell.",
                ));
            }
            html.push_str(&escape_html_text(text));
            plain.push_str(text);
            return Ok(());
        }
        if !node.is_element() {
            return Err(invalid_detail(
                "Only text and known elements are supported.".to_string(),
            ));
        }
        let tag = node.tag_name().name();
        let ns = node.tag_name().namespace();
        let is_svg = ns == Some(SVG_NS);
        let is_xhtml = ns == Some(XHTML_NS);
        // Inline (legacy block) fragments keep the original tight schema.
        // Whole-chapter documents accept the reader's structural and SVG
        // tags so copied originals round-trip instead of being rejected.
        let allowed = if is_xhtml {
            matches!(tag, "strong" | "em" | "u" | "s" | "br" | "img" | "a")
                || (document && (is_xhtml_document_tag(tag) || is_xhtml_inline(tag)))
        } else if document && is_svg {
            is_svg_tag(tag)
        } else {
            false
        };
        if !allowed {
            let ns_label: String = match ns {
                Some(SVG_NS) => " (SVG namespace)".to_string(),
                Some(XHTML_NS) => String::new(),
                Some(other) => format!(" (namespace {other})"),
                None => " (no namespace)".to_string(),
            };
            // This is the rejection the AI dialog surfaces. Name the element
            // so Retry can drop/convert exactly this tag.
            return Err(invalid_detail(format!(
                "Unsupported <{tag}>{ns_label} element."
            )));
        }
        if document {
            let (parent_tag, parent_ns) = node
                .parent_element()
                .map(|n| (n.tag_name().name().to_string(), n.tag_name().namespace()))
                .unwrap_or(("w".to_string(), None));
            let parent = parent_tag.as_str();
            // SVG subtrees stay SVG-only: book XHTML must not appear inside
            // a drawing, and drawings behave like blocks in running text.
            if parent_ns == Some(SVG_NS) && !is_svg && !node.is_text() {
                return Err(invalid_detail(format!(
                    "A <{tag}> element cannot appear inside SVG <{parent}>."
                )));
            }
            let is_block = (is_xhtml && is_xhtml_block(tag)) || is_svg && tag == "svg";
            if matches!(
                parent,
                "p" | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "strong"
                    | "em"
                    | "u"
                    | "s"
                    | "a"
                    | "time"
                    | "span"
                    | "b"
                    | "i"
                    | "code"
                    | "sup"
                    | "sub"
                    | "ruby"
                    | "rt"
                    | "rp"
                    | "small"
                    | "abbr"
                    | "cite"
                    | "dfn"
                    | "kbd"
                    | "samp"
                    | "var"
                    | "mark"
            ) && (is_block || is_svg)
            {
                return Err(AppError::new(
                    "invalid_translation",
                    format!(
                        "A <{tag}> block cannot be inside <{parent}>. Place it outside the paragraph or inline element."
                    ),
                ));
            }
            if matches!(parent, "ul" | "ol") && tag != "li" {
                return Err(invalid_detail(format!(
                    "Only <li> can appear inside <{parent}>, found <{tag}>."
                )));
            }
            if tag == "li" && !matches!(parent, "ul" | "ol") {
                return Err(invalid_detail(
                    "<li> must appear inside <ul> or <ol>.".to_string(),
                ));
            }
            if tag == "source" && parent != "picture" {
                return Err(invalid_detail(
                    "<source> must appear inside <picture>.".to_string(),
                ));
            }
            if parent == "picture" && !matches!(tag, "source" | "img") {
                return Err(invalid_detail(format!(
                    "Only <source> or <img> can appear inside <picture>, found <{tag}>."
                )));
            }
            if matches!(parent, "table")
                && !matches!(tag, "caption" | "thead" | "tbody" | "tfoot" | "tr")
                || matches!(parent, "thead" | "tbody" | "tfoot") && tag != "tr"
                || parent == "tr" && !matches!(tag, "td" | "th")
            {
                return Err(AppError::new(
                    "invalid_translation",
                    "Tables must contain rows, and rows must contain header or data cells.",
                ));
            }
            if matches!(tag, "td" | "th") && parent != "tr"
                || tag == "tr" && !matches!(parent, "table" | "thead" | "tbody" | "tfoot")
                || matches!(tag, "thead" | "tbody" | "tfoot" | "caption") && parent != "table"
            {
                return Err(invalid_detail(format!(
                    "A <{tag}> element is in the wrong table position (parent <{parent}>)."
                )));
            }
            if tag == "table" {
                let rows = node
                    .descendants()
                    .filter(|n| n.has_tag_name((XHTML_NS, "tr")))
                    .count();
                if rows == 0 || rows > 1000 {
                    return Err(AppError::new(
                        "invalid_translation",
                        "Tables must have between 1 and 1000 rows.",
                    ));
                }
                plain.push_str("[table]");
            }
            if tag == "a"
                && node
                    .ancestors()
                    .skip(1)
                    .any(|n| n.has_tag_name((XHTML_NS, "a")))
            {
                return Err(invalid_detail(
                    "Links cannot be nested inside another link.".to_string(),
                ));
            }
            if is_svg && tag == "image" && node.children().next().is_some() {
                return Err(invalid_detail(
                    "An SVG <image> must stay empty.".to_string(),
                ));
            }
        }
        // Attribute policy: unprefixed book attributes plus the namespaced
        // attributes originals rely on (SVG image references, EPUB semantics
        // and xml:lang). Everything else — event handlers, scripts, network
        // CSS — stays rejected.
        for attr in node.attributes() {
            let attr_ns = attr.namespace();
            let attr_name = attr.name();
            let namespaced_ok = document
                && ((attr_ns == Some(XLINK_NS) && attr_name == "href" && is_svg && tag == "image")
                    || (attr_ns == Some(EPUB_NS) && attr_name == "type" && is_xhtml)
                    || (attr_ns == Some("http://www.w3.org/XML/1998/namespace")
                        && attr_name == "lang"));
            if namespaced_ok {
                continue;
            }
            if attr_ns.is_some() {
                return Err(invalid_detail(format!(
                    "Unsupported attribute '{}' on <{tag}>.",
                    attr.name()
                )));
            }
            // Event-handler attributes are never book formatting.
            if attr_name.len() > 1 && attr_name.to_ascii_lowercase().starts_with("on") {
                return Err(invalid_detail(format!(
                    "Event attribute '{attr_name}' on <{tag}> is not supported."
                )));
            }
            let per_tag_ok = matches!(
                (tag, attr_name),
                (
                    "img",
                    "src" | "alt" | "srcset" | "sizes" | "loading" | "decoding"
                ) | ("image", "href" | "alt")
                    | ("source", "src" | "srcset" | "sizes" | "media" | "type")
                    | ("a", "href")
                    | ("time", "datetime")
                    | ("ol", "start")
                    | ("td" | "th", "colspan" | "rowspan")
            ) || (is_svg
                && matches!(
                    attr_name,
                    "viewBox"
                        | "preserveAspectRatio"
                        | "width"
                        | "height"
                        | "x"
                        | "y"
                        | "x1"
                        | "x2"
                        | "y1"
                        | "y2"
                        | "cx"
                        | "cy"
                        | "r"
                        | "rx"
                        | "ry"
                        | "d"
                        | "points"
                        | "transform"
                        | "opacity"
                        | "fill"
                        | "stroke"
                        | "stroke-width"
                        | "font-size"
                        | "text-anchor"
                        | "version"
                ));
            if !(per_tag_ok
                // Accept old/book targets, then omit them on serialization.
                || document && is_xhtml && tag == "a" && attr_name == "target"
                || document
                    && (matches!(
                        attr_name,
                        "class" | "id" | "style" | "title" | "lang" | "dir" | "role" | "data-parent"
                    ) || attr_name.starts_with("aria-")
                        || attr_name == "xml:lang"
                        || (is_xhtml && tag == "code" && attr_name == "data-highlighted")
                        || is_preserved_layout_attr(tag, attr_name)))
            {
                return Err(invalid_detail(format!(
                    "Unsupported attribute '{attr_name}' on <{tag}>."
                )));
            }
        }
        // Serialize with self-contained namespaces: an <svg> root carries
        // its SVG/XLink declarations so the stored body-inner fragment stays
        // well-formed when spliced back into the original chapter, and
        // EPUB semantics keep a local declaration for the same reason.
        // SVG image references are normalized to both href forms so legacy
        // (xlink) and current readers resolve the same file.
        let mut needs_epub_ns = false;
        let mut svg_image_href: Option<String> = None;
        if is_svg && tag == "svg" {
            html.push_str(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"",
            );
        } else if is_svg && tag == "image" {
            html.push_str("<image");
        } else {
            html.push('<');
            html.push_str(tag);
        }
        for attr in node.attributes() {
            let attr_ns = attr.namespace();
            let attr_name = attr.name();
            // Browsing-context targets are unnecessary in translated books.
            // Normalize them away, including when reopening an older draft.
            if document && is_xhtml && tag == "a" && attr_ns.is_none() && attr_name == "target" {
                continue;
            }
            if attr_ns == Some(XLINK_NS) && attr_name == "href" {
                svg_image_href = Some(attr.value().to_string());
                continue;
            }
            if attr_ns == Some(EPUB_NS) && attr_name == "type" {
                needs_epub_ns = true;
                html.push_str(&format!(
                    " epub:type=\"{}\"",
                    escape_html_text(attr.value())
                ));
                continue;
            }
            if attr_ns == Some("http://www.w3.org/XML/1998/namespace") && attr_name == "lang" {
                html.push_str(&format!(" xml:lang=\"{}\"", escape_html_text(attr.value())));
                continue;
            }
            if matches!(attr_name, "colspan" | "rowspan")
                && attr
                    .value()
                    .parse::<u16>()
                    .ok()
                    .is_none_or(|span| span == 0 || span > 100)
            {
                return Err(AppError::new(
                    "invalid_translation",
                    "Table cell spans must be integers from 1 to 100.",
                ));
            }
            if attr_name == "style" {
                validate_style(attr.value())?;
            }
            if tag == "img" && attr_name == "src" {
                let value = attr.value();
                if value.trim_start().starts_with("data:") {
                    image_bytes(value)?;
                } else if !is_relative_resource(value) {
                    return Err(invalid_detail(format!(
                        "Unsupported <img src=\"{value}\">: use a book-relative image or a PNG/JPEG/GIF/WebP/AVIF/BMP data URL under 8 MB."
                    )));
                }
            }
            if (tag == "img" || tag == "source") && attr_name == "srcset" {
                validate_srcset(attr.value())?;
            }
            if (tag == "img" || tag == "source") && attr_name == "sizes" {
                let value = attr.value();
                if value.contains("javascript:")
                    || value.contains("data:")
                    || value.contains("vbscript:")
                    || value.contains('<')
                    || value.contains('>')
                {
                    return Err(invalid_detail(format!(
                        "Unsupported sizes value on <{tag}>."
                    )));
                }
            }
            if tag == "source" && attr_name == "media" {
                let value = attr.value();
                if value.contains("javascript:")
                    || value.contains("data:")
                    || value.contains("vbscript:")
                    || value.contains('<')
                    || value.contains('>')
                {
                    return Err(invalid_detail(
                        "Unsupported media value on <source>.".to_string(),
                    ));
                }
            }
            if tag == "source" && attr_name == "type" {
                let value = attr.value().trim().to_ascii_lowercase();
                if !(value.starts_with("image/") || value.is_empty()) {
                    return Err(invalid_detail(format!(
                        "Unsupported <source type=\"{}\">: only image types are preserved.",
                        attr.value()
                    )));
                }
            }
            if tag == "img" && matches!(attr_name, "loading" | "decoding") {
                let value = attr.value().trim().to_ascii_lowercase();
                let ok = match attr_name {
                    "loading" => matches!(value.as_str(), "lazy" | "eager" | "auto"),
                    _ => matches!(value.as_str(), "async" | "sync" | "auto"),
                };
                if !ok {
                    return Err(invalid_detail(format!(
                        "Unsupported <img {attr_name}=\"{}\">.",
                        attr.value()
                    )));
                }
            }
            if tag == "source" && attr_name == "src" {
                let value = attr.value();
                if value.trim_start().starts_with("data:") {
                    image_bytes(value)?;
                } else if !is_relative_resource(value) {
                    return Err(invalid_detail(format!(
                        "Unsupported <source src=\"{value}\">: use a book-relative image."
                    )));
                }
            }
            if is_svg && tag == "image" && attr_name == "href" {
                svg_image_href = Some(attr.value().to_string());
                continue;
            }
            if tag == "a" && attr_name == "href" {
                // Book-relative links and in-chapter anchors ("#note") always
                // travel. Preserved outbound http(s)/mailto links are kept so
                // a correctly-translated proposal is not rejected; preview
                // keeps them inert and export carries the original reference.
                // Anything else with a scheme or absolute path stays rejected.
                let value = attr.value();
                if !is_relative_resource(value)
                    && !(value.starts_with('#') && !value.contains(':'))
                    && !(document && is_safe_external_link(value))
                {
                    return Err(invalid_detail(format!(
                        "Unsupported <a href=\"{value}\">: use a book-relative link, an in-chapter #anchor, or http(s)/mailto."
                    )));
                }
            }
            // The xmlns declarations themselves are namespace bindings, not
            // attributes in roxmltree, so re-emitting them would duplicate.
            if attr_name == "xmlns" || attr_name.starts_with("xmlns:") {
                continue;
            }
            html.push_str(&format!(
                " {}=\"{}\"",
                attr_name,
                escape_html_text(attr.value())
            ));
        }
        if is_svg && tag == "image" {
            let href = svg_image_href.filter(|v| !v.is_empty()).ok_or_else(|| {
                invalid_detail("An SVG <image> needs an href to a book-relative image.".to_string())
            })?;
            if href.trim_start().starts_with("data:") {
                image_bytes(&href)?;
            } else if !is_relative_resource(&href) {
                return Err(invalid_detail(format!(
                    "Unsupported SVG image href \"{href}\": use a book-relative image."
                )));
            }
            let escaped = escape_html_text(&href);
            html.push_str(&format!(" href=\"{escaped}\" xlink:href=\"{escaped}\""));
        }
        if needs_epub_ns {
            html.push_str(" xmlns:epub=\"http://www.idpf.org/2007/ops\"");
        }
        if tag == "img" && node.attribute("src").is_none() {
            return Err(invalid_detail("<img> needs a src attribute.".to_string()));
        }
        if tag == "source" && node.attribute("src").is_none() && node.attribute("srcset").is_none()
        {
            return Err(invalid_detail(
                "<source> needs a src or srcset attribute.".to_string(),
            ));
        }
        if matches!(tag, "img" | "br" | "hr" | "source")
            || (is_svg
                && matches!(
                    tag,
                    "image"
                        | "path"
                        | "rect"
                        | "circle"
                        | "ellipse"
                        | "line"
                        | "polyline"
                        | "polygon"
                ))
        {
            if node.children().next().is_some() {
                return Err(invalid_detail(format!("<{tag}> must stay empty.")));
            }
            html.push_str("/>");
            if tag == "br" {
                plain.push('\n');
            } else if tag == "img" || tag == "source" || (is_svg && tag == "image") {
                plain.push_str("[image]");
            }
        } else {
            html.push('>');
            for child in node.children() {
                walk(child, html, plain, depth + 1, document)?;
            }
            if is_svg && tag == "svg" {
                html.push_str("</svg>");
            } else if is_svg && tag == "image" {
                html.push_str("</image>");
            } else {
                html.push_str(&format!("</{tag}>"));
            }
        }
        Ok(())
    }
    for child in root.children() {
        walk(child, &mut html, &mut plain, 0, document)?;
    }
    Ok((html, plain))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn highlighted_code_round_trip() {
        let input = include_str!("../../../tests/fixtures/editor/highlighted-code.xhtml")
            .trim()
            .replace("\r\n", "\n");
        // XML normalizes CRLF to LF, including whitespace inside code blocks.
        // Exercise both checkout styles on every platform.
        for source in [input.clone(), input.replace('\n', "\r\n")] {
            let (saved, text) = validate_document(&source).expect("passive code metadata");
            assert_eq!(saved, input);
            assert!(text.contains("မြန်မာ"));
            assert_eq!(validate_document(&saved).expect("reload").0, saved);
        }
        for invalid in [
            "<p data-highlighted=\"yes\">text</p>",
            "<p><code onclick=\"alert(1)\">text</code></p>",
            "<p><code data-unknown=\"yes\">text</code></p>",
        ] {
            assert!(validate_document(invalid).is_err());
        }
    }

    #[test]
    fn formatting_and_unicode_round_trip() {
        let (html, text) =
            validate("<strong>မြန်မာ &amp; text</strong><br/><em>Next</em>").expect("valid");
        assert_eq!(text, "မြန်မာ & text\nNext");
        assert!(html.contains("<em>Next</em>"));
    }
    #[test]
    fn rejects_active_content() {
        for html in [
            "<script>x</script>",
            "<em onclick=\"x\">x</em>",
            "<img src=\"https://example.com/a.png\"/>",
            "<a href=\"javascript:alert(1)\">x</a>",
            "<p>x</p>",
        ] {
            assert!(validate(html).is_err(), "{html}");
        }
    }
}

#[cfg(test)]
mod document_tests {
    use super::*;
    #[test]
    fn aligned_div_preserves_myanmar_and_escaped_brackets() {
        // XML normalizes CRLF to LF, including fixtures checked out on Windows.
        let expected =
            include_str!("../../../tests/fixtures/editor/aligned-div.xhtml").replace("\r\n", "\n");
        for html in [expected.clone(), expected.replace('\n', "\r\n")] {
            let (saved, _) = validate_document(&html).expect("aligned div");
            assert_eq!(saved, expected);
            assert_eq!(validate_document(&saved).expect("reload").0, saved);
        }
        let nested = "<div align=\"center\"><p>မြန်မာ</p></div>";
        assert_eq!(validate_document(nested).expect("nested div").0, nested);
        assert!(validate_document("<div align=\"left\" onclick=\"evil()\">မြန်မာ</div>").is_err());
    }

    #[test]
    fn chapter_structure_is_preserved() {
        let html = "<h1>Title</h1><p>First <em>paragraph</em>.</p><ul><li><p>မြန်မာ</p></li></ul><blockquote><p>Quote</p></blockquote>";
        assert_eq!(validate_document(html).expect("document").0, html);
    }
    #[test]
    fn rejects_invalid_document_nesting_and_scripts() {
        for html in [
            "<p><h1>Nested heading</h1></p>",
            "<ul><p>bad</p></ul>",
            "<li>orphan</li>",
            "<script>bad</script>",
            "<a href=\"x\"><a href=\"y\">bad</a></a>",
        ] {
            assert!(validate_document(html).is_err(), "{html}");
        }
    }
    #[test]
    fn preserved_book_links_layout_attrs_and_xml_lang_round_trip() {
        // Real books carry outbound links, xml:lang and table presentational
        // attributes. A proposal that preserves them must not be rejected.
        let html = "<p xml:lang=\"en\">See <a href=\"https://example.com/page\">site</a> and <a href=\"mailto:a@example.com\">mail</a></p><table border=\"1\" cellpadding=\"4\"><tbody><tr><td align=\"center\" valign=\"top\" scope=\"col\">Hi</td></tr></tbody></table>";
        let (saved, _) = validate_document(html).expect("preserved book attrs");
        assert!(saved.contains("https://example.com/page"), "{saved}");
        assert!(saved.contains("xml:lang=\"en\""), "{saved}");
        assert!(saved.contains("border=\"1\""), "{saved}");
        // Active schemes and handlers still fail, with a named reason.
        for bad in [
            "<p><a href=\"javascript:alert(1)\">x</a></p>",
            "<p onclick=\"evil()\">x</p>",
            "<p><font>old</font></p>",
        ] {
            let err = validate_document(bad).expect_err(bad);
            assert!(
                err.message.contains("unsupported formatting")
                    && (err.message.contains("<font>")
                        || err.message.contains("href")
                        || err.message.contains("onclick")),
                "{bad}: {}",
                err.message
            );
        }
    }
    #[test]
    fn responsive_images_and_modern_formats_round_trip() {
        // Image chapters commonly use <picture>/<source>/srcset and WebP.
        // Preserving them must not block AI translation.
        let html = "<figure><picture><source srcset=\"images/a.webp\" type=\"image/webp\"/><img src=\"images/a.jpg\" srcset=\"images/a.jpg 1x, images/a@2x.jpg 2x\" sizes=\"100vw\" loading=\"lazy\" decoding=\"async\" alt=\"A\"/></picture><figcaption>Cap</figcaption></figure>";
        let (saved, text) = validate_document(html).expect("responsive images");
        assert!(saved.contains("srcset"), "{saved}");
        assert!(saved.contains("<picture>"), "{saved}");
        assert!(text.contains("[image]"), "{text}");
        // External srcset URLs and misplaced <source> still fail.
        assert!(
            validate_document("<picture><img src=\"https://example.com/a.jpg\"/></picture>")
                .is_err()
        );
        assert!(
            validate_document("<p><source srcset=\"a.jpg\"/></p>").is_err(),
            "source outside picture"
        );
        // WebP/AVIF/BMP data URLs validate by magic bytes.
        let mut webp_bytes = b"RIFF".to_vec();
        webp_bytes.extend_from_slice(&[0, 0, 0, 0]);
        webp_bytes.extend_from_slice(b"WEBP");
        assert!(image_mime_from_bytes(&webp_bytes) == Some("image/webp"));
        assert!(image_mime_from_bytes(b"BMxxxx") == Some("image/bmp"));
    }
}

/// Permit book typography and table presentation without network fetches,
/// overlays or active CSS.
pub fn validate_style(style: &str) -> Result<(), AppError> {
    for declaration in style.split(';').filter(|part| !part.trim().is_empty()) {
        let Some((property, value)) = declaration.split_once(':') else {
            return Err(AppError::new(
                "invalid_translation",
                "An inline style must use property: value.",
            ));
        };
        let property_name = property.trim().to_ascii_lowercase();
        let color_function = matches!(property_name.as_str(), "color" | "background-color")
            && ["rgb(", "rgba(", "hsl(", "hsla("].iter().any(|prefix| {
                value
                    .trim()
                    .strip_prefix(prefix)
                    .and_then(|body| body.strip_suffix(')'))
                    .is_some_and(|body| {
                        body.chars().all(|c| {
                            c.is_ascii_digit()
                                || c.is_ascii_whitespace()
                                || matches!(c, '.' | ',' | '%' | '+' | '-' | '/')
                        })
                    })
            });
        if !matches!(
            property_name.as_str(),
            "color"
                | "background-color"
                | "font-weight"
                | "font-style"
                | "font-size"
                | "font-family"
                | "line-height"
                | "text-align"
                | "text-decoration"
                | "text-indent"
                | "letter-spacing"
                | "word-spacing"
                | "border"
                | "border-width"
                | "border-style"
                | "border-color"
                | "border-top"
                | "border-right"
                | "border-bottom"
                | "border-left"
                | "border-top-width"
                | "border-right-width"
                | "border-bottom-width"
                | "border-left-width"
                | "border-top-style"
                | "border-right-style"
                | "border-bottom-style"
                | "border-left-style"
                | "border-top-color"
                | "border-right-color"
                | "border-bottom-color"
                | "border-left-color"
                | "border-collapse"
                | "border-spacing"
                | "table-layout"
                | "caption-side"
                | "empty-cells"
                | "vertical-align"
                // Flow placement keywords only (none/left/right/both/...):
                // the shared value guard below still rejects url(),
                // expression(), @, backslashes and angle brackets.
                | "clear"
                | "float"
                // Marker keywords and strings only (decimal, none, "-"...);
                // the same guard rejects symbols() and counter().
                | "list-style-type"
                | "width"
                | "height"
                | "max-width"
                | "padding"
                | "padding-top"
                | "padding-right"
                | "padding-bottom"
                | "padding-left"
        ) || value.contains(['@', '\\', '<', '>'])
            || value.contains(['(', ')']) && !color_function
        {
            return Err(AppError::new(
                "invalid_translation",
                format!(
                    "The inline CSS property {} or its value is not supported. Use typography or table styles without URLs or positioning.",
                    property.trim()
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod copied_document_tests {
    use super::*;
    #[test]
    fn borderless_table_and_safe_presentation_styles_round_trip() {
        let input = include_str!("../../../tests/fixtures/editor/borderless-table.xhtml").trim();
        let (saved, _) = validate_document(input).expect("borderless verse table");
        assert!(saved.contains("style=\"border: 0; \""));
        assert!(saved.contains("class=\"simplelist\""));
        assert!(saved.contains("epub:type=\"list\""));
        assert!(saved.contains("&quot;Ach! du lieber Augustin,"));
        assert_eq!(validate_document(&saved).expect("reload").0, saved);
        validate_style("border: none; border-width: 0px; border-bottom: 1px solid red; border-collapse: collapse; border-spacing: 0; padding: 0 1em; width: 80%; vertical-align: top; table-layout: auto;")
            .expect("safe table presentation");
        validate_style("list-style-type: lower-roman; list-style-type: none;")
            .expect("list marker keywords");
        assert!(validate_style("list-style-type: symbols(cyclic '*')").is_err());
        // Float flow placement (e.g. headings clearing floated cover art)
        // round-trips verbatim; only keyword values have any effect.
        validate_style("clear: both; float: left;").expect("flow placement keywords");
        for style in [
            "border: url(https://example.com/border)",
            "border-image: url(https://example.com/border)",
            "width: expression(alert(1))",
            "padding: var(--untrusted)",
            "border: 0; position: fixed",
            "border: 0; @import: evil",
            "border: \\75rl(remote)",
            "clear: url(https://example.com/x)",
            "float: expression(alert(1))",
        ] {
            assert!(validate_style(style).is_err(), "{style}");
        }
    }
    #[test]
    fn common_book_inline_semantics_and_protected_tables_are_export_safe() {
        let input = include_str!("../../../tests/fixtures/editor/book-compatibility.xhtml").trim();
        let (saved, text) = validate_document(input).expect("book formatting");
        assert_eq!(saved, input);
        assert!(text.contains("မြန်မာပုံပြင်"));
        for tag in ["small", "abbr", "cite", "dfn", "kbd", "samp", "var", "mark"] {
            assert!(validate_document(&format!("<{tag}><p>invalid block</p></{tag}>")).is_err());
            assert!(
                validate_document(&format!("<p><{tag} onclick=\"bad()\">text</{tag}></p>"))
                    .is_err()
            );
        }
    }
    #[test]
    fn translated_titlepage_removes_link_target_and_preserves_myanmar() {
        let input = include_str!("../../../tests/fixtures/ai/link-target.xhtml").trim();
        let (html, text) = validate_document(input).expect("translated titlepage");
        assert!(!html.contains("target="));
        assert!(html.contains("href=\"https://example.com/book\""));
        assert!(text.contains("မြန်မာပုံပြင်များ"));
        let (again, _) = validate_document(&html).expect("saved draft");
        assert_eq!(again, html);
        let preview = crate::epub::preview::sanitize_fragment(
            &html,
            "body",
            "chapter.xhtml",
            "epubres://test",
        );
        assert!(!preview.contains("target=\"_top\""));
        assert!(preview.contains("href=\"\""));
        for target in ["_self", "_blank", "_parent", "book-window"] {
            let link =
                format!("<p><a href=\"chapter.xhtml#note\" target=\"{target}\">note</a></p>");
            let (normalized, _) = validate_document(&link).expect("normalized link");
            assert!(!normalized.contains("target="));
            assert!(normalized.contains("chapter.xhtml#note"));
        }
        for bad in [
            "<p target=\"_top\">text</p>",
            "<a href=\"javascript:alert(1)\" target=\"_top\">text</a>",
            "<a href=\"https://example.com\" target=\"_top\" onclick=\"evil()\">text</a>",
        ] {
            assert!(validate_document(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn accepts_complete_original_toc_document_and_retains_attributes_links() {
        let input = "<?xml version='1.0' encoding='utf-8'?><html xmlns=\"http://www.w3.org/1999/xhtml\"><head><link rel=\"stylesheet\" href=\"stylesheet.css\"/></head><body><p class=\"border1\" style=\"color:#000000\">Contents</p><p class=\"toc2\"><a href=\"174.html#section\">Chapter 174</a></p></body></html>";
        validate_style("color: rgb(0, 0, 0);").expect("browser color serialization");
        let (html, text) = validate_document(input).expect("original XHTML");
        assert!(html.contains("class=\"toc2\""));
        assert!(html.contains("style=\"color:#000000\""));
        assert!(html.contains("href=\"174.html#section\""));
        assert!(!html.contains("<head"));
        assert!(!html.contains("<?xml"));
        assert!(text.contains("Chapter 174"));
    }
    #[test]
    fn rejects_active_css_and_event_attributes() {
        for input in [
            "<p style=\"background-color:url(https://example.com/a)\">x</p>",
            "<p style=\"position:fixed\">x</p>",
            "<p class=\"toc2\" onclick=\"evil()\">x</p>",
        ] {
            assert!(validate_document(input).is_err());
        }
    }
}

#[cfg(test)]
mod original_cover_tests {
    use super::*;
    #[test]
    fn accepts_ebookmaker_cover_div_svg_and_relative_image() {
        let input = "<div class=\"x-ebookmaker-cover\"><svg xmlns=\"http://www.w3.org/2000/svg\" height=\"100%\" preserveAspectRatio=\"xMidYMid meet\" version=\"1.1\" viewBox=\"0 0 800 1104\" width=\"100%\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><image width=\"800\" height=\"1104\" xlink:href=\"7981341119032183217_cover.jpg\"/></svg></div>";
        let (html, text) = validate_document(input).expect("original cover");
        assert!(html.contains("x-ebookmaker-cover"), "{html}");
        assert!(html.contains("<svg"), "{html}");
        assert!(html.contains("7981341119032183217_cover.jpg"), "{html}");
        // Image-only covers still count as content so they can be reviewed.
        assert!(html.contains("<image"), "{html}");
        assert!(!text.trim().is_empty() || html.contains("image"), "{text}");
        assert!(text.contains("[image]"), "{text}");
        // The stored fragment must re-parse as XML for export splicing.
        let wrapped = format!("<w>{html}</w>");
        crate::epub::xml::parse(&wrapped).expect("export-safe xml");
    }
    #[test]
    fn accepts_svg_href_form_and_relative_img_and_original_inline() {
        let (html, _) = validate_document(
            "<div><svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><image href=\"images/cover.png\" width=\"10\" height=\"10\"/></svg></div>",
        )
        .expect("svg href");
        assert!(html.contains("images/cover.png"), "{html}");
        let (html, _) = validate_document("<p>Text <img src=\"images/a.png\" alt=\"A\"/> tail</p>")
            .expect("relative img");
        assert!(html.contains("src=\"images/a.png\""), "{html}");
        let (html, _) = validate_document(
            "<section class=\"ch\"><div><p>Hi <span class=\"s\"><b>bold</b> and <i>it</i></span></p><figure><img src=\"i.png\" alt=\"I\"/><figcaption>Cap <sup>1</sup></figcaption></figure></div></section>",
        )
        .expect("original structure");
        assert!(html.contains("<section"), "{html}");
        assert!(html.contains("<figure"), "{html}");
        assert!(html.contains("<figcaption>"), "{html}");
    }
    #[test]
    fn still_rejects_external_and_active_svg_content() {
        for html in [
            "<div><svg xmlns=\"http://www.w3.org/2000/svg\"><image href=\"https://example.com/a.png\"/></svg></div>",
            "<div><svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"evil()\"><image href=\"a.png\"/></svg></div>",
            "<div><svg xmlns=\"http://www.w3.org/2000/svg\"><script>evil()</script></svg></div>",
            "<div><svg xmlns=\"http://www.w3.org/2000/svg\"><foreignObject><div>bad</div></foreignObject></svg></div>",
            "<p><svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"></svg></p>",
        ] {
            assert!(validate_document(html).is_err(), "{html}");
        }
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;
    #[test]
    fn editable_tables_preserve_structure_and_empty_tables_are_content() {
        let html = "<table><tbody><tr><th colspan=\"1\" rowspan=\"1\"><p>မြန်မာ</p></th><th><p>Header</p></th></tr><tr><td><p>A</p></td><td><p></p></td></tr></tbody></table>";
        let (saved, text) = validate_document(html).expect("table");
        assert_eq!(saved, html);
        assert!(text.contains("မြန်မာ"));
        let (_, text) =
            validate_document("<table><tbody><tr><td><p></p></td></tr></tbody></table>")
                .expect("empty grid");
        assert!(!text.trim().is_empty());
    }
    #[test]
    fn rejects_invalid_table_structure_and_spans() {
        for html in [
            "<table><p>outside cell</p></table>",
            "<table><tr>loose text</tr></table>",
            "<p><table><tr><td>x</td></tr></table></p>",
            "<table><tr><td colspan=\"999999\">x</td></tr></table>",
            "<td>orphan</td>",
        ] {
            assert!(validate_document(html).is_err(), "{html}");
        }
    }
}
