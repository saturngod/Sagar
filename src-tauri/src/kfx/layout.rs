//! Conversion-time layout normalization: CSS margin collapse (Kindle does not
//! collapse margins) and reader-relative font/leading scaling.

use std::collections::HashSet;

use super::dom::{Child, Dom, NodeId};
use super::model::{Book, TextStyle};
use super::util::{css_number, format_number};

const FLOW_BLOCKS: [&str; 22] = [
    "body",
    "div",
    "section",
    "article",
    "nav",
    "figure",
    "figcaption",
    "blockquote",
    "p",
    "pre",
    "ul",
    "ol",
    "li",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "img",
    "table",
];

pub fn absolute(value: Option<&str>, size: f64) -> Option<f64> {
    let value = value?;
    let n = css_number(value)?;
    if value.ends_with("rem") {
        Some(n)
    } else if value.ends_with("em") {
        Some(n * size)
    } else if value.ends_with("px") {
        Some(n / 16.0)
    } else if value.ends_with("pt") {
        Some(n / 12.0)
    } else if value.ends_with('%') {
        Some(n * 0.32)
    } else if n == 0.0 {
        Some(0.0)
    } else {
        None
    }
}

struct Collapse<'a> {
    dom: &'a mut Dom,
    done: HashSet<(NodeId, &'static str)>,
}

fn is_block(name: &str) -> bool {
    FLOW_BLOCKS.contains(&name)
}

