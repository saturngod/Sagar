//! Reads an EPUB (OCF/OPF/spine/nav) into the reflowable [`Book`] model.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::domain::ImportWarning;
use crate::epub::archive::SafeArchive;
use crate::error::AppError;
use crate::security::ArchiveLimits;

use super::css::{Stylesheet, is_inherited};
use super::dom::{Child, Dom, NodeId, ROOT};
use super::layout::collapse_margins;
use super::model::{
    BlockKind, Book, BookMetadata, Chapter, EmbeddedFont, LinkTarget, NavigationEntry, StyledSpan,
    TextAnchor, TextBlock, TextStyle,
};
use super::util::{normalize_path, percent_decode, resolve_epub_path, sha1};

/// Builds a navigation entry from (href, base document, label, children).
type NavTarget = dyn Fn(&str, &str, String, Vec<NavigationEntry>) -> NavigationEntry;

struct Archive {
    inner: SafeArchive,
    /// Normalized path → raw entry name.
    names: HashMap<String, String>,
}

impl Archive {
    fn open(path: &Path) -> Result<Self, AppError> {
        let inner = SafeArchive::open(path, &ArchiveLimits::default())?;
        let names = inner
            .entry_names()
            .iter()
            .map(|name| (normalize_path(name), name.clone()))
            .collect();
        Ok(Self { inner, names })
    }

    fn contains(&self, path: &str) -> bool {
        self.names.contains_key(&normalize_path(path))
    }

    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let raw = self.names.get(&normalize_path(path))?.clone();
        self.inner.read_entry(&raw).ok()
    }

    fn require(&mut self, path: &str) -> Result<Vec<u8>, AppError> {
        self.read(path).ok_or_else(|| {
            invalid(&format!(
                "A required file ({}) is missing from the book.",
                file_name(path)
            ))
        })
    }

    fn parse(&mut self, path: &str) -> Result<Dom, AppError> {
        let bytes = self.require(path)?;
        Dom::parse(&bytes)
            .map_err(|_| invalid(&format!("“{}” is not well-formed XHTML.", file_name(path))))
    }
}

