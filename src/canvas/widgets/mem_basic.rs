use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    terminal::Frame,
    text::{Span, Spans},
    widgets::{Block, Borders, Gauge},
};
use unicode_segmentation::UnicodeSegmentation;

use crate::{app::App, canvas::Painter};

impl Painter {
    pub fn draw_basic_memory<B: Backend>(
        &self, f: &mut Frame<'_, B>, app_state: &mut App, draw_loc: Rect, widget_id: u64,
    ) {
        let mut draw_widgets: Vec<Gauge<'_>> = Vec::new();

        let is_on_widget = widget_id == app_state.current_widget.widget_id;
        let border_style = if is_on_widget {
            self.colours.highlighted_border_style
        } else {
            self.colours.border_style
        };
        let title = if app_state.is_expanded {
            const TITLE_BASE: &str = " Memory ── Esc to go back ";
            Spans::from(vec![
                Span::styled(" Memory ", self.colours.widget_title_style),
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
            Spans::from(Span::styled(" Memory ", self.colours.widget_title_style))
        };

        f.render_widget(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style),
            draw_loc,
        );

        let ram_percentage = app_state.converted_data.mem_data.use_percent.unwrap_or(0.0);

        let memory_fraction_label =
            if let Some((_, label_frac)) = &app_state.converted_data.mem_labels {
                format!(
                    "RAM: {}% {}",
                    (ram_percentage * 100.0).round() / 100.0,
                    label_frac.trim()
                )
            } else {
                EMPTY_MEMORY_FRAC_STRING.to_string()
            };

        const EMPTY_MEMORY_FRAC_STRING: &str = "0.0B/0.0B";

        draw_widgets.push(
            Gauge::default()
                .ratio(ram_percentage / 100.0)
                .label(memory_fraction_label)
                .style(self.colours.ram_style)
                .gauge_style(self.colours.ram_style),
        );

        let swap_percentage = app_state
            .converted_data
            .swap_data
            .use_percent
            .unwrap_or(0.0);

        if let Some((_, label_frac)) = &app_state.converted_data.swap_labels {
            let swap_fraction_label = format!(
                "SWAP: {}% {}",
                (swap_percentage * 100.0).round() / 100.0,
                label_frac.trim()
            );
            draw_widgets.push(
                Gauge::default()
                    .ratio(swap_percentage / 100.0)
                    .label(swap_fraction_label)
                    .style(self.colours.swap_style)
                    .gauge_style(self.colours.swap_style),
            );
        }

        let margined_loc = Layout::default()
            .constraints(vec![Constraint::Length(1); draw_widgets.len()])
            .direction(Direction::Vertical)
            .horizontal_margin(1)
            .vertical_margin(1)
            .split(draw_loc);

        draw_widgets
            .into_iter()
            .enumerate()
            .for_each(|(index, widget)| {
                f.render_widget(widget, margined_loc[index]);
            });

        // Update draw loc in widget map
        if app_state.should_get_widget_bounds() {
            if let Some(widget) = app_state.widget_map.get_mut(&widget_id) {
                widget.top_left_corner = Some((draw_loc.x, draw_loc.y));
                widget.bottom_right_corner =
                    Some((draw_loc.x + draw_loc.width, draw_loc.y + draw_loc.height));
            }
        }
    }
}
