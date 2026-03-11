use crate::paint::Color;
use crate::source;
use crate::style::FontWeight;
use arboard::Clipboard;
use font8x8::UnicodeFonts;
use fontdb::{Database, Family, Query, Style, Weight};
use fontdue::layout::{CoordinateSystem, GlyphPosition, Layout, LayoutSettings, TextStyle};
use fontdue::{Font, FontSettings};
use pixels::{Pixels, SurfaceTexture};
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::Url;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};
use wry::dpi::{LogicalPosition as WryLogicalPosition, LogicalSize as WryLogicalSize};
use wry::{NewWindowResponse, Rect, WebView, WebViewBuilder};

const CHROME_HEIGHT: u32 = 112;
const TAB_BAR_HEIGHT: u32 = 36;
const ADDRESS_BAR_HEIGHT: u32 = 34;
const ADDRESS_BAR_PADDING: u32 = 12;
const NAV_BUTTON_SIZE: u32 = 28;
const NAV_BUTTON_SPACING: u32 = 10;
const NAV_BUTTON_MARGIN_LEFT: u32 = 12;
const ADDRESS_BAR_LEFT_OFFSET: u32 = 118;
const TAB_HEIGHT: u32 = 26;
const TAB_MIN_WIDTH: u32 = 160;
const TAB_MAX_WIDTH: u32 = 220;
const TAB_GAP: u32 = 8;
const NEW_TAB_BUTTON_SIZE: u32 = 26;
const DEFAULT_VIEWPORT_WIDTH: u32 = 1200;
const DEFAULT_VIEWPORT_HEIGHT: u32 = 820;
const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(530);
const CHAR_WIDTH: u32 = 8;

pub fn run(initial_source: &str) -> Result<(), String> {
    let initial_url = source::normalize_browser_url(initial_source)?;
    let event_loop = EventLoop::<BrowserEvent>::with_user_event()
        .build()
        .map_err(|err| format!("failed to create event loop: {err}"))?;
    let proxy = event_loop.create_proxy();
    let mut app = WebViewApp::new(initial_source, &initial_url, proxy);
    event_loop
        .run_app(&mut app)
        .map_err(|err| format!("failed to run WebView app: {err}"))
}

#[derive(Clone, Debug)]
enum BrowserEvent {
    NavigationStarted(String),
    PageFinished(String),
    TitleChanged(String),
    TopLevelUrlResolved(String),
    OpenInNewTab(String),
}

#[derive(Clone, Debug)]
struct TabState {
    source: String,
    current_url: String,
    title: String,
    is_loading: bool,
    history: Vec<String>,
    history_index: usize,
}

struct WebViewApp {
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    webview: Option<WebView>,
    proxy: EventLoopProxy<BrowserEvent>,
    text_rasterizer: TextRasterizer,
    clipboard: Option<Clipboard>,
    tabs: Vec<TabState>,
    active_tab: usize,
    address_input: String,
    address_focus: bool,
    address_cursor: usize,
    address_selection_anchor: Option<usize>,
    address_drag_active: bool,
    preedit_text: String,
    status_message: Option<String>,
    viewport_width: u32,
    viewport_height: u32,
    modifiers: ModifiersState,
    cursor_position: Option<PhysicalPosition<f64>>,
    address_blink_started_at: Instant,
}

impl WebViewApp {
    fn new(initial_source: &str, initial_url: &str, proxy: EventLoopProxy<BrowserEvent>) -> Self {
        let initial_cursor = initial_source.chars().count();
        Self {
            window: None,
            pixels: None,
            webview: None,
            proxy,
            text_rasterizer: TextRasterizer::load(),
            clipboard: Clipboard::new().ok(),
            tabs: vec![TabState {
                source: initial_source.to_string(),
                current_url: initial_url.to_string(),
                title: initial_source.to_string(),
                is_loading: true,
                history: vec![initial_url.to_string()],
                history_index: 0,
            }],
            active_tab: 0,
            address_input: initial_source.to_string(),
            address_focus: false,
            address_cursor: initial_cursor,
            address_selection_anchor: None,
            address_drag_active: false,
            preedit_text: String::new(),
            status_message: Some("Loading page".to_string()),
            viewport_width: DEFAULT_VIEWPORT_WIDTH,
            viewport_height: DEFAULT_VIEWPORT_HEIGHT,
            modifiers: ModifiersState::empty(),
            cursor_position: None,
            address_blink_started_at: Instant::now(),
        }
    }

    fn draw(&mut self) -> Result<(), String> {
        let address_selection = self.address_selection_range();
        let caret_visible = self.address_caret_visible();
        let active_loading = self.active_tab().is_some_and(|tab| tab.is_loading);
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
            &self.tabs,
            self.active_tab,
            active_loading,
            &self.address_input,
            &self.preedit_text,
            self.address_focus,
            self.address_cursor,
            address_selection,
            caret_visible,
            self.status_message.as_deref(),
        );

        pixels
            .render()
            .map_err(|err| format!("failed to render frame: {err}"))?;

        if self.address_focus {
            self.request_redraw();
        }

