//! KFX container writer: turns a [`Book`] into Kindle Format 10 bytes.
//! Ported from Yay's Swift `KFXWriter`; symbol ids are YJ_symbols v10.

use std::collections::{BTreeSet, HashMap};

use base64::Engine;

use crate::domain::ImportWarning;

use super::image::ImageResource;
use super::ion::{Ion, ion_data};
use super::layout::LayoutMetrics;
use super::model::{BlockKind, Book, Chapter, LinkTarget, NavigationEntry, TextBlock, TextStyle};
use super::style::{box_fields, dimension, fields as style_fields, needs_box};
use super::util::{sha1, sha1_hex};

mod s {
    pub const NAME: u64 = 4;
    pub const FONT_SIZE: u64 = 16;
    pub const LINE_HEIGHT: u64 = 42;
    pub const WIDTH: u64 = 56;
    pub const PAGE_TEMPLATES: u64 = 141;
    pub const OFFSET: u64 = 143;
    pub const CONTENT: u64 = 145;
    pub const CONTENT_LIST: u64 = 146;
    pub const ID: u64 = 155;
    pub const STYLE: u64 = 157;
    pub const TYPE: u64 = 159;
    pub const READING_ORDERS: u64 = 169;
    pub const SECTIONS: u64 = 170;
    pub const STYLE_NAME: u64 = 173;
    pub const SECTION_NAME: u64 = 174;
    pub const STORY_NAME: u64 = 176;
    pub const READING_ORDER_NAME: u64 = 178;
    pub const CONTAINS: u64 = 181;
    pub const LOCATIONS: u64 = 182;
    pub const PID: u64 = 184;
    pub const EID: u64 = 185;
    pub const DIRECTION: u64 = 192;
    pub const TOC: u64 = 212;
    pub const NAV_TYPE: u64 = 235;
    pub const LANDMARKS: u64 = 236;
    pub const LANDMARK_TYPE: u64 = 238;
    pub const NAV_CONTAINER_NAME: u64 = 239;
    pub const REPRESENTATION: u64 = 241;
    pub const LABEL: u64 = 244;
    pub const TARGET_POSITION: u64 = 246;
    pub const ENTRIES: u64 = 247;
    pub const CONTAINER_LIST: u64 = 252;
    pub const METADATA: u64 = 258;
    pub const STORYLINE: u64 = 259;
    pub const SECTION: u64 = 260;
    pub const POSITION_MAP: u64 = 264;
    pub const POSITION_ID_MAP: u64 = 265;
    pub const TEXT: u64 = 269;
    pub const LIST_ITEM: u64 = 277;
    pub const UNIT: u64 = 306;
    pub const VALUE: u64 = 307;
    pub const EM: u64 = 308;
    pub const NULL: u32 = 348;
    pub const DEFAULT: u64 = 351;
    pub const LTR: u64 = 376;
    pub const AUTO: u64 = 383;
    pub const BOOK_NAVIGATION: u64 = 389;
    pub const NAV_CONTAINER: u64 = 391;
    pub const NAV_CONTAINERS: u64 = 392;
    pub const NAV_UNIT: u64 = 393;
    pub const RESOURCE_PATH: u64 = 395;
    pub const SRL: u64 = 396;
    pub const INDEX: u64 = 403;
    pub const BC_CONT_ID: u64 = 409;
    pub const BC_COMPR_TYPE: u64 = 410;
    pub const BC_DRM_SCHEME: u64 = 411;
    pub const BC_CHUNK_SIZE: u64 = 412;
    pub const BC_INDEX_TAB_OFFSET: u64 = 413;
    pub const BC_INDEX_TAB_LENGTH: u64 = 414;
    pub const BC_DOC_SYMBOL_OFFSET: u64 = 415;
    pub const BC_DOC_SYMBOL_LENGTH: u64 = 416;
    pub const CONTAINER_ENTITY_MAP: u64 = 419;
    pub const SELECTION: u64 = 436;
    pub const ENABLED: u64 = 441;
    pub const SPACING_PERCENT_BASE: u64 = 477;
    pub const BOOK_METADATA: u64 = 490;
    pub const CATEGORISED_METADATA: u64 = 491;
    pub const KEY: u64 = 492;
    pub const CATEGORY: u64 = 495;
    pub const DOCUMENT_DATA: u64 = 538;
    pub const LOCATION_MAP: u64 = 550;
    pub const HORIZONTAL_TB: u64 = 557;
    pub const WRITING_MODE: u64 = 560;
    pub const CONTENT_FEATURES: u64 = 585;
    pub const NAMESPACE: u64 = 586;
    pub const MAJOR_VERSION: u64 = 587;
    pub const MINOR_VERSION: u64 = 588;
    pub const VERSION_INFO: u64 = 589;
    pub const FEATURES: u64 = 590;
    pub const FORMAT_CAPABILITIES: u64 = 593;
    pub const BC_F_CAPABILITIES_OFFSET: u64 = 594;
    pub const BC_F_CAPABILITIES_LENGTH: u64 = 595;
    pub const AUXILIARY_DATA: u64 = 597;
    pub const KFX_ID: u64 = 598;
}

struct Fragment {
    kind: u32,
    id: Option<String>,
    value: Ion,
    raw: Option<Vec<u8>>,
}

impl Fragment {
    fn new(kind: u64, id: Option<String>, value: Ion) -> Self {
        Self {
            kind: kind as u32,
            id,
            value,
            raw: None,
        }
    }
}

struct ChapterLayout {
    section_name: String,
    story_name: String,
    page_template_id: i64,
    block_ids: Vec<i64>,
    inner_ids: Vec<Option<i64>>,
    content_references: Vec<(String, usize)>,
}

#[derive(Default)]
struct SymbolTable {
    values: Vec<String>,
    ids: HashMap<String, u64>,
}

const SYMBOL_BASE: u64 = 852;

impl SymbolTable {
    fn intern(&mut self, value: &str) -> u64 {
        if let Some(id) = self.ids.get(value) {
            return *id;
        }
        let id = SYMBOL_BASE + self.values.len() as u64;
        self.values.push(value.to_string());
        self.ids.insert(value.to_string(), id);
        id
    }

