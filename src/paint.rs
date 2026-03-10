use crate::layout::LayoutBox;
use crate::style::FontWeight;

pub const CHAR_WIDTH: u32 = 8;
pub const LINE_HEIGHT: u32 = 18;
const H_PADDING: u32 = 16;
const V_PADDING: u32 = 16;
const INDENT_WIDTH: u32 = 20;
const BLOCK_SPACING: u32 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayList {
    pub width: u32,
    pub height: u32,
    pub commands: Vec<DisplayCommand>,
    pub background: Color,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayCommand {
    FillRect {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        color: Color,
    },
    DrawText {
        x: u32,
        y: u32,
        text: String,
        color: Color,
        font_weight: FontWeight,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 0xff }
    }
}

pub fn build_display_list(layout: &LayoutBox) -> DisplayList {
    let mut commands = Vec::new();
    let mut max_width = 0;
    let mut cursor_y = V_PADDING;

    paint_box(layout, 0, &mut cursor_y, &mut max_width, &mut commands);

    DisplayList {
        width: max_width.saturating_add(H_PADDING).max(160),
        height: cursor_y.saturating_add(V_PADDING).max(80),
        commands,
        background: Color::rgb(250, 248, 242),
    }
}

pub fn parse_color(value: &str) -> Color {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "default" | "black" => Color::rgb(30, 30, 30),
        "white" => Color::rgb(255, 255, 255),
        "red" => Color::rgb(204, 58, 58),
        "blue" => Color::rgb(51, 102, 204),
        "navy" => Color::rgb(27, 54, 93),
        "yellow" => Color::rgb(244, 208, 63),
        "beige" => Color::rgb(245, 245, 220),
        "tomato" => Color::rgb(255, 99, 71),
        "gray" | "grey" => Color::rgb(130, 130, 130),
        _ if normalized.starts_with('#') && normalized.len() == 7 => {
            let r = u8::from_str_radix(&normalized[1..3], 16).unwrap_or(30);
            let g = u8::from_str_radix(&normalized[3..5], 16).unwrap_or(30);
            let b = u8::from_str_radix(&normalized[5..7], 16).unwrap_or(30);
            Color::rgb(r, g, b)
        }
        _ => Color::rgb(30, 30, 30),
    }
}

fn paint_box(
    layout: &LayoutBox,
    depth: u32,
    cursor_y: &mut u32,
    max_width: &mut u32,
    commands: &mut Vec<DisplayCommand>,
) {
    let x = H_PADDING + depth * INDENT_WIDTH;
    let start_y = *cursor_y;

    if !layout.lines.is_empty() {
        let longest_line = layout
            .lines
            .iter()
            .map(|line| line.chars().count() as u32)
            .max()
            .unwrap_or(0);
        let text_width = (longest_line * CHAR_WIDTH).max(1);
        let text_height = layout.lines.len() as u32 * LINE_HEIGHT;

        if let Some(background) = &layout.style.background_color {
            commands.push(DisplayCommand::FillRect {
                x: x.saturating_sub(6),
                y: start_y.saturating_sub(2),
                width: text_width + 12,
                height: text_height + 4,
                color: parse_color(background),
            });
        }

        for line in &layout.lines {
            commands.push(DisplayCommand::DrawText {
                x,
                y: *cursor_y,
                text: line.clone(),
                color: parse_color(&layout.style.color),
                font_weight: layout.style.font_weight,
            });
            *cursor_y += LINE_HEIGHT;
        }

        *cursor_y += 4;
        *max_width = (*max_width).max(x + text_width + H_PADDING);
    }

    for child in &layout.children {
        paint_box(child, depth + 1, cursor_y, max_width, commands);
    }

    if !layout.lines.is_empty() || !layout.children.is_empty() {
        *cursor_y += BLOCK_SPACING;
    }
}

#[cfg(test)]
mod tests {
    use super::{build_display_list, parse_color, DisplayCommand};
    use crate::{html, layout, style};

    #[test]
    fn builds_fill_and_text_commands() {
        let document = html::parse(
            r#"
            <html>
              <head><style>.callout { background-color: beige; color: navy; }</style></head>
              <body><p class="callout">hello world</p></body>
            </html>
            "#,
        );
        let stylesheet = style::collect_stylesheets(&document);
        let styled = style::style_tree(&document, &stylesheet);
        let layout = layout::build(&styled, 40);
        let display_list = build_display_list(&layout);

        assert!(display_list.width >= 160);
        assert!(display_list.height >= 80);
        assert!(display_list.commands.iter().any(|command| {
            matches!(command, DisplayCommand::FillRect { .. })
        }));
        assert!(display_list.commands.iter().any(|command| {
            matches!(command, DisplayCommand::DrawText { text, .. } if text == "hello world")
        }));
    }

    #[test]
    fn parses_named_and_hex_colors() {
        assert_eq!(parse_color("navy"), super::Color::rgb(27, 54, 93));
        assert_eq!(parse_color("#ff0000"), super::Color::rgb(255, 0, 0));
    }
}
