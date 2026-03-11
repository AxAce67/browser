use arboard::Clipboard;
use crate::html;
use crate::layout;
use crate::layout::TextMeasurer;
use crate::paint::{Color, DisplayCommand, DisplayList, CHAR_WIDTH};
use crate::source;
use crate::style;
use font8x8::UnicodeFonts;
use fontdb::{Database, Family, Query, Style, Weight};
use fontdue::{Font, FontSettings};
use fontdue::layout::{CoordinateSystem, GlyphPosition, Layout, LayoutSettings, TextStyle};
use pixels::{Pixels, SurfaceTexture};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

const CHROME_HEIGHT: u32 = 72;
const TOP_MARGIN: u32 = 12;
const SIDE_MARGIN: u32 = 32;
const SCROLLBAR_WIDTH: u32 = 10;
const ADDRESS_BAR_HEIGHT: u32 = 32;
const ADDRESS_BAR_PADDING: u32 = 12;
const DEFAULT_VIEWPORT_WIDTH: u32 = 960;
const DEFAULT_VIEWPORT_HEIGHT: u32 = 720;
const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(530);

pub fn run(initial_source: &str) -> Result<(), String> {
    let text_rasterizer = TextRasterizer::load();
    let initial_page = load_page(
        initial_source,
        DEFAULT_VIEWPORT_WIDTH,
        DEFAULT_VIEWPORT_HEIGHT,
        1.0,
        &text_rasterizer,
    )?;
    let event_loop =
        EventLoop::new().map_err(|err| format!("failed to create event loop: {err}"))?;
    let mut app = GuiApp::new(initial_page, initial_source, text_rasterizer);
    event_loop
        .run_app(&mut app)
        .map_err(|err| format!("failed to run GUI app: {err}"))
}

struct GuiApp {
    page: PageData,
    current_source: String,
    address_input: String,
    address_focus: bool,
    address_cursor: usize,
    address_selection_anchor: Option<usize>,
    address_drag_active: bool,
    preedit_text: String,
    status_message: Option<String>,
    title: String,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    viewport_width: u32,
    viewport_height: u32,
    content_scale: u32,
    window_scale_factor: f64,
    scroll_y: u32,
    modifiers: ModifiersState,
    cursor_position: Option<PhysicalPosition<f64>>,
    text_rasterizer: TextRasterizer,
    clipboard: Option<Clipboard>,
    address_blink_started_at: Instant,
}

struct AddressTextLayout {
    display_text: String,
    font_size: usize,
    text_x: i32,
    text_y: i32,
    prefix_len_bytes: usize,
    address_input_len_bytes: usize,
    glyphs: Option<Vec<GlyphPosition>>,
}

impl GuiApp {
    fn new(page: PageData, initial_source: &str, text_rasterizer: TextRasterizer) -> Self {
        let viewport_width = DEFAULT_VIEWPORT_WIDTH;
        let viewport_height = DEFAULT_VIEWPORT_HEIGHT;
        Self {
            title: initial_source.to_string(),
            page,
            current_source: initial_source.to_string(),
            address_input: initial_source.to_string(),
            address_focus: false,
            address_cursor: initial_source.chars().count(),
            address_selection_anchor: None,
            address_drag_active: false,
            preedit_text: String::new(),
            status_message: Some(
                "Click or Cmd/Ctrl+L to edit address, Enter to load".to_string(),
            ),
            window: None,
            pixels: None,
            viewport_width,
            viewport_height,
            content_scale: content_scale_for_viewport(viewport_width, viewport_height, 1.0),
            window_scale_factor: 1.0,
            scroll_y: 0,
            modifiers: ModifiersState::empty(),
            cursor_position: None,
            text_rasterizer,
            clipboard: Clipboard::new().ok(),
            address_blink_started_at: Instant::now(),
        }
    }

    fn draw(&mut self) -> Result<(), String> {
        let address_selection = self.address_selection_range();
        let caret_visible = self.address_caret_visible();
        let Some(pixels) = self.pixels.as_mut() else {
            return Ok(());
        };

        let frame = pixels.frame_mut();
        clear_frame(frame, Color::rgb(240, 236, 228));
        draw_chrome(
            &self.text_rasterizer,
            frame,
            self.viewport_width,
            self.viewport_height,
            self.content_scale,
            &self.address_input,
            &self.preedit_text,
            self.address_focus,
            self.address_cursor,
            address_selection,
            caret_visible,
            self.status_message.as_deref(),
        );
        rasterize(
            &self.text_rasterizer,
            &self.page.display_list,
            frame,
            self.viewport_width,
            self.viewport_height,
            self.scroll_y,
            self.content_scale,
        );
        pixels
            .render()
            .map_err(|err| format!("failed to render frame: {err}"))?;

        if self.address_focus {
            self.request_redraw();
        }

        Ok(())
    }

    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn set_address_focus(&mut self, focused: bool) {
        self.address_focus = focused;
        self.reset_address_blink();
        if !focused {
            self.address_selection_anchor = None;
            self.address_drag_active = false;
        }
        self.preedit_text.clear();
        if let Some(window) = self.window.as_ref() {
            window.set_ime_allowed(focused);
        }
    }

    fn reset_address_blink(&mut self) {
        self.address_blink_started_at = Instant::now();
    }

    fn address_caret_visible(&self) -> bool {
        !self.address_focus
            || ((self.address_blink_started_at.elapsed().as_millis()
                / CARET_BLINK_INTERVAL.as_millis())
                % 2
                == 0)
    }

    fn select_all_address(&mut self) {
        self.address_selection_anchor = Some(0);
        self.address_cursor = self.address_input.chars().count();
    }

