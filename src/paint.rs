use crate::layout::{LayoutBox, LayoutKind};
use crate::style::FontWeight;
#[cfg(test)]
use crate::style::LineHeight;

pub const CHAR_WIDTH: u32 = 8;
#[cfg(test)]
pub const LINE_HEIGHT: u32 = 18;
const H_PADDING: u32 = 16;
const V_PADDING: u32 = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayList {
    pub width: u32,
    pub height: u32,
    pub commands: Vec<DisplayCommand>,
    pub link_regions: Vec<LinkRegion>,
    pub background: Color,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub target: String,
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
        width: u32,
        line_height: u32,
        color: Color,
        font_weight: FontWeight,
        underline: bool,
        font_size: u32,
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
    let mut link_regions = Vec::new();
    let mut max_width = 0;
    let mut cursor_y = V_PADDING;

    paint_box(
        layout,
        H_PADDING,
        &mut cursor_y,
        &mut max_width,
        &mut commands,
        &mut link_regions,
    );

    DisplayList {
        width: max_width.saturating_add(H_PADDING).max(160),
        height: cursor_y.saturating_add(V_PADDING).max(80),
        commands,
        link_regions,
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
    current_x: u32,
    cursor_y: &mut u32,
    max_width: &mut u32,
    commands: &mut Vec<DisplayCommand>,
    link_regions: &mut Vec<LinkRegion>,
) {
    *cursor_y = (*cursor_y).saturating_add(layout.style.margin.top as u32);
    let box_x = current_x.saturating_add(layout.style.margin.left as u32);
    let start_y = *cursor_y;
    let content_x = box_x.saturating_add(layout.style.padding.left as u32);
    let content_y = start_y.saturating_add(layout.style.padding.top as u32);
    let widest_line = layout
        .lines
        .iter()
        .map(|line| line.width as u32)
        .max()
        .unwrap_or(0);
    let text_width = widest_line.max(u32::from(!layout.lines.is_empty()));
    let text_height = layout.lines.iter().map(|line| line.height).sum::<u32>();
    let is_blockquote =
        matches!(&layout.kind, LayoutKind::Block { tag_name } if tag_name == "blockquote");
    let is_hr = matches!(&layout.kind, LayoutKind::Block { tag_name } if tag_name == "hr");

    if !layout.lines.is_empty() || !layout.children.is_empty() || is_hr {
        if let Some(background) = &layout.style.background_color {
            commands.push(DisplayCommand::FillRect {
                x: box_x,
                y: start_y,
                width: text_width
                    .saturating_add(layout.style.padding.left as u32)
                    .saturating_add(layout.style.padding.right as u32),
                height: text_height
                    .saturating_add(layout.style.padding.top as u32)
                    .saturating_add(layout.style.padding.bottom as u32),
                color: parse_color(background),
            });
        }

        if is_blockquote {
            commands.push(DisplayCommand::FillRect {
                x: box_x.saturating_add(6),
                y: start_y,
                width: 4,
                height: text_height
                    .saturating_add(layout.style.padding.top as u32)
                    .saturating_add(layout.style.padding.bottom as u32)
                    .max(24),
                color: Color::rgb(196, 188, 172),
            });
        }

        if is_hr {
            let rule_y = content_y.saturating_add(1);
            commands.push(DisplayCommand::FillRect {
                x: box_x,
                y: rule_y,
                width: text_width.max(32),
                height: 2,
                color: Color::rgb(210, 205, 194),
            });
        }

        *cursor_y = content_y;
        for (line_index, line) in layout.lines.iter().enumerate() {
            let mut cursor_x = content_x;
            if matches!(&layout.kind, LayoutKind::Block { tag_name } if tag_name == "li")
                && line_index == 0
            {
                let bullet_x = box_x.saturating_add(8);
                let bullet_width = (layout.style.font_size as u32 / 2).max(8);
                commands.push(DisplayCommand::DrawText {
                    x: bullet_x,
                    y: *cursor_y,
                    text: "\u{2022}".to_string(),
                    width: bullet_width,
                    line_height: line.height,
                    color: parse_color(&layout.style.color),
                    font_weight: layout.style.font_weight,
                    underline: false,
                    font_size: layout.style.font_size as u32,
                });
            }
            for fragment in &line.fragments {
                if let Some(background) = &fragment.background_color {
                    commands.push(DisplayCommand::FillRect {
                        x: cursor_x,
                        y: *cursor_y,
                        width: fragment.width as u32,
                        height: line.height,
                        color: parse_color(background),
                    });
                }

                commands.push(DisplayCommand::DrawText {
                    x: cursor_x,
                    y: *cursor_y,
                    text: fragment.text.clone(),
                    width: fragment.width as u32,
                    line_height: line.height,
                    color: parse_color(&fragment.color),
                    font_weight: fragment.font_weight,
                    underline: fragment.underline,
                    font_size: fragment.font_size as u32,
                });

                if let Some(target) = &fragment.href {
                    link_regions.push(LinkRegion {
                        x: cursor_x,
                        y: *cursor_y,
                        width: fragment.width as u32,
                        height: line.height,
                        target: target.clone(),
                    });
                }

                cursor_x = cursor_x.saturating_add(fragment.width as u32);
            }
            *cursor_y += line.height;
        }
    } else {
        *cursor_y = content_y;
    }

    for child in &layout.children {
        paint_box(
            child,
            content_x,
            cursor_y,
            max_width,
            commands,
            link_regions,
        );
    }

    if !layout.lines.is_empty() || !layout.children.is_empty() || is_hr {
        *cursor_y = (*cursor_y).saturating_add(layout.style.padding.bottom as u32);
        *cursor_y = (*cursor_y).saturating_add(layout.style.margin.bottom as u32);
    }

    let painted_width = box_x
        .saturating_add(text_width)
        .saturating_add(layout.style.padding.left as u32)
        .saturating_add(layout.style.padding.right as u32);
    *max_width = (*max_width).max(painted_width.saturating_add(H_PADDING));
}

#[cfg(test)]
fn line_height_for_font(font_size: usize, line_height: LineHeight) -> u32 {
    match line_height {
        LineHeight::Normal => ((font_size as f32) * 1.35).round().max(LINE_HEIGHT as f32) as u32,
        LineHeight::RelativePercent(percent) => ((font_size as f32) * (percent as f32 / 100.0))
            .round()
            .max(LINE_HEIGHT as f32) as u32,
        LineHeight::Px(px) => px.max(LINE_HEIGHT as usize) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_display_list, line_height_for_font, parse_color, DisplayCommand, LINE_HEIGHT,
    };
    use crate::style::LineHeight;
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
        let layout = layout::build(&styled, 200);
        let display_list = build_display_list(&layout);

        assert!(display_list.width >= 160);
        assert!(display_list.height >= 80);
        assert!(display_list
            .commands
            .iter()
            .any(|command| { matches!(command, DisplayCommand::FillRect { .. }) }));
        assert!(display_list.commands.iter().any(|command| {
            matches!(command, DisplayCommand::DrawText { text, .. } if text == "hello world")
        }));
    }

    #[test]
    fn scales_metrics_with_font_size() {
        assert!(line_height_for_font(24, LineHeight::Normal) > LINE_HEIGHT);
        assert!(line_height_for_font(16, LineHeight::RelativePercent(170)) > LINE_HEIGHT);
    }

    #[test]
    fn parses_named_and_hex_colors() {
        assert_eq!(parse_color("navy"), super::Color::rgb(27, 54, 93));
        assert_eq!(parse_color("#ff0000"), super::Color::rgb(255, 0, 0));
    }

    #[test]
    fn paints_semantic_document_accents() {
        let document = html::parse(
            r#"
            <html>
              <body>
                <blockquote><p>quote</p></blockquote>
                <p>use <code>cargo run</code></p>
                <hr>
              </body>
            </html>
            "#,
        );
        let stylesheet = style::collect_stylesheets(&document);
        let styled = style::style_tree(&document, &stylesheet);
        let layout = layout::build(&styled, 320);
        let display_list = build_display_list(&layout);

        let fill_rects = display_list
            .commands
            .iter()
            .filter(|command| matches!(command, DisplayCommand::FillRect { .. }))
            .count();
        assert!(fill_rects >= 3);
        assert!(display_list.commands.iter().any(|command| {
            matches!(command, DisplayCommand::DrawText { text, .. } if text.contains("cargo run"))
        }));
    }
}
