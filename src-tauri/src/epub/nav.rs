//! EPUB 3 navigation document parsing (the `toc` nav). Titles map onto
//! spine resources by resolving hrefs to ZIP paths; anchors (fragments) are
//! kept for later in-chapter navigation but never treated as separate files.

use crate::error::AppError;

use super::resolve_href;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NavGroup {
    pub resource: Option<String>,
    pub id: String,
    pub title: String,
}

pub struct NavEntry {
    /// The entry's own list item, in the same form as `NavGroup::id`, so a
    /// parent entry and the group it names can be recognized as one row.
    pub id: String,
    pub groups: Vec<NavGroup>,
    /// ZIP-root-relative resource path (fragment stripped).
    pub resource: String,
    pub fragment: Option<String>,
    pub title: String,
}

pub fn parse_nav(bytes: &[u8], nav_path: &str) -> Result<Vec<NavEntry>, AppError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| AppError::new("invalid_epub", "The navigation document is not UTF-8."))?;
    let doc = crate::epub::xml::parse(text)
        .map_err(|_| AppError::new("invalid_epub", "The navigation document is not valid XML."))?;

    // Prefer the nav element with epub:type="toc"; fall back to the first nav.
    let toc_nav = doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "nav")
        .find(|node| {
            node.attributes().any(|attr| {
                attr.name() == "type" && attr.value().split_whitespace().any(|v| v == "toc")
            })
        })
        .or_else(|| {
            doc.descendants()
                .find(|node| node.is_element() && node.tag_name().name() == "nav")
        });
    let Some(toc_nav) = toc_nav else {
        return Ok(Vec::new());
    };

    let mut entries = Vec::new();
    for node in toc_nav.descendants() {
        if !node.is_element() || node.tag_name().name() != "a" {
            continue;
        }
        let Some(href) = node.attribute("href") else {
            continue;
        };
        let (path_part, fragment) = match href.split_once('#') {
            Some((path, fragment)) => (path, Some(fragment.to_string())),
            None => (href, None),
        };
        let resource = resolve_href(nav_path, path_part);
        let title = collect_text(node);
        if title.trim().is_empty() {
            continue;
        }
        let mut ancestors: Vec<_> = node
            .ancestors()
            .filter(|ancestor| ancestor.is_element() && ancestor.tag_name().name() == "li")
            .collect();
        let id = ancestors.first().map_or_else(
            || format!("nav-a-{}", node.range().start),
            |li| format!("nav-{}", li.range().start),
        );
        if !ancestors.is_empty() {
            ancestors.remove(0);
        }
        ancestors.reverse();
        let groups = ancestors
            .into_iter()
            .filter_map(|li| {
                let label = li.children().find(|child| {
                    child.is_element() && matches!(child.tag_name().name(), "a" | "span")
                })?;
                Some(NavGroup {
                    resource: label
                        .attribute("href")
                        .map(|href| resolve_href(nav_path, href)),
                    id: format!("nav-{}", li.range().start),
                    title: collect_text(label).trim().to_string(),
                })
            })
            .collect();
        entries.push(NavEntry {
            id,
            groups,
            resource,
            fragment,
            title: title.trim().to_string(),
        });
    }
    Ok(entries)
}

