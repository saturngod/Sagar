//! Maps computed CSS to KFX style fields (YJ symbol ids).

use std::collections::BTreeMap;

use super::ion::Ion;
use super::layout::{LayoutMetrics, absolute};
use super::model::{EmbeddedFont, TextStyle};
use super::util::{css_number, unquoted};

pub fn dimension(value: f64, unit: u64) -> Ion {
    Ion::Struct(vec![
        (307, Ion::Decimal((value * 1e6).round() / 1e6)),
        (306, Ion::Symbol(unit)),
    ])
}

fn background(p: &TextStyle) -> Option<&str> {
    p.get("background-color").or_else(|| p.get("background"))
}

pub fn needs_box(style: &TextStyle) -> bool {
    background(style).and_then(color).is_some()
        || style
            .properties
            .keys()
            .any(|key| key == "border" || key.starts_with("border-"))
}

const LINE_STYLES: [(&str, u64); 6] = [
    ("solid", 328),
    ("double", 329),
    ("dashed", 330),
    ("dotted", 331),
    ("none", 349),
    ("hidden", 349),
];

fn line_style(name: &str) -> Option<u64> {
    LINE_STYLES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| *v)
}

const SPACING: [(&str, u64); 8] = [
    ("margin-top", 47),
    ("margin-bottom", 49),
    ("margin-left", 48),
    ("margin-right", 50),
    ("padding-top", 52),
    ("padding-left", 53),
    ("padding-bottom", 54),
    ("padding-right", 55),
];

pub fn box_fields(style: &TextStyle, metrics: &LayoutMetrics) -> Vec<(u64, Ion)> {
    let p = style;
    let size = LayoutMetrics::size(style) * metrics.font_scale;
    let mut fields: BTreeMap<u64, Ion> = BTreeMap::new();
    if let Some(bg) = background(p).and_then(color) {
        fields.insert(70, Ion::Int(bg));
    }
    for (name, key) in SPACING {
        if let Some(v) = spacing(name, style, metrics) {
            fields.insert(key, v);
        }
    }
    for (suffix, color_key, style_key, width_key) in [
        ("", 83, 88, 93),
        ("-top", 84, 89, 94),
        ("-left", 85, 90, 95),
        ("-bottom", 86, 91, 96),
        ("-right", 87, 92, 97),
    ] {
        let base = format!("border{suffix}");
        let parts: Vec<&str> = p.get(&base).unwrap_or("").split_whitespace().collect();
        if let Some(c) = p
            .get(&format!("{base}-color"))
            .or_else(|| parts.iter().find(|part| color(part).is_some()).copied())
            .and_then(color)
        {
            fields.insert(color_key, Ion::Int(c));
        }
        if let Some(symbol) = p
            .get(&format!("{base}-style"))
            .or_else(|| {
                parts
                    .iter()
                    .find(|part| line_style(part).is_some())
                    .copied()
            })
            .and_then(line_style)
        {
            fields.insert(style_key, Ion::Symbol(symbol));
        }
        if let Some(width) = p
            .get(&format!("{base}-width"))
            .or_else(|| {
                parts
                    .iter()
                    .find(|part| css_number(part).is_some())
                    .copied()
            })
            .and_then(|w| border_dimension(w, size))
        {
            fields.insert(width_key, width);
        }
    }
    fields.into_iter().collect()
}

pub fn fields(
    style: &TextStyle,
    fonts: &[EmbeddedFont],
    inline: bool,
    metrics: &LayoutMetrics,
) -> Vec<(u64, Ion)> {
    let p = style;
    let mut fields: BTreeMap<u64, Ion> = BTreeMap::new();
    let size = LayoutMetrics::size(style) * metrics.font_scale;
    if let Some(n) = p.get("font-size").and_then(css_number).filter(|n| *n > 0.0) {
        fields.insert(16, dimension(n * metrics.font_scale, 505));
    }
    if let Some(family) = p.get("font-family").and_then(|families| {
        families.split(',').map(unquoted).find(|name| {
            let lower = name.to_lowercase();
            lower == "monospace"
                || lower == "cursive"
                || fonts.iter().any(|f| f.family.to_lowercase() == lower)
        })
    }) {
        fields.insert(11, Ion::Str(family));
    }
    if let Some(weight) = p.get("font-weight") {
        let bold =
            weight == "bold" || weight == "bolder" || weight.parse::<i64>().unwrap_or(0) >= 600;
        fields.insert(13, Ion::Symbol(if bold { 361 } else { 350 }));
    }
    if let Some(font_style) = p.get("font-style") {
        let italic = font_style == "italic" || font_style == "oblique";
        fields.insert(12, Ion::Symbol(if italic { 382 } else { 350 }));
    }
    if let Some(c) = p.get("color").and_then(color) {
        fields.insert(19, Ion::Int(c));
    }
    // Only blocks own backgrounds; inline backgrounds paint glyph patches.
    if !inline && let Some(c) = background(p).and_then(color) {
        fields.insert(21, Ion::Int(c));
    }
    if let Some(decoration) = p
        .get("text-decoration")
        .or_else(|| p.get("text-decoration-line"))
    {
        fields.insert(
            23,
            Ion::Symbol(if decoration.contains("underline") {
                328
            } else {
                349
            }),
        );
        fields.insert(
            27,
            Ion::Symbol(if decoration.contains("line-through") {
                328
            } else {
                349
            }),
        );
    }
    if let Some(align) = p
        .get("vertical-align")
        .filter(|a| *a == "super" || *a == "sub")
    {
        fields.insert(44, Ion::Symbol(if align == "super" { 370 } else { 371 }));
    }
    if !inline {
        if let Some(whitespace) = p.get("white-space") {
            fields.insert(
                45,
                Ion::Bool(matches!(
                    whitespace,
                    "pre" | "pre-wrap" | "pre-line" | "nowrap"
                )),
            );
        }
        let align = match p.get("text-align") {
            Some("left" | "start") => Some(59),
            Some("right" | "end") => Some(61),
            Some("center") => Some(320),
            Some("justify") => Some(321),
            _ => None,
        };
        if let Some(symbol) = align {
            fields.insert(34, Ion::Symbol(symbol));
        }
        if let Some(direction) = p.get("direction") {
            fields.insert(192, Ion::Symbol(if direction == "rtl" { 375 } else { 376 }));
        }
        if LayoutMetrics::raw_line(style).is_some() {
            fields.insert(42, dimension(metrics.leading(style, true), 310));
        }
        for (name, key) in [
            ("margin-top", 47),
            ("margin-bottom", 49),
            ("margin-left", 48),
            ("margin-right", 50),
            ("text-indent", 36),
            ("padding-top", 52),
            ("padding-left", 53),
            ("padding-bottom", 54),
            ("padding-right", 55),
            ("width", 56),
            ("height", 57),
            ("max-width", 65),
            ("max-height", 64),
        ] {
            if name.starts_with("margin-") || name.starts_with("padding-") {
                if let Some(v) = spacing(name, style, metrics) {
                    fields.insert(key, v);
                }
            } else if let Some(d) = p.get(name).and_then(|v| css_dimension(v, size)) {
                fields.insert(key, d);
            }
        }
        for (name, key) in [
            ("break-before", 134),
            ("page-break-before", 134),
            ("break-after", 133),
            ("page-break-after", 133),
            ("break-inside", 135),
        ] {
            match p.get(name) {
                Some("page" | "always") => {
                    fields.insert(key, Ion::Symbol(352));
                }
                Some("avoid") => {
                    fields.insert(key, Ion::Symbol(353));
                }
                _ => {}
            }
        }
    }
    fields.into_iter().collect()
}