        Ok(())
    }

    fn active_tab(&self) -> Option<&TabState> {
        self.tabs.get(self.active_tab)
    }

    fn active_tab_mut(&mut self) -> Option<&mut TabState> {
        self.tabs.get_mut(self.active_tab)
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
        if !focused {
            self.focus_webview();
        }
    }

    fn focus_webview(&self) {
        if let Some(webview) = self.webview.as_ref() {
            let _ = webview.focus();
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

    fn address_char_count(&self) -> usize {
        self.address_input.chars().count()
    }

    fn select_all_address(&mut self) {
        self.address_selection_anchor = Some(0);
        self.address_cursor = self.address_char_count();
    }

    fn address_selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.address_selection_anchor?;
        if anchor == self.address_cursor {
            None
        } else {
            Some((
                anchor.min(self.address_cursor),
                anchor.max(self.address_cursor),
            ))
        }
    }

    fn clear_address_selection(&mut self) {
        self.address_selection_anchor = None;
    }

    fn selected_address_text(&self) -> Option<String> {
        let (start, end) = self.address_selection_range()?;
        Some(address_slice(&self.address_input, start, end))
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
        if self.address_cursor >= self.address_char_count() {
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

    fn sync_address_from_active_tab(&mut self) {
        if self.address_focus {
            return;
        }
        if let Some(tab) = self.active_tab() {
            self.address_input = tab.source.clone();
            self.address_cursor = self.address_char_count();
            self.clear_address_selection();
        }
    }

    fn active_history_url(&self) -> Option<String> {
        self.active_tab()
            .and_then(|tab| tab.history.get(tab.history_index))
            .cloned()
            .or_else(|| self.active_tab().map(|tab| tab.current_url.clone()))
    }

    fn record_active_tab_history(&mut self, url: &str) {
        if let Some(tab) = self.active_tab_mut() {
            record_history_for_tab(tab, url);
        }
    }

    fn navigate_to_input(&mut self, requested: String) {
        match source::normalize_browser_url(&requested) {
            Ok(url) => {
                if self.webview.is_some() {
                    if let Some(tab) = self.active_tab_mut() {
                        tab.source = requested.clone();
                        tab.current_url = url.clone();
                        tab.is_loading = true;
                    }
                    match self.webview.as_ref().expect("checked above").load_url(&url) {
                        Ok(()) => {
                            self.address_input = requested.clone();
                            self.status_message = Some(format!("Loading {url}"));
                            self.set_address_focus(false);
                        }
                        Err(err) => {
                            if let Some(tab) = self.active_tab_mut() {
                                tab.is_loading = false;
                            }
                            self.status_message =
                                Some(format!("failed to navigate to {url}: {err}"));
                        }
                    }
                }
            }
            Err(err) => self.status_message = Some(err),
        }
        self.request_redraw();
    }

    fn reload(&mut self) {
        if self.webview.is_some() {
            self.status_message = Some("Reloading page".to_string());
            if let Some(tab) = self.active_tab_mut() {
                tab.is_loading = true;
            }
            if let Err(err) = self.webview.as_ref().expect("checked above").reload() {
                if let Some(tab) = self.active_tab_mut() {
                    tab.is_loading = false;
                }
                self.status_message = Some(format!("failed to reload page: {err}"));
            }
        }
        self.request_redraw();
    }

    fn history_back(&mut self) {
        let Some(target_url) = self.active_tab().and_then(|tab| {
            tab.history_index
                .checked_sub(1)
                .and_then(|idx| tab.history.get(idx).cloned())
        }) else {
            return;
        };

        if let Some(tab) = self.active_tab_mut() {
            tab.history_index = tab.history_index.saturating_sub(1);
            tab.is_loading = true;
            tab.current_url = target_url.clone();
            tab.source = target_url.clone();
        }
        if self.webview.is_some() {
            let _ = self
                .webview
                .as_ref()
                .expect("checked above")
                .load_url(&target_url);
            self.status_message = Some("Going back".to_string());
        }
        self.request_redraw();
    }

    fn history_forward(&mut self) {
        let Some(target_url) = self.active_tab().and_then(|tab| {
            tab.history
                .get(tab.history_index.saturating_add(1))
                .cloned()
        }) else {
            return;
        };

        if let Some(tab) = self.active_tab_mut() {
            tab.history_index = (tab.history_index + 1).min(tab.history.len().saturating_sub(1));
            tab.is_loading = true;
            tab.current_url = target_url.clone();
            tab.source = target_url.clone();
        }
        if self.webview.is_some() {
            let _ = self
                .webview
                .as_ref()
                .expect("checked above")
                .load_url(&target_url);
            self.status_message = Some("Going forward".to_string());
        }
        self.request_redraw();
    }

    fn update_window_title(&self) {
        if let Some(window) = self.window.as_ref() {
            let label = if let Some(tab) = self.active_tab() {
                if tab.title.is_empty() {
                    tab.source.as_str()
                } else {
                    tab.title.as_str()
                }
            } else {
                "Browser"
            };
            window.set_title(&format!("Browser - {label}"));
        }
    }

    fn open_new_tab(&mut self) {
        self.tabs.push(TabState {
            source: String::new(),
            current_url: "about:blank".to_string(),
            title: "New Tab".to_string(),
            is_loading: true,
            history: vec!["about:blank".to_string()],
            history_index: 0,
        });
        self.active_tab = self.tabs.len().saturating_sub(1);
        self.address_input.clear();
        self.address_cursor = 0;
        self.clear_address_selection();
        self.status_message = Some("Opened new tab".to_string());
        if self.webview.is_some() {
            if let Err(err) = self
                .webview
                .as_ref()
                .expect("checked above")
                .load_url("about:blank")
            {
                self.status_message = Some(format!("failed to open new tab: {err}"));
                if let Some(tab) = self.active_tab_mut() {
                    tab.is_loading = false;
                }
            }
        }
        self.set_address_focus(true);
        self.update_window_title();
        self.request_redraw();
    }

    fn close_tab(&mut self, tab_index: usize) {
        if tab_index >= self.tabs.len() {
            return;
        }

        if self.tabs.len() == 1 {
            if let Some(tab) = self.tabs.get_mut(0) {
                tab.source.clear();
                tab.current_url = "about:blank".to_string();
                tab.title = "New Tab".to_string();
                tab.is_loading = true;
                tab.history = vec!["about:blank".to_string()];
                tab.history_index = 0;
            }
            self.active_tab = 0;
            self.address_input.clear();
            self.address_cursor = 0;
            self.clear_address_selection();
            if self.webview.is_some() {
                let _ = self
                    .webview
                    .as_ref()
                    .expect("checked above")
                    .load_url("about:blank");
            }
            self.status_message = Some("Reset current tab".to_string());
            self.set_address_focus(true);
            self.update_window_title();
            self.request_redraw();
            return;
        }

        self.tabs.remove(tab_index);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len().saturating_sub(1);
        } else if tab_index < self.active_tab {
            self.active_tab = self.active_tab.saturating_sub(1);
        }

        let target_url = self
            .active_history_url()
            .unwrap_or_else(|| "about:blank".to_string());
        if self.webview.is_some() {
            let _ = self
                .webview
                .as_ref()
                .expect("checked above")
                .load_url(&target_url);
        }
        self.sync_address_from_active_tab();
        self.status_message = Some("Closed tab".to_string());
        self.update_window_title();
        self.request_redraw();
    }

    fn switch_to_tab(&mut self, tab_index: usize) {
        if tab_index >= self.tabs.len() || tab_index == self.active_tab {
            return;
        }

        self.active_tab = tab_index;
        self.sync_address_from_active_tab();
        self.set_address_focus(false);
        let target_url = self
            .active_history_url()
            .unwrap_or_else(|| "about:blank".to_string());
        if let Some(tab) = self.active_tab_mut() {
            tab.is_loading = true;
        }
        if self.webview.is_some() {
            if let Err(err) = self
                .webview
                .as_ref()
                .expect("checked above")
                .load_url(&target_url)
            {
                if let Some(tab) = self.active_tab_mut() {
                    tab.is_loading = false;
                }
                self.status_message = Some(format!("failed to switch tab: {err}"));
            } else {
                self.status_message = Some(format!("Loading {target_url}"));
            }
        }
        self.update_window_title();
        self.request_redraw();
    }

    fn handle_primary_click(&mut self, position: PhysicalPosition<f64>) {
        if new_tab_button_hit_test(position.x, position.y, self.viewport_width) {
            self.open_new_tab();
            return;
        }

        if let Some(tab_index) =
            tab_close_hit_test(position.x, position.y, self.viewport_width, self.tabs.len())
        {
            self.close_tab(tab_index);
            return;
        }

        if let Some(tab_index) =
            tab_hit_test(position.x, position.y, self.viewport_width, self.tabs.len())
        {
            self.switch_to_tab(tab_index);
            return;
        }

        if let Some(action) = nav_button_hit_test(position.x, position.y) {
            match action {
                NavAction::Back => self.history_back(),
                NavAction::Forward => self.history_forward(),
                NavAction::Reload => self.reload(),
            }
            return;
        }

        if address_bar_hit_test(position.x, position.y, self.viewport_width) {
            self.set_address_focus(true);
            let caret = address_bar_cursor_from_position(
                &self.text_rasterizer,
                &self.address_input,
                position.x,
            );
            self.address_cursor = caret;
            self.clear_address_selection();
            self.address_drag_active = true;
            self.status_message = Some("Editing address".to_string());
        } else if self.address_focus {
            self.set_address_focus(false);
            self.status_message = Some("Address bar unfocused".to_string());
        } else {
            self.focus_webview();
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

    fn resize_webview(&self) -> Result<(), String> {
        let Some(window) = self.window.as_ref() else {
            return Ok(());
        };
        let Some(webview) = self.webview.as_ref() else {
            return Ok(());
        };
        let logical_size = window.inner_size().to_logical::<u32>(window.scale_factor());
        let content_height = logical_size.height.saturating_sub(CHROME_HEIGHT).max(1);
        webview
            .set_bounds(Rect {
                position: WryLogicalPosition::new(0, CHROME_HEIGHT).into(),
                size: WryLogicalSize::new(logical_size.width.max(1), content_height).into(),
            })
            .map_err(|err| format!("failed to resize webview: {err}"))
    }

    fn handle_browser_event(&mut self, event: BrowserEvent) {
        match event {
            BrowserEvent::NavigationStarted(url) => {
                if self.should_ignore_navigation_event(&url) {
                    return;
                }
                if let Some(tab) = self.active_tab_mut() {
                    tab.current_url = url.clone();
                    tab.source = url.clone();
                    tab.is_loading = true;
                }
                if !self.address_focus {
                    self.address_input = url.clone();
                    self.address_cursor = self.address_char_count();
                    self.clear_address_selection();
                }
                self.status_message = Some(format!("Loading {url}"));
            }
            BrowserEvent::PageFinished(url) => {
                if self.should_ignore_navigation_event(&url) {
                    return;
                }
                if let Some(tab) = self.active_tab_mut() {
                    tab.current_url = url.clone();
                    tab.source = url.clone();
                    tab.is_loading = false;
                }
                self.record_active_tab_history(&url);
                if !self.address_focus {
                    self.address_input = url;
                    self.address_cursor = self.address_char_count();
                    self.clear_address_selection();
                }
                self.status_message = Some("Page loaded".to_string());
                self.request_top_level_url();
            }
            BrowserEvent::TopLevelUrlResolved(url) => {
                if self.should_ignore_navigation_event(&url) {
                    return;
                }
                if let Some(tab) = self.active_tab_mut() {
                    tab.current_url = url.clone();
                    tab.source = url.clone();
                }
                self.record_active_tab_history(&url);
                if !self.address_focus {
                    self.address_input = url;
                    self.address_cursor = self.address_char_count();
                    self.clear_address_selection();
                }
            }
            BrowserEvent::TitleChanged(title) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.title = title;
                }
                self.update_window_title();
            }
            BrowserEvent::OpenInNewTab(url) => {
                self.open_background_tab(url);
            }
        }
        self.request_redraw();
    }

    fn should_ignore_navigation_event(&self, url: &str) -> bool {
        let Some(tab) = self.active_tab() else {
            return false;
        };

        should_ignore_navigation_for_tab(tab, url)
    }

    fn request_top_level_url(&self) {
        let Some(webview) = self.webview.as_ref() else {
            return;
        };
        let proxy = self.proxy.clone();
        let _ = webview.evaluate_script_with_callback("window.location.href", move |value| {
            if let Some(url) = parse_js_string_result(&value) {
                let _ = proxy.send_event(BrowserEvent::TopLevelUrlResolved(url));
            }
        });
    }

    fn open_background_tab(&mut self, requested: String) {
        let normalized = source::normalize_browser_url(&requested).unwrap_or(requested.clone());
        self.tabs.push(TabState {
            source: normalized.clone(),
            current_url: normalized.clone(),
            title: requested,
            is_loading: false,
            history: vec![normalized],
            history_index: 0,
        });
        self.status_message = Some("Opened link in new tab".to_string());
    }
}

fn should_ignore_navigation_for_tab(tab: &TabState, url: &str) -> bool {
    (is_internal_about_url(url)
        && !is_internal_about_url(&tab.current_url)
        && !tab.source.is_empty())
        || is_cloudflare_challenge_url_for_other_host(tab, url)
}

fn record_history_for_tab(tab: &mut TabState, url: &str) {
    if tab
        .history
        .get(tab.history_index)
        .is_some_and(|current| current == url)
    {
        return;
    }

    if tab.history_index + 1 < tab.history.len() {
        tab.history.truncate(tab.history_index + 1);
    }
    tab.history.push(url.to_string());
    tab.history_index = tab.history.len().saturating_sub(1);
}

fn is_internal_about_url(url: &str) -> bool {
    url == "about:blank" || url.starts_with("about:srcdoc")
}

fn is_cloudflare_challenge_url_for_other_host(tab: &TabState, url: &str) -> bool {
    let Ok(candidate) = Url::parse(url) else {
        return false;
    };
    if candidate.host_str() != Some("challenges.cloudflare.com")
        || !candidate.path().starts_with("/cdn-cgi/challenge-platform/")
    {
        return false;
    }

    let current_host = Url::parse(&tab.current_url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string));
    current_host.as_deref() != Some("challenges.cloudflare.com")
}