    fn address_selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.address_selection_anchor?;
        if anchor == self.address_cursor {
            None
        } else {
            Some((anchor.min(self.address_cursor), anchor.max(self.address_cursor)))
        }
    }

    fn clear_address_selection(&mut self) {
        self.address_selection_anchor = None;
    }

    fn address_char_count(&self) -> usize {
        self.address_input.chars().count()
    }

    fn selected_address_text(&self) -> Option<String> {
        let (start, end) = self.address_selection_range()?;
        Some(address_slice(&self.address_input, start, end).to_string())
    }

    fn replace_address_selection(&mut self, text: &str) {
        let (start, end) = self
            .address_selection_range()
            .unwrap_or((self.address_cursor, self.address_cursor));
        let start_byte = address_byte_index(&self.address_input, start);
        let end_byte = address_byte_index(&self.address_input, end);
        self.address_input.replace_range(start_byte..end_byte, text);
        self.address_cursor = start + text.chars().count();
        self.clear_address_selection();
        self.reset_address_blink();
    }

    fn insert_address_text(&mut self, text: &str) {
        self.replace_address_selection(text);
    }

    fn delete_address_backward(&mut self) {
        if self.address_selection_range().is_some() {
            self.replace_address_selection("");
            return;
        }
        if self.address_cursor == 0 {
            return;
        }
        let start = self.address_cursor - 1;
        let start_byte = address_byte_index(&self.address_input, start);
        let end_byte = address_byte_index(&self.address_input, self.address_cursor);
        self.address_input.replace_range(start_byte..end_byte, "");
        self.address_cursor = start;
        self.reset_address_blink();
    }

    fn delete_address_forward(&mut self) {
        if self.address_selection_range().is_some() {
            self.replace_address_selection("");
            return;
        }
        let char_count = self.address_char_count();
        if self.address_cursor >= char_count {
            return;
        }
        let start_byte = address_byte_index(&self.address_input, self.address_cursor);
        let end_byte = address_byte_index(&self.address_input, self.address_cursor + 1);
        self.address_input.replace_range(start_byte..end_byte, "");
        self.reset_address_blink();
    }

    fn move_address_cursor(&mut self, next_cursor: usize, extend_selection: bool) {
        let clamped = next_cursor.min(self.address_char_count());
        if extend_selection {
            if self.address_selection_anchor.is_none() {
                self.address_selection_anchor = Some(self.address_cursor);
            }
        } else {
            self.clear_address_selection();
        }
        self.address_cursor = clamped;
        if !extend_selection && self.address_selection_anchor == Some(self.address_cursor) {
            self.clear_address_selection();
        }
        self.reset_address_blink();
    }

    fn copy_address_to_clipboard(&mut self) -> Result<(), String> {
        let text = self
            .selected_address_text()
            .unwrap_or_else(|| self.address_input.clone());
        let Some(clipboard) = self.clipboard.as_mut() else {
            return Err("clipboard is unavailable".to_string());
        };
        clipboard
            .set_text(text)
            .map_err(|err| format!("failed to copy address: {err}"))
    }

    fn paste_address_from_clipboard(&mut self) -> Result<(), String> {
        let Some(clipboard) = self.clipboard.as_mut() else {
            return Err("clipboard is unavailable".to_string());
        };
        let text = clipboard
            .get_text()
            .map_err(|err| format!("failed to read clipboard: {err}"))?;
        let sanitized = sanitize_clipboard_text(&text);
        if sanitized.is_empty() {
            return Ok(());
        }
        self.insert_address_text(&sanitized);
        Ok(())
    }

    fn handle_address_shortcut(&mut self, text: &str) -> bool {
        if !(self.modifiers.control_key() || self.modifiers.super_key()) {
            return false;
        }

        if text.eq_ignore_ascii_case("a") {
            self.select_all_address();
            self.status_message = Some("Address selected".to_string());
            return true;
        }

        if text.eq_ignore_ascii_case("c") {
            self.status_message = Some(match self.copy_address_to_clipboard() {
                Ok(()) => "Address copied".to_string(),
                Err(err) => err,
            });
            return true;
        }

        if text.eq_ignore_ascii_case("x") {
            let copied = self.copy_address_to_clipboard();
            if self.address_selection_range().is_some() {
                self.replace_address_selection("");
            }
            self.status_message = Some(match copied {
                Ok(()) => "Address cut".to_string(),
                Err(err) => err,
            });
            return true;
        }

        if text.eq_ignore_ascii_case("v") {
            self.status_message = Some(match self.paste_address_from_clipboard() {
                Ok(()) => "Address pasted".to_string(),
                Err(err) => err,
            });
            return true;
        }

        false
    }

    fn relayout_current_page(&mut self) {
        self.content_scale = content_scale_for_viewport(
            self.viewport_width,
            self.viewport_height,
            self.window_scale_factor,
        );
        self.page.display_list = build_page_display_list(
            &self.page.html_input,
            self.viewport_width,
            self.viewport_height,
            self.window_scale_factor,
            &self.text_rasterizer,
        );
        self.scroll_y = clamp_scroll(
            self.scroll_y,
            self.page.display_list.height,
            self.content_viewport_height(),
        );
    }

    fn navigate(&mut self, source_input: String) {
        match load_page(
            &source_input,
            self.viewport_width,
            self.viewport_height,
            self.window_scale_factor,
            &self.text_rasterizer,
        ) {
            Ok(page) => {
                let cursor = source_input.chars().count();
                self.page = page;
                self.current_source = source_input.clone();
                self.address_input = source_input.clone();
                self.address_cursor = cursor;
                self.clear_address_selection();
                self.title = source_input;
                self.scroll_y = 0;
                self.status_message = Some("Page loaded".to_string());
                self.set_address_focus(false);
                if let Some(window) = self.window.as_ref() {
                    window.set_title(&format!("Toy Browser - {}", self.title));
                }
            }
            Err(err) => {
                self.status_message = Some(err);
            }
        }
        self.request_redraw();
    }

    fn content_viewport_height(&self) -> u32 {
        let available_pixels = self
            .viewport_height
            .saturating_sub(chrome_height(self.content_scale) + top_margin(self.content_scale) * 2)
            .max(self.content_scale);
        (available_pixels / self.content_scale).max(1)
    }

    fn handle_primary_click(&mut self, position: PhysicalPosition<f64>) {
        let focused = address_bar_hit_test(
            position.x,
            position.y,
            self.viewport_width,
            self.content_scale,
        );
        if focused {
            self.set_address_focus(true);
            let caret = address_bar_cursor_from_position(
                &self.text_rasterizer,
                &self.address_input,
                position.x,
                self.content_scale,
            );
            self.address_cursor = caret;
            self.clear_address_selection();
            self.address_drag_active = true;
            self.status_message = Some("Editing address".to_string());
        } else if let Some(target) = link_hit_test(
            &self.page.display_list,
            position.x,
            position.y,
            self.viewport_width,
            self.content_scale,
            self.scroll_y,
        ) {
            let resolved = source::resolve_reference(&self.current_source, &target);
            self.navigate(resolved);
            return;
        } else if self.address_focus {
            self.set_address_focus(false);
            self.status_message = Some("Address bar unfocused".to_string());
        }
        self.request_redraw();
    }

    fn update_address_selection_drag(&mut self, position: PhysicalPosition<f64>) {
        if !self.address_focus || !self.address_drag_active {
            return;
        }
        let caret = address_bar_cursor_from_position(
            &self.text_rasterizer,
            &self.address_input,
            position.x,
            self.content_scale,
        );
        if self.address_selection_anchor.is_none() {
            self.address_selection_anchor = Some(self.address_cursor);
        }
        self.address_cursor = caret;
        if self.address_selection_anchor == Some(self.address_cursor) {
            self.clear_address_selection();
        }
        self.reset_address_blink();
        self.request_redraw();
    }
}

struct TextRasterizer {
    regular: Option<Font>,
    bold: Option<Font>,
}

