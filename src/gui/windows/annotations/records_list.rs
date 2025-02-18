use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bstr::ByteSlice;
use crossbeam::channel::Sender;
use handlegraph::pathhandlegraph::PathId;
use rustc_hash::FxHashSet;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use crate::reactor::Reactor;
use crate::{
    annotations::{AnnotationCollection, AnnotationRecord, ColumnKey},
    app::AppMsg,
    graph_query::{GraphQuery, GraphQueryWorker},
    gui::{
        util::grid_row_label,
        windows::{graph_picker::PathPicker, overlays::OverlayCreatorMsg},
    },
};

use super::{filter::RecordFilter, ColumnPickerMany, OverlayLabelSetCreator};

pub struct RecordList<C>
where
    C: AnnotationCollection + Send + Sync + 'static,
{
    id: egui::Id,
    current_file: Option<String>,

    filtered_records: Vec<usize>,

    offset: usize,
    slot_count: usize,

    filter_open: bool,
    filters: HashMap<String, RecordFilter<C::ColumnKey>>,

    column_picker_open: bool,
    enabled_columns: HashMap<String, ColumnPickerMany<C::ColumnKey>>,
    default_enabled_columns: HashSet<C::ColumnKey>,
    default_hidden_columns: HashSet<C::ColumnKey>,

    path_picker_open: bool,
    path_picker: PathPicker,

    creator_open: bool,
    creator: OverlayLabelSetCreator<C>,
}