    fn id(&self, value: &str) -> u64 {
        // Every name is interned during the survey before it is referenced.
        self.ids.get(value).copied().unwrap_or(0)
    }
}

type Run = (usize, usize, u64, Option<u64>);

struct Builder {
    book: Book,
    symbols: SymbolTable,
    metrics: LayoutMetrics,
    layouts: Vec<ChapterLayout>,
    content_chunks: Vec<Vec<String>>,
    content_chunk_bytes: usize,
    next_id: i64,
    image_names: HashMap<String, String>,
    images: HashMap<String, ImageResource>,
    style_names: HashMap<Vec<u8>, String>,
    styles: Vec<(String, Vec<(u64, Ion)>)>,
    anchor_names: HashMap<LinkTarget, String>,
    anchor_fragments: Vec<Fragment>,
    warnings: Vec<ImportWarning>,
    creator: String,
    version: String,
}

/// Encodes a book as a KFX container. Unsupported images are omitted with a
/// warning rather than failing the whole conversion.
pub fn encode(book: Book, creator: &str, version: &str) -> (Vec<u8>, Vec<ImportWarning>) {
    let mut builder = Builder::new(book, creator, version);
    let bytes = builder.build();
    (bytes, builder.warnings)
}

fn le16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn le32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn le64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn chars(text: &str) -> usize {
    text.chars().count()
}

impl Builder {
    fn new(mut book: Book, creator: &str, version: &str) -> Self {
        if let Some(cover) = book.metadata.cover_image.clone()
            && book.resources.contains_key(&cover)
        {
            let first_is_cover = book.chapters.first().is_some_and(|c| {
                c.is_image_only()
                    && c.blocks
                        .iter()
                        .any(|b| b.image_path.as_deref() == Some(cover.as_str()))
            });
            if !first_is_cover {
                let mut block = TextBlock::new(BlockKind::Image, TextStyle::default());
                block.image_path = Some(cover.clone());
                book.chapters.insert(
                    0,
                    Chapter {
                        source_path: cover,
                        title: Some("Cover".into()),
                        blocks: vec![block],
                    },
                );
            }
        }
        let metrics = LayoutMetrics::for_book(&book);
        Self {
            book,
            symbols: SymbolTable::default(),
            metrics,
            layouts: Vec::new(),
            content_chunks: vec![Vec::new()],
            content_chunk_bytes: 0,
            next_id: 1,
            image_names: HashMap::new(),
            images: HashMap::new(),
            style_names: HashMap::new(),
            styles: Vec::new(),
            anchor_names: HashMap::new(),
            anchor_fragments: Vec::new(),
            warnings: Vec::new(),
            creator: creator.to_string(),
            version: version.to_string(),
        }
    }

    fn build(&mut self) -> Vec<u8> {
        self.survey();
        let container_id = self.make_container_id();
        let fragments = self.make_fragments(&container_id);
        let entities: Vec<(u32, u32, Vec<u8>)> = fragments
            .iter()
            .map(|f| {
                let id =
                    f.id.as_ref()
                        .map(|id| self.symbols.id(id) as u32)
                        .unwrap_or(s::NULL);
                (id, f.kind, entity_data(&f.value, f.raw.as_deref()))
            })
            .collect();
        self.container(&container_id, &entities)
    }

    fn take_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn survey(&mut self) {
        for name in ["s0", "toc", "landmarks", "headings", "page_list"] {
            self.symbols.intern(name);
        }
        let paths: BTreeSet<String> = self
            .book
            .chapters
            .iter()
            .flat_map(|c| c.blocks.iter())
            .filter_map(|b| b.image_path.clone())
            .collect();
        for (index, path) in paths.into_iter().enumerate() {
            let Some(data) = self.book.resources.get(&path) else {
                continue;
            };
            match ImageResource::probe(data.clone()) {
                Some(image) => {
                    let name = format!("e{index}");
                    self.symbols.intern(&name);
                    self.symbols.intern(&format!("resource/{name}"));
                    self.image_names.insert(path.clone(), name);
                    self.images.insert(path, image);
                }
                None => self.warnings.push(ImportWarning {
                    code: "kfx_unsupported_image".into(),
                    message: format!(
                        "Image “{}” is not JPEG, PNG or GIF and was omitted from the KFX file.",
                        path.rsplit('/').next().unwrap_or(&path)
                    ),
                    location: None,
                }),
            }
        }
        for chapter_index in 0..self.book.chapters.len() {
            let section_name = format!("c{chapter_index}");
            let story_name = format!("story_{section_name}");
            self.symbols.intern(&section_name);
            self.symbols.intern(&story_name);
            self.symbols.intern(&format!("{section_name}-ad"));
            let page_template_id = self.take_id();
            let mut block_ids = Vec::new();
            let mut inner_ids = Vec::new();
            let mut content_references = Vec::new();
            let blocks: Vec<(String, bool)> = self.book.chapters[chapter_index]
                .blocks
                .iter()
                .map(|b| (b.text.clone(), needs_box(&b.style)))
                .collect();
            for (text, boxed) in blocks {
                block_ids.push(self.take_id());
                inner_ids.push(if !text.is_empty() && boxed {
                    Some(self.take_id())
                } else {
                    None
                });
                content_references.push(if text.is_empty() {
                    (String::new(), 0)
                } else {
                    self.append_content(text)
                });
            }
            self.layouts.push(ChapterLayout {
                section_name,
                story_name,
                page_template_id,
                block_ids,
                inner_ids,
                content_references,
            });
        }
    }

    fn append_content(&mut self, text: String) -> (String, usize) {
        if self.content_chunk_bytes >= 8192 {
            self.content_chunks.push(Vec::new());
            self.content_chunk_bytes = 0;
        }
        let chunk = self.content_chunks.len();
        let name = format!("content_{chunk}");
        self.symbols.intern(&name);
        let index = self.content_chunks[chunk - 1].len();
        self.content_chunk_bytes += text.len();
        self.content_chunks[chunk - 1].push(text);
        (name, index)
    }