/// Legacy NCX navPoint nesting describes the same groups as EPUB 3 lists.
/// Only the navMap is read; page lists and navLists do not create chapters.
pub fn parse_ncx(bytes: &[u8], ncx_path: &str) -> Result<Vec<NavEntry>, AppError> {
    const NS: &str = "http://www.daisy.org/z3986/2005/ncx/";
    let text = std::str::from_utf8(bytes)
        .map_err(|_| AppError::new("invalid_epub", "The NCX navigation is not UTF-8."))?;
    let doc = crate::epub::xml::parse(text)
        .map_err(|_| AppError::new("invalid_epub", "The NCX navigation is not valid XML."))?;
    let Some(map) = doc
        .descendants()
        .find(|node| node.has_tag_name((NS, "navMap")))
    else {
        return Ok(Vec::new());
    };
    let label = |point: roxmltree::Node<'_, '_>| {
        point
            .children()
            .find(|node| node.has_tag_name((NS, "navLabel")))
            .and_then(|node| {
                node.children()
                    .find(|child| child.has_tag_name((NS, "text")))
            })
            .map(collect_text)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let target = |point: roxmltree::Node<'_, '_>| {
        point
            .children()
            .find(|node| node.has_tag_name((NS, "content")))
            .and_then(|node| node.attribute("src"))
            .map(str::to_string)
    };
    let mut entries = Vec::new();
    for point in map
        .descendants()
        .filter(|node| node.has_tag_name((NS, "navPoint")))
    {
        let Some(href) = target(point) else {
            continue;
        };
        let title = label(point);
        if title.is_empty() {
            continue;
        }
        let mut parents: Vec<_> = point
            .ancestors()
            .skip(1)
            .filter(|node| node.has_tag_name((NS, "navPoint")))
            .collect();
        parents.reverse();
        let groups = parents
            .into_iter()
            .map(|parent| NavGroup {
                id: format!("ncx-{}", parent.range().start),
                title: label(parent),
                resource: target(parent).map(|href| resolve_href(ncx_path, &href)),
            })
            .collect();
        entries.push(NavEntry {
            id: format!("ncx-{}", point.range().start),
            resource: resolve_href(ncx_path, &href),
            fragment: href
                .split_once('#')
                .map(|(_, fragment)| fragment.to_string()),
            title,
            groups,
        });
    }
    Ok(entries)
}

fn collect_text(node: roxmltree::Node<'_, '_>) -> String {
    let mut text = String::new();
    for descendant in node.descendants().skip(1) {
        if descendant.is_text()
            && let Some(chunk) = descendant.text()
        {
            text.push_str(chunk);
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ncx_retains_nested_volume_labels_and_ignores_page_lists() {
        let ncx = br#"<?xml version="1.0"?><!DOCTYPE ncx PUBLIC "-//NISO//DTD ncx 2005-1//EN" "http://www.daisy.org/z3986/2005/ncx-2005-1.dtd">
        <ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
          <navMap><navPoint id="v1"><navLabel><text>Volume 1</text></navLabel><content src="text/v1.xhtml"/>
            <navPoint id="c1"><navLabel><text>Chapter 1</text></navLabel><content src="text/c1.xhtml#start"/></navPoint>
          </navPoint><navPoint id="v2"><navLabel><text>Volume 2</text></navLabel><content src="text/v2.xhtml"/>
            <navPoint id="c2"><navLabel><text>Chapter 1</text></navLabel><content src="text/c2.xhtml"/></navPoint>
          </navPoint></navMap>
          <pageList><pageTarget><navLabel><text>Page</text></navLabel><content src="other.xhtml"/></pageTarget></pageList>
        </ncx>"#;
        let entries = parse_ncx(ncx, "OPS/toc.ncx").expect("NCX fixture");
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[1].resource, "OPS/text/c1.xhtml");
        assert_eq!(entries[1].fragment.as_deref(), Some("start"));
        assert_eq!(entries[1].groups[0].title, "Volume 1");
        assert_eq!(entries[3].groups[0].title, "Volume 2");
        assert_ne!(entries[1].groups[0].id, entries[3].groups[0].id);
        assert!(
            parse_ncx(
                br#"<!DOCTYPE ncx [<!ENTITY x SYSTEM "file:///private">]><ncx/>"#,
                "toc.ncx"
            )
            .is_err()
        );
    }

    #[test]
    fn preserves_linked_and_unlinked_nested_groups() {
        let nav = br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><nav><ol>
          <li><a href="v1.xhtml">Volume 1</a><ol>
            <li><a href="c1.xhtml#start">Chapter 1</a></li>
            <li><span>Part Two</span><ol><li><a href="c2.xhtml">Chapter 2</a></li></ol></li>
          </ol></li>
          <li><span>Volume 2</span><ol><li><a href="c3.xhtml">Chapter 1</a></li></ol></li>
        </ol></nav></body></html>"#;
        let entries = parse_nav(nav, "OPS/nav.xhtml").expect("synthetic nav");
        assert_eq!(entries.len(), 4);
        assert!(entries[0].groups.is_empty());
        assert_eq!(entries[1].groups[0].title, "Volume 1");
        assert_eq!(
            entries[1].groups[0].resource.as_deref(),
            Some("OPS/v1.xhtml")
        );
        assert_eq!(entries[1].fragment.as_deref(), Some("start"));
        assert_eq!(entries[2].groups.len(), 2);
        assert_eq!(entries[2].groups[1].title, "Part Two");
        assert_eq!(entries[3].groups[0].title, "Volume 2");
        assert_ne!(entries[1].groups[0].id, entries[3].groups[0].id);
    }

    #[test]
    fn parses_toc_entries_with_fragments() {
        let nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
 <body>
  <nav epub:type="toc"><h1>Contents</h1>
   <ol>
    <li><a href="text/chapter01.xhtml">Introduction</a></li>
    <li><a href="text/chapter02.xhtml#section-2">Chapter Two</a></li>
   </ol>
  </nav>
  <nav epub:type="landmarks"><ol><li><a href="cover.xhtml">Start</a></li></ol></nav>
 </body>
</html>"#;
        let entries = parse_nav(nav.as_bytes(), "OEBPS/nav.xhtml").expect("parse nav");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].resource, "OEBPS/text/chapter01.xhtml");
        assert_eq!(entries[0].title, "Introduction");
        assert_eq!(entries[1].fragment.as_deref(), Some("section-2"));
        // The landmarks nav must not leak into the toc.
        assert!(!entries.iter().any(|e| e.title == "Start"));
    }
}
