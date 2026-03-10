use crate::css::{Selector, Stylesheet};
use crate::dom::{ElementData, Node, NodeType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledNode {
    pub node_type: NodeType,
    pub style: ComputedStyle,
    pub children: Vec<StyledNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputedStyle {
    pub display: Display,
    pub color: String,
    pub background_color: Option<String>,
    pub font_weight: FontWeight,
    pub font_size: usize,
    pub line_height: LineHeight,
    pub width: Option<usize>,
    pub margin: EdgeSizes,
    pub padding: EdgeSizes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Block,
    Inline,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontWeight {
    Normal,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineHeight {
    Normal,
    RelativePercent(usize),
    Px(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeSizes {
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
    pub left: usize,
}

impl EdgeSizes {
    pub const fn zero() -> Self {
        Self {
            top: 0,
            right: 0,
            bottom: 0,
            left: 0,
        }
    }

    pub const fn uniform(value: usize) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub const fn vertical_horizontal(vertical: usize, horizontal: usize) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    pub const fn horizontal(self) -> usize {
        self.left + self.right
    }
}

impl ComputedStyle {
    fn initial() -> Self {
        Self {
            display: Display::Block,
            color: "default".to_string(),
            background_color: None,
            font_weight: FontWeight::Normal,
            font_size: 16,
            line_height: LineHeight::Normal,
            width: None,
            margin: EdgeSizes::zero(),
            padding: EdgeSizes::zero(),
        }
    }
}

pub fn collect_stylesheets(node: &Node) -> Stylesheet {
    let mut css = String::new();
    collect_style_text(node, &mut css);
    crate::css::parse(&css)
}

pub fn style_tree(node: &Node, stylesheet: &Stylesheet) -> StyledNode {
    build_styled_node(node, stylesheet)
}

fn build_styled_node(node: &Node, stylesheet: &Stylesheet) -> StyledNode {
    let style = compute_style(node, stylesheet);
    let children = node
        .children
        .iter()
        .map(|child| build_styled_node(child, stylesheet))
        .collect();

    StyledNode {
        node_type: node.node_type.clone(),
        style,
        children,
    }
}

fn compute_style(node: &Node, stylesheet: &Stylesheet) -> ComputedStyle {
    let mut style = default_style(node);

    let NodeType::Element(element) = &node.node_type else {
        return style;
    };

    let mut matched = Vec::new();
    for (rule_index, rule) in stylesheet.rules.iter().enumerate() {
        for selector in &rule.selectors {
            if matches_selector(element, selector) {
                matched.push((specificity(selector), rule_index, &rule.declarations));
            }
        }
    }

    matched.sort_by_key(|(specificity, rule_index, _)| (*specificity, *rule_index));

    for (_, _, declarations) in matched {
        for declaration in declarations {
            apply_declaration(&mut style, &declaration.name, &declaration.value);
        }
    }

    if element.attributes.iter().any(|(key, _)| key == "hidden") {
        style.display = Display::None;
    }

    style
}

fn default_style(node: &Node) -> ComputedStyle {
    let mut style = ComputedStyle::initial();

    match &node.node_type {
        NodeType::Element(element) => {
            style.display = match element.tag_name.as_str() {
                "span" | "a" => Display::Inline,
                "style" | "head" => Display::None,
                _ => Display::Block,
            };

            match element.tag_name.as_str() {
                "body" => {
                    style.margin = EdgeSizes::uniform(8);
                    style.line_height = LineHeight::RelativePercent(155);
                }
                "h1" => {
                    style.font_weight = FontWeight::Bold;
                    style.font_size = 32;
                    style.line_height = LineHeight::RelativePercent(115);
                    style.margin = EdgeSizes {
                        top: 20,
                        right: 0,
                        bottom: 16,
                        left: 0,
                    };
                }
                "h2" => {
                    style.font_weight = FontWeight::Bold;
                    style.font_size = 24;
                    style.line_height = LineHeight::RelativePercent(120);
                    style.margin = EdgeSizes::vertical_horizontal(18, 0);
                }
                "h3" | "strong" => {
                    style.font_weight = FontWeight::Bold;
                    style.font_size = 18;
                }
                "p" => {
                    style.line_height = LineHeight::RelativePercent(160);
                    style.margin = EdgeSizes::vertical_horizontal(12, 0);
                }
                "div" => {
                    style.line_height = LineHeight::RelativePercent(150);
                    style.margin = EdgeSizes::vertical_horizontal(8, 0);
                }
                "a" => {
                    style.color = "blue".to_string();
                }
                _ => {}
            }
        }
        NodeType::Text(_) => style.display = Display::Inline,
        NodeType::Document => {}
    }

    style
}

fn matches_selector(element: &ElementData, selector: &Selector) -> bool {
    if let Some(tag_name) = &selector.tag_name {
        if &element.tag_name != tag_name {
            return false;
        }
    }

    if let Some(id) = &selector.id {
        let element_id = attribute_value(element, "id");
        if element_id != Some(id.as_str()) {
            return false;
        }
    }

    let classes = attribute_value(element, "class")
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>();

    selector
        .classes
        .iter()
        .all(|class_name| classes.contains(&class_name.as_str()))
}

fn attribute_value<'a>(element: &'a ElementData, name: &str) -> Option<&'a str> {
    element
        .attributes
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value.as_str()))
}

fn specificity(selector: &Selector) -> (usize, usize, usize) {
    (
        usize::from(selector.id.is_some()),
        selector.classes.len(),
        usize::from(selector.tag_name.is_some()),
    )
}

fn apply_declaration(style: &mut ComputedStyle, name: &str, value: &str) {
    match name {
        "display" => {
            style.display = match value.trim().to_ascii_lowercase().as_str() {
                "none" => Display::None,
                "inline" => Display::Inline,
                _ => Display::Block,
            };
        }
        "color" => style.color = value.trim().to_ascii_lowercase(),
        "background-color" => style.background_color = Some(value.trim().to_ascii_lowercase()),
        "font-weight" => {
            style.font_weight = if value.trim().eq_ignore_ascii_case("bold") {
                FontWeight::Bold
            } else {
                FontWeight::Normal
            };
        }
        "font-size" => {
            if let Some(font_size) = parse_font_size(value) {
                style.font_size = font_size;
            }
        }
        "line-height" => {
            if let Some(line_height) = parse_line_height(value) {
                style.line_height = line_height;
            }
        }
        "width" => {
            style.width = value
                .trim()
                .strip_suffix("px")
                .or(Some(value.trim()))
                .and_then(|raw| raw.parse::<usize>().ok());
        }
        "margin" => {
            if let Some(edges) = parse_edge_sizes(value) {
                style.margin = edges;
            }
        }
        "padding" => {
            if let Some(edges) = parse_edge_sizes(value) {
                style.padding = edges;
            }
        }
        "margin-top" => apply_edge_value(value, &mut style.margin.top),
        "margin-right" => apply_edge_value(value, &mut style.margin.right),
        "margin-bottom" => apply_edge_value(value, &mut style.margin.bottom),
        "margin-left" => apply_edge_value(value, &mut style.margin.left),
        "padding-top" => apply_edge_value(value, &mut style.padding.top),
        "padding-right" => apply_edge_value(value, &mut style.padding.right),
        "padding-bottom" => apply_edge_value(value, &mut style.padding.bottom),
        "padding-left" => apply_edge_value(value, &mut style.padding.left),
        _ => {}
    }
}

fn apply_edge_value(value: &str, target: &mut usize) {
    if let Some(size) = parse_length_px(value) {
        *target = size;
    }
}

fn parse_length_px(value: &str) -> Option<usize> {
    value
        .trim()
        .strip_suffix("px")
        .unwrap_or(value.trim())
        .parse::<usize>()
        .ok()
}

fn parse_font_size(value: &str) -> Option<usize> {
    let normalized = value.trim().to_ascii_lowercase();
    parse_length_px(&normalized).or_else(|| match normalized.as_str() {
        "small" => Some(14),
        "medium" => Some(16),
        "large" => Some(20),
        "x-large" => Some(24),
        "xx-large" => Some(32),
        _ => None,
    })
}

fn parse_line_height(value: &str) -> Option<LineHeight> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "normal" {
        return Some(LineHeight::Normal);
    }
    if let Some(px) = normalized.strip_suffix("px").and_then(|raw| raw.parse::<usize>().ok()) {
        return Some(LineHeight::Px(px));
    }
    if let Ok(multiplier) = normalized.parse::<f32>() {
        if multiplier.is_finite() && multiplier > 0.0 {
            return Some(LineHeight::RelativePercent((multiplier * 100.0).round() as usize));
        }
    }
    None
}

fn parse_edge_sizes(value: &str) -> Option<EdgeSizes> {
    let values = value
        .split_whitespace()
        .map(parse_length_px)
        .collect::<Option<Vec<_>>>()?;

    match values.as_slice() {
        [all] => Some(EdgeSizes::uniform(*all)),
        [vertical, horizontal] => Some(EdgeSizes::vertical_horizontal(*vertical, *horizontal)),
        [top, horizontal, bottom] => Some(EdgeSizes {
            top: *top,
            right: *horizontal,
            bottom: *bottom,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Some(EdgeSizes {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => None,
    }
}

fn collect_style_text(node: &Node, output: &mut String) {
    match &node.node_type {
        NodeType::Element(element) if element.tag_name == "style" => {
            for child in &node.children {
                if let NodeType::Text(text) = &child.node_type {
                    output.push_str(text);
                    output.push('\n');
                }
            }
        }
        _ => {
            for child in &node.children {
                collect_style_text(child, output);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{collect_stylesheets, style_tree, Display, EdgeSizes, FontWeight, LineHeight};
    use crate::dom::{Node, NodeType};

    #[test]
    fn extracts_styles_from_style_tag() {
        let document = Node::document(vec![Node::element(
            "style",
            vec![Node::text("p { color: tomato; }")],
        )]);

        let stylesheet = collect_stylesheets(&document);
        assert_eq!(stylesheet.rules.len(), 1);
        assert_eq!(stylesheet.rules[0].declarations[0].value, "tomato");
    }

    #[test]
    fn applies_basic_styles() {
        let document = crate::html::parse(
            r#"
            <html>
              <head>
                <style>
                  #hero { color: blue; }
                  .accent { background-color: yellow; }
                  p { width: 12px; font-size: 20px; line-height: 1.7; margin: 10px 4px; padding: 6px; }
                </style>
              </head>
              <body>
                <p id="hero" class="accent">hello</p>
              </body>
            </html>
            "#,
        );

        let stylesheet = collect_stylesheets(&document);
        let styled = style_tree(&document, &stylesheet);
        let NodeType::Element(body) = &styled.children[0].children[1].node_type else {
            panic!("expected body");
        };
        assert_eq!(body.tag_name, "body");

        let paragraph = &styled.children[0].children[1].children[0];
        assert_eq!(paragraph.style.color, "blue");
        assert_eq!(paragraph.style.background_color.as_deref(), Some("yellow"));
        assert_eq!(paragraph.style.width, Some(12));
        assert_eq!(paragraph.style.font_size, 20);
        assert_eq!(paragraph.style.line_height, LineHeight::RelativePercent(170));
        assert_eq!(
            paragraph.style.margin,
            EdgeSizes {
                top: 10,
                right: 4,
                bottom: 10,
                left: 4,
            }
        );
        assert_eq!(paragraph.style.padding, EdgeSizes::uniform(6));
    }

    #[test]
    fn hides_style_and_hidden_elements() {
        let document = crate::html::parse(
            r#"<html><body><style>p { color: red; }</style><p hidden>secret</p></body></html>"#,
        );

        let stylesheet = collect_stylesheets(&document);
        let styled = style_tree(&document, &stylesheet);

        let style_node = &styled.children[0].children[0].children[0];
        assert_eq!(style_node.style.display, Display::None);

        let hidden_paragraph = &styled.children[0].children[0].children[1];
        assert_eq!(hidden_paragraph.style.display, Display::None);
        assert_eq!(hidden_paragraph.style.font_weight, FontWeight::Normal);
    }
}