    fn make_fragments(&mut self, container_id: &str) -> Vec<Fragment> {
        let mut fragments = vec![
            Fragment::new(s::CONTENT_FEATURES, None, self.content_features()),
            Fragment::new(s::BOOK_METADATA, None, self.book_metadata(container_id)),
            Fragment::new(
                s::METADATA,
                None,
                Ion::Struct(vec![(
                    s::READING_ORDERS,
                    Ion::List(vec![self.reading_order()]),
                )]),
            ),
            Fragment::new(s::DOCUMENT_DATA, None, self.document_data()),
            Fragment::new(s::BOOK_NAVIGATION, None, self.navigation()),
        ];
        for index in 0..self.layouts.len() {
            fragments.push(Fragment::new(
                s::SECTION,
                Some(self.layouts[index].section_name.clone()),
                self.section(index),
            ));
        }
        for index in 0..self.layouts.len() {
            let value = self.storyline(index);
            fragments.push(Fragment::new(
                s::STORYLINE,
                Some(self.layouts[index].story_name.clone()),
                value,
            ));
        }
        // Only link destinations actually referenced are emitted.
        fragments.append(&mut self.anchor_fragments);
        for (chunk_index, strings) in self.content_chunks.iter().enumerate() {
            if strings.is_empty() {
                continue;
            }
            let name = format!("content_{}", chunk_index + 1);
            fragments.push(Fragment::new(
                s::CONTENT,
                Some(name.clone()),
                Ion::Struct(vec![
                    (s::NAME, Ion::Symbol(self.symbols.id(&name))),
                    (
                        s::CONTENT_LIST,
                        Ion::List(strings.iter().cloned().map(Ion::Str).collect()),
                    ),
                ]),
            ));
        }
        for (name, style) in &self.styles {
            let mut value = vec![(s::STYLE_NAME, Ion::Symbol(self.symbols.id(name)))];
            value.extend(style.iter().cloned());
            fragments.push(Fragment::new(
                s::STYLE,
                Some(name.clone()),
                Ion::Struct(value),
            ));
        }
        for layout in &self.layouts {
            let id = format!("{}-ad", layout.section_name);
            fragments.push(Fragment::new(
                s::AUXILIARY_DATA,
                Some(id.clone()),
                Ion::Struct(vec![
                    (s::KFX_ID, Ion::Symbol(self.symbols.id(&id))),
                    (
                        s::METADATA,
                        Ion::List(vec![metadata_pair("IS_TARGET_SECTION", Ion::Bool(true))]),
                    ),
                ]),
            ));
        }
        let mut image_paths: Vec<&String> = self.image_names.keys().collect();
        image_paths.sort();
        for path in image_paths {
            let name = &self.image_names[path];
            let image = &self.images[path];
            let resource = format!("resource/{name}");
            fragments.push(Fragment::new(
                164,
                Some(name.clone()),
                Ion::Struct(vec![
                    (175, Ion::Symbol(self.symbols.id(name))),
                    (165, Ion::str(resource.clone())),
                    (161, Ion::Symbol(image.format)),
                    (422, Ion::Int(image.width as i64)),
                    (423, Ion::Int(image.height as i64)),
                    (162, Ion::str(image.mime)),
                ]),
            ));
            fragments.push(Fragment {
                kind: 417,
                id: Some(resource),
                value: Ion::Null,
                raw: Some(image.data.clone()),
            });
        }
        let mut emitted_fonts = BTreeSet::new();
        for font in self.book.fonts.clone() {
            let Some(data) = self.book.resources.get(&font.path) else {
                continue;
            };
            let location = format!("font/{}", font.path);
            self.symbols.intern(&location);
            if emitted_fonts.insert(location.clone()) {
                fragments.push(Fragment {
                    kind: 418,
                    id: Some(location.clone()),
                    value: Ion::Null,
                    raw: Some(data.clone()),
                });
            }
            let bold = font.weight == "bold" || font.weight.parse::<i64>().unwrap_or(0) >= 600;
            fragments.push(Fragment::new(
                262,
                None,
                Ion::Struct(vec![
                    (11, Ion::str(font.family.clone())),
                    (
                        12,
                        Ion::Symbol(if font.style == "normal" { 350 } else { 382 }),
                    ),
                    (165, Ion::str(location)),
                    (13, Ion::Symbol(if bold { 361 } else { 350 })),
                    (15, Ion::Symbol(350)),
                ]),
            ));
        }
        fragments.push(Fragment::new(s::POSITION_MAP, None, self.position_map()));
        fragments.push(Fragment::new(
            s::POSITION_ID_MAP,
            None,
            self.position_id_map(),
        ));
        fragments.push(Fragment::new(s::LOCATION_MAP, None, self.location_map()));
        fragments.push(Fragment::new(
            s::RESOURCE_PATH,
            None,
            Ion::Struct(vec![(s::ENTRIES, Ion::List(Vec::new()))]),
        ));
        let entity_map = self.entity_map(container_id, &fragments);
        fragments.push(Fragment::new(s::CONTAINER_ENTITY_MAP, None, entity_map));
        fragments
    }

    fn content_features(&self) -> Ion {
        fn feature(namespace: &str, key: &str, major: i64) -> Ion {
            Ion::Struct(vec![
                (s::NAMESPACE, Ion::str(namespace)),
                (s::KEY, Ion::str(key)),
                (
                    s::VERSION_INFO,
                    Ion::Struct(vec![(
                        5,
                        Ion::Struct(vec![
                            (s::MAJOR_VERSION, Ion::Int(major)),
                            (s::MINOR_VERSION, Ion::Int(0)),
                        ]),
                    )]),
                ),
            ])
        }
        let mut features = vec![
            feature("com.amazon.yjconversion", "reflow-style", 6),
            feature("SDK.Marker", "CanonicalFormat", 1),
        ];
        if self
            .images
            .values()
            .any(|i| i.width > 1920 || i.height > 1920)
        {
            features.push(feature("com.amazon.yjconversion", "yj_hdv", 1));
        }
        if self.images.values().any(|i| {
            i.format == 285
                && i.data
                    .windows(2)
                    .any(|w| w[0] == 255 && (208..=215).contains(&w[1]))
        }) {
            features.push(feature(
                "com.amazon.yjconversion",
                "yj_jpg_rst_marker_present",
                1,
            ));
        }
        let max_positions = self
            .book
            .chapters
            .iter()
            .map(|c| {
                c.blocks
                    .iter()
                    .fold(1usize, |acc, b| acc + chars(&b.text).max(1))
            })
            .max()
            .unwrap_or(0);
        if max_positions > 65536 {
            let size = ((max_positions - 65536) / 16384 + 2).min(256);
            features.push(feature(
                "com.amazon.yjconversion",
                "reflow-section-size",
                size as i64,
            ));
        }
        Ion::Struct(vec![(s::FEATURES, Ion::List(features))])
    }