impl<C> RecordList<C>
where
    C: AnnotationCollection + Send + Sync + 'static,
{
    pub fn new(
        reactor: &mut Reactor,
        id: egui::Id,
        path_picker: PathPicker,
    ) -> Self {
        let filtered_records = Vec::new();

        Self {
            id,
            current_file: None,

            filtered_records,

            offset: 0,
            slot_count: 15,
            // slot_count: 20,
            filter_open: false,
            filters: HashMap::default(),

            column_picker_open: false,
            enabled_columns: HashMap::default(),
            default_enabled_columns: Default::default(),
            default_hidden_columns: Default::default(),

            path_picker_open: false,
            path_picker,

            creator_open: false,
            creator: OverlayLabelSetCreator::new(
                reactor,
                egui::Id::new("overlay_label_set_creator"),
            ),
        }
    }

    pub fn set_default_columns(
        &mut self,
        enabled_columns: impl IntoIterator<Item = C::ColumnKey>,
        hidden_columns: impl IntoIterator<Item = C::ColumnKey>,
    ) {
        self.default_enabled_columns.clear();
        self.default_enabled_columns.extend(enabled_columns);

        self.default_hidden_columns.clear();
        self.default_hidden_columns.extend(hidden_columns);
    }

    pub fn scroll_to_label_record(
        &mut self,
        records: &C,
        column: &C::ColumnKey,
        value: &[u8],
    ) {
        let ix = self
            .filtered_records
            .iter()
            .enumerate()
            .find(|&(_ix, record_ix)| {
                let record = &records.records()[*record_ix];
                let column_values = record.get_all(column);
                column_values.iter().any(|&rec_val| rec_val == value)
            })
            .map(|(ix, _)| ix);

        if let Some(ix) = ix {
            self.offset = ix;
        }
    }

    fn ui_row(
        &self,
        ui: &mut egui::Ui,
        file_name: &str,
        records: &C,
        record: &C::Record,
        index: usize,
    ) -> egui::Response {
        let mut fields: Vec<String> = vec![
            format!("{}", record.seq_id().as_bstr()),
            format!("{}", record.start()),
            format!("{}", record.end()),
        ];

        let enabled_columns = self.enabled_columns.get(file_name).unwrap();

        let mut mandatory = records.mandatory_columns();
        mandatory.retain(|c| {
            c != &C::ColumnKey::seq_id()
                && c != &C::ColumnKey::start()
                && c != &C::ColumnKey::end()
        });

        for column in mandatory.into_iter().chain(records.optional_columns()) {
            if enabled_columns.get_column(&column) {
                let values = record.get_all(&column);

                let mut label = String::new();

                for (count, value) in values.into_iter().enumerate() {
                    if count != 0 {
                        label.push_str(";");
                    }
                    let val_str = value.to_str().unwrap();
                    label.push_str(val_str);
                }

                fields.push(label);
            }
        }

        let fields_ref: Vec<&str> =
            fields.iter().map(|f| f.as_str()).collect::<Vec<_>>();

        let resp = grid_row_label(
            ui,
            egui::Id::new(ui.id().with(index)),
            &fields_ref,
            false,
        );

        resp
    }

    fn select_record(
        &self,
        app_msg_tx: &crossbeam::channel::Sender<AppMsg>,
        graph_query: &GraphQuery,
        record: &C::Record,
    ) {
        let active_path = self.path_picker.active_path();

        if let Some((path_id, name)) = active_path {
            let mut start = record.start();
            let mut end = record.end();

            if let Some(offset) =
                crate::annotations::path_name_offset(name.as_bytes())
            {
                start -= offset;
                end -= offset;
            }

            if let Some(range) =
                graph_query.path_basepair_range(path_id, start, end)
            {
                let nodes = range
                    .into_iter()
                    .map(|(handle, _, _)| handle.id())
                    .collect::<FxHashSet<_>>();

                use crate::app::Select;

                let select = Select::Many { nodes, clear: true };
                let msg = AppMsg::Selection(select);
                app_msg_tx.send(msg).unwrap();
            }
        }
    }

    fn apply_filter(&mut self, file_name: &str, records: &C) {
        self.filtered_records.clear();

        debug!("applying filter");
        let total = records.records().len();

        let records = &records.records();
        let filter = self.filters.get(file_name).unwrap();
        let filtered_records = &mut self.filtered_records;

        filtered_records.extend(records.iter().enumerate().filter_map(
            |(ix, rec)| {
                if filter.filter_record(rec) {
                    Some(ix)
                } else {
                    None
                }
            },
        ));
        let filtered = self.filtered_records.len();
        debug!(
            "filter complete, showing {} out of {} records",
            filtered, total
        );

        self.offset = 0;
    }

    fn clear_filter(&mut self) {
        self.filtered_records.clear();
    }

    pub fn active_path_id(&self) -> Option<PathId> {
        let (path, _) = self.path_picker.active_path()?;
        Some(path)
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        graph_query: &GraphQueryWorker,
        app_msg_tx: &crossbeam::channel::Sender<AppMsg>,
        file_name: &str,
        records: &Arc<C>,
    ) {
        let active_path_name = self
            .path_picker
            .active_path()
            .map(|(_id, name)| name.to_owned());

        if !self.enabled_columns.contains_key(file_name) {
            let mut enabled_columns: ColumnPickerMany<C::ColumnKey> =
                ColumnPickerMany::new(egui::Id::new(file_name));

            enabled_columns.update_columns(records.as_ref());

            for col in self.default_enabled_columns.iter() {
                enabled_columns.set_column(col, true);
            }

            for col in self.default_hidden_columns.iter() {
                enabled_columns.hide_column_from_gui(col, true);
            }

            self.enabled_columns
                .insert(file_name.to_string(), enabled_columns);
        }

        {
            let filter = self
                .filters
                .entry(file_name.to_string())
                .or_insert(RecordFilter::new(self.id, records.as_ref()));

            let ctx = ui.ctx();

            egui::Window::new("Filter records")
                .id(self.id)
                .default_pos(egui::Pos2::new(600.0, 200.0))
                .open(&mut self.filter_open)
                .show(ctx, |ui| {
                    ui.set_max_width(400.0);
                    filter.ui(ui);
                });
        }

        if self.current_file.as_ref().map(|s| s.as_str()) != Some(file_name) {
            self.current_file = Some(file_name.to_string());
            self.apply_filter(file_name, records.as_ref());
        }

        self.path_picker.ui(ui.ctx(), &mut self.path_picker_open);

        if let Some(path) = self.path_picker.active_path().map(|(p, _)| p) {
            if self
                .creator
                .current_annotation_file
                .as_ref()
                .map(|s| s.as_str())
                != Some(file_name)
            {
                self.creator.current_annotation_file =
                    Some(file_name.to_string());
                self.creator.column_picker.update_columns(records.as_ref());
            }

            self.creator.ui(
                ui.ctx(),
                app_msg_tx,
                graph_query,
                &mut self.creator_open,
                file_name,
                path,
                records.clone(),
                &self.filtered_records,
            );
        }

        ui.set_min_height(200.0);
        ui.set_max_height(ui.input().screen_rect.height() - 100.0);

        ui.label(file_name);
        ui.separator();

        let apply_filter = {
            let filters = self.filters.get_mut(file_name).unwrap();
            let qf_cols = filters.quick_filter.column_picker_mut();

            let popup_id = ui
                .make_persistent_id(self.id.with("quick_filter_columns_popup"));

            let button_inner = ui.horizontal(|ui| {
                ui.heading("Quick filter");
                let btn = ui.button("Choose columns");

                if btn.clicked() {
                    trace!("popup clicked");
                    ui.memory().toggle_popup(popup_id);
                }

                btn
            });

            let button = &button_inner.response;

            crate::gui::windows::util::popup_below_widget(
                ui,
                popup_id,
                &button,
                |ui| qf_cols.compact_widget(ui),
            );

            filters.add_quick_filter(ui)
        };

        ui.separator();

        ui.horizontal(|ui| {
            let filter_config_open = self.filter_open;
            if ui
                .selectable_label(filter_config_open, "Configure filter")
                .clicked()
            {
                self.filter_open = !self.filter_open;
            }

            let column_picker_open = self.column_picker_open;

            if ui
                .selectable_label(column_picker_open, "Enabled columns")
                .clicked()
            {
                self.column_picker_open = !self.column_picker_open;
            }
        });

        ui.horizontal(|ui| {
            if ui.button("Apply filter").clicked() || apply_filter {
                self.apply_filter(file_name, records.as_ref());
            }

            if ui.button("Clear filter").clicked() {
                self.clear_filter();
            }
        });

        ui.horizontal(|ui| {
            let path_picker_btn = {
                let label = if let Some(name) = &active_path_name {
                    format!("Path: {}", name)
                } else {
                    "Select a path".to_string()
                };

                ui.button(label)
            };

            if path_picker_btn.clicked() {
                self.path_picker_open = !self.path_picker_open;
            }

            let creator_btn = ui.button("Label & Overlay creator");

            if creator_btn.clicked() {
                self.creator_open = !self.creator_open;
            }
        });

        ui.horizontal(|ui| {
            let path_name_range = if let Some(name) = &active_path_name {
                let n = name.as_bytes();
                crate::annotations::path_name_chr_range(n.as_bytes())
            } else {
                None
            };

            let range_filter_btn = ui.add(
                egui::Button::new("Filter by path range")
                    .enabled(path_name_range.is_some()),
            );

            if let Some((chr, start, end)) = path_name_range {
                if range_filter_btn.clicked() {
                    let filter = self.filters.get_mut(file_name).unwrap();
                    filter.chr_range_filter(chr, start, end);
                }
            }
        });

        let enabled_columns = self.enabled_columns.get(file_name).unwrap();

        let mut spacing = ui.spacing().item_spacing;
        spacing.y = 3.0;

        self.slot_count = {
            // top part is around 250px, round up to 300px to be on the safe side
            let usable_height = ui.input().screen_rect.height() - 300.0;

            // again, roughly 30px
            let row_height = 30.0;

            (usable_height / row_height) as usize
        };

        let record_count = if self.filtered_records.is_empty() {
            records.records().len()
        } else {
            self.filtered_records.len()
        };

        let end = {
            let end = self.offset + self.slot_count;

            if end > record_count {
                if record_count > self.slot_count {
                    self.offset = record_count - self.slot_count;
                } else {
                    self.offset = 0;
                }
            }

            end
        };

        let label_str = format!(
            "Rows {} - {} out of {}",
            self.offset + 1,
            end + 1,
            record_count
        );
        ui.label(label_str);

        let grid = egui::Grid::new("record_list_grid")
            .striped(true)
            .spacing(spacing)
            .show(ui, |ui| {
                ui.label(C::ColumnKey::seq_id().to_string());
                ui.label(C::ColumnKey::start().to_string());
                ui.label(C::ColumnKey::end().to_string());

                let mut mandatory = records.mandatory_columns();
                mandatory.retain(|c| {
                    c != &C::ColumnKey::seq_id()
                        && c != &C::ColumnKey::start()
                        && c != &C::ColumnKey::end()
                });

                for col in mandatory {
                    if enabled_columns.get_column(&col) {
                        ui.label(col.to_string());
                    }
                }

                for col in records.optional_columns() {
                    if enabled_columns.get_column(&col) {
                        ui.label(col.to_string());
                    }
                }

                ui.end_row();

                for i in 0..self.slot_count {
                    let row_record = if self.filtered_records.is_empty() {
                        records.records().get(self.offset + i).map(|record| {
                            (
                                self.ui_row(
                                    ui,
                                    file_name,
                                    records.as_ref(),
                                    record,
                                    i,
                                ),
                                record,
                            )
                        })
                    } else {
                        self.filtered_records.get(self.offset + i).and_then(
                            |&ix| {
                                let record = records.records().get(ix)?;
                                let row = self.ui_row(
                                    ui,
                                    file_name,
                                    records.as_ref(),
                                    record,
                                    i,
                                );
                                Some((row, record))
                            },
                        )
                    };

                    if let Some((row, record)) = row_record {
                        let row_interact = ui.interact(
                            row.rect,
                            egui::Id::new(ui.id().with(i)),
                            egui::Sense::click(),
                        );

                        if row_interact.clicked() {
                            self.select_record(
                                app_msg_tx,
                                graph_query.graph(),
                                record,
                            );
                        }
                        if row_interact.double_clicked() {
                            app_msg_tx.send(AppMsg::GotoSelection).unwrap();
                        }
                    }
                }
            });

        if grid.response.hover_pos().is_some() {
            let scroll = ui.input().scroll_delta;
            if scroll.y.abs() >= 4.0 {
                let sig = (scroll.y.signum() as isize) * -1;
                let delta = sig * ((scroll.y.abs() as isize) / 4);

                let mut offset = self.offset as isize;

                offset += delta;

                let end = records
                    .records()
                    .len()
                    .checked_sub(self.slot_count)
                    .unwrap_or(records.records().len().min(self.slot_count));

                offset = offset.clamp(0, end as isize);
                self.offset = offset as usize;
            }
        }

        let enabled_columns = self.enabled_columns.get_mut(file_name).unwrap();
        enabled_columns.ui(
            ui.ctx(),
            None,
            &mut self.column_picker_open,
            "Enabled Columns",
        );
    }
}
