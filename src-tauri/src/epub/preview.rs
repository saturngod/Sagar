//! Safe preview rendering: converts stored source fragments into sanitized
//! HTML for the read-only pane, rewriting resource references to the
//! app-owned `epubres://` protocol so book content can never touch the
//! network or invoke privileged APIs.

use super::resolve_href;

/// Attribute names (case-insensitive) never rendered in preview.
fn is_event_attribute(name: &str) -> bool {
    name.len() > 1 && name.to_ascii_lowercase().starts_with("on")
}

/// Rewrites a fragment for preview. `resource_prefix` is the `epubres://`
/// URL prefix including the project id, without a trailing slash.
pub fn sanitize_fragment(
    fragment: &str,
    element_name: &str,
    base_resource: &str,
    resource_prefix: &str,
) -> String {
    // The stored fragment is inner content; wrap it with the declarations it
    // is known to reference. Unknown prefixes fall back to escaped text.
    let wrapped = format!(
        "<w xmlns=\"http://www.w3.org/1999/xhtml\" \
         xmlns:epub=\"http://www.idpf.org/2007/ops\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\">{fragment}</w>"
    );
    let Ok(doc) = roxmltree::Document::parse(&wrapped) else {
        return escape_html_text(fragment);
    };
    let root = doc.root_element();
    let mut out = String::with_capacity(fragment.len());
    for child in root.children() {
        if child.is_element() {
            serialize_safe(
                child,
                &mut out,
                base_resource,
                resource_prefix,
                Some(element_name),
            );
        } else if child.is_text()
            && let Some(text) = child.text()
        {
            out.push_str(&escape_html_text(text));
        }
    }
    out
}

fn serialize_safe(
    element: roxmltree::Node<'_, '_>,
    out: &mut String,
    base_resource: &str,
    resource_prefix: &str,
    block_element: Option<&str>,
) {
    let svg = element.tag_name().namespace() == Some("http://www.w3.org/2000/svg");
    let tag = element.tag_name().name().to_ascii_lowercase();
    let tag = if tag == "image"
        && element.tag_name().namespace() == Some("http://www.w3.org/1999/xhtml")
    {
        "img".to_string()
    } else {
        tag
    };
    if svg {
        serialize_svg(element, out, base_resource, resource_prefix);
        return;
    }
    // Active, form, embedded and metadata content is dropped with its
    // subtree; it never renders in the read-only pane.
    if matches!(
        tag.as_str(),
        "script"
            | "style"
            | "noscript"
            | "template"
            | "iframe"
            | "frame"
            | "frameset"
            | "object"
            | "embed"
            | "applet"
            | "form"
            | "input"
            | "button"
            | "select"
            | "textarea"
            | "link"
            | "meta"
            | "base"
            | "head"
            | "title"
            | "audio"
            | "video"
            | "canvas"
            | "math"
    ) || element.tag_name().namespace() != Some("http://www.w3.org/1999/xhtml")
    {
        return;
    }
    if !matches!(
        tag.as_str(),
        "body"
            | "div"
            | "section"
            | "article"
            | "main"
            | "header"
            | "hgroup"
            | "nav"
            | "address"
            | "footer"
            | "aside"
            | "dl"
            | "dt"
            | "dd"
            | "caption"
            | "tfoot"
            | "cite"
            | "abbr"
            | "small"
            | "q"
            | "mark"
            | "dfn"
            | "var"
            | "samp"
            | "kbd"
            | "del"
            | "ins"
            | "bdi"
            | "bdo"
            | "wbr"
            | "figure"
            | "figcaption"
            | "p"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
            | "li"
            | "blockquote"
            | "strong"
            | "em"
            | "b"
            | "i"
            | "u"
            | "s"
            | "span"
            | "time"
            | "a"
            | "img"
            | "br"
            | "hr"
            | "sup"
            | "sub"
            | "ruby"
            | "rt"
            | "rp"
            | "table"
            | "thead"
            | "tbody"
            | "tr"
            | "td"
            | "th"
            | "pre"
            | "code"
            | "picture"
            | "source"
    ) {
        // Unknown but passive XHTML (e.g. `big`, `center`, `figure`-like
        // custom wrappers): render its content without the element so book
        // text is never silently hidden from the reader.
        serialize_children_safe(element, out, base_resource, resource_prefix);
        return;
    }
    out.push('<');
    out.push_str(&tag);
    // EPUB structural semantics (e.g. `title`, `bridgehead`, `label`) let the
    // pane style common book patterns without loading untrusted book CSS.
    if let Some(types) = element.attribute(("http://www.idpf.org/2007/ops", "type")) {
        let types = types.split_whitespace().collect::<Vec<_>>().join(" ");
        if !types.is_empty() {
            out.push_str(" data-epub-type=\"");
            out.push_str(&escape_html_text(&types));
            out.push('"');
        }
    }
    for attr in element.attributes() {
        let name = attr.name().to_ascii_lowercase();
        if is_event_attribute(&name)
            || !matches!(
                name.as_str(),
                "src" | "href" | "alt" | "title" | "id" | "lang" | "dir" | "colspan" | "rowspan"
            ) && !(tag == "time" && name == "datetime" && attr.namespace().is_none())
        {
            continue;
        }
        let value = attr.value();
        let (attr_out_name, rewritten) = if (name == "src" && tag == "img")
            || (name == "xlink:href" && tag == "img")
            || (name == "href" && tag == "a")
        {
            // Keep URI fragments (in-chapter anchors) when resolving.
            let (path_part, fragment_part) = match value.split_once('#') {
                Some((path, fragment)) => (path, Some(fragment)),
                None => (value, None),
            };
            let mut rewritten = rewrite_url(path_part, base_resource, resource_prefix);
            if let Some(fragment) = fragment_part {
                rewritten.push('#');
                rewritten.push_str(fragment);
            }
            (name.as_str(), Some(rewritten))
        } else if name == "style" {
            // Inline styles could hide content or position overlays; drop them
            // in preview rather than build a CSS allowlist for the MVP.
            continue;
        } else {
            (name.as_str(), None)
        };
        out.push(' ');
        out.push_str(attr_out_name);
        out.push_str("=\"");
        out.push_str(&escape_html_text(
            &rewritten.unwrap_or_else(|| value.to_string()),
        ));
        out.push('"');
    }
    if tag == "a" {
        out.push_str(" target=\"_blank\" rel=\"noopener noreferrer\"");
    }
    if block_element == Some(tag.as_str()) {
        out.push_str(&format!(
            " data-block-element=\"{}\"",
            escape_html_text(&tag)
        ));
    }
    const VOID: [&str; 4] = ["img", "br", "hr", "source"];
    if VOID.contains(&tag.as_str()) {
        out.push_str("/>");
        return;
    }
    let has_children = element
        .children()
        .any(|c| c.is_element() || c.text().is_some_and(|t| !t.is_empty()));
    if has_children {
        out.push('>');
        serialize_children_safe(element, out, base_resource, resource_prefix);
        out.push_str("</");
        out.push_str(&tag);
        out.push('>');
    } else {
        out.push_str("></");
        out.push_str(&tag);
        out.push('>');
    }
}

