use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    terminal::Frame,
    text::{Span, Spans},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    app::App,
    canvas::{drawing_utils::calculate_basic_use_bars, Painter},
    constants::*,
    data_conversion::BatteryDuration,
};

impl Painter {
    pub fn draw_battery_display<B: Backend>(
        &self, f: &mut Frame<'_, B>, app_state: &mut App, draw_loc: Rect, draw_border: bool,
        widget_id: u64,
    ) {
        if let Some(battery_widget_state) =
            app_state.battery_state.widget_states.get_mut(&widget_id)
        {
            let is_on_widget = widget_id == app_state.current_widget.widget_id;
            let border_style = if is_on_widget {
                self.colours.highlighted_border_style
            } else {
                self.colours.border_style
            };
            let table_gap = if draw_loc.height < TABLE_GAP_HEIGHT_LIMIT {
                0
            } else {
                app_state.app_config_fields.table_gap
            };

            let title = if app_state.is_expanded {
                const TITLE_BASE: &str = " Battery ── Esc to go back ";
                Spans::from(vec![
                    Span::styled(" Battery ", self.colours.widget_title_style),
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
                Spans::from(Span::styled(" Battery ", self.colours.widget_title_style))
            };

            let battery_block = if draw_border {
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

            let margined_draw_loc = Layout::default()
                .constraints([Constraint::Percentage(100)])
                .horizontal_margin(u16::from(!(is_on_widget || draw_border)))
                .direction(Direction::Horizontal)
                .split(draw_loc)[0];

            if let Some(battery_details) = app_state
                .converted_data
                .battery_data
                .get(battery_widget_state.currently_selected_battery_index)
            {
                // Assuming a 50/50 split in width
                let half_width = draw_loc.width.saturating_sub(2) / 2;
                let bar_length = usize::from(half_width.saturating_sub(8));
                let charge_percentage = battery_details.charge_percentage;
                let num_bars = calculate_basic_use_bars(charge_percentage, bar_length);
                let bars = format!(
                    "[{}{}{:3.0}%]",
                    "|".repeat(num_bars),
                    " ".repeat(bar_length - num_bars),
                    charge_percentage,
                );

                fn long_time(secs: i64) -> String {
                    let time = time::Duration::seconds(secs);
                    let num_minutes = time.whole_minutes() - time.whole_hours() * 60;
                    let num_seconds = time.whole_seconds() - time.whole_minutes() * 60;
                    format!(
                        "{} hour{}, {} minute{}, {} second{}",
                        time.whole_hours(),
                        if time.whole_hours() == 1 { "" } else { "s" },
                        num_minutes,
                        if num_minutes == 1 { "" } else { "s" },
                        num_seconds,
                        if num_seconds == 1 { "" } else { "s" },
                    )
                }

                fn short_time(secs: i64) -> String {
                    let time = time::Duration::seconds(secs);
                    let num_minutes = time.whole_minutes() - time.whole_hours() * 60;
                    let num_seconds = time.whole_seconds() - time.whole_minutes() * 60;
                    format!("{}h {}m {}s", time.whole_hours(), num_minutes, num_seconds,)
                }

                let mut battery_rows = Vec::with_capacity(4);
                battery_rows.push(Row::new(vec![
                    Cell::from("Charge %").style(self.colours.text_style),
                    Cell::from(bars).style(if charge_percentage < 10.0 {
                        self.colours.low_battery_colour
                    } else if charge_percentage < 50.0 {
                        self.colours.medium_battery_colour
                    } else {
                        self.colours.high_battery_colour
                    }),
                ]));
                battery_rows.push(
                    Row::new(vec!["Consumption", &battery_details.watt_consumption])
                        .style(self.colours.text_style),
                );

                let s: String; // Keep string in scope.
                {
                    let style = self.colours.text_style;
                    match &battery_details.battery_duration {
                        BatteryDuration::ToEmpty(secs) => {
                            if half_width > 25 {
                                s = long_time(*secs);
                                battery_rows.push(Row::new(vec!["Time to empty", &s]).style(style));
                            } else {
                                s = short_time(*secs);
                                battery_rows.push(Row::new(vec!["To empty", &s]).style(style));
                            }
                        }
                        BatteryDuration::ToFull(secs) => {
                            if half_width > 25 {
                                s = long_time(*secs);
                                battery_rows.push(Row::new(vec!["Time to full", &s]).style(style));
                            } else {
                                s = short_time(*secs);
                                battery_rows.push(Row::new(vec!["To full", &s]).style(style));
                            }
                        }
                        BatteryDuration::Unknown => {}
                    }
                }

                battery_rows.push(
                    Row::new(vec!["Health %", &battery_details.health])
                        .style(self.colours.text_style),
                );

                // Draw
                f.render_widget(
                    Table::new(battery_rows)
                        .block(battery_block)
                        .widths(&[Constraint::Percentage(50), Constraint::Percentage(50)]),
                    margined_draw_loc,
                );
            } else {
                let mut contents = vec![Spans::default(); table_gap.into()];

                contents.push(Spans::from(Span::styled(
                    "No data found for this battery",
                    self.colours.text_style,
                )));

                f.render_widget(
                    Paragraph::new(contents).block(battery_block),
                    margined_draw_loc,
                );
            }
        }
    }
}
