//! Arena DOM for the KFX converter. Element names are lowercased local names;
//! attributes are keyed by local name. Parsing goes through the app's safe
//! XML entry point (no DTD entity expansion).

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::model::TextStyle;

pub type NodeId = usize;

#[derive(Debug, Clone)]
pub enum Child {
    Text(String),
    Element(NodeId),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub attributes: HashMap<String, String>,
    pub parent: Option<NodeId>,
    pub children: Vec<Child>,
    pub style: TextStyle,
}

#[derive(Debug, Clone)]
pub struct Dom {
    pub nodes: Vec<Node>,
}

pub const ROOT: NodeId = 0;

/// Common HTML named entities seen in EPUB XHTML that XML does not define.
const ENTITIES: [(&str, u32); 10] = [
    ("nbsp", 160),
    ("copy", 169),
    ("reg", 174),
    ("hellip", 8230),
    ("mdash", 8212),
    ("ndash", 8211),
    ("lsquo", 8216),
    ("rsquo", 8217),
    ("ldquo", 8220),
    ("rdquo", 8221),
];

impl Dom {
    pub fn parse(bytes: &[u8]) -> Result<Dom, String> {
        let mut source = String::from_utf8_lossy(bytes).into_owned();
        for (name, code) in ENTITIES {
            source = source.replace(&format!("&{name};"), &format!("&#{code};"));
        }
        let document = crate::epub::xml::parse(&source).map_err(|error| error.to_string())?;
        let mut dom = Dom {
            nodes: vec![Node {
                name: "#document".into(),
                attributes: HashMap::new(),
                parent: None,
                children: Vec::new(),
                style: TextStyle::default(),
            }],
        };
        dom.append(document.root(), ROOT);
        Ok(dom)
    }

    fn append(&mut self, source: roxmltree::Node, parent: NodeId) {
        for child in source.children() {
            if child.is_element() {
                let mut attributes = HashMap::new();
                for attribute in child.attributes() {
                    attributes
                        .entry(attribute.name().to_string())
                        .or_insert_with(|| attribute.value().to_string());
                }
                let id = self.nodes.len();
                self.nodes.push(Node {
                    name: child.tag_name().name().to_lowercase(),
                    attributes,
                    parent: Some(parent),
                    children: Vec::new(),
                    style: TextStyle::default(),
                });
                self.nodes[parent].children.push(Child::Element(id));
                self.append(child, id);
            } else if child.is_text()
                && let Some(text) = child.text()
            {
                self.nodes[parent]
                    .children
                    .push(Child::Text(text.to_string()));
            }
        }
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub fn attr(&self, id: NodeId, name: &str) -> Option<&str> {
        self.nodes[id].attributes.get(name).map(String::as_str)
    }

    pub fn elements(&self, id: NodeId) -> Vec<NodeId> {
        self.nodes[id]
            .children
            .iter()
            .filter_map(|child| match child {
                Child::Element(e) => Some(*e),
                Child::Text(_) => None,
            })
            .collect()
    }

    pub fn text(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out);
        out
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        for child in &self.nodes[id].children {
            match child {
                Child::Text(text) => out.push_str(text),
                Child::Element(e) => self.collect_text(*e, out),
            }
        }
    }

    /// Preorder descendants (including `id`) with the given name.
    pub fn descendants(&self, id: NodeId, name: &str) -> Vec<NodeId> {
        let mut out = Vec::new();
        self.collect_descendants(id, name, &mut out);
        out
    }

    fn collect_descendants(&self, id: NodeId, name: &str, out: &mut Vec<NodeId>) {
        if self.nodes[id].name == name {
            out.push(id);
        }
        for child in self.elements(id) {
            self.collect_descendants(child, name, out);
        }
    }

    pub fn first(&self, id: NodeId, name: &str) -> Option<NodeId> {
        self.descendants(id, name).into_iter().next()
    }

