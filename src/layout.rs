use crate::dom::{ElementData, NodeType};
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
    pub fragments: Vec<LayoutFragment>,
    pub width: usize,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutFragment {
    pub text: String,
    pub width: usize,
    pub color: String,
    pub font_weight: FontWeight,
    pub underline: bool,
    pub font_size: usize,
    pub line_height: LineHeight,
    pub href: Option<String>,
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
    pub underline: bool,
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
            if element.tag_name == "br" {
                return LayoutBox {
                    kind: LayoutKind::Block {
                        tag_name: element.tag_name.clone(),
                    },
                    lines: vec![LayoutLine {
                        fragments: vec![LayoutFragment {
                            text: "\n".to_string(),
                            width: 0,
                            color: to_layout_style(node).color.clone(),
                            font_weight: node.style.font_weight,
                            underline: node.style.underline,
                            font_size: node.style.font_size,
                            line_height: node.style.line_height,
                            href: None,
                        }],
                        width: 0,
                        height: fragment_line_height(&LayoutFragment {
                            text: "\n".to_string(),
                            width: 0,
                            color: to_layout_style(node).color.clone(),
                            font_weight: node.style.font_weight,
                            underline: node.style.underline,
                            font_size: node.style.font_size,
                            line_height: node.style.line_height,
                            href: None,
                        }),
                    }],
                    children: Vec::new(),
                    style: to_layout_style(node),
                };
            }
            let mut children = Vec::new();
            let mut lines = Vec::new();
            let box_overhead = node.style.margin.horizontal() + node.style.padding.horizontal();
            let available_width = node
                .style
                .width
                .unwrap_or_else(|| max_width.saturating_sub(box_overhead))
                .min(max_width.saturating_sub(node.style.margin.horizontal()))
                .max(1);

            let base_style = to_layout_style(node);
            let mut inline_fragments = Vec::new();

            for child in &node.children {
                if child.style.display == Display::None {
                    continue;
                }

                if is_inline_node(child) {
                    collect_inline_fragments(child, &base_style, None, &mut inline_fragments);
                    continue;
                }

                if !inline_fragments.is_empty() {
                    lines.extend(wrap_fragments(
                        &inline_fragments,
                        available_width,
                        measurer,
                    ));
                    inline_fragments.clear();
                }

                children.push(build_with_measurer(child, available_width, measurer));
            }

            if !inline_fragments.is_empty() {
                lines.extend(wrap_fragments(
                    &inline_fragments,
                    available_width,
                    measurer,
                ));
            }

            LayoutBox {
                kind: LayoutKind::Block {
                    tag_name: element.tag_name.clone(),
                },
                lines,
                children,
                style: base_style,
            }
        }
        NodeType::Text(text) => LayoutBox {
            kind: LayoutKind::Text,
            lines: wrap_fragments(
                &[LayoutFragment {
                    text: text.clone(),
                    width: 0,
                    color: "default".to_string(),
                    font_weight: FontWeight::Normal,
                    underline: false,
                    font_size: 16,
                    line_height: LineHeight::Normal,
                    href: None,
                }],
                max_width,
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

fn is_inline_node(node: &StyledNode) -> bool {
    match node.node_type {
        NodeType::Text(_) => true,
        NodeType::Element(_) => node.style.display == Display::Inline,
        NodeType::Document => false,
    }
}

fn to_layout_style(node: &StyledNode) -> LayoutStyle {
    LayoutStyle {
        color: node.style.color.clone(),
        background_color: node.style.background_color.clone(),
        font_weight: node.style.font_weight,
        underline: node.style.underline,
        font_size: node.style.font_size,
        line_height: node.style.line_height,
        margin: node.style.margin,
        padding: node.style.padding,
    }
}

fn merge_inline_style(parent: &LayoutStyle, child: &LayoutStyle) -> LayoutStyle {
    LayoutStyle {
        color: if child.color != "default" {
            child.color.clone()
        } else {
            parent.color.clone()
        },
        background_color: child
            .background_color
            .clone()
            .or_else(|| parent.background_color.clone()),
        font_weight: if child.font_weight == FontWeight::Bold {
            FontWeight::Bold
        } else {
            parent.font_weight
        },
        underline: child.underline || parent.underline,
        font_size: if child.font_size != 16 {
            child.font_size
        } else {
            parent.font_size
        },
        line_height: if child.line_height != LineHeight::Normal {
            child.line_height
        } else {
            parent.line_height
        },
        margin: EdgeSizes::zero(),
        padding: EdgeSizes::zero(),
    }
}

fn collect_inline_fragments(
    node: &StyledNode,
    inherited_style: &LayoutStyle,
    inherited_href: Option<String>,
    output: &mut Vec<LayoutFragment>,
) {
    match &node.node_type {
        NodeType::Text(text) => {
            if text.is_empty() {
                return;
            }
            output.push(LayoutFragment {
                text: text.clone(),
                width: 0,
                color: inherited_style.color.clone(),
                font_weight: inherited_style.font_weight,
                underline: inherited_style.underline,
                font_size: inherited_style.font_size,
                line_height: inherited_style.line_height,
                href: inherited_href,
            });
        }
        NodeType::Element(element) => {
            if element.tag_name == "br" {
                output.push(LayoutFragment {
                    text: "\n".to_string(),
                    width: 0,
                    color: inherited_style.color.clone(),
                    font_weight: inherited_style.font_weight,
                    underline: false,
                    font_size: inherited_style.font_size,
                    line_height: inherited_style.line_height,
                    href: None,
                });
                return;
            }
            let style = merge_inline_style(inherited_style, &to_layout_style(node));
            let href = if element.tag_name == "a" {
                attribute_value(element, "href")
                    .map(|value| value.to_string())
                    .or(inherited_href)
            } else {
                inherited_href
            };

            for child in &node.children {
                if child.style.display == Display::None {
                    continue;
                }

                if is_inline_node(child) {
                    collect_inline_fragments(child, &style, href.clone(), output);
                }
            }
        }
        NodeType::Document => {}
    }
}

fn wrap_fragments(
    fragments: &[LayoutFragment],
    max_width: usize,
    measurer: &dyn TextMeasurer,
) -> Vec<LayoutLine> {
    let width_limit = max_width.max(1);
    let mut lines = Vec::new();
    let mut current_fragments = Vec::new();
    let mut current_width = 0;
    let mut current_height = 0;
    let mut pending_space: Option<LayoutFragment> = None;

    for fragment in fragments {
        if fragment.text == "\n" {
            if !current_fragments.is_empty() {
                lines.push(LayoutLine {
                    fragments: std::mem::take(&mut current_fragments),
                    width: current_width,
                    height: current_height.max(1),
                });
            } else {
                lines.push(LayoutLine {
                    fragments: Vec::new(),
                    width: 0,
                    height: fragment_line_height(fragment),
                });
            }
            current_width = 0;
            current_height = 0;
            pending_space = None;
            continue;
        }
        for token in tokenize_fragment(fragment) {
            if token.text.trim().is_empty() {
                if !current_fragments.is_empty() {
                    pending_space = Some(with_measured_width(token, measurer));
                }
                continue;
            }

            let token = with_measured_width(token, measurer);

            if token.width > width_limit {
                if !current_fragments.is_empty() {
                    lines.push(LayoutLine {
                        fragments: std::mem::take(&mut current_fragments),
                        width: current_width,
                        height: current_height.max(1),
                    });
                    current_width = 0;
                    current_height = 0;
                }

                for broken in break_fragment(&token, width_limit, measurer) {
                    lines.push(LayoutLine {
                        width: broken.width,
                        height: fragment_line_height(&broken),
                        fragments: vec![broken],
                    });
                }
                pending_space = None;
                continue;
            }

            let pending_width = pending_space.as_ref().map(|space| space.width).unwrap_or(0);
            if !current_fragments.is_empty() && current_width + pending_width + token.width > width_limit {
                lines.push(LayoutLine {
                    fragments: std::mem::take(&mut current_fragments),
                    width: current_width,
                    height: current_height.max(1),
                });
                current_width = 0;
                current_height = 0;
                pending_space = None;
            }

            if let Some(space) = pending_space.take() {
                if !current_fragments.is_empty() {
                    push_or_merge_fragment(&mut current_fragments, space.clone());
                    current_width += space.width;
                    current_height = current_height.max(fragment_line_height(&space));
                }
            }

            push_or_merge_fragment(&mut current_fragments, token.clone());
            current_width += token.width;
            current_height = current_height.max(fragment_line_height(&token));
        }
    }

    if !current_fragments.is_empty() {
        lines.push(LayoutLine {
            fragments: current_fragments,
            width: current_width,
            height: current_height.max(1),
        });
    }

    lines
}

fn tokenize_fragment(fragment: &LayoutFragment) -> Vec<LayoutFragment> {
    let mut tokens = Vec::new();
    let mut buffer = String::new();
    let mut whitespace = None;

    for ch in fragment.text.chars() {
        let is_whitespace = ch.is_whitespace();
        match whitespace {
            Some(current) if current != is_whitespace => {
                tokens.push(fragment_with_text(fragment, std::mem::take(&mut buffer)));
                whitespace = Some(is_whitespace);
            }
            None => whitespace = Some(is_whitespace),
            _ => {}
        }
        buffer.push(if is_whitespace { ' ' } else { ch });
    }

    if !buffer.is_empty() {
        tokens.push(fragment_with_text(fragment, buffer));
    }

    tokens
}

fn fragment_with_text(template: &LayoutFragment, text: String) -> LayoutFragment {
    LayoutFragment {
        text,
        width: 0,
        color: template.color.clone(),
        font_weight: template.font_weight,
        underline: template.underline,
        font_size: template.font_size,
        line_height: template.line_height,
        href: template.href.clone(),
    }
}

fn with_measured_width(mut fragment: LayoutFragment, measurer: &dyn TextMeasurer) -> LayoutFragment {
    fragment.width = measurer.measure_text(&fragment.text, fragment.font_size, fragment.font_weight);
    fragment
}

fn break_fragment(
    fragment: &LayoutFragment,
    max_width: usize,
    measurer: &dyn TextMeasurer,
) -> Vec<LayoutFragment> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for ch in fragment.text.chars() {
        let piece = ch.to_string();
        let glyph_width = measurer.measure_text(&piece, fragment.font_size, fragment.font_weight).max(1);

        if current_width + glyph_width > max_width && !current.is_empty() {
            pieces.push(LayoutFragment {
                text: std::mem::take(&mut current),
                width: current_width,
                color: fragment.color.clone(),
                font_weight: fragment.font_weight,
                underline: fragment.underline,
                font_size: fragment.font_size,
                line_height: fragment.line_height,
                href: fragment.href.clone(),
            });
            current_width = 0;
        }

        current.push(ch);
        current_width += glyph_width;
    }

    if !current.is_empty() {
        pieces.push(LayoutFragment {
            text: current,
            width: current_width,
            color: fragment.color.clone(),
            font_weight: fragment.font_weight,
            underline: fragment.underline,
            font_size: fragment.font_size,
            line_height: fragment.line_height,
            href: fragment.href.clone(),
        });
    }

    pieces
}

fn push_or_merge_fragment(fragments: &mut Vec<LayoutFragment>, fragment: LayoutFragment) {
    if let Some(last) = fragments.last_mut() {
        if last.color == fragment.color
            && last.font_weight == fragment.font_weight
            && last.underline == fragment.underline
            && last.font_size == fragment.font_size
            && last.line_height == fragment.line_height
            && last.href == fragment.href
        {
            last.text.push_str(&fragment.text);
            last.width += fragment.width;
            return;
        }
    }
    fragments.push(fragment);
}

fn fragment_line_height(fragment: &LayoutFragment) -> u32 {
    match fragment.line_height {
        LineHeight::Normal => ((fragment.font_size as f32) * 1.35).round().max(18.0) as u32,
        LineHeight::RelativePercent(percent) => ((fragment.font_size as f32)
            * (percent as f32 / 100.0))
            .round()
            .max(18.0) as u32,
        LineHeight::Px(px) => px.max(18) as u32,
    }
}

fn attribute_value<'a>(element: &'a ElementData, name: &str) -> Option<&'a str> {
    element
        .attributes
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value.as_str()))
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
    use super::{
        approximate_text_width, build, build_with_measurer, wrap_fragments, LayoutKind,
        MonospaceTextMeasurer, TextMeasurer,
    };
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
        let fragments = vec![super::LayoutFragment {
            text: "alpha beta gamma".to_string(),
            width: 0,
            color: "default".to_string(),
            font_weight: FontWeight::Normal,
            underline: false,
            font_size: 16,
            line_height: crate::style::LineHeight::Normal,
            href: None,
        }];
        let lines = wrap_fragments(&fragments, 10, &FixedWidthTextMeasurer);
        assert_eq!(lines[0].fragments[0].text, "alpha beta");
        assert_eq!(lines[1].fragments[0].text, "gamma");
    }

    #[test]
    fn measures_wide_glyphs_larger_than_narrow_glyphs() {
        assert!(
            approximate_text_width("WWW", 16, FontWeight::Normal)
                > approximate_text_width("iii", 16, FontWeight::Normal)
        );
    }

    #[test]
    fn keeps_inline_links_in_same_paragraph() {
        let document = crate::html::parse(r#"<p>Hello <a href="https://example.com">world</a></p>"#);
        let stylesheet = style::collect_stylesheets(&document);
        let styled = style::style_tree(&document, &stylesheet);
        let layout = build_with_measurer(&styled, 240, &MonospaceTextMeasurer);
        assert_eq!(layout.children[0].lines.len(), 1);
        assert_eq!(layout.children[0].lines[0].fragments.len(), 2);
        assert_eq!(layout.children[0].lines[0].fragments[1].href.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn keeps_inline_strong_text_in_same_paragraph_flow() {
        let document =
            crate::html::parse(r#"<p>We <strong>keep bold text</strong> inline.</p>"#);
        let stylesheet = style::collect_stylesheets(&document);
        let styled = style::style_tree(&document, &stylesheet);
        let layout = build_with_measurer(&styled, 240, &MonospaceTextMeasurer);

        assert_eq!(layout.children[0].children.len(), 0);
        assert_eq!(layout.children[0].lines.len(), 1);
        assert_eq!(layout.children[0].lines[0].fragments.len(), 3);
        assert_eq!(layout.children[0].lines[0].fragments[1].font_weight, FontWeight::Bold);
        assert_eq!(layout.children[0].lines[0].fragments[1].text, "keep bold text");
    }

    #[test]
    fn breaks_lines_on_br_elements() {
        let document = crate::html::parse(r#"<p>hello<br>world</p>"#);
        let stylesheet = style::collect_stylesheets(&document);
        let styled = style::style_tree(&document, &stylesheet);
        let layout = build_with_measurer(&styled, 240, &MonospaceTextMeasurer);
        assert_eq!(layout.children[0].lines.len(), 2);
        assert_eq!(layout.children[0].lines[0].fragments[0].text, "hello");
        assert_eq!(layout.children[0].lines[1].fragments[0].text, "world");
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