impl Collapse<'_> {
    fn visible(&self, id: NodeId) -> bool {
        let node = self.dom.node(id);
        node.style.get("display") != Some("none")
            && node.style.get("visibility") != Some("hidden")
            && !matches!(node.name.as_str(), "script" | "style" | "head")
    }

    fn has_content(&self, child: &Child) -> bool {
        match child {
            Child::Text(text) => !text.trim().is_empty(),
            Child::Element(id) => {
                if !self.visible(*id) {
                    return false;
                }
                let node = self.dom.node(*id);
                matches!(node.name.as_str(), "br" | "img" | "image" | "hr" | "table")
                    || node.children.iter().any(|c| self.has_content(c))
            }
        }
    }

    fn is_empty_block(&self, id: NodeId) -> bool {
        let node = self.dom.node(id);
        matches!(
            node.name.as_str(),
            "p" | "div"
                | "section"
                | "article"
                | "blockquote"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
        ) && !node.children.iter().any(|c| self.has_content(c))
    }

    fn is_blocking_inline(&self, child: &Child) -> bool {
        if let Child::Element(id) = child
            && is_block(&self.dom.node(*id).name)
        {
            return false;
        }
        self.has_content(child)
    }

    fn flow(&self, id: NodeId) -> Vec<NodeId> {
        self.dom
            .elements(id)
            .into_iter()
            .filter(|e| {
                is_block(&self.dom.node(*e).name) && self.visible(*e) && !self.is_empty_block(*e)
            })
            .collect()
    }

    fn amount(&self, id: NodeId, side: &str) -> f64 {
        let style = &self.dom.node(id).style;
        let size = css_number(style.get("font-size").unwrap_or("1")).unwrap_or(1.0);
        absolute(style.get(&format!("margin-{side}")), size).unwrap_or(0.0)
    }

    fn chain(&self, id: NodeId, side: &'static str) -> Vec<NodeId> {
        let node = self.dom.node(id);
        if !matches!(
            node.name.as_str(),
            "body"
                | "div"
                | "section"
                | "article"
                | "nav"
                | "figure"
                | "blockquote"
                | "ul"
                | "ol"
                | "li"
        ) {
            return vec![id];
        }
        let p = &node.style;
        let padding = css_number(p.get(&format!("padding-{side}")).unwrap_or("0")).unwrap_or(0.0);
        let border: Vec<&str> = p
            .get(&format!("border-{side}"))
            .or_else(|| p.get("border"))
            .unwrap_or("")
            .split_whitespace()
            .collect();
        let line_styles = ["solid", "double", "dashed", "dotted", "none", "hidden"];
        let border_style = p
            .get(&format!("border-{side}-style"))
            .or_else(|| p.get("border-style"))
            .or_else(|| border.iter().find(|b| line_styles.contains(b)).copied())
            .unwrap_or("none");
        let border_width = p
            .get(&format!("border-{side}-width"))
            .or_else(|| p.get("border-width"))
            .or_else(|| border.iter().find(|b| css_number(b).is_some()).copied());
        let has_border = !matches!(border_style, "none" | "hidden")
            && border_width.and_then(css_number) != Some(0.0);
        if padding != 0.0 || has_border {
            return vec![id];
        }
        let children = self.flow(id);
        let child = if side == "top" {
            children.first()
        } else {
            children.last()
        };
        let Some(&child) = child else {
            return vec![id];
        };
        let mut ordered = node.children.clone();
        if side != "top" {
            ordered.reverse();
        }
        for item in &ordered {
            if let Child::Element(e) = item
                && *e == child
            {
                break;
            }
            if self.is_blocking_inline(item) {
                return vec![id];
            }
        }
        let mut out = vec![id];
        out.extend(self.chain(child, side));
        out
    }

    fn collapse(&mut self, set: &[(NodeId, &'static str)], carrier: (NodeId, &'static str)) {
        if set.iter().any(|entry| self.done.contains(entry)) {
            return;
        }
        let values: Vec<f64> = set.iter().map(|(n, s)| self.amount(*n, s)).collect();
        let max = values.iter().copied().fold(f64::MIN, f64::max).max(0.0);
        let min = values.iter().copied().fold(f64::MAX, f64::min).min(0.0);
        let combined = max + min;
        for &(node, side) in set {
            self.done.insert((node, side));
            let value = if (node, side) == carrier {
                combined
            } else {
                0.0
            };
            if (self.amount(node, side) - value).abs() > 0.0001 {
                self.dom.nodes[node]
                    .style
                    .properties
                    .insert(format!("-kfx-margin-{side}"), format_number(value));
            }
        }
    }

    fn process(&mut self, id: NodeId) {
        if !self.visible(id) || matches!(self.dom.node(id).name.as_str(), "img" | "hr") {
            return;
        }
        let mut pending: Vec<(NodeId, &'static str)> = Vec::new();
        let children = self.dom.node(id).children.clone();
        // Walk source order including inline text: visible content between
        // blocks stops their margins from collapsing.
        for item in &children {
            if self.is_blocking_inline(item) {
                if pending.len() > 1 {
                    let carrier = pending[0];
                    self.collapse(&pending.clone(), carrier);
                }
                pending.clear();
                continue;
            }
            let Child::Element(child) = item else {
                continue;
            };
            let child = *child;
            if !is_block(&self.dom.node(child).name) || !self.visible(child) {
                continue;
            }
            if self.is_empty_block(child) {
                for side in ["top", "bottom"] {
                    self.dom.nodes[child]
                        .style
                        .properties
                        .insert(format!("-kfx-margin-{side}"), "0".into());
                    self.done.insert((child, side));
                }
                continue;
            }
            let leading: Vec<(NodeId, &'static str)> = self
                .chain(child, "top")
                .into_iter()
                .map(|n| (n, "top"))
                .collect();
            if !pending.is_empty() {
                let mut set = pending.clone();
                set.extend(leading.iter().copied());
                self.collapse(&set, (child, "top"));
            } else if leading.len() > 1 {
                self.collapse(&leading, (child, "top"));
            }
            pending = self
                .chain(child, "bottom")
                .into_iter()
                .map(|n| (n, "bottom"))
                .collect();
        }
        if pending.len() > 1 {
            let carrier = pending[0];
            self.collapse(&pending.clone(), carrier);
        }
        for child in self.dom.elements(id) {
            self.process(child);
        }
    }
}

pub fn collapse_margins(dom: &mut Dom, root: NodeId) {
    Collapse {
        dom,
        done: HashSet::new(),
    }
    .process(root);
}

/// Normalizes the text-dominant font size and leading to the reader's
/// defaults, weighting by UTF-8 bytes as the reference exporter does.
#[derive(Debug, Clone, Copy)]
pub struct LayoutMetrics {
    pub font_scale: f64,
    pub line_scale: f64,
}

impl Default for LayoutMetrics {
    fn default() -> Self {
        Self {
            font_scale: 1.0,
            line_scale: 1.0,
        }
    }
}

fn add_weight(weights: &mut Vec<(f64, usize)>, key: f64, weight: usize) {
    match weights.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 += weight,
        None => weights.push((key, weight)),
    }
}

fn dominant(mut weights: Vec<(f64, usize)>) -> Option<f64> {
    weights.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut best: Option<(f64, usize)> = None;
    for entry in weights {
        if best.is_none_or(|b| entry.1 > b.1) {
            best = Some(entry);
        }
    }
    best.map(|b| b.0)
}

impl LayoutMetrics {
    pub fn for_book(book: &Book) -> Self {
        let mut sizes = Vec::new();
        let mut lines = Vec::new();
        for block in book.chapters.iter().flat_map(|c| c.blocks.iter()) {
            if block.text.is_empty() {
                continue;
            }
            let chars: Vec<char> = block.text.chars().collect();
            let mut remaining = block.text.len() as i64;
            let mut record = |style: &TextStyle, weight: usize| {
                add_weight(&mut sizes, Self::size(style), weight);
                add_weight(&mut lines, Self::raw_line(style).unwrap_or(1.2), weight);
            };
            for span in &block.spans {
                if span.offset + span.length <= chars.len() {
                    let bytes: usize = chars[span.offset..span.offset + span.length]
                        .iter()
                        .map(|c| c.len_utf8())
                        .sum();
                    record(&span.style, bytes);
                    remaining -= bytes as i64;
                }
            }
            record(&block.style, remaining.max(0) as usize);
        }
        let mut metrics = Self::default();
        if let Some(size) = dominant(sizes)
            && size > 0.0
            && (size - 1.0).abs() > 0.02
        {
            metrics.font_scale = (1.0 / size).clamp(0.5, 2.0);
        }
        if let Some(line) = dominant(lines)
            && line > 0.0
            && (line - 1.2).abs() > 0.024
        {
            metrics.line_scale = (1.2 / line).clamp(0.5, 2.0);
        }
        metrics
    }

    pub fn size(style: &TextStyle) -> f64 {
        css_number(style.get("font-size").unwrap_or("1")).unwrap_or(1.0)
    }

    pub fn raw_line(style: &TextStyle) -> Option<f64> {
        let value = style.get("line-height")?;
        let n = css_number(value)?;
        Some(if value.ends_with("rem") {
            n / Self::size(style)
        } else if value.ends_with("px") {
            n / 16.0 / Self::size(style)
        } else if value.ends_with("pt") {
            n / 12.0 / Self::size(style)
        } else if value.ends_with('%') {
            n / 100.0
        } else {
            n
        })
    }

    pub fn leading(&self, style: &TextStyle, normalized: bool) -> f64 {
        let Some(em) = Self::raw_line(style) else {
            return 1.0;
        };
        let base = if em / 1.2 <= 1.0 { 0.99 } else { em / 1.2 };
        base * if normalized { self.line_scale } else { 1.0 }
    }
}