impl ApplicationHandler<BrowserEvent> for WebViewApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = Window::default_attributes()
            .with_title("Browser")
            .with_inner_size(LogicalSize::new(
                self.viewport_width as f64,
                self.viewport_height as f64,
            ))
            .with_min_inner_size(LogicalSize::new(720.0, 540.0));

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("failed to create window: {err}");
                event_loop.exit();
                return;
            }
        };

        let window_size = window.inner_size();
        self.viewport_width = window_size.width.max(1);
        self.viewport_height = window_size.height.max(1);

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

        let proxy = self.proxy.clone();
        let initial_url = self
            .active_tab()
            .map(|tab| tab.current_url.clone())
            .unwrap_or_else(|| "about:blank".to_string());
        let webview = match WebViewBuilder::new()
            .with_url(&initial_url)
            .with_bounds(Rect {
                position: WryLogicalPosition::new(0, CHROME_HEIGHT).into(),
                size: WryLogicalSize::new(
                    window_size.width.max(1),
                    window_size.height.saturating_sub(CHROME_HEIGHT).max(1),
                )
                .into(),
            })
            .with_back_forward_navigation_gestures(true)
            .with_navigation_handler(move |url| {
                let _ = proxy.send_event(BrowserEvent::NavigationStarted(url));
                true
            })
            .with_new_window_req_handler({
                let proxy = self.proxy.clone();
                move |url, _features| {
                    let _ = proxy.send_event(BrowserEvent::OpenInNewTab(url));
                    NewWindowResponse::Deny
                }
            })
            .with_on_page_load_handler({
                let proxy = self.proxy.clone();
                move |event, url| {
                    if matches!(event, wry::PageLoadEvent::Finished) {
                        let _ = proxy.send_event(BrowserEvent::PageFinished(url));
                    }
                }
            })
            .with_document_title_changed_handler({
                let proxy = self.proxy.clone();
                move |title| {
                    let _ = proxy.send_event(BrowserEvent::TitleChanged(title));
                }
            })
            .build_as_child(&window)
        {
            Ok(webview) => webview,
            Err(err) => {
                eprintln!("failed to create webview: {err}");
                event_loop.exit();
                return;
            }
        };

        window.set_ime_allowed(false);
        window.request_redraw();
        self.pixels = Some(pixels);
        self.webview = Some(webview);
        self.window = Some(window);
        let _ = self.resize_webview();
        self.update_window_title();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: BrowserEvent) {
        self.handle_browser_event(event);
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
                if let Some(pixels) = self.pixels.as_mut() {
                    if let Err(err) = pixels.resize_surface(size.width, size.height) {
                        eprintln!("failed to resize surface: {err}");
                        event_loop.exit();
                        return;
                    }
                    if let Err(err) = pixels.resize_buffer(size.width.max(1), size.height.max(1)) {
                        eprintln!("failed to resize buffer: {err}");
                        event_loop.exit();
                        return;
                    }
                }
                if let Err(err) = self.resize_webview() {
                    eprintln!("{err}");
                    event_loop.exit();
                    return;
                }
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Err(err) = self.resize_webview() {
                    eprintln!("{err}");
                    event_loop.exit();
                    return;
                }
                self.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if matches!(event.logical_key.as_ref(), Key::Character(ch) if ch.eq_ignore_ascii_case("l"))
                    && (self.modifiers.control_key() || self.modifiers.super_key())
                {
                    self.set_address_focus(true);
                    if let Some(tab) = self.active_tab() {
                        self.address_input = tab.source.clone();
                    }
                    self.address_cursor = self.address_char_count();
                    self.select_all_address();
                    self.status_message = Some("Editing address".to_string());
                    self.request_redraw();
                    return;
                }

                if matches!(event.logical_key.as_ref(), Key::Character(ch) if ch.eq_ignore_ascii_case("t"))
                    && (self.modifiers.control_key() || self.modifiers.super_key())
                {
                    self.open_new_tab();
                    return;
                }

                if matches!(event.logical_key.as_ref(), Key::Character(ch) if ch.eq_ignore_ascii_case("w"))
                    && (self.modifiers.control_key() || self.modifiers.super_key())
                {
                    let current = self.active_tab;
                    self.close_tab(current);
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
                                self.navigate_to_input(requested);
                            }
                        }
                        Key::Named(NamedKey::Escape) => {
                            self.set_address_focus(false);
                            if let Some(tab) = self.active_tab() {
                                self.address_input = tab.source.clone();
                            }
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

                match event.logical_key.as_ref() {
                    Key::Character(ch)
                        if ch.eq_ignore_ascii_case("w")
                            && (self.modifiers.control_key() || self.modifiers.super_key()) =>
                    {
                        let current = self.active_tab;
                        self.close_tab(current);
                    }
                    Key::Character(ch)
                        if ch.eq_ignore_ascii_case("r")
                            && (self.modifiers.control_key() || self.modifiers.super_key()) =>
                    {
                        self.reload();
                    }
                    Key::Named(NamedKey::ArrowLeft) if self.modifiers.alt_key() => {
                        self.history_back();
                    }
                    Key::Named(NamedKey::ArrowRight) if self.modifiers.alt_key() => {
                        self.history_forward();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
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

    fn font_for_weight(&self, font_weight: FontWeight) -> Option<&Font> {
        match font_weight {
            FontWeight::Bold => self.bold.as_ref().or(self.regular.as_ref()),
            FontWeight::Normal => self.regular.as_ref().or(self.bold.as_ref()),
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
        font_weight: FontWeight,
        font_size: f32,
    ) {
        let font = match font_weight {
            FontWeight::Bold => self.bold.as_ref().or(self.regular.as_ref()),
            FontWeight::Normal => self.regular.as_ref().or(self.bold.as_ref()),
        };

        if let Some(font) = font {
            draw_text_fontdue(frame, width, height, x, y, text, color, font, font_size);
        } else {
            draw_text_bitmap(frame, width, height, x, y, text, color, font_weight, 1);
        }
    }

    fn layout_text_glyphs(
        &self,
        text: &str,
        font_weight: FontWeight,
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
        font_weight: FontWeight,
    ) -> bool {
        let Some(font) = self.font_for_weight(font_weight) else {
            return false;
        };
        draw_positioned_glyphs(frame, width, height, glyphs, color, font);
        true
    }

    fn exact_text_width(&self, text: &str, font_weight: FontWeight, font_size: usize) -> i32 {
        let font_size = font_size.max(1) as f32;
        let Some(font) = self.font_for_weight(font_weight) else {
            return text.chars().count() as i32 * CHAR_WIDTH as i32;
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
    tabs: &[TabState],
    active_tab: usize,
    is_loading: bool,
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
        CHROME_HEIGHT as i32,
        Color::rgb(232, 228, 220),
    );

    for (index, tab) in tabs.iter().enumerate() {
        if let Some(rect) = tab_rect(index, width, tabs.len()) {
            draw_rect(
                frame,
                width,
                height,
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                if index == active_tab {
                    Color::rgb(250, 248, 243)
                } else {
                    Color::rgb(223, 219, 210)
                },
            );

            let label = if tab.title.is_empty() {
                "New Tab".to_string()
            } else if tab.title.chars().count() > 24 {
                let mut shortened = tab.title.chars().take(21).collect::<String>();
                shortened.push_str("...");
                shortened
            } else {
                tab.title.clone()
            };

            text_rasterizer.draw_text(
                frame,
                width,
                height,
                rect.x + 10,
                rect.y + 6,
                &label,
                Color::rgb(52, 56, 64),
                if index == active_tab {
                    FontWeight::Bold
                } else {
                    FontWeight::Normal
                },
                14.0,
            );

            let close_rect = tab_close_rect(index, width, tabs.len());
            draw_rect(
                frame,
                width,
                height,
                close_rect.x,
                close_rect.y,
                close_rect.width,
                close_rect.height,
                if index == active_tab {
                    Color::rgb(233, 228, 220)
                } else {
                    Color::rgb(214, 209, 201)
                },
            );
            text_rasterizer.draw_text(
                frame,
                width,
                height,
                close_rect.x + 4,
                close_rect.y + 1,
                "x",
                Color::rgb(92, 96, 104),
                FontWeight::Bold,
                12.0,
            );
        }
    }

    let plus_rect = new_tab_button_rect(width);
    draw_rect(
        frame,
        width,
        height,
        plus_rect.x,
        plus_rect.y,
        plus_rect.width,
        plus_rect.height,
        Color::rgb(246, 243, 236),
    );
    text_rasterizer.draw_text(
        frame,
        width,
        height,
        plus_rect.x + 8,
        plus_rect.y + 5,
        "+",
        Color::rgb(72, 76, 84),
        FontWeight::Bold,
        18.0,
    );

    draw_rect(
        frame,
        width,
        height,
        0,
        TAB_BAR_HEIGHT as i32,
        width as i32,
        1,
        Color::rgb(210, 205, 196),
    );

    for action in [NavAction::Back, NavAction::Forward, NavAction::Reload] {
        let rect = nav_button_rect(action);
        draw_rect(
            frame,
            width,
            height,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            if is_loading && matches!(action, NavAction::Reload) {
                Color::rgb(224, 232, 248)
            } else {
                Color::rgb(246, 243, 236)
            },
        );

        let symbol = match action {
            NavAction::Back => "<",
            NavAction::Forward => ">",
            NavAction::Reload => "R",
        };
        let symbol_color = if is_loading && matches!(action, NavAction::Reload) {
            Color::rgb(36, 88, 164)
        } else {
            Color::rgb(88, 92, 100)
        };
        text_rasterizer.draw_text(
            frame,
            width,
            height,
            rect.x + 9,
            rect.y + 6,
            symbol,
            symbol_color,
            FontWeight::Bold,
            16.0,
        );
    }

    draw_rect(
        frame,
        width,
        height,
        ADDRESS_BAR_LEFT_OFFSET as i32,
        nav_row_y() as i32 + 4,
        width.saturating_sub(ADDRESS_BAR_LEFT_OFFSET + ADDRESS_BAR_PADDING) as i32,
        ADDRESS_BAR_HEIGHT as i32,
        if address_focus {
            Color::rgb(255, 255, 255)
        } else {
            Color::rgb(246, 243, 236)
        },
    );

    let layout =
        build_address_text_layout(text_rasterizer, address_input, preedit_text, address_focus);

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
            nav_row_y() as i32 + 6,
            selection_width.max(1),
            ADDRESS_BAR_HEIGHT.saturating_sub(4) as i32,
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
            FontWeight::Normal,
        ) {
            text_rasterizer.draw_text(
                frame,
                width,
                height,
                layout.text_x,
                layout.text_y,
                &layout.display_text,
                Color::rgb(32, 35, 40),
                FontWeight::Normal,
                17.0,
            );
        }
    }

    if address_focus && caret_visible {
        let caret_x = layout.x_for_char_index(text_rasterizer, address_cursor);
        draw_rect(
            frame,
            width,
            height,
            caret_x,
            nav_row_y() as i32 + 8,
            2,
            ADDRESS_BAR_HEIGHT.saturating_sub(8) as i32,
            Color::rgb(46, 50, 56),
        );
    }

    if let Some(message) = status_message {
        let rendered_message = if is_loading {
            format!("{message}...")
        } else {
            message.to_string()
        };
        text_rasterizer.draw_text(
            frame,
            width,
            height,
            (ADDRESS_BAR_PADDING + 8) as i32,
            88,
            &rendered_message,
            Color::rgb(96, 100, 110),
            FontWeight::Normal,
            13.0,
        );
    }
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
    font_weight: FontWeight,
    scale: u32,
) {
    let mut cursor_x = x;
    for ch in text.chars() {
        if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    if (bits >> col) & 1 == 1 {
                        let px = cursor_x + col * scale as i32;
                        let py = y + row as i32 * scale as i32;
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
                        if font_weight == FontWeight::Bold {
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
) {
    for glyph in layout_text_glyphs(font, text, font_size, x, y) {
        let (metrics, bitmap) = font.rasterize_config(glyph.key);
        draw_glyph_bitmap(
            frame,
            width,
            height,
            glyph.x.round() as i32,
            glyph.y.round() as i32,
            metrics.width,
            metrics.height,
            &bitmap,
            color,
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
) {
    for glyph in glyphs {
        let (metrics, bitmap) = font.rasterize_config(glyph.key);
        draw_glyph_bitmap(
            frame,
            width,
            height,
            glyph.x.round() as i32,
            glyph.y.round() as i32,
            metrics.width,
            metrics.height,
            &bitmap,
            color,
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
    layout.reset(&LayoutSettings {
        x: x as f32,
        y: y as f32,
        line_height: 1.2,
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
) {
    for row in 0..glyph_height {
        for col in 0..glyph_width {
            let coverage = bitmap[row * glyph_width + col];
            if coverage == 0 {
                continue;
            }
            blend_pixel(
                frame,
                width,
                height,
                x + col as i32,
                y + row as i32,
                color,
                coverage,
            );
        }
    }
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

struct AddressTextLayout {
    display_text: String,
    font_size: usize,
    text_x: i32,
    text_y: i32,
    prefix_len_bytes: usize,
    address_input_len_bytes: usize,
    glyphs: Option<Vec<GlyphPosition>>,
}

fn build_address_text_layout(
    text_rasterizer: &TextRasterizer,
    address_input: &str,
    preedit_text: &str,
    address_focus: bool,
) -> AddressTextLayout {
    let text_x = (ADDRESS_BAR_LEFT_OFFSET + 8) as i32;
    let text_y = nav_row_y() as i32 + 12;
    let prefix = if address_focus { "> " } else { "" };
    let display_text = format!("{prefix}{address_input}{preedit_text}");
    let font_size = 17usize;
    let glyphs = text_rasterizer
        .layout_text_glyphs(&display_text, FontWeight::Normal, font_size, text_x, text_y)
        .map(|mut glyphs| {
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
        self.text_x + text_rasterizer.exact_text_width(prefix, FontWeight::Normal, self.font_size)
    }

    fn x_for_char_index(&self, text_rasterizer: &TextRasterizer, char_index: usize) -> i32 {
        let address_only = &self.display_text
            [self.prefix_len_bytes..self.prefix_len_bytes + self.address_input_len_bytes];
        let byte_offset = self.address_byte_offset_for_char_index(address_only, char_index);
        self.x_for_byte_offset(text_rasterizer, byte_offset)
    }

    fn char_index_from_x(&self, text_rasterizer: &TextRasterizer, x: f64) -> usize {
        let address_only = &self.display_text
            [self.prefix_len_bytes..self.prefix_len_bytes + self.address_input_len_bytes];
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

fn address_bar_hit_test(x: f64, y: f64, viewport_width: u32) -> bool {
    let min_x = ADDRESS_BAR_LEFT_OFFSET as f64;
    let max_x = viewport_width.saturating_sub(ADDRESS_BAR_PADDING) as f64;
    let min_y = (nav_row_y() + 4) as f64;
    let max_y = (nav_row_y() + 4 + ADDRESS_BAR_HEIGHT) as f64;
    x >= min_x && x <= max_x && y >= min_y && y <= max_y
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NavAction {
    Back,
    Forward,
    Reload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ButtonRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

fn nav_button_rect(action: NavAction) -> ButtonRect {
    let index = match action {
        NavAction::Back => 0,
        NavAction::Forward => 1,
        NavAction::Reload => 2,
    };
    ButtonRect {
        x: (NAV_BUTTON_MARGIN_LEFT + index * (NAV_BUTTON_SIZE + NAV_BUTTON_SPACING)) as i32,
        y: nav_row_y() as i32 + 7,
        width: NAV_BUTTON_SIZE as i32,
        height: NAV_BUTTON_SIZE as i32,
    }
}

fn nav_row_y() -> u32 {
    TAB_BAR_HEIGHT
}

fn tab_rect(index: usize, viewport_width: u32, tab_count: usize) -> Option<ButtonRect> {
    if tab_count == 0 {
        return None;
    }
    let available_width = viewport_width
        .saturating_sub(NEW_TAB_BUTTON_SIZE + ADDRESS_BAR_PADDING * 3)
        .max(TAB_MIN_WIDTH);
    let total_gaps = TAB_GAP.saturating_mul(tab_count.saturating_sub(1) as u32);
    let width_per_tab = ((available_width.saturating_sub(total_gaps)) / tab_count as u32)
        .clamp(TAB_MIN_WIDTH, TAB_MAX_WIDTH);
    let x = ADDRESS_BAR_PADDING + index as u32 * (width_per_tab + TAB_GAP);
    Some(ButtonRect {
        x: x as i32,
        y: 6,
        width: width_per_tab as i32,
        height: TAB_HEIGHT as i32,
    })
}

fn tab_hit_test(x: f64, y: f64, viewport_width: u32, tab_count: usize) -> Option<usize> {
    for index in 0..tab_count {
        if let Some(rect) = tab_rect(index, viewport_width, tab_count) {
            if x >= rect.x as f64
                && x <= (rect.x + rect.width) as f64
                && y >= rect.y as f64
                && y <= (rect.y + rect.height) as f64
            {
                return Some(index);
            }
        }
    }
    None
}

fn tab_close_rect(index: usize, viewport_width: u32, tab_count: usize) -> ButtonRect {
    let rect = tab_rect(index, viewport_width, tab_count).unwrap_or(ButtonRect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    });
    ButtonRect {
        x: rect.x + rect.width - 20,
        y: rect.y + 4,
        width: 14,
        height: 14,
    }
}

fn tab_close_hit_test(x: f64, y: f64, viewport_width: u32, tab_count: usize) -> Option<usize> {
    for index in 0..tab_count {
        let rect = tab_close_rect(index, viewport_width, tab_count);
        if x >= rect.x as f64
            && x <= (rect.x + rect.width) as f64
            && y >= rect.y as f64
            && y <= (rect.y + rect.height) as f64
        {
            return Some(index);
        }
    }
    None
}

fn new_tab_button_rect(viewport_width: u32) -> ButtonRect {
    ButtonRect {
        x: viewport_width.saturating_sub(ADDRESS_BAR_PADDING + NEW_TAB_BUTTON_SIZE) as i32,
        y: 6,
        width: NEW_TAB_BUTTON_SIZE as i32,
        height: NEW_TAB_BUTTON_SIZE as i32,
    }
}

fn new_tab_button_hit_test(x: f64, y: f64, viewport_width: u32) -> bool {
    let rect = new_tab_button_rect(viewport_width);
    x >= rect.x as f64
        && x <= (rect.x + rect.width) as f64
        && y >= rect.y as f64
        && y <= (rect.y + rect.height) as f64
}

fn nav_button_hit_test(x: f64, y: f64) -> Option<NavAction> {
    for action in [NavAction::Back, NavAction::Forward, NavAction::Reload] {
        let rect = nav_button_rect(action);
        if x >= rect.x as f64
            && x <= (rect.x + rect.width) as f64
            && y >= rect.y as f64
            && y <= (rect.y + rect.height) as f64
        {
            return Some(action);
        }
    }
    None
}

fn address_bar_cursor_from_position(text_rasterizer: &TextRasterizer, text: &str, x: f64) -> usize {
    let layout = build_address_text_layout(text_rasterizer, text, "", true);
    layout.char_index_from_x(text_rasterizer, x)
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

fn sanitize_clipboard_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_js_string_result(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "null" || trimmed == "undefined" {
        return None;
    }

    if let Some(stripped) = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        let mut decoded = String::new();
        let mut chars = stripped.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('"') => decoded.push('"'),
                    Some('\\') => decoded.push('\\'),
                    Some('/') => decoded.push('/'),
                    Some('b') => decoded.push('\u{0008}'),
                    Some('f') => decoded.push('\u{000C}'),
                    Some('n') => decoded.push('\n'),
                    Some('r') => decoded.push('\r'),
                    Some('t') => decoded.push('\t'),
                    Some('u') => {
                        let hex = chars.by_ref().take(4).collect::<String>();
                        if let Ok(code) = u16::from_str_radix(&hex, 16) {
                            if let Some(decoded_char) = char::from_u32(code as u32) {
                                decoded.push(decoded_char);
                            }
                        }
                    }
                    Some(other) => decoded.push(other),
                    None => break,
                }
            } else {
                decoded.push(ch);
            }
        }
        return Some(decoded);
    }

    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        address_bar_cursor_from_position, address_bar_hit_test, address_slice,
        build_address_text_layout, nav_button_hit_test, new_tab_button_hit_test,
        parse_js_string_result, record_history_for_tab, sanitize_clipboard_text,
        should_ignore_navigation_for_tab, tab_close_hit_test, tab_hit_test, NavAction, TabState,
        TextRasterizer,
    };

    #[test]
    fn detects_address_bar_hits() {
        assert!(address_bar_hit_test(140.0, 50.0, 900));
        assert!(!address_bar_hit_test(30.0, 90.0, 900));
    }

    #[test]
    fn detects_nav_button_hits() {
        assert_eq!(nav_button_hit_test(18.0, 50.0), Some(NavAction::Back));
        assert_eq!(nav_button_hit_test(58.0, 50.0), Some(NavAction::Forward));
        assert_eq!(nav_button_hit_test(98.0, 50.0), Some(NavAction::Reload));
        assert_eq!(nav_button_hit_test(200.0, 50.0), None);
    }

    #[test]
    fn detects_tab_hits() {
        assert_eq!(tab_hit_test(24.0, 12.0, 900, 2), Some(0));
        assert_eq!(tab_hit_test(260.0, 12.0, 900, 2), Some(1));
        assert_eq!(tab_hit_test(700.0, 12.0, 900, 2), None);
    }

    #[test]
    fn detects_new_tab_button_hits() {
        assert!(new_tab_button_hit_test(870.0, 18.0, 900));
        assert!(!new_tab_button_hit_test(820.0, 18.0, 900));
    }

    #[test]
    fn detects_tab_close_hits() {
        assert_eq!(tab_close_hit_test(216.0, 14.0, 900, 2), Some(0));
        assert_eq!(tab_close_hit_test(446.0, 14.0, 900, 2), Some(1));
        assert_eq!(tab_close_hit_test(700.0, 14.0, 900, 2), None);
    }

    #[test]
    fn ignores_stale_about_blank_events_for_non_blank_tabs() {
        let tab = TabState {
            source: "https://example.com".to_string(),
            current_url: "https://example.com/".to_string(),
            title: "Example".to_string(),
            is_loading: true,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
        };
        assert!(should_ignore_navigation_for_tab(&tab, "about:blank"));
        assert!(should_ignore_navigation_for_tab(&tab, "about:srcdoc"));
        assert!(!should_ignore_navigation_for_tab(
            &tab,
            "https://example.com/next"
        ));
        assert!(should_ignore_navigation_for_tab(
            &tab,
            "https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/g/turnstile/f/ov2/av0/rch/test/0x4AAAAA/auto/fbE/new/normal?lang=auto"
        ));
    }

    #[test]
    fn parses_js_string_results() {
        assert_eq!(
            parse_js_string_result("\"https://example.com/path\""),
            Some("https://example.com/path".to_string())
        );
        assert_eq!(
            parse_js_string_result("\"https:\\/\\/example.com\\/a\\n\""),
            Some("https://example.com/a\n".to_string())
        );
        assert_eq!(parse_js_string_result("null"), None);
    }

    #[test]
    fn records_tab_history_without_duplicates() {
        let mut tab = TabState {
            source: "https://example.com".to_string(),
            current_url: "https://example.com/".to_string(),
            title: "Example".to_string(),
            is_loading: false,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
        };

        record_history_for_tab(&mut tab, "https://example.com/");
        record_history_for_tab(&mut tab, "https://example.com/docs");
        record_history_for_tab(&mut tab, "https://example.com/docs");

        assert_eq!(
            tab.history,
            vec![
                "https://example.com/".to_string(),
                "https://example.com/docs".to_string()
            ]
        );
        assert_eq!(tab.history_index, 1);
    }

    #[test]
    fn cursor_hit_testing_maps_points_to_character_indices() {
        let text_rasterizer = TextRasterizer::load();
        let text = "hello";
        let start_x = (super::ADDRESS_BAR_LEFT_OFFSET + 8) as f64;

        assert_eq!(
            address_bar_cursor_from_position(&text_rasterizer, text, start_x + 2.0),
            0
        );
        assert_eq!(
            address_bar_cursor_from_position(&text_rasterizer, text, start_x + 80.0),
            5
        );
    }

    #[test]
    fn address_layout_keeps_first_glyph_inside_padding() {
        let text_rasterizer = TextRasterizer::load();
        let layout = build_address_text_layout(&text_rasterizer, "example.com", "", true);
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
    fn sanitizes_clipboard_text_for_address_bar() {
        assert_eq!(
            sanitize_clipboard_text(" https://example.com/\npath\t?q=1 "),
            "https://example.com/ path ?q=1"
        );
    }

    #[test]
    fn address_slice_returns_selected_range() {
        assert_eq!(address_slice("abcdef", 1, 4), "bcd");
    }
}
