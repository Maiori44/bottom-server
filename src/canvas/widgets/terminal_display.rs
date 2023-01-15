use tui::{
    backend::Backend,
    layout::Rect,
    terminal::Frame,
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;

use crate::{app::App, canvas::Painter, constants::*};

impl Painter {
    pub fn draw_terminal_display<B: Backend>(
        &self, f: &mut Frame<'_, B>, app_state: &mut App, draw_loc: Rect, draw_border: bool,
        widget_id: u64,
    ) {
        let should_get_widget_bounds = app_state.should_get_widget_bounds();
        if let Some(terminal_widget_state) =
            app_state.terminal_state.widget_states.get_mut(&widget_id)
        {
            let is_on_widget = widget_id == app_state.current_widget.widget_id;
            let border_style = if is_on_widget {
                self.colours.highlighted_border_style
            } else {
                self.colours.border_style
            };

            let title = if app_state.is_expanded {
                const TITLE_BASE: &str = " Terminal ── Esc to go back ";
                Spans::from(vec![
                    Span::styled(" Terminal ", self.colours.widget_title_style),
                    Span::styled(
                        format!(
                            "─{}─ Esc to go back ",
                            "─".repeat(usize::from(draw_loc.width).saturating_sub(
                                UnicodeSegmentation::graphemes(TITLE_BASE, true).count() + 2
                            ))
                        ),
                        border_style,
                    ),
                ])
            } else {
                Spans::from(Span::styled(" Terminal ", self.colours.widget_title_style))
            };

            let terminal_block = if draw_border {
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style)
            } else if is_on_widget {
                Block::default()
                    .borders(SIDE_BORDERS)
                    .border_style(self.colours.highlighted_border_style)
            } else {
                Block::default().borders(Borders::NONE)
            };

            let mut contents = Vec::new();
            let mut offset = terminal_widget_state.offset;
            let stdout_height = (draw_loc.height - 3) as usize;
            for line in terminal_widget_state.stdout.lines().rev() {
                if offset > 0 {
                    if contents.len() == stdout_height {
                        terminal_widget_state.offset = offset;
                        offset = 0;
                    } else {
                        offset -= 1;
                        continue;
                    }
                }
                contents.push(Spans::from(Span::styled(line, self.colours.text_style)));
                if contents.len() == stdout_height {
                    break;
                }
            }
            contents.reverse();
            if terminal_widget_state.offset > 0 && contents.len() < stdout_height {
                terminal_widget_state.offset -= 1;
                contents.push(Spans::from(Span::styled(
                    "<End reached>",
                    self.colours.currently_selected_text_style,
                )));
            }
            while contents.len() < stdout_height {
                contents.push(Spans::from(Span::styled("", self.colours.text_style)));
            }
            contents.push(Spans::from(Span::styled(
                format!(
                    "Input: {}",
                    if terminal_widget_state.is_elaborating {
                        String::from("<Elaborating...>")
                    } else if app_state.is_expanded {
                        let mut input = terminal_widget_state.stdin.clone();
                        let right =
                            input.split_off(input.len() - terminal_widget_state.input_offset);
                        if right.is_empty() {
                            input
                        } else {
                            format!("{input}|{right}")
                        }
                    } else {
                        String::from("<Extend to write>")
                    }
                ),
                self.colours.currently_selected_text_style,
            )));

            f.render_widget(Paragraph::new(contents).block(terminal_block), draw_loc);

            if should_get_widget_bounds {
                if let Some(widget) = app_state.widget_map.get_mut(&widget_id) {
                    widget.top_left_corner = Some((draw_loc.x, draw_loc.y));
                    widget.bottom_right_corner =
                        Some((draw_loc.x + draw_loc.width, draw_loc.y + draw_loc.height));
                }
            }
        }
    }
}
