use crate::layout::{LayoutBox, LayoutKind};
use crate::style::FontWeight;

pub fn render(layout: &LayoutBox) -> String {
    let mut output = String::new();
    render_box(layout, 0, &mut output);
    output.trim_end().to_string()
}

fn render_box(layout: &LayoutBox, depth: usize, output: &mut String) {
    let indent = "  ".repeat(depth);

    match &layout.kind {
        LayoutKind::Document => {}
        LayoutKind::Block { tag_name } => {
            output.push_str(&format!(
                "{indent}<{}{}>\n",
                tag_name,
                style_suffix(layout)
            ));
        }
        LayoutKind::Text => {}
    }

    for line in &layout.lines {
        output.push_str(&format!("{indent}{}\n", line.text));
    }

    for child in &layout.children {
        render_box(
            child,
            depth + usize::from(!matches!(layout.kind, LayoutKind::Document)),
            output,
        );
    }
}

fn style_suffix(layout: &LayoutBox) -> String {
    let mut parts = Vec::new();

    if layout.style.color != "default" {
        parts.push(format!(" color={}", layout.style.color));
    }

    if let Some(background) = &layout.style.background_color {
        parts.push(format!(" bg={background}"));
    }

    if layout.style.font_weight == FontWeight::Bold {
        parts.push(" weight=bold".to_string());
    }

    parts.concat()
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::dom::Node;
    use crate::{layout::build, style};

    #[test]
    fn renders_basic_structure() {
        let doc = Node::document(vec![Node::element("p", vec![Node::text("hello world")])]);
        let stylesheet = style::collect_stylesheets(&doc);
        let styled = style::style_tree(&doc, &stylesheet);
        let rendered = render(&build(&styled, 200));
        assert!(rendered.contains("<p>"));
        assert!(rendered.contains("hello world"));
    }

    #[test]
    fn renders_style_annotations() {
        let doc = crate::html::parse(
            r#"<html><head><style>p { color: red; font-weight: bold; }</style></head><body><p>hello</p></body></html>"#,
        );
        let stylesheet = style::collect_stylesheets(&doc);
        let styled = style::style_tree(&doc, &stylesheet);
        let rendered = render(&build(&styled, 200));
        assert!(rendered.contains("<p color=red weight=bold>"));
    }
}
