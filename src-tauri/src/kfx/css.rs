//! The reflowable CSS subset mapped to KFX styles. Unsupported selectors are
//! deliberately treated as non-matching.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::dom::{Dom, NodeId};
use super::model::{EmbeddedFont, TextStyle};
use super::util::{css_number, format_number, resolve_epub_path, unquoted};

#[derive(Debug, Clone)]
pub struct Declaration {
    pub name: String,
    pub value: String,
    pub important: bool,
}

#[derive(Debug, Clone)]
struct Rule {
    selector: String,
    declarations: Vec<Declaration>,
    specificity: i64,
    tag: String,
    classes: HashSet<String>,
    id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    rules: Vec<Rule>,
    pub fonts: Vec<EmbeddedFont>,
}

pub const INHERITED: [&str; 12] = [
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "color",
    "line-height",
    "text-align",
    "text-indent",
    "white-space",
    "direction",
    "visibility",
    "text-decoration",
];

pub fn is_inherited(name: &str) -> bool {
    INHERITED.contains(&name)
}

macro_rules! regex {
    ($pattern:expr) => {{
        static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).expect("valid regex"));
        &*RE
    }};
}

impl Stylesheet {
    /// Appends a stylesheet. `load` resolves `@import`ed files; missing
    /// imports are skipped.
    pub fn append(
        &mut self,
        css: &str,
        path: &str,
        load: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
        visited: &mut HashSet<String>,
    ) {
        if !visited.insert(path.to_string()) {
            return;
        }
        let cleaned = regex!(r"/\*[\s\S]*?\*/").replace_all(css, "").into_owned();
        let imports: Vec<String> = regex!(r#"@import\s+(?:url\(\s*)?['"]([^'"]+)['"]\s*\)?[^;]*;"#)
            .captures_iter(&cleaned)
            .map(|c| c[1].to_string())
            .collect();
        for import in imports {
            let imported = resolve_epub_path(&import, path);
            if let Some(bytes) = load(&imported) {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                self.append(&text, &imported, load, visited);
            }
        }
        let source = regex!(r"@import[^;]+;")
            .replace_all(&cleaned, "")
            .into_owned();
        for (selector, body) in blocks(&source) {
            if selector.eq_ignore_ascii_case("@font-face") {
                let values: HashMap<String, String> = declarations(&body)
                    .into_iter()
                    .map(|d| (d.name, d.value))
                    .collect();
                if let (Some(family), Some(src)) = (values.get("font-family"), values.get("src"))
                    && let Some(url) = regex!(r#"url\(\s*['"]?([^'")]+)['"]?\s*\)"#).captures(src)
                {
                    self.fonts.push(EmbeddedFont {
                        family: unquoted(family),
                        path: resolve_epub_path(&url[1], path),
                        weight: values
                            .get("font-weight")
                            .cloned()
                            .unwrap_or_else(|| "normal".into()),
                        style: values
                            .get("font-style")
                            .cloned()
                            .unwrap_or_else(|| "normal".into()),
                    });
                }
            } else if selector.starts_with("@media") {
                // Reflow rules only; never apply dark-mode or print styles.
                if selector == "@media all" || selector == "@media screen" {
                    let mut nested = Stylesheet::default();
                    let mut seen = HashSet::new();
                    nested.append(&body, path, load, &mut seen);
                    self.rules.extend(nested.rules);
                    self.fonts.extend(nested.fonts);
                }
            } else if !selector.starts_with('@') {
                for item in selector.split(',') {
                    let selector = item.trim();
                    // Highlight.js themes create per-token background patches.
                    if regex!(r"\.([\w-]+)")
                        .captures_iter(selector)
                        .any(|c| c[1].starts_with("hljs"))
                    {
                        continue;
                    }
                    if selector.contains('(')
                        || selector.contains("::")
                        || selector.contains('+')
                        || selector.contains('~')
                    {
                        continue;
                    }
                    let ids = selector.matches('#').count() as i64;
                    let classes = selector
                        .chars()
                        .filter(|c| matches!(c, '.' | '[' | ':'))
                        .count() as i64;
                    let tags = regex!(r"(?:^|[\s>+~])([a-zA-Z][\w-]*)")
                        .find_iter(selector)
                        .count() as i64;
                    let last = selector
                        .split(|c: char| c.is_whitespace() || c == '>')
                        .rfind(|part| !part.is_empty())
                        .unwrap_or("");
                    let tag: String = last
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '-')
                        .collect::<String>()
                        .to_lowercase();
                    let required: HashSet<String> = regex!(r"\.([\w-]+)")
                        .captures_iter(last)
                        .map(|c| c[1].to_string())
                        .collect();
                    let id = regex!(r"#([\w-]+)")
                        .captures(last)
                        .map(|c| c[1].to_string());
                    self.rules.push(Rule {
                        selector: selector.to_string(),
                        declarations: declarations(&body),
                        specificity: ids * 10000 + classes * 100 + tags,
                        tag,
                        classes: required,
                        id,
                    });
                }
            }
        }
    }

    pub fn computed(&self, dom: &Dom, id: NodeId, parent: &TextStyle) -> TextStyle {
        let node = dom.node(id);
        let mut values: BTreeMap<String, String> = parent
            .properties
            .iter()
            .filter(|(key, _)| is_inherited(key))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let set = |values: &mut BTreeMap<String, String>, key: &str, value: &str| {
            values.insert(key.to_string(), value.to_string());
        };
        // Semantic defaults have lower priority than author CSS.
        match node.name.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let (size, margin) = match node.name.as_str() {
                    "h1" => ("2em", "0.67em"),
                    "h2" => ("1.5em", "0.83em"),
                    "h3" => ("1.17em", "1em"),
                    "h4" => ("1em", "1.33em"),
                    "h5" => ("0.83em", "1.67em"),
                    _ => ("0.67em", "2.33em"),
                };
                set(&mut values, "font-weight", "bold");
                set(&mut values, "font-size", size);
                set(&mut values, "margin-top", margin);
                set(&mut values, "margin-bottom", margin);
            }
            "b" | "strong" => set(&mut values, "font-weight", "bold"),
            "i" | "em" | "cite" => set(&mut values, "font-style", "italic"),
            "u" => set(&mut values, "text-decoration", "underline"),
            "s" | "del" | "strike" => set(&mut values, "text-decoration", "line-through"),
            "pre" => {
                set(&mut values, "white-space", "pre-wrap");
                set(&mut values, "font-family", "monospace");
                set(&mut values, "text-align", "left");
                set(&mut values, "margin-top", "1em");
                set(&mut values, "margin-bottom", "1em");
            }
            "code" | "kbd" | "samp" | "tt" => set(&mut values, "font-family", "monospace"),
            "small" => set(&mut values, "font-size", "0.8em"),
            "sup" => set(&mut values, "vertical-align", "super"),
            "sub" => set(&mut values, "vertical-align", "sub"),
            "blockquote" | "figure" => {
                set(&mut values, "margin-left", "2.5em");
                set(&mut values, "margin-right", "2.5em");
                set(&mut values, "margin-top", "1em");
                set(&mut values, "margin-bottom", "1em");
            }
            "p" | "ul" | "ol" => {
                set(&mut values, "margin-top", "1em");
                set(&mut values, "margin-bottom", "1em");
                if node.name == "ul" {
                    set(&mut values, "list-style-type", "disc");
                }
                if node.name == "ol" {
                    set(&mut values, "list-style-type", "decimal");
                }
            }
            "hr" => {
                set(&mut values, "margin-top", "0.5em");
                set(&mut values, "margin-bottom", "0.5em");
            }
            _ => {}
        }
        let mut winners: HashMap<String, (i64, usize)> = HashMap::new();
        let mut apply = |values: &mut BTreeMap<String, String>,
                         declarations: &[Declaration],
                         specificity: i64,
                         order: usize| {
            for declaration in declarations {
                let rank = specificity + if declaration.important { 1_000_000 } else { 0 };
                if let Some(previous) = winners.get(&declaration.name)
                    && (previous.0 > rank || (previous.0 == rank && previous.1 > order))
                {
                    continue;
                }
                winners.insert(declaration.name.clone(), (rank, order));
                match declaration.value.as_str() {
                    "inherit" => match parent.properties.get(&declaration.name) {
                        Some(value) => {
                            values.insert(declaration.name.clone(), value.clone());
                        }
                        None => {
                            values.remove(&declaration.name);
                        }
                    },
                    "initial" | "unset" => {
                        values.remove(&declaration.name);
                    }
                    value => {
                        values.insert(declaration.name.clone(), value.to_string());
                    }
                }
            }
        };
        let classes: HashSet<&str> = node
            .attributes
            .get("class")
            .map(|c| c.split_whitespace().collect())
            .unwrap_or_default();
        for (index, rule) in self.rules.iter().enumerate() {
            if (rule.tag.is_empty() || rule.tag == node.name)
                && rule
                    .id
                    .as_deref()
                    .is_none_or(|id| node.attributes.get("id").map(String::as_str) == Some(id))
                && rule.classes.iter().all(|c| classes.contains(c.as_str()))
                && dom.matches(id, &rule.selector)
            {
                apply(&mut values, &rule.declarations, rule.specificity, index);
            }
        }
        if let Some(inline) = node.attributes.get("style") {
            apply(
                &mut values,
                &declarations(inline),
                100_000,
                self.rules.len(),
            );
        }
        let parent_size = parent
            .get("font-size")
            .and_then(|size| size.replace("rem", "").parse::<f64>().ok())
            .unwrap_or(1.0);
        if let Some(size) = values.get("font-size").cloned() {
            let number = css_number(&size);
            let resolved = if size.ends_with("rem") {
                number
            } else if size.ends_with("em") {
                number.map(|n| n * parent_size)
            } else if size.ends_with('%') {
                number.map(|n| n * parent_size / 100.0)
            } else if size.ends_with("px") {
                number.map(|n| n / 16.0)
            } else if size.ends_with("pt") {
                number.map(|n| n / 12.0)
            } else {
                match size.as_str() {
                    "small" => Some(0.8),
                    "medium" => Some(1.0),
                    "large" => Some(1.2),
                    "larger" => Some(parent_size * 1.2),
                    "smaller" => Some(parent_size / 1.2),
                    _ => None,
                }
            };
            if let Some(resolved) = resolved.filter(|r| *r > 0.0) {
                values.insert(
                    "font-size".into(),
                    format!("{}rem", format_number(resolved)),
                );
            }
        }
        TextStyle { properties: values }
    }
}