fn invalid(message: &str) -> AppError {
    AppError::new(
        "kfx_invalid_epub",
        format!("The book could not be converted to KFX. {message}"),
    )
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn warning(code: &str, message: String, location: Option<String>) -> ImportWarning {
    ImportWarning {
        code: code.into(),
        message,
        location,
    }
}

pub fn read_epub(path: &Path) -> Result<(Book, Vec<ImportWarning>), AppError> {
    let mut archive = Archive::open(path)?;
    let mut warnings = Vec::new();
    let container = archive.parse("META-INF/container.xml")?;
    let rootfile = container
        .first(ROOT, "rootfile")
        .and_then(|id| container.attr(id, "full-path"))
        .map(str::to_string)
        .ok_or_else(|| invalid("container.xml has no package document."))?;
    let package = archive.parse(&rootfile)?;
    let metadata_node = package
        .first(ROOT, "metadata")
        .ok_or_else(|| invalid("The package document has no metadata."))?;
    let identifiers = package.descendants(metadata_node, "identifier");
    let unique_id = package
        .first(ROOT, "package")
        .and_then(|p| package.attr(p, "unique-identifier"));
    let identifier = identifiers
        .iter()
        .find(|id| unique_id.is_some() && package.attr(**id, "id") == unique_id)
        .or(identifiers.first())
        .map(|id| package.text(*id).trim().to_string())
        .unwrap_or_default();
    let text_of = |name: &str| {
        package
            .first(metadata_node, name)
            .map(|id| package.text(id).trim().to_string())
    };
    let metas = package.descendants(metadata_node, "meta");
    let mut metadata = BookMetadata {
        title: text_of("title")
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Untitled".into()),
        authors: package
            .descendants(metadata_node, "creator")
            .into_iter()
            .map(|id| package.text(id).trim().to_string())
            .collect(),
        language: text_of("language")
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "en".into()),
        identifier: identifier.clone(),
        publisher: text_of("publisher"),
        cover_image: None,
        modified_date: metas
            .iter()
            .find(|m| package.attr(**m, "property") == Some("dcterms:modified"))
            .map(|m| package.text(*m).trim().to_string()),
    };
    let items: Vec<NodeId> = package
        .first(ROOT, "manifest")
        .map(|m| package.elements(m))
        .unwrap_or_default();
    let mut manifest: HashMap<String, NodeId> = HashMap::new();
    for item in &items {
        if let Some(id) = package.attr(*item, "id") {
            manifest.entry(id.to_string()).or_insert(*item);
        }
    }
    let has_property = |item: NodeId, property: &str| {
        package
            .attr(item, "properties")
            .is_some_and(|p| p.split_whitespace().any(|v| v == property))
    };
    let cover_id = metas
        .iter()
        .find(|m| package.attr(**m, "name") == Some("cover"))
        .and_then(|m| package.attr(*m, "content"));
    let cover_item = items
        .iter()
        .copied()
        .find(|item| has_property(*item, "cover-image"))
        .or_else(|| cover_id.and_then(|id| manifest.get(id).copied()));
    if let Some(href) = cover_item.and_then(|item| package.attr(item, "href")) {
        metadata.cover_image = Some(resolve_epub_path(href, &rootfile));
    }
    // An EPUB 2 guide may point at a cover XHTML page rather than an image.
    if metadata.cover_image.is_none()
        && let Some(href) = package
            .descendants(ROOT, "reference")
            .into_iter()
            .find(|r| package.attr(*r, "type") == Some("cover"))
            .and_then(|r| package.attr(r, "href"))
    {
        let path = resolve_epub_path(href, &rootfile);
        let lower = path.to_lowercase();
        if lower.ends_with(".xhtml") || lower.ends_with(".html") {
            if let Ok(page) = archive.parse(&path)
                && let Some(src) = page
                    .first(ROOT, "img")
                    .and_then(|img| page.attr(img, "src"))
            {
                metadata.cover_image = Some(resolve_epub_path(src, &path));
            }
        } else {
            metadata.cover_image = Some(path);
        }
    }

    let mut chapters = Vec::new();
    let mut resources: HashMap<String, Vec<u8>> = HashMap::new();
    let mut fonts: Vec<EmbeddedFont> = Vec::new();
    let spine = package.first(ROOT, "spine");
    for itemref in spine.map(|s| package.elements(s)).unwrap_or_default() {
        if package.attr(itemref, "linear") == Some("no") {
            continue;
        }
        let Some(href) = package
            .attr(itemref, "idref")
            .and_then(|id| manifest.get(id))
            .and_then(|item| package.attr(*item, "href"))
        else {
            continue;
        };
        let path = resolve_epub_path(href, &rootfile);
        let mut root = archive.parse(&path)?;
        let mut stylesheet = Stylesheet::default();
        let mut visited = HashSet::new();
        // Head order matters: later rules win equal-specificity ties.
        let head_children = root
            .first(ROOT, "head")
            .map(|h| root.elements(h))
            .unwrap_or_default();
        for node in head_children {
            let name = root.node(node).name.clone();
            if name == "link"
                && root
                    .attr(node, "rel")
                    .is_some_and(|rel| rel.split_whitespace().any(|r| r == "stylesheet"))
                && let Some(href) = root.attr(node, "href")
            {
                let css_path = resolve_epub_path(href, &path);
                match archive.read(&css_path) {
                    Some(bytes) => {
                        let css = String::from_utf8_lossy(&bytes).into_owned();
                        stylesheet.append(&css, &css_path, &mut |p| archive.read(p), &mut visited);
                    }
                    None => warnings.push(warning(
                        "kfx_missing_stylesheet",
                        format!(
                            "Stylesheet “{}” is missing and was ignored.",
                            file_name(&css_path)
                        ),
                        Some(file_name(&path).to_string()),
                    )),
                }
            } else if name == "style" {
                let css = root.text(node);
                stylesheet.append(&css, &path, &mut |p| archive.read(p), &mut HashSet::new());
            }
        }
        let blocks = extract_blocks(&mut root, &stylesheet, &path);
        for font in &stylesheet.fonts {
            if fonts.contains(font) {
                continue;
            }
            match archive.read(&font.path) {
                Some(bytes) => {
                    resources.insert(font.path.clone(), bytes);
                    fonts.push(font.clone());
                }
                None => warnings.push(warning(
                    "kfx_missing_font",
                    format!(
                        "Font “{}” is missing and was not embedded.",
                        file_name(&font.path)
                    ),
                    None,
                )),
            }
        }
        for block in &blocks {
            if let Some(image) = &block.image_path
                && !resources.contains_key(image)
            {
                match archive.read(image) {
                    Some(bytes) => {
                        resources.insert(image.clone(), bytes);
                    }
                    None => warnings.push(warning(
                        "kfx_missing_image",
                        format!("Image “{}” is missing and was omitted.", file_name(image)),
                        Some(file_name(&path).to_string()),
                    )),
                }
            }
        }
        let title = blocks
            .iter()
            .find(|b| matches!(b.kind, BlockKind::Heading(_)))
            .map(|b| b.text.clone())
            .or_else(|| root.first(ROOT, "title").map(|t| root.text(t)));
        chapters.push(Chapter {
            source_path: path,
            title,
            blocks,
        });
    }
    if let Some(cover) = metadata.cover_image.clone()
        && !resources.contains_key(&cover)
    {
        match archive.read(&cover) {
            Some(bytes) => {
                resources.insert(cover, bytes);
            }
            None => metadata.cover_image = None,
        }
    }
    if archive.contains("META-INF/encryption.xml") {
        let encryption = archive.parse("META-INF/encryption.xml")?;
        for entry in encryption.descendants(ROOT, "encrypteddata") {
            let uri = encryption
                .first(entry, "cipherreference")
                .and_then(|c| encryption.attr(c, "URI"));
            let algorithm = encryption
                .first(entry, "encryptionmethod")
                .and_then(|m| encryption.attr(m, "Algorithm"));
            let (Some(uri), Some(algorithm)) = (uri, algorithm) else {
                continue;
            };
            let path = normalize_path(&percent_decode(uri).unwrap_or_else(|| uri.to_string()));
            let Some(bytes) = resources.get_mut(&path) else {
                continue;
            };
            let (key, count): (Vec<u8>, usize) = match algorithm {
                "http://www.idpf.org/2008/embedding" => {
                    let clean: String = identifier.chars().filter(|c| !c.is_whitespace()).collect();
                    (sha1(clean.as_bytes()).to_vec(), 1040)
                }
                "http://ns.adobe.com/pdf/enc#RC" => {
                    let clean = identifier.replace("urn:uuid:", "").replace('-', "");
                    let key: Vec<u8> = (0..clean.len())
                        .step_by(2)
                        .filter_map(|i| {
                            clean
                                .get(i..i + 2)
                                .and_then(|h| u8::from_str_radix(h, 16).ok())
                        })
                        .collect();
                    if clean.len() != 32 || key.len() != 16 {
                        return Err(invalid("An embedded font uses an invalid obfuscation key."));
                    }
                    (key, 1024)
                }
                _ => {
                    return Err(AppError::new(
                        "kfx_encrypted_resource",
                        "This book contains encrypted resources and cannot be converted to KFX.",
                    ));
                }
            };
            let limit = count.min(bytes.len());
            for (index, byte) in bytes.iter_mut().take(limit).enumerate() {
                *byte ^= key[index % key.len()];
            }
        }
    }
    if chapters.is_empty() {
        return Err(invalid("The reading order contains no chapters."));
    }

    let mut navigation = Vec::new();
    let target = |href: &str, base: &str, title: String, children: Vec<NavigationEntry>| {
        let (file, fragment) = match href.split_once('#') {
            Some((file, fragment)) => (file, Some(fragment)),
            None => (href, None),
        };
        NavigationEntry {
            title,
            path: if file.is_empty() {
                base.to_string()
            } else {
                resolve_epub_path(file, base)
            },
            fragment: fragment.and_then(percent_decode),
            children,
        }
    };
    if let Some(nav_item) = items
        .iter()
        .copied()
        .find(|item| has_property(*item, "nav"))
        && let Some(href) = package.attr(nav_item, "href")
    {
        let path = resolve_epub_path(href, &rootfile);
        if let Ok(root) = archive.parse(&path)
            && let Some(toc) = root.descendants(ROOT, "nav").into_iter().find(|n| {
                root.attr(*n, "type")
                    .is_some_and(|t| t.split_whitespace().any(|v| v == "toc"))
            })
        {
            fn entries(
                root: &Dom,
                list: NodeId,
                path: &str,
                target: &NavTarget,
            ) -> Vec<NavigationEntry> {
                let mut out = Vec::new();
                for item in root
                    .elements(list)
                    .into_iter()
                    .filter(|e| root.node(*e).name == "li")
                {
                    let children = root
                        .elements(item)
                        .into_iter()
                        .find(|e| root.node(*e).name == "ol")
                        .map(|ol| entries(root, ol, path, target))
                        .unwrap_or_default();
                    // Unlinked part headings keep their linked descendants.
                    match root
                        .elements(item)
                        .into_iter()
                        .find(|e| root.node(*e).name == "a")
                        .and_then(|a| root.attr(a, "href").map(|href| (a, href)))
                    {
                        Some((a, href)) => out.push(target(href, path, root.text(a), children)),
                        None => out.extend(children),
                    }
                }
                out
            }
            if let Some(list) = root
                .elements(toc)
                .into_iter()
                .find(|e| root.node(*e).name == "ol")
            {
                navigation = entries(&root, list, &path, &target);
            }
        }
    } else if let Some(href) = spine
        .and_then(|s| package.attr(s, "toc"))
        .and_then(|id| manifest.get(id))
        .and_then(|item| package.attr(*item, "href"))
    {
        let path = resolve_epub_path(href, &rootfile);
        if let Ok(root) = archive.parse(&path) {
            fn entries(
                root: &Dom,
                node: NodeId,
                path: &str,
                target: &NavTarget,
            ) -> Vec<NavigationEntry> {
                let mut out = Vec::new();
                for point in root
                    .elements(node)
                    .into_iter()
                    .filter(|e| root.node(*e).name == "navpoint")
                {
                    let children = entries(root, point, path, target);
                    let src = root
                        .elements(point)
                        .into_iter()
                        .find(|e| root.node(*e).name == "content")
                        .and_then(|c| root.attr(c, "src"));
                    match src {
                        Some(src) => {
                            let label = root
                                .elements(point)
                                .into_iter()
                                .find(|e| root.node(*e).name == "navlabel")
                                .map(|l| root.text(l))
                                .unwrap_or_default();
                            out.push(target(src, path, label, children));
                        }
                        None => out.extend(children),
                    }
                }
                out
            }
            if let Some(map) = root.first(ROOT, "navmap") {
                navigation = entries(&root, map, &path, &target);
            }
        }
    }
    Ok((
        Book {
            metadata,
            chapters,
            resources,
            fonts,
            navigation,
        },
        warnings,
    ))
}