impl TextRasterizer {
    fn load() -> Self {
        let mut database = Database::new();
        database.load_system_fonts();

        Self {
            regular: load_font(&database, Weight::NORMAL),
            bold: load_font(&database, Weight::BOLD)
                .or_else(|| load_font(&database, Weight::NORMAL)),
        }
    }

    fn draw_text(
        &self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        x: i32,
        y: i32,
        text: &str,
        color: Color,
        font_weight: crate::style::FontWeight,
        font_size: f32,
        clip_top: Option<i32>,
        clip_bottom: Option<i32>,
    ) {
        let font = match font_weight {
            crate::style::FontWeight::Bold => self.bold.as_ref().or(self.regular.as_ref()),
            crate::style::FontWeight::Normal => self.regular.as_ref().or(self.bold.as_ref()),
        };

        if let Some(font) = font {
            draw_text_fontdue(
                frame,
                width,
                height,
                x,
                y,
                text,
                color,
                font,
                font_size,
                clip_top,
                clip_bottom,
            );
        } else {
            let bitmap_scale = if font_size >= 24.0 { 2 } else { 1 };
            draw_text_bitmap(
                frame,
                width,
                height,
                x,
                y,
                text,
                color,
                font_weight,
                bitmap_scale,
                clip_top,
                clip_bottom,
            );
        }
    }

    fn font_for_weight(&self, font_weight: crate::style::FontWeight) -> Option<&Font> {
        match font_weight {
            crate::style::FontWeight::Bold => self.bold.as_ref().or(self.regular.as_ref()),
            crate::style::FontWeight::Normal => self.regular.as_ref().or(self.bold.as_ref()),
        }
    }

    fn measure_text_width(
        &self,
        text: &str,
        font_weight: crate::style::FontWeight,
        font_size: usize,
    ) -> usize {
        let font_size = font_size.max(1) as f32;
        if let Some(font) = self.font_for_weight(font_weight) {
            text.chars()
                .map(|ch| font.metrics(ch, font_size).advance_width.max(0.0))
                .sum::<f32>()
                .ceil() as usize
        } else {
            text.chars().count() * CHAR_WIDTH as usize
        }
    }

    fn exact_text_width(
        &self,
        text: &str,
        font_weight: crate::style::FontWeight,
        font_size: usize,
    ) -> i32 {
        let font_size = font_size.max(1) as f32;
        let Some(font) = self.font_for_weight(font_weight) else {
            return self.measure_text_width(text, font_weight, font_size as usize) as i32;
        };
        let fonts = [font];
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: 0.0,
            y: 0.0,
            line_height: 1.0,
            ..LayoutSettings::default()
        });
        layout.append(&fonts, &TextStyle::new(text, font_size, 0));

        layout
            .glyphs()
            .iter()
            .map(|glyph| glyph.x + glyph.width as f32)
            .fold(0.0, f32::max)
            .ceil() as i32
    }

    fn layout_text_glyphs(
        &self,
        text: &str,
        font_weight: crate::style::FontWeight,
        font_size: usize,
        x: i32,
        y: i32,
    ) -> Option<Vec<GlyphPosition>> {
        let font = self.font_for_weight(font_weight)?;
        Some(layout_text_glyphs(font, text, font_size as f32, x, y))
    }

    fn draw_positioned_text(
        &self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        glyphs: &[GlyphPosition],
        color: Color,
        font_weight: crate::style::FontWeight,
        clip_top: Option<i32>,
        clip_bottom: Option<i32>,
    ) -> bool {
        let Some(font) = self.font_for_weight(font_weight) else {
            return false;
        };

        draw_positioned_glyphs(frame, width, height, glyphs, color, font, clip_top, clip_bottom);
        true
    }
}

impl TextMeasurer for TextRasterizer {
    fn measure_text(&self, text: &str, font_size: usize, font_weight: crate::style::FontWeight) -> usize {
        self.measure_text_width(text, font_weight, font_size)
    }
}

