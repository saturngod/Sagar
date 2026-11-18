//! Reflowable book model produced from an EPUB and consumed by the KFX writer.
//! Text offsets and lengths are Unicode scalar (`char`) counts.

use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Default)]
pub struct BookMetadata {
    pub title: String,
    pub authors: Vec<String>,
    pub language: String,
    pub identifier: String,
    pub publisher: Option<String>,
    pub cover_image: Option<String>,
    pub modified_date: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading(u8),
    ListItem,
    Preformatted,
    Image,
    Container,
    UnorderedList,
    OrderedList,
    HorizontalRule,
}

/// Computed CSS properties. Ordered so equal styles compare and hash equally.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TextStyle {
    pub properties: BTreeMap<String, String>,
}

impl TextStyle {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.properties.get(name).map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LinkTarget {
    Location {
        path: String,
        fragment: Option<String>,
    },
    External(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct StyledSpan {
    pub offset: usize,
    pub length: usize,
    pub style: TextStyle,
    pub link: Option<LinkTarget>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextAnchor {
    pub id: String,
    pub offset: usize,
}

#[derive(Debug, Clone)]
pub struct TextBlock {
    pub kind: BlockKind,
    pub text: String,
    pub style: TextStyle,
    pub spans: Vec<StyledSpan>,
    pub image_path: Option<String>,
    /// Index of the enclosing block in the chapter's preorder block array.
    pub parent_index: Option<usize>,
    pub anchor_id: Option<String>,
    pub list_start: Option<i64>,
    pub anchors: Vec<TextAnchor>,
    /// A link on a replaced element, such as an image.
    pub link: Option<LinkTarget>,
}

impl TextBlock {
    pub fn new(kind: BlockKind, style: TextStyle) -> Self {
        Self {
            kind,
            text: String::new(),
            style,
            spans: Vec::new(),
            image_path: None,
            parent_index: None,
            anchor_id: None,
            list_start: None,
            anchors: Vec::new(),
            link: None,
        }
    }

    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }
}

#[derive(Debug, Clone)]
pub struct Chapter {
    pub source_path: String,
    pub title: Option<String>,
    pub blocks: Vec<TextBlock>,
}

impl Chapter {
    pub fn is_image_only(&self) -> bool {
        self.blocks
            .iter()
            .filter(|block| block.kind == BlockKind::Image)
            .count()
            == 1
            && self.blocks.iter().all(|block| block.text.is_empty())
    }
}

#[derive(Debug, Clone)]
pub struct NavigationEntry {
    pub title: String,
    pub path: String,
    pub fragment: Option<String>,
    pub children: Vec<NavigationEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedFont {
    pub family: String,
    pub path: String,
    pub weight: String,
    pub style: String,
}

#[derive(Debug, Clone, Default)]
pub struct Book {
    pub metadata: BookMetadata,
    pub chapters: Vec<Chapter>,
    pub resources: HashMap<String, Vec<u8>>,
    pub fonts: Vec<EmbeddedFont>,
    pub navigation: Vec<NavigationEntry>,
}
