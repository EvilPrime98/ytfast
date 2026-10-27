//! Single-line text field. GPUI core has no text input, so this handles typing,
//! caret movement (arrows, Ctrl+arrows for words, Home/End), selection (shift+arrows,
//! select all, mouse click and drag), copy/cut/paste, backspace/delete and Enter/Escape.
//! The text is shaped as one line by a custom element, which gives pixel-accurate hit-testing,
//! a blinking caret and horizontal scrolling that keeps the caret in view.
//! Known gaps: no IME composition, caret is char-based (not grapheme-based).

use std::time::Duration;

use gpui::{
    App, Bounds, ClipboardItem, Context, DispatchPhase, Element, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, InspectorElementId, InteractiveElement, IntoElement,
    KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    ParentElement, Pixels, Point, Render, ShapedLine, SharedString, Styled, Task, TextRun, Window,
    div, fill, point, px, relative, size,
};

use crate::ui::theme;

const BLINK_INTERVAL: Duration = Duration::from_millis(530);

pub enum TextInputEvent {
    Changed,
    Submit,
    Cancel,
}

pub struct TextInput {
    value: String,
    /// Caret position in chars.
    caret: usize,
    /// Other end of the selection (in chars); `None` or equal to `caret` means no selection.
    anchor: Option<usize>,
    placeholder: SharedString,
    focus: FocusHandle,
    scroll_x: Pixels,
    line: Option<ShapedLine>,
    bounds: Bounds<Pixels>,
    dragging: bool,
    blink_on: bool,
    blink_task: Option<Task<()>>,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            value: String::new(),
            caret: 0,
            anchor: None,
            placeholder: placeholder.into(),
            focus: cx.focus_handle(),
            scroll_x: px(0.0),
            line: None,
            bounds: Bounds::default(),
            dragging: false,
            blink_on: true,
            blink_task: None,
        }
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self.caret = self.len();
        self
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    fn len(&self) -> usize {
        self.value.chars().count()
    }

    fn byte_index(&self, chars: usize) -> usize {
        self.value
            .char_indices()
            .nth(chars)
            .map(|(i, _)| i)
            .unwrap_or(self.value.len())
    }

    /// Selected range in chars, ordered.
    fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        (anchor != self.caret).then(|| (anchor.min(self.caret), anchor.max(self.caret)))
    }

    fn selected_text(&self) -> Option<String> {
        let (from, to) = self.selection()?;
        Some(self.value[self.byte_index(from)..self.byte_index(to)].to_string())
    }

    /// Delete the selection, leaving the caret where it was. Returns whether anything was removed.
    fn delete_selection(&mut self) -> bool {
        let Some((from, to)) = self.selection() else {
            return false;
        };
        let (b_from, b_to) = (self.byte_index(from), self.byte_index(to));
        self.value.replace_range(b_from..b_to, "");
        self.caret = from;
        self.anchor = None;
        true
    }

    fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        // Single line: drop control characters and newlines.
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        self.delete_selection();
        let at = self.byte_index(self.caret);
        self.value.insert_str(at, &clean);
        self.caret += clean.chars().count();
        cx.emit(TextInputEvent::Changed);
    }

    /// Move the caret, extending the selection when `extend` is set.
    fn move_caret(&mut self, to: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = to;
    }

    fn reset_blink(&mut self, cx: &mut Context<Self>) {
        self.blink_on = true;
        self.blink_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK_INTERVAL).await;
                let alive = this.update(cx, |this, cx| {
                    this.blink_on = !this.blink_on;
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }

    fn char_at(&self, position: Point<Pixels>) -> usize {
        let Some(line) = &self.line else {
            return self.len();
        };
        let x = position.x - self.bounds.left() + self.scroll_x;
        let byte = line.closest_index_for_x(x).min(self.value.len());
        if self.value.is_char_boundary(byte) {
            self.value[..byte].chars().count()
        } else {
            self.len()
        }
    }

    fn prev_word(&self, from: usize) -> usize {
        let chars: Vec<char> = self.value.chars().collect();
        let mut i = from.min(chars.len());
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    fn next_word(&self, from: usize) -> usize {
        let chars: Vec<char> = self.value.chars().collect();
        let mut i = from.min(chars.len());
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        i
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let len = self.len();
        let command = ks.modifiers.control || ks.modifiers.platform;
        let shift = ks.modifiers.shift;
        match ks.key.as_str() {
            "enter" => cx.emit(TextInputEvent::Submit),
            "escape" => {
                cx.emit(TextInputEvent::Cancel);
                window.blur();
            }
            "backspace" => {
                if !self.delete_selection() {
                    if self.caret == 0 {
                        return cx.stop_propagation();
                    }
                    let (from, to) = (self.byte_index(self.caret - 1), self.byte_index(self.caret));
                    self.value.replace_range(from..to, "");
                    self.caret -= 1;
                }
                cx.emit(TextInputEvent::Changed);
            }
            "delete" => {
                if !self.delete_selection() {
                    if self.caret >= len {
                        return cx.stop_propagation();
                    }
                    let (from, to) = (self.byte_index(self.caret), self.byte_index(self.caret + 1));
                    self.value.replace_range(from..to, "");
                }
                cx.emit(TextInputEvent::Changed);
            }
            "left" if command => self.move_caret(self.prev_word(self.caret), shift),
            "right" if command => self.move_caret(self.next_word(self.caret), shift),
            "left" => match self.selection() {
                Some((from, _)) if !shift => {
                    self.anchor = None;
                    self.caret = from;
                }
                _ => self.move_caret(self.caret.saturating_sub(1), shift),
            },
            "right" => match self.selection() {
                Some((_, to)) if !shift => {
                    self.anchor = None;
                    self.caret = to;
                }
                _ => self.move_caret((self.caret + 1).min(len), shift),
            },
            "home" => self.move_caret(0, shift),
            "end" => self.move_caret(len, shift),
            "a" if command => {
                self.anchor = Some(0);
                self.caret = len;
            }
            "c" if command => {
                if let Some(text) = self.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            "x" if command => {
                if let Some(text) = self.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    self.delete_selection();
                    cx.emit(TextInputEvent::Changed);
                }
            }
            "v" if command => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    self.insert(&text, cx);
                }
            }
            _ if command || ks.modifiers.alt => return,
            _ => match &ks.key_char {
                Some(ch) => self.insert(ch, cx),
                None => return,
            },
        }
        self.reset_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus.is_focused(window) {
            if self.blink_task.is_none() {
                self.reset_blink(cx);
            }
        } else if self.blink_task.take().is_some() {
            self.blink_on = true;
        }

        div()
            .id("text-input")
            .key_context("TextInput")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e, window, cx| this.on_key(e, window, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    // Keep the root from taking the focus back.
                    cx.stop_propagation();
                    window.focus(&this.focus, cx);
                    this.caret = this.char_at(e.position);
                    this.anchor = Some(this.caret);
                    this.dragging = true;
                    this.reset_blink(cx);
                    cx.notify();
                }),
            )
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .overflow_hidden()
            .text_size(px(13.0))
            .text_color(theme::text())
            .cursor_text()
            .child(InputText { input: cx.entity() })
    }
}

