use crate::dom::NodeType;
use crate::style::{Display, FontWeight, StyledNode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutBox {
    pub kind: LayoutKind,
    pub lines: Vec<String>,
    pub children: Vec<LayoutBox>,
    pub style: LayoutStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutKind {
    Document,
    Block { tag_name: String },
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutStyle {
    pub color: String,
    pub background_color: Option<String>,
    pub font_weight: FontWeight,
}

pub fn build(node: &StyledNode, max_width: usize) -> LayoutBox {
    match &node.node_type {
        NodeType::Document => LayoutBox {
            kind: LayoutKind::Document,
            lines: Vec::new(),
            children: node
                .children
                .iter()
                .filter(|child| child.style.display != Display::None)
                .map(|child| build(child, max_width))
                .collect(),
            style: to_layout_style(node),
        },
        NodeType::Element(element) => {
            let mut children = Vec::new();
            let mut lines = Vec::new();
            let available_width = node.style.width.unwrap_or(max_width).min(max_width).max(1);

            for child in &node.children {
                if child.style.display == Display::None {
                    continue;
                }

                match &child.node_type {
                    NodeType::Text(text) => {
                        if !text.is_empty() {
                            lines.extend(wrap_text(text, available_width));
                        }
                    }
                    _ => children.push(build(child, available_width)),
                }
            }

            LayoutBox {
                kind: LayoutKind::Block {
                    tag_name: element.tag_name.clone(),
                },
                lines,
                children,
                style: to_layout_style(node),
            }
        }
        NodeType::Text(text) => LayoutBox {
            kind: LayoutKind::Text,
            lines: wrap_text(text, max_width),
            children: Vec::new(),
            style: to_layout_style(node),
        },
    }
}

fn to_layout_style(node: &StyledNode) -> LayoutStyle {
    LayoutStyle {
        color: node.style.color.clone(),
        background_color: node.style.background_color.clone(),
        font_weight: node.style.font_weight,
    }
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let width = max_width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let next_len = if current.is_empty() {
            word.len()
        } else {
            current.len() + 1 + word.len()
        };

        if next_len > width && !current.is_empty() {
            lines.push(current);
            current = word.to_string();
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::{build, wrap_text, LayoutKind};
    use crate::dom::Node;
    use crate::style::{self, Display};

    #[test]
    fn wraps_text_to_requested_width() {
        let lines = wrap_text("alpha beta gamma", 10);
        assert_eq!(lines, vec!["alpha beta".to_string(), "gamma".to_string()]);
    }

    #[test]
    fn skips_display_none_nodes() {
        let document = Node::document(vec![Node::element_with_attrs(
            "div",
            vec![("hidden".to_string(), String::new())],
            vec![Node::text("secret")],
        )]);
        let stylesheet = style::collect_stylesheets(&document);
        let styled = style::style_tree(&document, &stylesheet);
        assert_eq!(styled.children[0].style.display, Display::None);

        let layout = build(&styled, 40);
        assert!(matches!(layout.kind, LayoutKind::Document));
        assert!(layout.children.is_empty());
    }
}