impl ApplicationHandler for GuiApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = Window::default_attributes()
            .with_title(format!("Toy Browser - {}", self.title))
            .with_inner_size(LogicalSize::new(
                self.viewport_width as f64,
                self.viewport_height as f64,
            ))
            .with_min_inner_size(LogicalSize::new(480.0, 360.0));

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("failed to create window: {err}");
                event_loop.exit();
                return;
            }
        };

        let window_size = window.inner_size();
        self.window_scale_factor = window.scale_factor();
        self.viewport_width = window_size.width.max(1);
        self.viewport_height = window_size.height.max(1);
        self.relayout_current_page();

        let surface_texture =
            SurfaceTexture::new(window_size.width, window_size.height, window.clone());
        let pixels = match Pixels::new(self.viewport_width, self.viewport_height, surface_texture) {
            Ok(pixels) => pixels,
            Err(err) => {
                eprintln!("failed to create pixel buffer: {err}");
                event_loop.exit();
                return;
            }
        };

        window.set_ime_allowed(false);
        window.request_redraw();
        self.pixels = Some(pixels);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = Some(position);
                self.update_address_selection_drag(position);
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Pressed,
                ..
            } => {
                if let Some(position) = self.cursor_position {
                    self.handle_primary_click(position);
                }
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Released,
                ..
            } => {
                self.address_drag_active = false;
            }
            WindowEvent::Ime(event) => match event {
                Ime::Commit(text) if self.address_focus => {
                    self.insert_address_text(&text);
                    self.preedit_text.clear();
                    self.request_redraw();
                }
                Ime::Preedit(text, _) if self.address_focus => {
                    self.preedit_text = text;
                    self.request_redraw();
                }
                Ime::Disabled | Ime::Enabled | Ime::Commit(_) | Ime::Preedit(_, _) => {}
            },
            WindowEvent::RedrawRequested => {
                if let Err(err) = self.draw() {
                    eprintln!("{err}");
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                self.viewport_width = size.width.max(1);
                self.viewport_height = size.height.max(1);
                self.relayout_current_page();

                if let Some(pixels) = self.pixels.as_mut() {
                    if let Err(err) = pixels.resize_surface(size.width, size.height) {
                        eprintln!("failed to resize surface: {err}");
                        event_loop.exit();
                        return;
                    }
                    if let Err(err) = pixels.resize_buffer(self.viewport_width, self.viewport_height)
                    {
                        eprintln!("failed to resize buffer: {err}");
                        event_loop.exit();
                        return;
                    }
                }
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.window_scale_factor = scale_factor;
                self.relayout_current_page();
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } if !self.address_focus => {
                let scroll_delta = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => (-(lines * 18.0)).round() as i32,
                    MouseScrollDelta::PixelDelta(position) => {
                        -(position.y.round() as i32) / self.content_scale.max(1) as i32
                    }
                };
                self.scroll_y = apply_scroll_delta(
                    self.scroll_y,
                    scroll_delta,
                    self.page.display_list.height,
                    self.content_viewport_height(),
                );
                self.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if matches!(event.logical_key.as_ref(), Key::Character(ch) if ch.eq_ignore_ascii_case("l"))
                    && (self.modifiers.control_key() || self.modifiers.super_key())
                {
                    self.set_address_focus(true);
                    self.address_input = self.current_source.clone();
                    self.address_cursor = self.address_char_count();
                    self.select_all_address();
                    self.status_message = Some("Editing address".to_string());
                    self.request_redraw();
                    return;
                }

                if self.address_focus {
                    if let Key::Character(text) = event.logical_key.as_ref() {
                        if self.handle_address_shortcut(text) {
                            self.request_redraw();
                            return;
                        }
                    }

                    match event.logical_key.as_ref() {
                        Key::Named(NamedKey::Enter) => {
                            let requested = self.address_input.trim().to_string();
                            if !requested.is_empty() {
                                self.navigate(requested);
                            }
                        }
                        Key::Named(NamedKey::Escape) => {
                            self.set_address_focus(false);
                            self.address_input = self.current_source.clone();
                            self.address_cursor = self.address_char_count();
                            self.clear_address_selection();
                            self.status_message = Some("Address edit cancelled".to_string());
                            self.request_redraw();
                        }
                        Key::Named(NamedKey::Backspace) => {
                            self.delete_address_backward();
                            self.request_redraw();
                        }
                        Key::Named(NamedKey::Delete) => {
                            self.delete_address_forward();
                            self.request_redraw();
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            let next = self.address_cursor.saturating_sub(1);
                            self.move_address_cursor(next, self.modifiers.shift_key());
                            self.request_redraw();
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            self.move_address_cursor(
                                self.address_cursor.saturating_add(1),
                                self.modifiers.shift_key(),
                            );
                            self.request_redraw();
                        }
                        Key::Named(NamedKey::Home) => {
                            self.move_address_cursor(0, self.modifiers.shift_key());
                            self.request_redraw();
                        }
                        Key::Named(NamedKey::End) => {
                            self.move_address_cursor(
                                self.address_char_count(),
                                self.modifiers.shift_key(),
                            );
                            self.request_redraw();
                        }
                        Key::Character(text)
                            if !(self.modifiers.control_key() || self.modifiers.super_key()) =>
                        {
                            self.insert_address_text(text);
                            self.request_redraw();
                        }
                        _ => {}
                    }
                    return;
                }

                let next = match event.logical_key.as_ref() {
                    Key::Named(NamedKey::ArrowDown) => Some(apply_scroll_delta(
                        self.scroll_y,
                        24,
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Named(NamedKey::ArrowUp) => Some(apply_scroll_delta(
                        self.scroll_y,
                        -24,
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Named(NamedKey::PageDown) => Some(apply_scroll_delta(
                        self.scroll_y,
                        self.content_viewport_height() as i32 - 8,
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Named(NamedKey::PageUp) => Some(apply_scroll_delta(
                        self.scroll_y,
                        -(self.content_viewport_height() as i32 - 8),
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Named(NamedKey::Home) => Some(0),
                    Key::Named(NamedKey::End) => Some(clamp_scroll(
                        u32::MAX,
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Character(ch) if ch.eq_ignore_ascii_case("r") => {
                        self.navigate(self.current_source.clone());
                        None
                    }
                    _ => None,
                };

                if let Some(next_scroll) = next {
                    self.scroll_y = next_scroll;
                    self.request_redraw();
                }
            }
            _ => {}
        }
    }
}

struct PageData {
    html_input: String,
    display_list: DisplayList,
}

fn load_page(
    source_input: &str,
    viewport_width: u32,
    viewport_height: u32,
    scale_factor: f64,
    text_measurer: &dyn TextMeasurer,
) -> Result<PageData, String> {
    let (html_input, _) = source::load_html(Some(source_input))?;
    let display_list = build_page_display_list(
        &html_input,
        viewport_width,
        viewport_height,
        scale_factor,
        text_measurer,
    );
    Ok(PageData {
        html_input,
        display_list,
    })
}

fn build_page_display_list(
    html_input: &str,
    viewport_width: u32,
    viewport_height: u32,
    scale_factor: f64,
    text_measurer: &dyn TextMeasurer,
) -> DisplayList {
    let content_scale = content_scale_for_viewport(viewport_width, viewport_height, scale_factor);
    let available_width = viewport_width
        .saturating_sub(SIDE_MARGIN * 2 + SCROLLBAR_WIDTH + 18)
        .max(420);
    let layout_width = (available_width / content_scale.max(1)) as usize;

    let document = html::parse(html_input);
    let stylesheet = style::collect_stylesheets(&document);
    let styled = style::style_tree(&document, &stylesheet);
    let layout = layout::build_with_measurer(&styled, layout_width.max(180), text_measurer);
    crate::paint::build_display_list(&layout)
}

fn content_scale_for_viewport(viewport_width: u32, viewport_height: u32, scale_factor: f64) -> u32 {
    let device_scale = scale_factor.round().clamp(1.0, 4.0) as u32;
    let logical_width = (viewport_width as f64 / scale_factor.max(1.0)).round() as u32;
    let logical_height = (viewport_height as f64 / scale_factor.max(1.0)).round() as u32;
    let large_viewport_scale = if logical_width >= 2200 || logical_height >= 1500 {
        2
    } else {
        1
    };
    device_scale.saturating_mul(large_viewport_scale)
}

fn load_font(database: &Database, weight: Weight) -> Option<Font> {
    let query = Query {
        families: &[Family::SansSerif],
        weight,
        style: Style::Normal,
        ..Query::default()
    };
    let id = database.query(&query)?;
    let font = database.with_face_data(id, |data, face_index| {
        Font::from_bytes(
            data.to_vec(),
            FontSettings {
                collection_index: face_index,
                ..FontSettings::default()
            },
        )
    })?;
    font.ok()
}

fn clear_frame(frame: &mut [u8], color: Color) {
    for pixel in frame.chunks_exact_mut(4) {
        pixel[0] = color.r;
        pixel[1] = color.g;
        pixel[2] = color.b;
        pixel[3] = color.a;
    }
}

fn draw_chrome(
    text_rasterizer: &TextRasterizer,
    frame: &mut [u8],
    width: u32,
    height: u32,
    scale: u32,
    address_input: &str,
    preedit_text: &str,
    address_focus: bool,
    address_cursor: usize,
    address_selection: Option<(usize, usize)>,
    caret_visible: bool,
    status_message: Option<&str>,
) {
    draw_rect(
        frame,
        width,
        height,
        0,
        0,
        width as i32,
        chrome_height(scale) as i32,
        Color::rgb(232, 228, 220),
    );
    draw_rect(
        frame,
        width,
        height,
        address_bar_padding(scale) as i32,
        (12 * scale) as i32,
        width.saturating_sub(address_bar_padding(scale) * 2) as i32,
        address_bar_height(scale) as i32,
        if address_focus {
            Color::rgb(255, 255, 255)
        } else {
            Color::rgb(246, 243, 236)
        },
    );

    let layout = build_address_text_layout(
        text_rasterizer,
        address_input,
        preedit_text,
        address_focus,
        scale,
    );

    if let Some((start, end)) = address_selection {
        let selection_x = layout.x_for_char_index(text_rasterizer, start);
        let selection_width = layout
            .x_for_char_index(text_rasterizer, end)
            .saturating_sub(selection_x);
        draw_rect(
            frame,
            width,
            height,
            selection_x,
            (14 * scale) as i32,
            selection_width.max(scale as i32),
            address_bar_height(scale).saturating_sub(4 * scale) as i32,
            Color::rgb(205, 222, 246),
        );
    }

    if let Some(glyphs) = &layout.glyphs {
        if !text_rasterizer.draw_positioned_text(
            frame,
            width,
            height,
            glyphs,
            Color::rgb(32, 35, 40),
            crate::style::FontWeight::Normal,
            None,
            None,
        ) {
            text_rasterizer.draw_text(
                frame,
                width,
                height,
                layout.text_x,
                layout.text_y,
                &layout.display_text,
                Color::rgb(32, 35, 40),
                crate::style::FontWeight::Normal,
                17.0 * scale as f32,
                None,
                None,
            );
        }
    } else {
        text_rasterizer.draw_text(
            frame,
            width,
            height,
            layout.text_x,
            layout.text_y,
            &layout.display_text,
            Color::rgb(32, 35, 40),
            crate::style::FontWeight::Normal,
            17.0 * scale as f32,
            None,
            None,
        );
    }

    if address_focus && caret_visible {
        let caret_x = layout.x_for_char_index(text_rasterizer, address_cursor);
        draw_rect(
            frame,
            width,
            height,
            caret_x,
            (16 * scale) as i32,
            scale.min(2) as i32,
            address_bar_height(scale).saturating_sub(8 * scale) as i32,
            Color::rgb(46, 50, 56),
        );
    }

    if let Some(message) = status_message {
        text_rasterizer.draw_text(
            frame,
            width,
            height,
            (address_bar_padding(scale) + 8 * scale) as i32,
            (52 * scale) as i32,
            message,
            Color::rgb(96, 100, 110),
            crate::style::FontWeight::Normal,
            13.0 * scale as f32,
            None,
            None,
        );
    }
}

fn rasterize(
    text_rasterizer: &TextRasterizer,
    display_list: &DisplayList,
    frame: &mut [u8],
    width: u32,
    height: u32,
    scroll_y: u32,
    scale: u32,
) {
    let content_pixel_width = display_list.width.saturating_mul(scale);
    let origin_x = compute_content_origin_x(width, content_pixel_width) as i32;
    let origin_y = (chrome_height(scale) + top_margin(scale)) as i32;
    let clip_top = origin_y;
    let clip_bottom = height.saturating_sub(top_margin(scale)) as i32;

    for command in &display_list.commands {
        match command {
            DisplayCommand::FillRect {
                x,
                y,
                width: rect_width,
                height: rect_height,
                color,
            } => {
                let draw_y = origin_y + ((*y as i32 - scroll_y as i32) * scale as i32);
                let draw_height = *rect_height as i32 * scale as i32;
                if draw_y >= clip_bottom || draw_y + draw_height <= clip_top {
                    continue;
                }
                draw_rect_clipped(
                    frame,
                    width,
                    height,
                    origin_x + (*x as i32 * scale as i32),
                    draw_y,
                    *rect_width as i32 * scale as i32,
                    draw_height,
                    *color,
                    clip_top,
                    clip_bottom,
                )
            }
            DisplayCommand::DrawText {
                x,
                y,
                text,
                width: _,
                line_height,
                color,
                font_weight,
                underline: _,
                font_size,
            } => {
                let draw_y = origin_y + ((*y as i32 - scroll_y as i32) * scale as i32);
                let draw_height = *line_height as i32 * scale as i32;
                if draw_y >= clip_bottom || draw_y + draw_height <= clip_top {
                    continue;
                }
                text_rasterizer.draw_text(
                    frame,
                    width,
                    height,
                    origin_x + (*x as i32 * scale as i32),
                    draw_y,
                    text,
                    *color,
                    *font_weight,
                    *font_size as f32 * scale as f32,
                    Some(clip_top),
                    Some(clip_bottom),
                )
            }
        }
        if let DisplayCommand::DrawText {
            x,
            y,
            width: text_width,
            line_height,
            underline: true,
            ..
        } = command
        {
            draw_rect_clipped(
                frame,
                width,
                height,
                origin_x + (*x as i32 * scale as i32),
                origin_y
                    + ((*y as i32 - scroll_y as i32) * scale as i32)
                    + (*line_height as i32 * scale as i32)
                    - (scale.min(2) as i32 * 2),
                *text_width as i32 * scale as i32,
                scale.min(2) as i32,
                Color::rgb(51, 102, 204),
                clip_top,
                clip_bottom,
            );
        }
    }

    draw_scrollbar(
        frame,
        width,
        height,
        display_list.height,
        scroll_y,
        content_viewport_height_for(height, scale),
        scale,
    );
}

fn draw_rect_clipped(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    rect_width: i32,
    rect_height: i32,
    color: Color,
    clip_top: i32,
    clip_bottom: i32,
) {
    let clipped_y = y.max(clip_top);
    let clipped_bottom = (y + rect_height).min(clip_bottom);
    if clipped_bottom <= clipped_y {
        return;
    }

    draw_rect(
        frame,
        width,
        height,
        x,
        clipped_y,
        rect_width,
        clipped_bottom - clipped_y,
        color,
    );
}

fn draw_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    rect_width: i32,
    rect_height: i32,
    color: Color,
) {
    let start_x = x.max(0) as u32;
    let start_y = y.max(0) as u32;
    let max_x = (x + rect_width).max(0) as u32;
    let max_y = (y + rect_height).max(0) as u32;
    let max_x = max_x.min(width);
    let max_y = max_y.min(height);

    for py in start_y..max_y {
        for px in start_x..max_x {
            set_pixel(frame, width, height, px as i32, py as i32, color);
        }
    }
}

fn draw_text_bitmap(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    font_weight: crate::style::FontWeight,
    scale: u32,
    clip_top: Option<i32>,
    clip_bottom: Option<i32>,
) {
    let mut cursor_x = x;
    for ch in text.chars() {
        if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    if (bits >> col) & 1 == 1 {
                        let px = cursor_x + col * scale as i32;
                        let py = y + row as i32 * scale as i32;
                        if clip_top.is_some_and(|top| py + scale as i32 <= top)
                            || clip_bottom.is_some_and(|bottom| py >= bottom)
                        {
                            continue;
                        }
                        draw_rect(
                            frame,
                            width,
                            height,
                            px,
                            py,
                            scale as i32,
                            scale as i32,
                            color,
                        );
                        if font_weight == crate::style::FontWeight::Bold {
                            draw_rect(
                                frame,
                                width,
                                height,
                                px + scale as i32,
                                py,
                                scale as i32,
                                scale as i32,
                                color,
                            );
                        }
                    }
                }
            }
        }
        cursor_x += (CHAR_WIDTH * scale) as i32;
    }
}

fn draw_text_fontdue(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    font: &Font,
    font_size: f32,
    clip_top: Option<i32>,
    clip_bottom: Option<i32>,
) {
    for glyph in layout_text_glyphs(font, text, font_size, x, y) {
        let (metrics, bitmap) = font.rasterize_config(glyph.key);
        let glyph_y = glyph.y.round() as i32;
        if clip_top.is_some_and(|top| glyph_y + metrics.height as i32 <= top)
            || clip_bottom.is_some_and(|bottom| glyph_y >= bottom)
        {
            continue;
        }
        draw_glyph_bitmap(
            frame,
            width,
            height,
            glyph.x.round() as i32,
            glyph_y,
            metrics.width,
            metrics.height,
            &bitmap,
            color,
            clip_top,
            clip_bottom,
        );
    }
}

fn draw_positioned_glyphs(
    frame: &mut [u8],
    width: u32,
    height: u32,
    glyphs: &[GlyphPosition],
    color: Color,
    font: &Font,
    clip_top: Option<i32>,
    clip_bottom: Option<i32>,
) {
    for glyph in glyphs {
        let (metrics, bitmap) = font.rasterize_config(glyph.key);
        let glyph_y = glyph.y.round() as i32;
        if clip_top.is_some_and(|top| glyph_y + metrics.height as i32 <= top)
            || clip_bottom.is_some_and(|bottom| glyph_y >= bottom)
        {
            continue;
        }
        draw_glyph_bitmap(
            frame,
            width,
            height,
            glyph.x.round() as i32,
            glyph_y,
            metrics.width,
            metrics.height,
            &bitmap,
            color,
            clip_top,
            clip_bottom,
        );
    }
}

fn layout_text_glyphs(
    font: &Font,
    text: &str,
    font_size: f32,
    x: i32,
    y: i32,
) -> Vec<GlyphPosition> {
    let fonts = [font];
    let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
    let line_height = if font_size >= 28.0 { 1.1 } else { 1.35 };
    layout.reset(&LayoutSettings {
        x: x as f32,
        y: y as f32,
        line_height,
        ..LayoutSettings::default()
    });
    layout.append(&fonts, &TextStyle::new(text, font_size, 0));
    layout.glyphs().clone()
}

fn draw_glyph_bitmap(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    glyph_width: usize,
    glyph_height: usize,
    bitmap: &[u8],
    color: Color,
    clip_top: Option<i32>,
    clip_bottom: Option<i32>,
) {
    for row in 0..glyph_height {
        for col in 0..glyph_width {
            let coverage = bitmap[row * glyph_width + col];
            if coverage == 0 {
                continue;
            }

            let py = y + row as i32;
            if clip_top.is_some_and(|top| py < top) || clip_bottom.is_some_and(|bottom| py >= bottom)
            {
                continue;
            }

            blend_pixel(
                frame,
                width,
                height,
                x + col as i32,
                py,
                color,
                coverage,
            );
        }
    }
}

fn draw_scrollbar(
    frame: &mut [u8],
    width: u32,
    height: u32,
    content_height: u32,
    scroll_y: u32,
    viewport_height: u32,
    scale: u32,
) {
    if content_height <= viewport_height || width < 12 || viewport_height == 0 {
        return;
    }

    let track_x = width.saturating_sub(scaled(14, scale));
    let track_y = chrome_height(scale) + top_margin(scale);
    let track_height = height.saturating_sub(track_y + top_margin(scale));

    draw_rect(
        frame,
        width,
        height,
        track_x as i32,
        track_y as i32,
        scaled(SCROLLBAR_WIDTH, scale.min(2)) as i32,
        track_height as i32,
        Color::rgb(228, 224, 214),
    );

    let thumb_height = ((viewport_height as f32 / content_height as f32) * track_height as f32)
        .round()
        .max(28.0) as u32;
    let travel = track_height.saturating_sub(thumb_height).max(1);
    let max_scroll = content_height.saturating_sub(viewport_height).max(1);
    let thumb_y = ((scroll_y as f32 / max_scroll as f32) * travel as f32).round() as u32;

    draw_rect(
        frame,
        width,
        height,
        track_x as i32,
        (track_y + thumb_y) as i32,
        scaled(SCROLLBAR_WIDTH, scale.min(2)) as i32,
        thumb_height as i32,
        Color::rgb(122, 128, 138),
    );
}

fn set_pixel(frame: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: Color) {
    if x < 0 || y < 0 {
        return;
    }

    let x = x as u32;
    let y = y as u32;
    if x >= width || y >= height {
        return;
    }

    let index = ((y * width + x) * 4) as usize;
    if let Some(pixel) = frame.get_mut(index..index + 4) {
        pixel[0] = color.r;
        pixel[1] = color.g;
        pixel[2] = color.b;
        pixel[3] = color.a;
    }
}

fn blend_pixel(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    color: Color,
    coverage: u8,
) {
    if x < 0 || y < 0 {
        return;
    }

    let x = x as u32;
    let y = y as u32;
    if x >= width || y >= height {
        return;
    }

    let index = ((y * width + x) * 4) as usize;
    if let Some(pixel) = frame.get_mut(index..index + 4) {
        let alpha = coverage as f32 / 255.0;
        pixel[0] = ((1.0 - alpha) * pixel[0] as f32 + alpha * color.r as f32).round() as u8;
        pixel[1] = ((1.0 - alpha) * pixel[1] as f32 + alpha * color.g as f32).round() as u8;
        pixel[2] = ((1.0 - alpha) * pixel[2] as f32 + alpha * color.b as f32).round() as u8;
        pixel[3] = 0xff;
    }
}

fn compute_content_origin_x(viewport_width: u32, content_pixel_width: u32) -> u32 {
    let _ = content_pixel_width;
    SIDE_MARGIN.min(viewport_width.saturating_sub(1))
}

fn content_viewport_height_for(viewport_height: u32, scale: u32) -> u32 {
    let available_pixels = viewport_height
        .saturating_sub(chrome_height(scale) + top_margin(scale) * 2)
        .max(scale);
    (available_pixels / scale).max(1)
}

fn address_byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn address_slice(text: &str, start: usize, end: usize) -> String {
    let start_byte = address_byte_index(text, start);
    let end_byte = address_byte_index(text, end);
    text[start_byte..end_byte].to_string()
}

fn build_address_text_layout(
    text_rasterizer: &TextRasterizer,
    address_input: &str,
    preedit_text: &str,
    address_focus: bool,
    scale: u32,
) -> AddressTextLayout {
    let text_x = (address_bar_padding(scale) + 8 * scale) as i32;
    let text_y = (20 * scale) as i32;
    let prefix = if address_focus { "> " } else { "" };
    let display_text = format!("{prefix}{address_input}{preedit_text}");
    let font_size = 17usize.saturating_mul(scale as usize);
    let glyphs = text_rasterizer.layout_text_glyphs(
        &display_text,
        crate::style::FontWeight::Normal,
        font_size,
        text_x,
        text_y,
    ).map(|mut glyphs| {
        if let Some(min_x) = glyphs
            .iter()
            .map(|glyph| glyph.x.floor() as i32)
            .min()
            .filter(|min_x| *min_x < text_x)
        {
            let shift = (text_x - min_x) as f32;
            for glyph in &mut glyphs {
                glyph.x += shift;
            }
        }
        glyphs
    });

    AddressTextLayout {
        display_text,
        font_size,
        text_x,
        text_y,
        prefix_len_bytes: prefix.len(),
        address_input_len_bytes: address_input.len(),
        glyphs,
    }
}

impl AddressTextLayout {
    fn address_byte_offset_for_char_index(&self, address_input: &str, char_index: usize) -> usize {
        self.prefix_len_bytes + address_byte_index(address_input, char_index)
    }

    fn x_for_byte_offset(&self, text_rasterizer: &TextRasterizer, byte_offset: usize) -> i32 {
        if let Some(glyphs) = &self.glyphs {
            let mut trailing_x = self.text_x;
            for glyph in glyphs {
                if glyph.byte_offset >= byte_offset {
                    return glyph.x.round() as i32;
                }
                trailing_x = (glyph.x + glyph.width as f32).round() as i32;
            }
            return trailing_x;
        }

        let prefix = &self.display_text[..byte_offset.min(self.display_text.len())];
        self.text_x
            + text_rasterizer.exact_text_width(
                prefix,
                crate::style::FontWeight::Normal,
                self.font_size,
            )
    }

    fn x_for_char_index(&self, text_rasterizer: &TextRasterizer, char_index: usize) -> i32 {
        let address_only = &self.display_text[self.prefix_len_bytes
            ..self.prefix_len_bytes + self.address_input_len_bytes];
        let byte_offset = self.address_byte_offset_for_char_index(address_only, char_index);
        self.x_for_byte_offset(text_rasterizer, byte_offset)
    }

    fn char_index_from_x(&self, text_rasterizer: &TextRasterizer, x: f64) -> usize {
        let address_only = &self.display_text[self.prefix_len_bytes
            ..self.prefix_len_bytes + self.address_input_len_bytes];
        let char_count = address_only.chars().count();

        for index in 0..char_count {
            let current_x = self.x_for_char_index(text_rasterizer, index) as f64;
            let next_x = self.x_for_char_index(text_rasterizer, index + 1) as f64;
            if x <= current_x + ((next_x - current_x) / 2.0) {
                return index;
            }
        }

        char_count
    }
}

fn address_bar_hit_test(x: f64, y: f64, viewport_width: u32, scale: u32) -> bool {
    let min_x = address_bar_padding(scale) as f64;
    let max_x = viewport_width.saturating_sub(address_bar_padding(scale)) as f64;
    let min_y = (12 * scale) as f64;
    let max_y = (12 * scale + address_bar_height(scale)) as f64;
    x >= min_x && x <= max_x && y >= min_y && y <= max_y
}

fn address_bar_cursor_from_position(
    text_rasterizer: &TextRasterizer,
    text: &str,
    x: f64,
    scale: u32,
) -> usize {
    let layout = build_address_text_layout(text_rasterizer, text, "", true, scale);
    layout.char_index_from_x(text_rasterizer, x)
}

fn link_hit_test(
    display_list: &DisplayList,
    x: f64,
    y: f64,
    viewport_width: u32,
    scale: u32,
    scroll_y: u32,
) -> Option<String> {
    let content_pixel_width = display_list.width.saturating_mul(scale);
    let origin_x = compute_content_origin_x(viewport_width, content_pixel_width) as f64;
    let origin_y = (chrome_height(scale) + top_margin(scale)) as f64;

    for region in &display_list.link_regions {
        let left = origin_x + (region.x.saturating_mul(scale)) as f64;
        let top = origin_y + (region.y.saturating_sub(scroll_y).saturating_mul(scale)) as f64;
        let right = left + (region.width.saturating_mul(scale)) as f64;
        let bottom = top + (region.height.saturating_mul(scale)) as f64;
        if x >= left && x <= right && y >= top && y <= bottom {
            return Some(region.target.clone());
        }
    }

    None
}

fn chrome_height(scale: u32) -> u32 {
    scaled(CHROME_HEIGHT, scale)
}

fn top_margin(scale: u32) -> u32 {
    scaled(TOP_MARGIN, scale)
}

fn address_bar_height(scale: u32) -> u32 {
    scaled(ADDRESS_BAR_HEIGHT, scale)
}

fn address_bar_padding(scale: u32) -> u32 {
    scaled(ADDRESS_BAR_PADDING, scale)
}

fn scaled(value: u32, scale: u32) -> u32 {
    value.saturating_mul(scale.max(1))
}

fn sanitize_clipboard_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clamp_scroll(scroll_y: u32, content_height: u32, viewport_height: u32) -> u32 {
    scroll_y.min(content_height.saturating_sub(viewport_height))
}

fn apply_scroll_delta(
    current_scroll: u32,
    delta: i32,
    content_height: u32,
    viewport_height: u32,
) -> u32 {
    let next = if delta.is_negative() {
        current_scroll.saturating_sub(delta.unsigned_abs())
    } else {
        current_scroll.saturating_add(delta as u32)
    };
    clamp_scroll(next, content_height, viewport_height)
}

#[cfg(test)]
mod tests {
    use super::{
        address_bar_cursor_from_position, address_bar_hit_test, address_slice,
        apply_scroll_delta, chrome_height, clamp_scroll,
        build_address_text_layout, content_scale_for_viewport, link_hit_test, load_page,
        rasterize, sanitize_clipboard_text, top_margin, GuiApp, TextRasterizer,
    };
    use crate::paint::{Color, DisplayCommand, DisplayList, LinkRegion};

    #[test]
    fn clamps_scroll_to_content_height() {
        assert_eq!(clamp_scroll(10, 200, 300), 0);
        assert_eq!(clamp_scroll(500, 800, 300), 500);
        assert_eq!(clamp_scroll(900, 800, 300), 500);
    }

    #[test]
    fn applies_scroll_delta_with_bounds() {
        assert_eq!(apply_scroll_delta(0, 40, 1000, 200), 40);
        assert_eq!(apply_scroll_delta(10, -40, 1000, 200), 0);
        assert_eq!(apply_scroll_delta(900, 100, 1000, 200), 800);
    }

    #[test]
    fn scales_up_for_large_viewports() {
        assert_eq!(content_scale_for_viewport(800, 600, 1.0), 1);
        assert_eq!(content_scale_for_viewport(1400, 900, 1.0), 1);
        assert_eq!(content_scale_for_viewport(2400, 1600, 1.0), 2);
        assert_eq!(content_scale_for_viewport(1920, 1440, 2.0), 2);
    }

    #[test]
    fn detects_address_bar_hits() {
        assert!(address_bar_hit_test(30.0, 20.0, 900, 1));
        assert!(!address_bar_hit_test(30.0, 90.0, 900, 1));
        assert!(address_bar_hit_test(60.0, 40.0, 1800, 2));
    }

    #[test]
    fn detects_link_hits() {
        let display_list = DisplayList {
            width: 240,
            height: 120,
            commands: Vec::new(),
            link_regions: vec![LinkRegion {
                x: 32,
                y: 24,
                width: 80,
                height: 24,
                target: "https://example.com".to_string(),
            }],
            background: Color::rgb(255, 255, 255),
        };

        let hit = link_hit_test(&display_list, 80.0, 110.0, 320, 1, 0);
        assert_eq!(hit.as_deref(), Some("https://example.com"));
        assert!(link_hit_test(&display_list, 10.0, 10.0, 320, 1, 0).is_none());
    }

    #[test]
    fn rasterizes_with_scroll_offset() {
        let text_rasterizer = TextRasterizer::load();
        let scale = 2;
        let display_list = DisplayList {
            width: 120,
            height: 300,
            commands: vec![DisplayCommand::FillRect {
                x: 10,
                y: 40,
                width: 20,
                height: 10,
                color: Color::rgb(255, 0, 0),
            }],
            link_regions: Vec::new(),
            background: Color::rgb(255, 255, 255),
        };

        let mut frame = vec![255_u8; (240 * 280 * 4) as usize];
        rasterize(&text_rasterizer, &display_list, &mut frame, 240, 280, 35, scale);

        let pixel_y = chrome_height(scale) + top_margin(scale) + ((40 - 35) as u32 * scale) + 4;
        let pixel_index = ((pixel_y * 240 + 72) * 4) as usize;
        assert_eq!(frame[pixel_index], 255);
        assert_eq!(frame[pixel_index + 1], 0);
        assert_eq!(frame[pixel_index + 2], 0);
    }

    #[test]
    fn clips_scrolled_content_below_chrome() {
        let text_rasterizer = TextRasterizer::load();
        let scale = 1;
        let display_list = DisplayList {
            width: 120,
            height: 300,
            commands: vec![DisplayCommand::FillRect {
                x: 10,
                y: 0,
                width: 40,
                height: 40,
                color: Color::rgb(255, 0, 0),
            }],
            link_regions: Vec::new(),
            background: Color::rgb(255, 255, 255),
        };

        let mut frame = vec![255_u8; (220 * 220 * 4) as usize];
        rasterize(&text_rasterizer, &display_list, &mut frame, 220, 220, 25, scale);

        let chrome_pixel_index = (((chrome_height(scale) - 4) * 220 + 48) * 4) as usize;
        assert_eq!(frame[chrome_pixel_index], 255);
        assert_eq!(frame[chrome_pixel_index + 1], 255);
        assert_eq!(frame[chrome_pixel_index + 2], 255);

        let content_pixel_y = chrome_height(scale) + top_margin(scale) + 2;
        let content_pixel_index = ((content_pixel_y * 220 + 48) * 4) as usize;
        assert_eq!(frame[content_pixel_index], 255);
        assert_eq!(frame[content_pixel_index + 1], 0);
        assert_eq!(frame[content_pixel_index + 2], 0);
    }

    #[test]
    fn loads_page_fixture() {
        let text_rasterizer = TextRasterizer::load();
        let page = load_page("examples/welcome.html", 960, 720, 1.0, &text_rasterizer)
            .expect("fixture should load");
        assert!(!page.display_list.commands.is_empty());
        assert!(!page.display_list.link_regions.is_empty());
    }

    #[test]
    fn sanitizes_clipboard_text_for_address_bar() {
        assert_eq!(
            sanitize_clipboard_text(" https://example.com/\npath\t?q=1 "),
            "https://example.com/ path ?q=1"
        );
    }

    #[test]
    fn replacing_selected_address_text_overwrites_existing_value() {
        let text_rasterizer = TextRasterizer::load();
        let page = load_page("examples/welcome.html", 960, 720, 1.0, &text_rasterizer)
            .expect("fixture should load");
        let mut app = GuiApp::new(page, "examples/welcome.html", text_rasterizer);
        app.address_input = "https://old.example".to_string();
        app.address_cursor = 0;
        app.select_all_address();

        app.insert_address_text("https://new.example");

        assert_eq!(app.address_input, "https://new.example");
        assert_eq!(app.address_cursor, "https://new.example".chars().count());
        assert!(app.address_selection_range().is_none());
    }

    #[test]
    fn cursor_hit_testing_maps_points_to_character_indices() {
        let text_rasterizer = TextRasterizer::load();
        let text = "hello";
        let start_x = (super::address_bar_padding(1) + 8) as f64;

        assert_eq!(
            address_bar_cursor_from_position(&text_rasterizer, text, start_x + 2.0, 1),
            0
        );
        assert_eq!(
            address_bar_cursor_from_position(&text_rasterizer, text, start_x + 80.0, 1),
            5
        );
    }

    #[test]
    fn address_layout_keeps_first_glyph_inside_padding() {
        let text_rasterizer = TextRasterizer::load();
        let layout = build_address_text_layout(&text_rasterizer, "example.com", "", true, 1);
        if let Some(glyphs) = layout.glyphs.as_ref() {
            let min_x = glyphs
                .iter()
                .map(|glyph| glyph.x.floor() as i32)
                .min()
                .unwrap_or(layout.text_x);
            assert!(min_x >= layout.text_x);
        }
    }

    #[test]
    fn address_slice_returns_selected_range() {
        assert_eq!(address_slice("abcdef", 1, 4), "bcd");
    }
}