struct InputText {
    input: Entity<TextInput>,
}

struct InputTextPrepaint {
    line: ShapedLine,
    scroll_x: Pixels,
    selection: Option<PaintQuad>,
    caret: Option<PaintQuad>,
}

impl IntoElement for InputText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for InputText {
    type RequestLayoutState = ();
    type PrepaintState = InputTextPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = gpui::Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let input = self.input.read(cx);
        let focused = input.focus.is_focused(window);
        let caret_color = theme::accent();

        let shape = |text: SharedString, color: gpui::Hsla, window: &mut Window| {
            let run = TextRun {
                len: text.len(),
                font: style.font(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
                letter_spacing: None,
            };
            window
                .text_system()
                .shape_line(text, font_size, &[run], None)
        };

        let line = if input.value.is_empty() {
            shape(
                input.placeholder.clone(),
                gpui::rgb_to_hsla(gpui::rgb(0x8a8a8a)),
                window,
            )
        } else {
            shape(input.value.clone().into(), style.color, window)
        };
        let x_of = |chars: usize| line.x_for_index(input.byte_index(chars));
        let caret_x = x_of(input.caret);

        let mut scroll_x = input.scroll_x;
        if caret_x - scroll_x < px(0.0) {
            scroll_x = caret_x;
        } else if caret_x - scroll_x > bounds.size.width - px(1.0) {
            scroll_x = caret_x - bounds.size.width + px(1.0);
        }
        let max_scroll = (line.width() + px(1.0) - bounds.size.width).max(px(0.0));
        scroll_x = scroll_x.max(px(0.0)).min(max_scroll);

        let selection = input.selection().map(|(from, to)| {
            let color = if focused {
                theme::accent_soft()
            } else {
                gpui::rgba(0x8a8a8a55)
            };
            fill(
                Bounds::new(
                    point(bounds.left() + x_of(from) - scroll_x, bounds.top()),
                    size(x_of(to) - x_of(from), line_height),
                ),
                color,
            )
        });
        let caret = (focused && input.blink_on).then(|| {
            fill(
                Bounds::new(
                    point(bounds.left() + caret_x - scroll_x, bounds.top()),
                    size(px(1.0), line_height),
                ),
                caret_color,
            )
        });

        let stored = line.clone();
        self.input.update(cx, |input, _| {
            input.scroll_x = scroll_x;
            input.line = Some(stored);
            input.bounds = bounds;
        });
        InputTextPrepaint {
            line,
            scroll_x,
            selection,
            caret,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let input = self.input.clone();
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            input.update(cx, |this, cx| {
                if !this.dragging {
                    return;
                }
                if e.pressed_button != Some(MouseButton::Left) {
                    this.dragging = false;
                    return;
                }
                let at = this.char_at(e.position);
                if at != this.caret {
                    this.caret = at;
                    this.reset_blink(cx);
                    cx.notify();
                }
            });
        });
        let input = self.input.clone();
        window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                input.update(cx, |this, _| this.dragging = false);
            }
        });

        let line_height = window.line_height();
        let origin = point(bounds.left() - prepaint.scroll_x, bounds.top());
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection);
            }
            prepaint
                .line
                .paint(origin, line_height, gpui::TextAlign::Left, None, window, cx)
                .ok();
            if let Some(caret) = prepaint.caret.take() {
                window.paint_quad(caret);
            }
        });
    }
}