    /// Simple descendant/child selector matching. Sibling combinators,
    /// pseudo-elements and functional pseudo-classes never match.
    pub fn matches(&self, id: NodeId, selector: &str) -> bool {
        if selector.contains('+')
            || selector.contains('~')
            || selector.contains("::")
            || selector.contains('(')
        {
            return false;
        }
        let spaced = selector.replace('>', " > ");
        let tokens: Vec<&str> = spaced.split_whitespace().collect();
        let Some(last) = tokens.last() else {
            return false;
        };
        if !self.simple_match(id, last) {
            return false;
        }
        let mut current = id;
        let mut index = tokens.len() as isize - 2;
        while index >= 0 {
            if tokens[index as usize] == ">" {
                index -= 1;
                if index < 0 {
                    return false;
                }
                match self.nodes[current].parent {
                    Some(parent) if self.simple_match(parent, tokens[index as usize]) => {
                        current = parent
                    }
                    _ => return false,
                }
            } else {
                let mut ancestor = self.nodes[current].parent;
                while let Some(node) = ancestor {
                    if self.simple_match(node, tokens[index as usize]) {
                        break;
                    }
                    ancestor = self.nodes[node].parent;
                }
                match ancestor {
                    Some(found) => current = found,
                    None => return false,
                }
            }
            index -= 1;
        }
        true
    }

    fn simple_match(&self, id: NodeId, source: &str) -> bool {
        static ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"\[([\w:-]+)(?:=['"]?([^'"\]]+)['"]?)?\]"#).expect("valid regex")
        });
        static PSEUDO: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r":([\w-]+)").expect("valid regex"));
        static CLASS_ID: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"([.#])([\w-]+)").expect("valid regex"));
        let node = &self.nodes[id];
        let mut selector = source.to_string();
        let attribute_matches: Vec<(String, String, String)> = ATTRIBUTE
            .captures_iter(source)
            .map(|c| {
                (
                    c[0].to_string(),
                    c[1].to_string(),
                    c.get(2).map(|m| m.as_str().to_string()).unwrap_or_default(),
                )
            })
            .collect();
        for (whole, name, expected) in attribute_matches {
            let local = name.rsplit(':').next().unwrap_or(&name);
            let Some(value) = node
                .attributes
                .get(&name)
                .or_else(|| node.attributes.get(local))
            else {
                return false;
            };
            if !expected.is_empty() && value != &expected {
                return false;
            }
            selector = selector.replace(&whole, "");
        }
        let pseudo: Vec<(String, String)> = PSEUDO
            .captures_iter(&selector)
            .map(|c| (c[0].to_string(), c[1].to_string()))
            .collect();
        for (whole, name) in pseudo {
            let siblings = node
                .parent
                .map(|parent| self.elements(parent))
                .unwrap_or_default();
            match name.as_str() {
                "first-child" if siblings.first() != Some(&id) => return false,
                "last-child" if siblings.last() != Some(&id) => return false,
                "root" if node.parent.map(|p| self.nodes[p].name.as_str()) != Some("#document") => {
                    return false;
                }
                "first-child" | "last-child" | "root" => {}
                _ => return false,
            }
            selector = selector.replace(&whole, "");
        }
        if selector.contains('[') || selector.contains(':') {
            return false;
        }
        for capture in CLASS_ID.captures_iter(&selector) {
            if &capture[1] == "#" {
                if node.attributes.get("id").map(String::as_str) != Some(&capture[2]) {
                    return false;
                }
            } else if !node
                .attributes
                .get("class")
                .is_some_and(|classes| classes.split_whitespace().any(|c| c == &capture[2]))
            {
                return false;
            }
        }
        let tag: String = selector
            .chars()
            .take_while(|c| *c != '.' && *c != '#')
            .collect();
        tag.is_empty() || tag == "*" || tag.to_lowercase() == node.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_matches_selectors() {
        let dom = Dom::parse(
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div class="a b" id="d"><p class="x">Hi&nbsp;<em>there</em></p></div></body></html>"#,
        )
        .expect("parse");
        let p = dom.first(ROOT, "p").expect("p");
        assert_eq!(dom.text(p), "Hi\u{a0}there");
        assert!(dom.matches(p, "div.a > p.x"));
        assert!(dom.matches(p, "body p"));
        assert!(dom.matches(p, "#d p:first-child"));
        assert!(!dom.matches(p, "section p"));
        assert!(!dom.matches(p, "div + p"));
        assert!(dom.matches(p, "p[class=x]"));
    }
}