pub fn spacing(name: &str, style: &TextStyle, metrics: &LayoutMetrics) -> Option<Ion> {
    let vertical = name.ends_with("top") || name.ends_with("bottom");
    if vertical
        && let Some(value) = style
            .get(&format!("-kfx-{name}"))
            .and_then(|v| v.parse::<f64>().ok())
    {
        return Some(dimension(
            value / LayoutMetrics::size(style) / (metrics.leading(style, false) * 1.2),
            310,
        ));
    }
    let source = style.get(name)?;
    let size = LayoutMetrics::size(style) * metrics.font_scale;
    let value = absolute(Some(source), size)?;
    if vertical {
        return Some(dimension(
            value / size / (metrics.leading(style, true) * 1.2),
            310,
        ));
    }
    if source.ends_with('%')
        && let Some(n) = css_number(source)
    {
        return Some(dimension(n, 314));
    }
    Some(dimension(value / size, 308))
}

fn border_dimension(source: &str, size: f64) -> Option<Ion> {
    let n = css_number(source)?;
    if source.ends_with("px") {
        return Some(dimension(n * 0.75, 318));
    }
    if source.ends_with("pt") {
        return Some(dimension(n, 318));
    }
    css_dimension(source, size)
}

fn css_dimension(source: &str, font_size: f64) -> Option<Ion> {
    let n = css_number(source).filter(|n| n.is_finite())?;
    if source.ends_with('%') {
        Some(dimension(n, 314))
    } else if source.ends_with("rem") {
        Some(dimension(n, 505))
    } else if source.ends_with("em") {
        Some(dimension(n * font_size, 505))
    } else if source.ends_with("px") {
        Some(dimension(n / 16.0, 505))
    } else if source.ends_with("pt") {
        Some(dimension(n / 12.0, 505))
    } else if n == 0.0 {
        Some(dimension(0.0, 505))
    } else {
        None
    }
}

/// CSS color → opaque ARGB (KFX colors carry alpha).
pub fn color(value: &str) -> Option<i64> {
    let lower = value.trim().to_lowercase();
    let named = match lower.as_str() {
        "black" => "#000000",
        "white" => "#ffffff",
        "red" => "#ff0000",
        "blue" => "#0000ff",
        "green" => "#008000",
        "gray" | "grey" => "#808080",
        "yellow" => "#ffff00",
        "navy" => "#000080",
        other => other,
    };
    if let Some(hex) = named.strip_prefix('#') {
        let expanded = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 => hex.to_string(),
            _ => return None,
        };
        return i64::from_str_radix(&expanded, 16)
            .ok()
            .map(|v| 0xff000000 | v);
    }
    let inner = named.strip_prefix("rgb(")?.strip_suffix(')')?;
    let parts: Vec<i64> = inner
        .split(',')
        .map(|p| p.trim().parse::<i64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if parts.len() != 3 || parts.iter().any(|p| *p < 0) {
        return None;
    }
    Some(0xff000000 | (parts[0].min(255) << 16) | (parts[1].min(255) << 8) | parts[2].min(255))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colors() {
        assert_eq!(color("#fff"), Some(0xffffffff));
        assert_eq!(color("navy"), Some(0xff000080));
        assert_eq!(color("rgb(1, 2, 300)"), Some(0xff0102ff));
        assert_eq!(color("transparent"), None);
    }
}