/// Top-level `selector { body }` blocks, honoring nested braces and quotes.
pub fn blocks(source: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = source.chars().collect();
    let mut result = Vec::new();
    let mut start = 0;
    let mut cursor = 0;
    while cursor < chars.len() {
        let Some(open) = chars[cursor..]
            .iter()
            .position(|c| *c == '{')
            .map(|p| p + cursor)
        else {
            break;
        };
        let selector: String = chars[start..open]
            .iter()
            .collect::<String>()
            .trim()
            .to_string();
        let mut depth = 1;
        let mut quote: Option<char> = None;
        cursor = open + 1;
        let body_start = cursor;
        while cursor < chars.len() && depth > 0 {
            let c = chars[cursor];
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
            } else if c == '"' || c == '\'' {
                quote = Some(c);
            } else if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
            }
            if depth == 0 {
                break;
            }
            cursor += 1;
        }
        if cursor >= chars.len() {
            break;
        }
        result.push((selector, chars[body_start..cursor].iter().collect()));
        cursor += 1;
        start = cursor;
    }
    result
}

pub fn declarations(body: &str) -> Vec<Declaration> {
    let mut result = Vec::new();
    let mut push = |name: String, value: String, important: bool| {
        result.push(Declaration {
            name,
            value,
            important,
        })
    };
    for part in body.split(';') {
        let Some((name, raw)) = part.split_once(':') else {
            continue;
        };
        let name = name.trim().to_lowercase();
        let raw = raw.trim();
        let cut = raw.len().saturating_sub(10);
        let important = raw.len() >= 10
            && raw.is_char_boundary(cut)
            && raw[cut..].eq_ignore_ascii_case("!important");
        let value = if important { &raw[..cut] } else { raw }.trim().to_string();
        if name == "font"
            && let Some(m) =
                regex!(r"^(.*?)\s*(\d*\.?\d+(?:px|pt|rem|em|%))(?:\s*/\s*([^\s]+))?\s+(.+)$")
                    .captures(&value)
        {
            let prefix: Vec<&str> = m[1].split_whitespace().collect();
            let italic = prefix.contains(&"italic") || prefix.contains(&"oblique");
            push(
                "font-style".into(),
                if italic { "italic" } else { "normal" }.into(),
                important,
            );
            let weight = prefix
                .iter()
                .find(|p| **p == "bold" || p.parse::<i64>().is_ok())
                .copied()
                .unwrap_or("normal");
            push("font-weight".into(), weight.into(), important);
            push("font-size".into(), m[2].to_string(), important);
            if let Some(line) = m.get(3) {
                push("line-height".into(), line.as_str().to_string(), important);
            }
            push("font-family".into(), m[4].to_string(), important);
        } else if name == "margin" || name == "padding" {
            let parts: Vec<&str> = value.split_whitespace().collect();
            if (1..=4).contains(&parts.len()) {
                let top = parts[0];
                let right = parts.get(1).copied().unwrap_or(top);
                let bottom = parts.get(2).copied().unwrap_or(top);
                let left = parts.get(3).copied().unwrap_or(right);
                for (side, v) in [
                    ("top", top),
                    ("right", right),
                    ("bottom", bottom),
                    ("left", left),
                ] {
                    push(format!("{name}-{side}"), v.to_string(), important);
                }
            }
        } else {
            push(name, value, important);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kfx::dom::ROOT;

    #[test]
    fn computes_cascade_and_font_size() {
        let dom = Dom::parse(br#"<html><body><p class="c" style="color: red">x</p></body></html>"#)
            .expect("parse");
        let mut sheet = Stylesheet::default();
        let mut visited = HashSet::new();
        sheet.append(
            "p { margin: 2em 1em; font-size: 150% } .c { text-align: center !important } p.c { text-align: left }",
            "a.css",
            &mut |_| None,
            &mut visited,
        );
        let p = dom.first(ROOT, "p").expect("p");
        let style = sheet.computed(&dom, p, &TextStyle::default());
        assert_eq!(style.get("font-size"), Some("1.5rem"));
        assert_eq!(style.get("margin-left"), Some("1em"));
        assert_eq!(style.get("text-align"), Some("center"));
        assert_eq!(style.get("color"), Some("red"));
    }

    #[test]
    fn collects_font_faces() {
        let mut sheet = Stylesheet::default();
        sheet.append(
            "@font-face { font-family: 'Pyidaungsu'; src: url(../fonts/p.ttf); }",
            "OEBPS/css/a.css",
            &mut |_| None,
            &mut HashSet::new(),
        );
        assert_eq!(sheet.fonts[0].family, "Pyidaungsu");
        assert_eq!(sheet.fonts[0].path, "OEBPS/fonts/p.ttf");
    }
}