    fn book_metadata(&self, container_id: &str) -> Ion {
        let meta = &self.book.metadata;
        let seed = if meta.identifier.is_empty() {
            format!("{}\n{}", meta.title, meta.authors.join("&"))
        } else {
            meta.identifier.clone()
        };
        let content_id = sha1_hex(seed.as_bytes())[..32].to_uppercase();
        let mut book_id_bytes = vec![5u8];
        book_id_bytes.extend_from_slice(&sha1(meta.identifier.as_bytes())[..16]);
        let book_id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(book_id_bytes);
        let author = meta
            .authors
            .first()
            .cloned()
            .unwrap_or_else(|| "Unknown".into());
        let ebook = category(
            "kindle_ebook_metadata",
            vec![
                metadata_pair("selection", Ion::str("enabled")),
                metadata_pair("nested_span", Ion::str("enabled")),
            ],
        );
        let mut title_fields = vec![
            metadata_pair("title", Ion::str(meta.title.clone())),
            metadata_pair("language", Ion::str(meta.language.clone())),
            metadata_pair("author", Ion::str(author)),
            metadata_pair("asset_id", Ion::str(container_id)),
            metadata_pair("book_id", Ion::str(book_id)),
            metadata_pair("ASIN", Ion::str(content_id.clone())),
            metadata_pair("content_id", Ion::str(content_id)),
            metadata_pair("cde_content_type", Ion::str("PDOC")),
            metadata_pair("is_sample", Ion::Bool(false)),
            metadata_pair("override_kindle_font", Ion::Bool(false)),
        ];
        if let Some(publisher) = meta.publisher.as_ref().filter(|p| !p.is_empty()) {
            title_fields.push(metadata_pair("publisher", Ion::str(publisher.clone())));
        }
        if let Some(modified) = &meta.modified_date {
            title_fields.push(metadata_pair("modified_date", Ion::str(modified.clone())));
        }
        if let Some(name) = meta
            .cover_image
            .as_ref()
            .and_then(|c| self.image_names.get(c))
        {
            title_fields.push(metadata_pair("cover_image", Ion::str(name.clone())));
        }
        let audit = category(
            "kindle_audit_metadata",
            vec![
                metadata_pair("file_creator", Ion::str(self.creator.clone())),
                metadata_pair("creator_version", Ion::str(self.version.clone())),
            ],
        );
        Ion::Struct(vec![(
            s::CATEGORISED_METADATA,
            Ion::List(vec![
                ebook,
                category("kindle_title_metadata", title_fields),
                audit,
            ]),
        )])
    }

    fn reading_order(&self) -> Ion {
        Ion::Struct(vec![
            (s::READING_ORDER_NAME, Ion::Symbol(s::DEFAULT)),
            (
                s::SECTIONS,
                Ion::List(
                    self.layouts
                        .iter()
                        .map(|l| Ion::Symbol(self.symbols.id(&l.section_name)))
                        .collect(),
                ),
            ),
        ])
    }

    fn document_data(&self) -> Ion {
        Ion::Struct(vec![
            (s::DIRECTION, Ion::Symbol(s::LTR)),
            (112, Ion::Symbol(s::AUTO)),
            (s::FONT_SIZE, em_dimension(1.0)),
            (s::WRITING_MODE, Ion::Symbol(s::HORIZONTAL_TB)),
            (s::SELECTION, Ion::Symbol(s::ENABLED)),
            (8, Ion::Int((self.next_id - 1).max(0))),
            (s::LINE_HEIGHT, em_dimension(1.2)),
            (s::SPACING_PERCENT_BASE, Ion::Symbol(s::WIDTH)),
            (s::READING_ORDERS, Ion::List(vec![self.reading_order()])),
        ])
    }

    fn section(&self, index: usize) -> Ion {
        let layout = &self.layouts[index];
        let chapter = &self.book.chapters[index];
        let mut template = vec![
            (s::ID, Ion::Int(layout.page_template_id)),
            (
                s::STORY_NAME,
                Ion::Symbol(self.symbols.id(&layout.story_name)),
            ),
            (s::TYPE, Ion::Symbol(s::TEXT)),
        ];
        if chapter.is_image_only()
            && let Some(image) = chapter
                .blocks
                .iter()
                .find_map(|b| b.image_path.as_ref())
                .and_then(|p| self.images.get(p))
        {
            template[2] = (s::TYPE, Ion::Symbol(270));
            template.extend([
                (66, Ion::Int(image.width as i64)),
                (67, Ion::Int(image.height as i64)),
                (156, Ion::Symbol(326)),
                (140, Ion::Symbol(320)),
            ]);
        }
        Ion::Struct(vec![
            (
                s::SECTION_NAME,
                Ion::Symbol(self.symbols.id(&layout.section_name)),
            ),
            (s::PAGE_TEMPLATES, Ion::List(vec![Ion::Struct(template)])),
        ])
    }

    fn register_style(&mut self, fields: Vec<(u64, Ion)>) -> u64 {
        let key = ion_data(&Ion::Struct(fields.clone()), None);
        if let Some(name) = self.style_names.get(&key) {
            return self.symbols.id(name);
        }
        let name = if fields.is_empty() {
            "s0".to_string()
        } else {
            format!("s{}", self.styles.len() + 1)
        };
        self.style_names.insert(key, name.clone());
        self.styles.push((name.clone(), fields));
        self.symbols.intern(&name)
    }

    fn storyline(&mut self, chapter_index: usize) -> Ion {
        let chapter = self.book.chapters[chapter_index].clone();
        let mut children: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
        for (index, block) in chapter.blocks.iter().enumerate() {
            children.entry(block.parent_index).or_default().push(index);
        }
        let roots = children.get(&None).cloned().unwrap_or_default();
        let content = roots
            .into_iter()
            .map(|index| self.element(chapter_index, &chapter, &children, index))
            .collect();
        Ion::Struct(vec![
            (
                s::STORY_NAME,
                Ion::Symbol(self.symbols.id(&self.layouts[chapter_index].story_name)),
            ),
            (s::CONTENT_LIST, Ion::List(content)),
        ])
    }