fn serialize_children_safe(
    element: roxmltree::Node<'_, '_>,
    out: &mut String,
    base_resource: &str,
    resource_prefix: &str,
) {
    for child in element.children() {
        if child.is_element() {
            serialize_safe(child, out, base_resource, resource_prefix, None);
        } else if child.is_text()
            && let Some(text) = child.text()
        {
            out.push_str(&escape_html_text(text));
        }
    }
}

// SVG has its own small passive allowlist. Scripts, foreignObject, use,
// animation, CSS and URL-valued paints are deliberately not rendered.
fn serialize_svg(element: roxmltree::Node<'_, '_>, out: &mut String, base: &str, prefix: &str) {
    let tag = element.tag_name().name();
    if !matches!(
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
    ) {
        return;
    }
    out.push('<');
    out.push_str(tag);
    if tag == "svg" {
        out.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
    }
    for attr in element.attributes() {
        let name = attr.name();
        if name == "href"
            && tag == "image"
            && (attr.namespace().is_none()
                || attr.namespace() == Some("http://www.w3.org/1999/xlink"))
        {
            let url = rewrite_url(attr.value(), base, prefix);
            if !url.is_empty() {
                out.push_str(&format!(" href=\"{}\"", escape_html_text(&url)));
            }
        } else if attr.namespace().is_none()
            && matches!(
                name,
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
            )
        {
            let value = attr.value();
            if matches!(name, "fill" | "stroke")
                && !value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '#' || c == '-')
            {
                continue;
            }
            out.push_str(&format!(" {name}=\"{}\"", escape_html_text(value)));
        }
    }
    out.push('>');
    for child in element.children() {
        if child.is_element() {
            serialize_safe(child, out, base, prefix, None);
        } else if let Some(text) = child.text() {
            out.push_str(&escape_html_text(text));
        }
    }
    out.push_str(&format!("</{tag}>"));
}

/// Rewrites a URL for preview: relative paths point at the snapshot via
/// `epubres://`; external URLs are kept inert (never auto-fetched); unsafe
/// schemes are stripped.
fn rewrite_url(value: &str, base_resource: &str, resource_prefix: &str) -> String {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("vbscript:")
    {
        return String::new();
    }
    if trimmed.starts_with('#') {
        // In-chapter anchor.
        return trimmed.to_string();
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return String::new();
    }
    if trimmed.contains(':') || trimmed.starts_with("//") {
        return String::new();
    }
    format!("{resource_prefix}/{}", resolve_href(base_resource, trimmed))
}

