use std::{borrow::Cow, cmp::max};

use tui::text::Text;

use crate::{
    app::AppConfigFields,
    canvas::canvas_styling::CanvasColours,
    components::data_table::{
        ColumnHeader, DataTableColumn, DataTableProps, DataTableStyling, DataToCell, SortColumn,
        SortDataTable, SortDataTableProps, SortOrder, SortsRow,
    },
    utils::gen_util::{sort_partial_fn, truncate_to_text},
};

#[derive(Clone, Debug)]
pub struct ConnectionsWidgetData {
    pub name: String,
    pub local_address: String,
    pub remote_address: String,
    pub status: String,
}

pub enum ConnectionsWidgetColumn {
    Name,
    LocalAddress,
    RemoteAddress,
    Status,
}

impl ColumnHeader for ConnectionsWidgetColumn {
    fn text(&self) -> Cow<'static, str> {
        match self {
            ConnectionsWidgetColumn::Name => "PID/Name".into(),
            ConnectionsWidgetColumn::LocalAddress => "Local Address".into(),
            ConnectionsWidgetColumn::RemoteAddress => "Remote Address".into(),
            ConnectionsWidgetColumn::Status => "Status".into(),
        }
    }
}

impl DataToCell<ConnectionsWidgetColumn> for ConnectionsWidgetData {
    fn to_cell<'a>(
        &'a self, column: &ConnectionsWidgetColumn, calculated_width: u16,
    ) -> Option<Text<'a>> {
        if calculated_width == 0 {
            return None;
        }

        Some(truncate_to_text(
            match column {
                ConnectionsWidgetColumn::Name => &self.name,
                ConnectionsWidgetColumn::LocalAddress => &self.local_address,
                ConnectionsWidgetColumn::RemoteAddress => &self.remote_address,
                ConnectionsWidgetColumn::Status => &self.status,
            },
            calculated_width,
        ))
    }

    fn column_widths<C: DataTableColumn<ConnectionsWidgetColumn>>(
        data: &[ConnectionsWidgetData], _columns: &[C],
    ) -> Vec<u16>
    where
        Self: Sized,
    {
        let mut widths = vec![0; 4];

        data.iter().for_each(|row| {
            widths[0] = max(widths[0], row.name.len() as u16);
            widths[1] = max(widths[1], row.local_address.len() as u16);
            widths[2] = max(widths[2], row.remote_address.len() as u16);
            widths[3] = max(widths[3], row.status.len() as u16);
        });

        widths
    }
}

impl SortsRow for ConnectionsWidgetColumn {
    type DataType = ConnectionsWidgetData;

    fn sort_data(&self, data: &mut [Self::DataType], descending: bool) {
        match self {
            ConnectionsWidgetColumn::Name => {
                data.sort_by(move |a, b| {
                    sort_partial_fn(descending)(
                        a.name
                            .split('/')
                            .next()
                            .unwrap()
                            .parse::<u32>()
                            .unwrap_or(0),
                        b.name
                            .split('/')
                            .next()
                            .unwrap()
                            .parse::<u32>()
                            .unwrap_or(0),
                    )
                });
            }
            ConnectionsWidgetColumn::LocalAddress => {
                data.sort_by(move |a, b| {
                    sort_partial_fn(descending)(&a.local_address, &b.local_address)
                });
            }
            ConnectionsWidgetColumn::RemoteAddress => {
                data.sort_by(move |a, b| {
                    sort_partial_fn(descending)(&a.remote_address, &b.remote_address)
                });
            }
            ConnectionsWidgetColumn::Status => {
                data.sort_by(move |a, b| sort_partial_fn(descending)(&a.status, &b.status));
            }
        }
    }
}

pub struct ConnectionsWidgetState {
    pub table: SortDataTable<ConnectionsWidgetData, ConnectionsWidgetColumn>,
}

impl ConnectionsWidgetState {
    pub fn new(config: &AppConfigFields, colours: &CanvasColours) -> Self {
        let columns = [
            SortColumn::soft(ConnectionsWidgetColumn::Name, None),
            SortColumn::soft(ConnectionsWidgetColumn::LocalAddress, None),
            SortColumn::soft(ConnectionsWidgetColumn::RemoteAddress, None),
            SortColumn::soft(ConnectionsWidgetColumn::Status, None),
        ];

        let props = SortDataTableProps {
            inner: DataTableProps {
                title: Some(" Connections ".into()),
                table_gap: config.table_gap,
                left_to_right: false,
                is_basic: config.use_basic_mode,
                show_table_scroll_position: config.show_table_scroll_position,
                show_current_entry_when_unfocused: false,
            },
            sort_index: 0,
            order: SortOrder::Descending,
        };

        let styling = DataTableStyling::from_colours(colours);

        Self {
            table: SortDataTable::new_sortable(columns, props, styling),
        }
    }

    pub fn ingest_data(&mut self, data: &[ConnectionsWidgetData]) {
        let mut data = data.to_vec();
        if let Some(column) = self.table.columns.get(self.table.sort_index()) {
            column.sort_by(&mut data, self.table.order());
        }
        self.table.set_data(data);
    }
}
