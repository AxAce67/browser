use crate::dom::NodeType;
use crate::style::{Display, EdgeSizes, FontWeight, LineHeight, StyledNode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutBox {
    pub kind: LayoutKind,
    pub lines: Vec<LayoutLine>,
    pub children: Vec<LayoutBox>,
    pub style: LayoutStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutLine {
    pub text: String,
    pub width: usize,
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
    pub font_size: usize,
    pub line_height: LineHeight,
    pub margin: EdgeSizes,
    pub padding: EdgeSizes,
}

#[allow(dead_code)]
pub fn build(node: &StyledNode, max_width: usize) -> LayoutBox {
    build_with_measurer(node, max_width, &ApproximateTextMeasurer)
}

pub fn build_with_measurer(
    node: &StyledNode,
    max_width: usize,
    measurer: &dyn TextMeasurer,
) -> LayoutBox {
    match &node.node_type {
        NodeType::Document => LayoutBox {
            kind: LayoutKind::Document,
            lines: Vec::new(),
            children: node
                .children
                .iter()
                .filter(|child| child.style.display != Display::None)
                .map(|child| build_with_measurer(child, max_width, measurer))
                .collect(),
            style: to_layout_style(node),
        },
        NodeType::Element(element) => {
            let mut children = Vec::new();
            let mut lines = Vec::new();
            let box_overhead = node.style.margin.horizontal() + node.style.padding.horizontal();
            let available_width = node
                .style
                .width
                .unwrap_or_else(|| max_width.saturating_sub(box_overhead))
                .min(max_width.saturating_sub(node.style.margin.horizontal()))
                .max(1);

            for child in &node.children {
                if child.style.display == Display::None {
                    continue;
                }

                match &child.node_type {
                    NodeType::Text(text) => {
                        if !text.is_empty() {
                            lines.extend(wrap_text(
                                text,
                                available_width,
                                node.style.font_size,
                                node.style.font_weight,
                                measurer,
                            ));
                        }
                    }
                    _ => children.push(build_with_measurer(child, available_width, measurer)),
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
            lines: wrap_text(
                text,
                max_width,
                node.style.font_size,
                node.style.font_weight,
                measurer,
            ),
            children: Vec::new(),
            style: to_layout_style(node),
        },
    }
}

pub trait TextMeasurer {
    fn measure_text(&self, text: &str, font_size: usize, font_weight: FontWeight) -> usize;
}

#[allow(dead_code)]
struct ApproximateTextMeasurer;
#[allow(dead_code)]
pub struct MonospaceTextMeasurer;

impl TextMeasurer for ApproximateTextMeasurer {
    fn measure_text(&self, text: &str, font_size: usize, font_weight: FontWeight) -> usize {
        approximate_text_width(text, font_size, font_weight)
    }
}

impl TextMeasurer for MonospaceTextMeasurer {
    fn measure_text(&self, text: &str, font_size: usize, _font_weight: FontWeight) -> usize {
        let cell_width = (font_size / 2).max(8);
        text.chars().count() * cell_width
    }
}

fn to_layout_style(node: &StyledNode) -> LayoutStyle {
    LayoutStyle {
        color: node.style.color.clone(),
        background_color: node.style.background_color.clone(),
        font_weight: node.style.font_weight,
        font_size: node.style.font_size,
        line_height: node.style.line_height,
        margin: node.style.margin,
        padding: node.style.padding,
    }
}

fn wrap_text(
    text: &str,
    max_width: usize,
    font_size: usize,
    font_weight: FontWeight,
    measurer: &dyn TextMeasurer,
) -> Vec<LayoutLine> {
    let width = max_width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    let space_width = measurer.measure_text(" ", font_size, font_weight).max(1);

    for word in text.split_whitespace() {
        let word_width = measurer.measure_text(word, font_size, font_weight).max(1);

        if word_width > width {
            if !current.is_empty() {
                lines.push(LayoutLine {
                    text: std::mem::take(&mut current),
                    width: current_width,
                });
                current_width = 0;
            }
            lines.extend(break_word(word, width, font_size, font_weight, measurer));
            continue;
        }

        let next_width = if current.is_empty() {
            word_width
        } else {
            current_width + space_width + word_width
        };

        if next_width > width && !current.is_empty() {
            lines.push(LayoutLine {
                text: std::mem::take(&mut current),
                width: current_width,
            });
            current = word.to_string();
            current_width = word_width;
        } else {
            if !current.is_empty() {
                current.push(' ');
                current_width += space_width;
            }
            current.push_str(word);
            current_width = if current_width == 0 {
                word_width
            } else {
                current_width + word_width
            };
        }
    }

    if !current.is_empty() {
        lines.push(LayoutLine {
            text: current,
            width: current_width,
        });
    }

    lines
}

fn break_word(
    word: &str,
    max_width: usize,
    font_size: usize,
    font_weight: FontWeight,
    measurer: &dyn TextMeasurer,
) -> Vec<LayoutLine> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for ch in word.chars() {
        let glyph = ch.to_string();
        let glyph_width = measurer.measure_text(&glyph, font_size, font_weight).max(1);
        if current_width + glyph_width > max_width && !current.is_empty() {
            lines.push(LayoutLine {
                text: std::mem::take(&mut current),
                width: current_width,
            });
            current_width = 0;
        }
        current.push(ch);
        current_width += glyph_width;
    }

    if !current.is_empty() {
        lines.push(LayoutLine {
            text: current,
            width: current_width,
        });
    }

    lines
}

#[allow(dead_code)]
fn approximate_text_width(text: &str, font_size: usize, font_weight: FontWeight) -> usize {
    let weight_bonus = usize::from(font_weight == FontWeight::Bold);
    let unit = (font_size.max(12) + 1) / 2 + weight_bonus;
    text.chars()
        .map(|ch| match ch {
            'i' | 'l' | '!' | '|' | '\'' | '.' | ',' | ':' | ';' => unit / 2,
            'm' | 'w' | 'M' | 'W' | '@' | '#' | '%' => unit + unit / 2,
            ' ' => unit / 2,
            _ if ch.is_ascii_uppercase() => unit + unit / 4,
            _ => unit,
        })
        .sum::<usize>()
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::{approximate_text_width, build, wrap_text, LayoutKind, TextMeasurer};
    use crate::dom::Node;
    use crate::style::{self, Display, FontWeight};

    struct FixedWidthTextMeasurer;

    impl TextMeasurer for FixedWidthTextMeasurer {
        fn measure_text(&self, text: &str, _font_size: usize, _font_weight: FontWeight) -> usize {
            text.chars().count()
        }
    }

    #[test]
    fn wraps_text_to_requested_width() {
        let lines = wrap_text("alpha beta gamma", 10, 16, FontWeight::Normal, &FixedWidthTextMeasurer);
        assert_eq!(lines[0].text, "alpha beta");
        assert_eq!(lines[1].text, "gamma");
    }

    #[test]
    fn measures_wide_glyphs_larger_than_narrow_glyphs() {
        assert!(approximate_text_width("WWW", 16, FontWeight::Normal)
            > approximate_text_width("iii", 16, FontWeight::Normal));
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