pub fn escape_html_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sanitize(fragment: &str) -> String {
        sanitize_fragment(fragment, "p", "OEBPS/c1.xhtml", "epubres://pid")
    }

    #[test]
    fn keeps_plain_text_and_inline_markup() {
        let html = sanitize("See <em>this</em> &amp; that");
        assert_eq!(html, "See <em>this</em> &amp; that");
    }

    #[test]
    fn keeps_time_text_in_chapter_and_block_previews() {
        let source = include_str!("../../../tests/fixtures/editor/time-preview.xhtml");
        let doc = roxmltree::Document::parse(source).unwrap();
        for tag in ["body", "p"] {
            let node = doc.descendants().find(|n| n.has_tag_name(tag)).unwrap();
            let html = sanitize_fragment(
                &source[node.range()],
                tag,
                "OEBPS/c1.xhtml",
                "epubres://pid",
            );
            assert!(
                html.contains("In the year <time datetime=\"1878\">1878</time> I took"),
                "{html}"
            );
            let rendered = roxmltree::Document::parse(&html).unwrap();
            let text: String = rendered
                .descendants()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect();
            assert_eq!(text, "In the year 1878 I took my degree.");
        }
    }

    #[test]
    fn time_keeps_nested_text_but_removes_active_content() {
        assert_eq!(
            sanitize(
                r#"<time datetime="1878" onclick="bad()" style="display:none"><em>1878</em><script>bad()</script></time>"#
            ),
            r#"<time datetime="1878"><em>1878</em></time>"#,
        );
    }

    #[test]
    fn rewrites_relative_images_and_links_to_epubres() {
        let html = sanitize(
            "<a href=\"c2.xhtml#n1\">note</a> and <img src=\"../images/a.png\" alt=\"A\"/>",
        );
        assert!(
            html.contains("href=\"epubres://pid/OEBPS/c2.xhtml#n1\""),
            "{html}"
        );
        assert!(
            html.contains("src=\"epubres://pid/images/a.png\""),
            "{html}"
        );
        assert!(html.contains("alt=\"A\""));
    }

    #[test]
    fn strips_event_handlers_and_script_schemes() {
        let html = sanitize(
            "<a href=\"javascript:alert(1)\" onclick=\"evil()\">x</a><img src=\"a.png\" onerror=\"evil()\"/>",
        );
        assert!(!html.contains("onclick"), "{html}");
        assert!(!html.contains("onerror"), "{html}");
        assert!(html.contains("href=\"\""), "{html}");
    }

    #[test]
    fn strips_external_urls_to_prevent_network_fetches() {
        let html = sanitize("<a href=\"https://example.com/page\">site</a>");
        assert!(html.contains("href=\"\""), "{html}");
        assert!(!sanitize("<img src=\"https://example.com/a.png\"/>").contains("https://"));
        assert!(!sanitize("<script>evil()</script><iframe src=\"a.xhtml\"/>").contains("evil"));
    }

    #[test]
    fn hgroup_headings_and_epub_semantics_are_rendered() {
        // Standard Ebooks chapter opener (e.g. The Odyssey, Book I).
        let html = sanitize_fragment(
            r#"<body epub:type="bodymatter"><section id="book-1" epub:type="chapter z3998:poem"><header><hgroup><h2><span epub:type="label">Book</span> <span epub:type="ordinal z3998:roman">I</span></h2><p epub:type="title">Visit of Pallas to Telemachus</p></hgroup><p epub:type="bridgehead">A council of the gods</p></header><p><span>Tell me, O Muse</span><br/><span>Who, having</span></p></section></body>"#,
            "body",
            "epub/text/book-1.xhtml",
            "epubres://pid",
        );
        assert!(html.contains("<hgroup><h2><span data-epub-type=\"label\">Book</span> <span data-epub-type=\"ordinal z3998:roman\">I</span></h2>"), "{html}");
        assert!(
            html.contains("<p data-epub-type=\"title\">Visit of Pallas to Telemachus</p>"),
            "{html}"
        );
        assert!(
            html.contains("<p data-epub-type=\"bridgehead\">A council of the gods</p>"),
            "{html}"
        );
        assert!(html.contains("<span>Tell me, O Muse</span><br/>"), "{html}");
    }

    #[test]
    fn unknown_passive_elements_keep_their_text() {
        assert_eq!(
            sanitize("<big>Large <em>text</em></big> <center>mid</center>"),
            "Large <em>text</em> mid"
        );
        assert_eq!(sanitize("a<noscript>b</noscript><object>c</object>d"), "ad");
    }

    #[test]
    fn falls_back_to_escaped_text_for_unknown_prefixes() {
        let html = sanitize("<x:weird>text</x:weird>");
        assert!(!html.contains("<x:weird>"), "{html}");
        assert!(html.contains("text"), "{html}");
    }
}

#[cfg(test)]
mod svg_tests {
    use super::*;
    #[test]
    fn svg_cover_is_local_and_active_content_is_removed() {
        let output = sanitize_fragment(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 800 1104" onload="bad()"><image xlink:href="cover.jpg"/><image href="https://example.com/a.png"/><script>bad()</script><foreignObject><div>bad</div></foreignObject><rect fill="url(https://example.com/a)" width="10"/></svg>"#,
            "body",
            "OEBPS/cover.xhtml",
            "epubres://pid",
        );
        assert!(output.contains("viewBox=\"0 0 800 1104\""));
        assert!(output.contains("href=\"epubres://pid/OEBPS/cover.jpg\""));
        for unsafe_value in ["onload", "script", "foreignObject", "https:", "url(", "bad"] {
            assert!(!output.contains(unsafe_value), "{output}");
        }
    }
}