const BLOCK_NAMES: [&str; 25] = [
    "body",
    "div",
    "section",
    "article",
    "p",
    "li",
    "blockquote",
    "pre",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "table",
    "thead",
    "tbody",
    "tr",
    "td",
    "th",
    "ul",
    "ol",
    "figure",
    "figcaption",
    "nav",
];

fn is_block_name(name: &str) -> bool {
    BLOCK_NAMES.contains(&name)
}

fn hidden(style: &TextStyle) -> bool {
    style.get("display") == Some("none") || style.get("visibility") == Some("hidden")
}

/// Per-block inline text accumulation.
#[derive(Default)]
struct Run {
    text: String,
    spans: Vec<StyledSpan>,
    anchors: Vec<TextAnchor>,
    offset: usize,
    pending_space: Option<(TextStyle, Option<LinkTarget>)>,
}

struct Extractor<'a> {
    dom: &'a Dom,
    path: &'a str,
    result: Vec<TextBlock>,
    pending_anchors: Vec<String>,
}

impl Extractor<'_> {
    fn record_anchor(&mut self, id: NodeId) {
        let node = self.dom.node(id);
        let anchor = node.attributes.get("id").or_else(|| {
            if node.name == "a" {
                node.attributes.get("name")
            } else {
                None
            }
        });
        if let Some(anchor) = anchor {
            self.pending_anchors.push(anchor.clone());
        }
    }

    fn append_block(&mut self, mut block: TextBlock) {
        block.anchors.extend(
            self.pending_anchors
                .drain(..)
                .map(|id| TextAnchor { id, offset: 0 }),
        );
        self.result.push(block);
    }

    fn append_char(
        &mut self,
        run: &mut Run,
        c: char,
        style: &TextStyle,
        link: &Option<LinkTarget>,
        consume_anchors: bool,
    ) {
        if consume_anchors {
            let offset = run.offset;
            run.anchors.extend(
                self.pending_anchors
                    .drain(..)
                    .map(|id| TextAnchor { id, offset }),
            );
        }
        run.text.push(c);
        match run.spans.last_mut() {
            Some(last)
                if &last.style == style
                    && &last.link == link
                    && last.offset + last.length == run.offset =>
            {
                last.length += 1;
            }
            _ => run.spans.push(StyledSpan {
                offset: run.offset,
                length: 1,
                style: style.clone(),
                link: link.clone(),
            }),
        }
        run.offset += 1;
    }

    fn append_text(
        &mut self,
        run: &mut Run,
        value: &str,
        style: &TextStyle,
        link: &Option<LinkTarget>,
    ) {
        let preserve = style
            .get("white-space")
            .is_some_and(|w| w.starts_with("pre"));
        for c in value.chars() {
            if !preserve && c != '\u{a0}' && c.is_whitespace() {
                if run.offset > 0 {
                    run.pending_space = Some((style.clone(), link.clone()));
                }
            } else {
                if let Some((space_style, space_link)) = run.pending_space.take() {
                    self.append_char(run, ' ', &space_style, &space_link, false);
                }
                self.append_char(run, c, style, link, true);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn flush(
        &mut self,
        run: &mut Run,
        node: NodeId,
        kind: BlockKind,
        owner: Option<usize>,
        parent: Option<usize>,
    ) {
        if !run.text.is_empty() {
            let dom = self.dom;
            let node_style = &dom.node(node).style;
            let text_kind = if owner != parent {
                BlockKind::Paragraph
            } else {
                kind
            };
            let mut text_style = node_style.clone();
            if owner != parent {
                // The container owns the box; anonymous text keeps only text styling.
                text_style.properties.retain(|key, _| is_inherited(key));
            }
            let offset = run.offset;
            run.anchors.extend(
                self.pending_anchors
                    .drain(..)
                    .map(|id| TextAnchor { id, offset }),
            );
            let anchor_id = dom
                .attr(node, "id")
                .filter(|id| run.anchors.iter().any(|a| a.id == *id && a.offset == 0))
                .map(str::to_string);
            let mut block = TextBlock::new(text_kind, text_style);
            block.text = std::mem::take(&mut run.text);
            block.spans = run
                .spans
                .drain(..)
                .filter(|s| &s.style != node_style || s.link.is_some())
                .collect();
            block.parent_index = owner;
            block.anchor_id = anchor_id;
            block.anchors = std::mem::take(&mut run.anchors);
            self.append_block(block);
        }
        *run = Run::default();
    }

    fn visit(&mut self, node: NodeId, parent: Option<usize>, link: Option<LinkTarget>) {
        let dom = self.dom;
        let n = dom.node(node);
        if hidden(&n.style) || matches!(n.name.as_str(), "script" | "style" | "head") {
            return;
        }
        self.record_anchor(node);
        let name = n.name.as_str();
        let kind = match name {
            "h1" => BlockKind::Heading(1),
            "h2" => BlockKind::Heading(2),
            "h3" => BlockKind::Heading(3),
            "h4" => BlockKind::Heading(4),
            "h5" => BlockKind::Heading(5),
            "h6" => BlockKind::Heading(6),
            "pre" => BlockKind::Preformatted,
            "li" => BlockKind::ListItem,
            _ => BlockKind::Paragraph,
        };
        let id_attr = n.attributes.get("id").cloned();
        if name == "hr" {
            let mut block = TextBlock::new(BlockKind::HorizontalRule, n.style.clone());
            block.parent_index = parent;
            block.anchor_id = id_attr;
            self.append_block(block);
            return;
        }
        let page_break = [
            "break-before",
            "page-break-before",
            "break-after",
            "page-break-after",
        ]
        .iter()
        .any(|key| matches!(n.style.get(key), Some("page" | "always")));
        if page_break && dom.text(node).trim().is_empty() && dom.elements(node).is_empty() {
            let mut block = TextBlock::new(BlockKind::Container, n.style.clone());
            block.parent_index = parent;
            block.anchor_id = id_attr;
            self.append_block(block);
            return;
        }
        let list = name == "ul" || name == "ol";
        let grouping = matches!(
            name,
            "div" | "section" | "article" | "nav" | "figure" | "blockquote" | "li"
        ) && dom.elements(node).into_iter().any(|child| {
            let child_name = dom.node(child).name.as_str();
            is_block_name(child_name)
                || child_name == "hr"
                || !dom.descendants(child, "img").is_empty()
                || !dom.descendants(child, "image").is_empty()
        });
        let mut owner = parent;
        if list || grouping {
            owner = Some(self.result.len());
            let container_kind = if list {
                if name == "ol" {
                    BlockKind::OrderedList
                } else {
                    BlockKind::UnorderedList
                }
            } else if name == "li" {
                BlockKind::ListItem
            } else {
                BlockKind::Container
            };
            let mut block = TextBlock::new(container_kind, n.style.clone());
            block.parent_index = parent;
            block.anchor_id = id_attr;
            block.list_start = n
                .attributes
                .get("start")
                .and_then(|s| s.trim().parse().ok());
            self.append_block(block);
        }
        let mut run = Run::default();
        for child in &n.children {
            match child {
                Child::Text(value) => self.append_text(&mut run, value, &n.style, &link),
                Child::Element(element) => {
                    self.inline(&mut run, node, kind, owner, parent, *element, &link)
                }
            }
        }
        self.flush(&mut run, node, kind, owner, parent);
    }

    #[allow(clippy::too_many_arguments)]
    fn inline(
        &mut self,
        run: &mut Run,
        node: NodeId,
        kind: BlockKind,
        owner: Option<usize>,
        parent: Option<usize>,
        child: NodeId,
        inherited: &Option<LinkTarget>,
    ) {
        let dom = self.dom;
        let c = dom.node(child);
        if hidden(&c.style) || matches!(c.name.as_str(), "script" | "style") {
            return;
        }
        let active = if c.name == "a" && c.attributes.contains_key("href") {
            c.attributes
                .get("href")
                .and_then(|href| LinkTarget::from_href(href, self.path))
        } else {
            inherited.clone()
        };
        if c.name == "img" || c.name == "image" {
            self.flush(run, node, kind, owner, parent);
            self.record_anchor(child);
            if let Some(src) = c.attributes.get("src").or_else(|| c.attributes.get("href")) {
                let mut block = TextBlock::new(BlockKind::Image, c.style.clone());
                block.image_path = Some(resolve_epub_path(src, self.path));
                block.parent_index = owner;
                block.anchor_id = c.attributes.get("id").cloned();
                block.link = active;
                self.append_block(block);
            }
        } else if c.name == "br" {
            // A collapsed space before a hard break still occupies a position.
            if let Some((style, link)) = run.pending_space.take() {
                self.append_char(run, ' ', &style, &link, false);
            }
            self.record_anchor(child);
            self.append_char(run, '\n', &c.style, &active, true);
        } else if is_block_name(&c.name) || c.name == "hr" {
            self.flush(run, node, kind, owner, parent);
            self.visit(child, owner, active);
        } else {
            self.record_anchor(child);
            for item in &c.children {
                match item {
                    Child::Text(text) => self.append_text(run, text, &c.style, &active),
                    Child::Element(element) => {
                        self.inline(run, node, kind, owner, parent, *element, &active)
                    }
                }
            }
        }
    }
}

/// Computes styles, collapses margins and flattens `<body>` into preorder blocks.
pub fn extract_blocks(dom: &mut Dom, stylesheet: &Stylesheet, path: &str) -> Vec<TextBlock> {
    fn compute(dom: &mut Dom, sheet: &Stylesheet, id: NodeId, parent: &TextStyle) {
        let style = sheet.computed(dom, id, parent);
        dom.nodes[id].style = style.clone();
        for child in dom.elements(id) {
            compute(dom, sheet, child, &style);
        }
    }
    compute(dom, stylesheet, ROOT, &TextStyle::default());
    let Some(body) = dom.first(ROOT, "body") else {
        return Vec::new();
    };
    collapse_margins(dom, body);
    let mut extractor = Extractor {
        dom,
        path,
        result: Vec::new(),
        pending_anchors: Vec::new(),
    };
    extractor.visit(body, None, None);
    if !extractor.pending_anchors.is_empty() {
        if let Some(last) = extractor.result.last_mut() {
            let offset = last.char_count();
            last.anchors.extend(
                extractor
                    .pending_anchors
                    .drain(..)
                    .map(|id| TextAnchor { id, offset }),
            );
        } else {
            extractor.append_block(TextBlock::new(BlockKind::Container, TextStyle::default()));
        }
    }
    extractor.result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(xhtml: &str, css: &str) -> Vec<TextBlock> {
        let mut dom = Dom::parse(xhtml.as_bytes()).expect("parse");
        let mut sheet = Stylesheet::default();
        sheet.append(css, "OEBPS/a.css", &mut |_| None, &mut HashSet::new());
        extract_blocks(&mut dom, &sheet, "OEBPS/c1.xhtml")
    }

    #[test]
    fn extracts_headings_paragraphs_spans_links_and_anchors() {
        let result = blocks(
            r##"<html><body><h1 id="top">Title</h1><p>Hello   <em>မြန်မာ</em> <a href="c2.xhtml#n">link</a><span id="mid"/>end</p></body></html>"##,
            "",
        );
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].kind, BlockKind::Heading(1));
        assert_eq!(result[0].anchor_id.as_deref(), Some("top"));
        assert_eq!(result[1].text, "Hello မြန်မာ linkend");
        let em = result[1]
            .spans
            .iter()
            .find(|s| s.style.get("font-style") == Some("italic"))
            .expect("em span");
        assert_eq!((em.offset, em.length), (6, "မြန်မာ".chars().count()));
        assert!(result[1].spans.iter().any(|s| s.link
            == Some(LinkTarget::Location {
                path: "OEBPS/c2.xhtml".into(),
                fragment: Some("n".into())
            })));
        assert!(
            result[1]
                .anchors
                .iter()
                .any(|a| a.id == "mid" && a.offset == result[1].text.chars().count() - 3)
        );
    }

    #[test]
    fn groups_lists_and_images() {
        let result = blocks(
            r#"<html><body><ol start="3"><li>One</li><li>Two</li></ol><div><img src="img/a.png"/><p>Cap</p></div></body></html>"#,
            "",
        );
        let kinds: Vec<BlockKind> = result.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            vec![
                BlockKind::OrderedList,
                BlockKind::ListItem,
                BlockKind::ListItem,
                BlockKind::Container,
                BlockKind::Image,
                BlockKind::Paragraph
            ]
        );
        assert_eq!(result[0].list_start, Some(3));
        assert_eq!(result[1].parent_index, Some(0));
        assert_eq!(result[4].image_path.as_deref(), Some("OEBPS/img/a.png"));
        assert_eq!(result[4].parent_index, Some(3));
    }

    #[test]
    fn hides_display_none() {
        let result = blocks(
            r#"<html><body><p class="h">x</p><p>y</p></body></html>"#,
            ".h{display:none}",
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "y");
    }
}