    fn element(
        &mut self,
        chapter_index: usize,
        chapter: &Chapter,
        children: &HashMap<Option<usize>, Vec<usize>>,
        block_index: usize,
    ) -> Ion {
        let block = &chapter.blocks[block_index];
        let layout = &self.layouts[chapter_index];
        let id = layout.block_ids[block_index];
        let (content_name, content_index) = layout.content_references[block_index].clone();
        let inner_id = layout.inner_ids[block_index];
        let fonts = self.book.fonts.clone();
        let metrics = self.metrics;
        if let Some(name) = block
            .image_path
            .as_ref()
            .and_then(|p| self.image_names.get(p))
            .cloned()
        {
            let image_fields: Vec<(u64, Ion)> = if chapter.is_image_only() {
                Vec::new()
            } else {
                style_fields(&block.style, &fonts, false, &metrics)
                    .into_iter()
                    .filter(|(k, _)| {
                        [47, 48, 49, 50, 52, 53, 54, 55, 56, 57, 62, 63, 64, 65, 580].contains(k)
                    })
                    .collect()
            };
            let style = self.register_style(image_fields);
            let mut fields = vec![
                (s::ID, Ion::Int(id)),
                (s::STYLE, Ion::Symbol(style)),
                (s::TYPE, Ion::Symbol(271)),
                (175, Ion::Symbol(self.symbols.id(&name))),
            ];
            if let Some(anchor) = self.link_symbol(block.link.as_ref()) {
                fields.push((179, Ion::Symbol(anchor)));
            }
            return Ion::Struct(fields);
        }
        let list = matches!(
            block.kind,
            BlockKind::OrderedList | BlockKind::UnorderedList
        );
        let structural = list
            || matches!(block.kind, BlockKind::Container | BlockKind::HorizontalRule)
            || block.text.is_empty();
        if structural {
            let mut fields = style_fields(&block.style, &fonts, false, &metrics);
            fields.retain(|(k, _)| ![21, 47, 48, 49, 50, 52, 53, 54, 55].contains(k));
            fields.extend(box_fields(&block.style, &metrics));
            if list {
                fields.retain(|(k, _)| *k != 48 && *k != 53); // Native list gutter.
            }
            let kind: u64 = if list {
                276
            } else if block.kind == BlockKind::HorizontalRule {
                596
            } else if block.kind == BlockKind::ListItem {
                277
            } else {
                270
            };
            let style = self.register_style(fields);
            let mut node = vec![
                (155, Ion::Int(id)),
                (157, Ion::Symbol(style)),
                (159, Ion::Symbol(kind)),
            ];
            if list {
                node.push((551, Ion::Symbol(553)));
                if block.kind == BlockKind::OrderedList {
                    node.push((100, Ion::Symbol(343)));
                }
                if let Some(start) = block.list_start {
                    node.push((104, Ion::Int(start)));
                }
            }
            if kind == 270 {
                node.push((156, Ion::Symbol(323)));
            }
            if block.kind != BlockKind::HorizontalRule {
                let nested = children
                    .get(&Some(block_index))
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|child| self.element(chapter_index, chapter, children, child))
                    .collect();
                node.push((146, Ion::List(nested)));
            }
            return Ion::Struct(node);
        }
        let kind = if block.kind == BlockKind::ListItem {
            s::LIST_ITEM
        } else {
            s::TEXT
        };
        let mut text_fields = style_fields(&block.style, &fonts, false, &metrics);
        if block.kind == BlockKind::Preformatted {
            text_fields.retain(|(k, _)| *k != 34);
            text_fields.push((34, Ion::Symbol(59))); // Never justify literal code.
        }
        // Headings need treat_as_title in the style and a level on the node.
        if matches!(block.kind, BlockKind::Heading(_)) {
            text_fields.push((761, Ion::List(vec![Ion::Symbol(760)])));
        }
        if inner_id.is_some() {
            text_fields.retain(|(k, _)| ![21, 47, 48, 49, 50, 52, 53, 54, 55].contains(k));
            // Explicit zeros defeat reader defaults; the outer frame owns spacing.
            for key in [47, 48, 49, 50] {
                text_fields.push((key, dimension(0.0, 308)));
            }
        }
        let text_style = self.register_style(text_fields);
        let mut fields = vec![
            (s::ID, Ion::Int(inner_id.unwrap_or(id))),
            (s::STYLE, Ion::Symbol(text_style)),
            (s::TYPE, Ion::Symbol(kind)),
            (
                s::CONTENT,
                Ion::Struct(vec![
                    (s::NAME, Ion::Symbol(self.symbols.id(&content_name))),
                    (s::INDEX, Ion::Int(content_index as i64)),
                ]),
            ),
        ];
        if let BlockKind::Heading(level) = block.kind {
            fields.push((790, Ion::Int(level as i64)));
        }
        let base_inline = ion_data(
            &Ion::Struct(style_fields(&block.style, &fonts, true, &metrics)),
            None,
        );
        let mut runs: Vec<Run> = Vec::new();
        let scalar_count = chars(&block.text);
        for span in &block.spans {
            if span.offset > scalar_count
                || span.length == 0
                || span.length > scalar_count - span.offset
            {
                continue;
            }
            let resolved = style_fields(&span.style, &fonts, true, &metrics);
            let anchor = self.link_symbol(span.link.as_ref());
            if ion_data(&Ion::Struct(resolved.clone()), None) == base_inline && anchor.is_none() {
                continue;
            }
            let style = self.register_style(resolved);
            match runs.last_mut() {
                Some(last)
                    if last.0 + last.1 == span.offset && last.2 == style && last.3 == anchor =>
                {
                    last.1 += span.length;
                }
                _ => runs.push((span.offset, span.length, style, anchor)),
            }
        }
        let events: Vec<Ion> = runs
            .into_iter()
            .map(|(offset, length, style, link)| {
                let mut f = vec![
                    (143, Ion::Int(offset as i64)),
                    (144, Ion::Int(length as i64)),
                    (157, Ion::Symbol(style)),
                ];
                if let Some(link) = link {
                    f.push((179, Ion::Symbol(link)));
                }
                Ion::Struct(f)
            })
            .collect();
        if !events.is_empty() {
            fields.push((142, Ion::List(events)));
        }
        if inner_id.is_some() {
            let frame = self.register_style(box_fields(&block.style, &metrics));
            return Ion::Struct(vec![
                (155, Ion::Int(id)),
                (157, Ion::Symbol(frame)),
                (159, Ion::Symbol(270)),
                (156, Ion::Symbol(323)),
                (146, Ion::List(vec![Ion::Struct(fields)])),
            ]);
        }
        Ion::Struct(fields)
    }

    fn resolve_position(&self, path: &str, fragment: Option<&str>) -> Option<(i64, usize)> {
        let index = self
            .book
            .chapters
            .iter()
            .position(|c| c.source_path == path)?;
        let chapter = &self.book.chapters[index];
        let layout = &self.layouts[index];
        let Some(fragment) = fragment.filter(|f| !f.is_empty()) else {
            return Some((
                layout
                    .block_ids
                    .first()
                    .copied()
                    .unwrap_or(layout.page_template_id),
                0,
            ));
        };
        for (block_index, block) in chapter.blocks.iter().enumerate() {
            if block.anchor_id.as_deref() == Some(fragment) {
                return Some((layout.block_ids[block_index], 0));
            }
            if let Some(anchor) = block.anchors.iter().find(|a| a.id == fragment) {
                let offset = anchor.offset.min(chars(&block.text));
                return Some((
                    layout.inner_ids[block_index].unwrap_or(layout.block_ids[block_index]),
                    offset,
                ));
            }
        }
        None
    }

    fn link_symbol(&mut self, target: Option<&LinkTarget>) -> Option<u64> {
        let target = target?;
        if let Some(name) = self.anchor_names.get(target) {
            return Some(self.symbols.id(name));
        }
        let mut fields = match target {
            LinkTarget::External(uri) => vec![(186, Ion::str(uri.clone()))],
            LinkTarget::Location { path, fragment } => {
                // Broken destinations stay plain text, never a jump elsewhere.
                let (id, offset) = self.resolve_position(path, fragment.as_deref())?;
                let mut location = vec![(155, Ion::Int(id))];
                if offset > 0 {
                    location.push((143, Ion::Int(offset as i64)));
                }
                vec![(183, Ion::Struct(location))]
            }
        };
        let name = format!("link_{}", self.anchor_names.len() + 1);
        let symbol = self.symbols.intern(&name);
        self.anchor_names.insert(target.clone(), name.clone());
        fields.push((180, Ion::Symbol(symbol)));
        self.anchor_fragments
            .push(Fragment::new(266, Some(name), Ion::Struct(fields)));
        Some(symbol)
    }

    fn nav_unit(fields: Vec<(u64, Ion)>) -> Ion {
        Ion::Annotated(vec![s::NAV_UNIT], Box::new(Ion::Struct(fields)))
    }

    fn target_position(id: i64, offset: i64) -> (u64, Ion) {
        (
            s::TARGET_POSITION,
            Ion::Struct(vec![(s::ID, Ion::Int(id)), (s::OFFSET, Ion::Int(offset))]),
        )
    }

    fn label(text: &str) -> (u64, Ion) {
        (
            s::REPRESENTATION,
            Ion::Struct(vec![(s::LABEL, Ion::str(text))]),
        )
    }

    fn nav_container(fields: Vec<(u64, Ion)>) -> Ion {
        Ion::Annotated(vec![s::NAV_CONTAINER], Box::new(Ion::Struct(fields)))
    }

    fn navigation(&self) -> Ion {
        fn entry(builder: &Builder, item: &NavigationEntry) -> Vec<Ion> {
            let nested: Vec<Ion> = item
                .children
                .iter()
                .flat_map(|c| entry(builder, c))
                .collect();
            let Some((id, offset)) = builder.resolve_position(&item.path, item.fragment.as_deref())
            else {
                return nested;
            };
            let mut fields = vec![
                Builder::label(&item.title),
                Builder::target_position(id, offset as i64),
            ];
            if !nested.is_empty() {
                fields.push((s::ENTRIES, Ion::List(nested)));
            }
            vec![Builder::nav_unit(fields)]
        }
        let items: Vec<NavigationEntry> = if self.book.navigation.is_empty() {
            self.book
                .chapters
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.is_image_only())
                .map(|(i, c)| NavigationEntry {
                    title: c
                        .title
                        .clone()
                        .unwrap_or_else(|| format!("Chapter {}", i + 1)),
                    path: c.source_path.clone(),
                    fragment: None,
                    children: Vec::new(),
                })
                .collect()
        } else {
            self.book.navigation.clone()
        };
        let toc_entries: Vec<Ion> = items.iter().flat_map(|i| entry(self, i)).collect();
        let mut containers = vec![Self::nav_container(vec![
            (s::NAV_TYPE, Ion::Symbol(s::TOC)),
            (s::NAV_CONTAINER_NAME, Ion::Symbol(self.symbols.id("toc"))),
            (s::ENTRIES, Ion::List(toc_entries)),
        ])];
        let mut landmarks = Vec::new();
        let landmark = |kind: u64, target: i64, label: &str| {
            Self::nav_unit(vec![
                (s::LANDMARK_TYPE, Ion::Symbol(kind)),
                Self::label(label),
                Self::target_position(target, 0),
            ])
        };
        if let Some(cover) = &self.book.metadata.cover_image
            && let Some(index) = self.book.chapters.iter().position(|c| {
                c.blocks
                    .iter()
                    .any(|b| b.image_path.as_ref() == Some(cover))
            })
        {
            landmarks.push(landmark(
                233,
                self.layouts[index].page_template_id,
                "cover-nav-unit",
            ));
        }
        let reading_index = self
            .book
            .chapters
            .iter()
            .position(|c| c.blocks.iter().any(|b| !b.text.is_empty()))
            .unwrap_or(0);
        if let Some(first) = self.layouts.get(reading_index) {
            landmarks.push(landmark(
                s::SRL,
                first
                    .block_ids
                    .first()
                    .copied()
                    .unwrap_or(first.page_template_id),
                &self.book.metadata.title,
            ));
        }
        if !landmarks.is_empty() {
            containers.push(Self::nav_container(vec![
                (s::NAV_TYPE, Ion::Symbol(s::LANDMARKS)),
                (
                    s::NAV_CONTAINER_NAME,
                    Ion::Symbol(self.symbols.id("landmarks")),
                ),
                (s::ENTRIES, Ion::List(landmarks)),
            ]));
        }
        let mut headings = Vec::new();
        for level in 2u8..=6 {
            let mut entries = Vec::new();
            let mut first_target = None;
            for (index, chapter) in self.book.chapters.iter().enumerate() {
                for (block_index, block) in chapter.blocks.iter().enumerate() {
                    if block.kind == BlockKind::Heading(level) {
                        let target = self.layouts[index].block_ids[block_index];
                        first_target.get_or_insert(target);
                        entries.push(Self::nav_unit(vec![
                            Self::label("heading-nav-unit"),
                            Self::target_position(target, 0),
                        ]));
                    }
                }
            }
            if let Some(first) = first_target {
                headings.push(Self::nav_unit(vec![
                    (s::LANDMARK_TYPE, Ion::Symbol(798 + level as u64)),
                    Self::label("heading-nav-unit"),
                    Self::target_position(first, 0),
                    (s::ENTRIES, Ion::List(entries)),
                ]));
            }
        }
        containers.insert(
            0,
            Self::nav_container(vec![
                (s::NAV_TYPE, Ion::Symbol(798)),
                (
                    s::NAV_CONTAINER_NAME,
                    Ion::Symbol(self.symbols.id("headings")),
                ),
                (s::ENTRIES, Ion::List(headings)),
            ]),
        );
        let mut pages = Vec::new();
        let (mut position, mut next_page) = (0i64, 0i64);
        for (id, length) in self.chunks() {
            while next_page < position + length {
                pages.push(Self::nav_unit(vec![
                    Self::label(&(pages.len() + 1).to_string()),
                    Self::target_position(id, next_page - position),
                ]));
                next_page += 1850;
            }
            position += length;
        }
        containers.push(Self::nav_container(vec![
            (s::NAV_TYPE, Ion::Symbol(237)),
            (
                s::NAV_CONTAINER_NAME,
                Ion::Symbol(self.symbols.id("page_list")),
            ),
            (s::ENTRIES, Ion::List(pages)),
        ]));
        Ion::List(vec![Ion::Struct(vec![
            (s::READING_ORDER_NAME, Ion::Symbol(s::DEFAULT)),
            (s::NAV_CONTAINERS, Ion::List(containers)),
        ])])
    }

    fn chunks(&self) -> Vec<(i64, i64)> {
        let mut values = Vec::new();
        for (chapter_index, layout) in self.layouts.iter().enumerate() {
            values.push((layout.page_template_id, 1));
            for (index, block) in self.book.chapters[chapter_index].blocks.iter().enumerate() {
                let id = layout.block_ids[index];
                let length = chars(&block.text).max(1) as i64;
                match layout.inner_ids[index] {
                    Some(inner) => {
                        values.push((id, 1));
                        values.push((inner, length));
                    }
                    None => values.push((id, length)),
                }
            }
        }
        values
    }

    fn position_map(&self) -> Ion {
        Ion::List(
            self.layouts
                .iter()
                .map(|layout| {
                    let mut ids = vec![layout.page_template_id];
                    for (index, id) in layout.block_ids.iter().enumerate() {
                        ids.push(*id);
                        if let Some(inner) = layout.inner_ids[index] {
                            ids.push(inner);
                        }
                    }
                    Ion::Struct(vec![
                        (
                            s::CONTAINS,
                            Ion::List(ids.into_iter().map(Ion::Int).collect()),
                        ),
                        (
                            s::SECTION_NAME,
                            Ion::Symbol(self.symbols.id(&layout.section_name)),
                        ),
                    ])
                })
                .collect(),
        )
    }

    fn position_id_map(&self) -> Ion {
        let mut pid = 0;
        let mut entries = Vec::new();
        for (id, length) in self.chunks() {
            entries.push(Ion::Struct(vec![
                (s::EID, Ion::Int(id)),
                (s::PID, Ion::Int(pid)),
            ]));
            pid += length;
        }
        entries.push(Ion::Struct(vec![
            (s::EID, Ion::Int(0)),
            (s::PID, Ion::Int(pid)),
        ]));
        Ion::List(entries)
    }

    fn location_map(&self) -> Ion {
        let mut entries = Vec::new();
        for (id, length) in self.chunks() {
            let mut offset = 0;
            loop {
                entries.push(Ion::Struct(vec![
                    (s::ID, Ion::Int(id)),
                    (s::OFFSET, Ion::Int(offset)),
                ]));
                offset += 150;
                if offset >= length {
                    break;
                }
            }
        }
        Ion::List(vec![Ion::Struct(vec![(s::LOCATIONS, Ion::List(entries))])])
    }

    fn entity_map(&self, container_id: &str, fragments: &[Fragment]) -> Ion {
        let names = fragments
            .iter()
            .filter_map(|f| f.id.as_ref())
            .map(|id| Ion::Symbol(self.symbols.id(id)))
            .collect();
        let mut fields = vec![(
            s::CONTAINER_LIST,
            Ion::List(vec![Ion::Struct(vec![
                (s::ID, Ion::str(container_id)),
                (s::CONTAINS, Ion::List(names)),
            ])]),
        )];
        let mut dependencies = Vec::new();
        for (index, chapter) in self.book.chapters.iter().enumerate() {
            let refs: BTreeSet<&String> = chapter
                .blocks
                .iter()
                .filter_map(|b| b.image_path.as_ref())
                .filter_map(|p| self.image_names.get(p))
                .collect();
            if !refs.is_empty() {
                dependencies.push(Ion::Struct(vec![
                    (
                        155,
                        Ion::Symbol(self.symbols.id(&self.layouts[index].section_name)),
                    ),
                    (
                        254,
                        Ion::List(
                            refs.into_iter()
                                .map(|r| Ion::Symbol(self.symbols.id(r)))
                                .collect(),
                        ),
                    ),
                ]));
            }
        }
        let mut names: Vec<&String> = self.image_names.values().collect();
        names.sort();
        for name in names {
            dependencies.push(Ion::Struct(vec![
                (155, Ion::Symbol(self.symbols.id(name))),
                (
                    254,
                    Ion::List(vec![Ion::Symbol(
                        self.symbols.id(&format!("resource/{name}")),
                    )]),
                ),
            ]));
        }
        if !dependencies.is_empty() {
            fields.push((253, Ion::List(dependencies)));
        }
        Ion::Struct(fields)
    }

    fn make_container_id(&self) -> String {
        let seed = format!(
            "{}\n{}",
            self.book.metadata.identifier, self.book.metadata.title
        );
        let digest = sha1(seed.as_bytes());
        let mut state = u128::from_be_bytes(digest[..16].try_into().unwrap_or([0; 16]));
        let alphabet = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let mut id = String::from("CR!");
        for _ in 0..28 {
            // The reference exporter's wrapping 128-bit LCG.
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            id.push(alphabet[((state >> 56) as u64 % 36) as usize] as char);
        }
        id
    }

    fn symbol_table_data(&self) -> Vec<u8> {
        let imported = Ion::Struct(vec![
            (4, Ion::str("YJ_symbols")),
            (5, Ion::Int(10)),
            (8, Ion::Int(851)),
        ]);
        ion_data(
            &Ion::Struct(vec![
                (6, Ion::List(vec![imported])),
                (
                    7,
                    Ion::List(self.symbols.values.iter().cloned().map(Ion::Str).collect()),
                ),
            ]),
            Some(3),
        )
    }

    fn format_capabilities_data(&self) -> Vec<u8> {
        let has_text = self.content_chunks.iter().any(|c| !c.is_empty());
        let value = if has_text {
            vec![Ion::Struct(vec![
                (s::KEY, Ion::str("kfxgen.textBlock")),
                (5, Ion::Int(1)),
            ])]
        } else {
            Vec::new()
        };
        ion_data(&Ion::List(value), Some(s::FORMAT_CAPABILITIES))
    }

    fn container(&self, container_id: &str, entities: &[(u32, u32, Vec<u8>)]) -> Vec<u8> {
        const FIXED_HEADER: usize = 18;
        let mut table = Vec::new();
        let mut payload = Vec::new();
        for (id, kind, data) in entities {
            le32(&mut table, *id);
            le32(&mut table, *kind);
            le64(&mut table, payload.len() as u64);
            le64(&mut table, data.len() as u64);
            payload.extend_from_slice(data);
        }
        let symbols = self.symbol_table_data();
        let capabilities = self.format_capabilities_data();
        let symbols_offset = FIXED_HEADER + table.len();
        let capabilities_offset = symbols_offset + symbols.len();
        let info_offset = capabilities_offset + capabilities.len();
        let info = ion_data(
            &Ion::Struct(vec![
                (s::BC_CONT_ID, Ion::str(container_id)),
                (s::BC_COMPR_TYPE, Ion::Int(0)),
                (s::BC_DRM_SCHEME, Ion::Int(0)),
                (s::BC_CHUNK_SIZE, Ion::Int(4096)),
                (s::BC_INDEX_TAB_OFFSET, Ion::Int(FIXED_HEADER as i64)),
                (s::BC_INDEX_TAB_LENGTH, Ion::Int(table.len() as i64)),
                (s::BC_DOC_SYMBOL_OFFSET, Ion::Int(symbols_offset as i64)),
                (s::BC_DOC_SYMBOL_LENGTH, Ion::Int(symbols.len() as i64)),
                (
                    s::BC_F_CAPABILITIES_OFFSET,
                    Ion::Int(capabilities_offset as i64),
                ),
                (
                    s::BC_F_CAPABILITIES_LENGTH,
                    Ion::Int(capabilities.len() as i64),
                ),
            ]),
            None,
        );
        let trailer = format!(
            "[{{key:\"kfxgen_package_version\",value:\"{creator}-{version}\"}},{{key:\"kfxgen_application_version\",value:\"{creator}\"}},{{key:\"kfxgen_payload_sha1\",value:\"{sha}\"}},{{key:\"kfxgen_acr\",value:\"{container_id}\"}}]",
            creator = self.creator,
            version = self.version,
            sha = sha1_hex(&payload),
        );
        let header_length = info_offset + info.len() + trailer.len();
        let mut out = b"CONT".to_vec();
        le16(&mut out, 2);
        le32(&mut out, header_length as u32);
        le32(&mut out, info_offset as u32);
        le32(&mut out, info.len() as u32);
        out.extend_from_slice(&table);
        out.extend_from_slice(&symbols);
        out.extend_from_slice(&capabilities);
        out.extend_from_slice(&info);
        out.extend_from_slice(trailer.as_bytes());
        out.extend_from_slice(&payload);
        out
    }
}

fn em_dimension(value: f64) -> Ion {
    Ion::Struct(vec![
        (s::VALUE, Ion::Decimal(value)),
        (s::UNIT, Ion::Symbol(s::EM)),
    ])
}

fn category(name: &str, fields: Vec<Ion>) -> Ion {
    Ion::Struct(vec![
        (s::CATEGORY, Ion::str(name)),
        (s::METADATA, Ion::List(fields)),
    ])
}

fn metadata_pair(key: &str, value: Ion) -> Ion {
    Ion::Struct(vec![(s::KEY, Ion::str(key)), (s::VALUE, value)])
}

fn entity_data(value: &Ion, raw: Option<&[u8]>) -> Vec<u8> {
    let header = ion_data(
        &Ion::Struct(vec![
            (s::BC_COMPR_TYPE, Ion::Int(0)),
            (s::BC_DRM_SCHEME, Ion::Int(0)),
        ]),
        None,
    );
    let content = match raw {
        Some(raw) => raw.to_vec(),
        None => ion_data(value, None),
    };
    let mut out = b"ENTY".to_vec();
    le16(&mut out, 1);
    le32(&mut out, (10 + header.len()) as u32);
    out.extend_from_slice(&header);
    out.extend_from_slice(&content);
    out
}
