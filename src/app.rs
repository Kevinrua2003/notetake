use std::collections::HashSet;
use std::path::PathBuf;
use std::time::SystemTime;

use eframe::egui;

use crate::store::{self, Note};

/// Clave usada en el storage de eframe.
const STORAGE_KEY: &str = "notetake";

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Persisted {
    tabs: Vec<String>,
    active: Option<String>,
}

/// Acción de cierre que quedó esperando resolución del modal.
#[derive(Clone, Copy)]
enum CloseTarget {
    Tab(usize),
    Window(usize),
}

#[derive(Clone, Copy)]
enum Choice {
    Save,
    Discard,
    Cancel,
}

pub struct App {
    data_dir: PathBuf,
    /// Orden de carga, nunca se reordena: los índices de `tabs` apuntan acá.
    notes: Vec<Note>,
    tabs: Vec<usize>,
    active: usize,
    search: String,
    /// Carpetas con cambios sin guardar (por `dir`, que cambia al renombrar).
    dirty: HashSet<PathBuf>,
    error: Option<String>,
    hovered_row: Option<usize>,
    focus_title: bool,
    focus_search: bool,
    pending: Option<CloseTarget>,
    close_now: bool,
    was_focused: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, data_dir: PathBuf) -> Self {
        let mut app = Self {
            notes: store::discover(&data_dir),
            data_dir,
            tabs: Vec::new(),
            active: 0,
            search: String::new(),
            dirty: HashSet::new(),
            error: None,
            hovered_row: None,
            focus_title: false,
            focus_search: false,
            pending: None,
            close_now: false,
            was_focused: true,
        };

        if let Some(storage) = cc.storage
            && let Some(persisted) = eframe::get_value::<Persisted>(storage, STORAGE_KEY)
        {
            app.restore_tabs(persisted);
        }

        app
    }

    fn restore_tabs(&mut self, persisted: Persisted) {
        for title in persisted.tabs {
            if let Some(idx) = self.notes.iter().position(|note| note.title == title)
                && !self.tabs.contains(&idx)
            {
                self.tabs.push(idx);
            }
        }

        self.active = persisted
            .active
            .and_then(|title| self.notes.iter().position(|note| note.title == title))
            .and_then(|idx| self.tabs.iter().position(|&tab| tab == idx))
            .unwrap_or(0);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
    }

    fn active_note(&self) -> Option<usize> {
        let &idx = self.tabs.get(self.active)?;
        (idx < self.notes.len()).then_some(idx)
    }

    fn is_dirty(&self, idx: usize) -> bool {
        self.notes
            .get(idx)
            .is_some_and(|note| self.dirty.contains(&note.dir))
    }

    fn mark_dirty(&mut self, idx: usize) {
        if let Some(note) = self.notes.get(idx) {
            self.dirty.insert(note.dir.clone());
        }
    }

    fn clear_dirty(&mut self, idx: usize) {
        if let Some(note) = self.notes.get(idx) {
            self.dirty.remove(&note.dir);
        }
    }

    fn next_dirty_tab(&self) -> Option<usize> {
        self.tabs.iter().copied().find(|&idx| self.is_dirty(idx))
    }

    fn open_tab(&mut self, idx: usize) {
        match self.tabs.iter().position(|&tab| tab == idx) {
            Some(pos) => self.active = pos,
            None => {
                self.tabs.push(idx);
                self.active = self.tabs.len() - 1;
            }
        }
    }

    fn close_tab(&mut self, note_idx: usize) {
        if let Some(pos) = self.tabs.iter().position(|&tab| tab == note_idx) {
            self.tabs.remove(pos);
            if pos < self.active {
                self.active -= 1;
            }
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
    }

    fn request_close_tab(&mut self, note_idx: usize) {
        if self.is_dirty(note_idx) {
            self.pending = Some(CloseTarget::Tab(note_idx));
        } else {
            self.close_tab(note_idx);
        }
    }

    fn new_note(&mut self) {
        match store::create(&self.data_dir) {
            Ok(note) => {
                self.notes.push(note);
                let idx = self.notes.len() - 1;
                self.open_tab(idx);
                self.focus_title = true;
                self.error = None;
            }
            Err(err) => self.error = Some(format!("No se pudo crear la nota: {err}")),
        }
    }

    fn save_note(&mut self, idx: usize) -> Result<(), String> {
        let note = self.notes.get(idx).ok_or("la nota ya no existe")?;
        let old_dir = note.dir.clone();
        let new_dir = store::save(&self.data_dir, &old_dir, &note.title, &note.body)
            .map_err(|err| err.to_string())?;

        let title = new_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mtime = std::fs::metadata(new_dir.join("note.md"))
            .and_then(|meta| meta.modified())
            .unwrap_or_else(|_| SystemTime::now());

        if let Some(note) = self.notes.get_mut(idx) {
            note.dir = new_dir;
            note.title = title;
            note.mtime = mtime;
        }
        self.dirty.remove(&old_dir);
        Ok(())
    }

    fn save_current(&mut self, idx: usize) {
        if !self.is_dirty(idx) {
            return;
        }
        match self.save_note(idx) {
            Ok(()) => self.error = None,
            Err(err) => self.error = Some(format!("No se pudo guardar: {err}")),
        }
    }

    fn trash_note(&mut self, idx: usize) {
        let Some(note) = self.notes.get(idx) else {
            return;
        };
        let dir = note.dir.clone();
        match store::trash(&self.data_dir, &dir) {
            Ok(()) => self.remove_note(idx),
            Err(err) => self.error = Some(format!("No se pudo mover a la papelera: {err}")),
        }
    }

    fn remove_note(&mut self, idx: usize) {
        if let Some(pos) = self.tabs.iter().position(|&tab| tab == idx) {
            self.tabs.remove(pos);
            if pos < self.active {
                self.active -= 1;
            }
        }
        if let Some(note) = self.notes.get(idx) {
            self.dirty.remove(&note.dir);
        }
        self.notes.remove(idx);
        for tab in &mut self.tabs {
            if *tab > idx {
                *tab -= 1;
            }
        }
        // Corrige índices viejos que quedaron colgados por la eliminación.
        self.tabs.retain(|&tab| tab < self.notes.len());
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.pending.is_some() {
            return;
        }

        let mut new_note = false;
        let mut save = false;
        let mut focus_search = false;
        let mut close_tab = false;
        let mut clear_search = false;

        ctx.input_mut(|input| {
            new_note = input.consume_key(egui::Modifiers::COMMAND, egui::Key::N);
            save = input.consume_key(egui::Modifiers::COMMAND, egui::Key::S);
            focus_search = input.consume_key(egui::Modifiers::COMMAND, egui::Key::F);
            close_tab = input.consume_key(egui::Modifiers::COMMAND, egui::Key::W);
            clear_search = input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
        });

        if new_note {
            self.new_note();
        }
        if save && let Some(idx) = self.active_note() {
            self.save_current(idx);
        }
        if focus_search {
            self.focus_search = true;
        }
        if clear_search {
            self.search.clear();
        }
        if close_tab && let Some(idx) = self.active_note() {
            self.request_close_tab(idx);
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.viewport().close_requested()) {
            return;
        }
        if self.pending.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            return;
        }
        if let Some(idx) = self.next_dirty_tab() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(CloseTarget::Window(idx));
        }
        // Sin tabs sucios: no se cancela, eframe cierra la ventana.
    }

    fn rescan_on_focus(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|input| input.viewport().focused).unwrap_or(false);
        if focused && !self.was_focused {
            self.rescan();
        }
        self.was_focused = focused;
    }

    fn rescan(&mut self) {
        let disk = store::discover(&self.data_dir);
        let mut gone = Vec::new();

        for idx in 0..self.notes.len() {
            // Las notas sucias tienen prioridad: no se tocan.
            if self.dirty.contains(&self.notes[idx].dir) {
                continue;
            }
            match disk.iter().find(|found| found.dir == self.notes[idx].dir) {
                Some(found) => {
                    if found.mtime != self.notes[idx].mtime {
                        let note = &mut self.notes[idx];
                        note.title = found.title.clone();
                        note.body = found.body.clone();
                        note.mtime = found.mtime;
                    }
                }
                None => gone.push(idx),
            }
        }

        for idx in gone.into_iter().rev() {
            self.remove_note(idx);
        }

        for found in disk {
            if !self.notes.iter().any(|note| note.dir == found.dir) {
                self.notes.push(found);
            }
        }
    }

    fn advance_close(&mut self, target: CloseTarget) {
        match target {
            CloseTarget::Tab(idx) => self.close_tab(idx),
            CloseTarget::Window(_) => {
                if let Some(next) = self.next_dirty_tab() {
                    self.pending = Some(CloseTarget::Window(next));
                } else {
                    self.close_now = true;
                }
            }
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("barra").show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if ui
                    .button("+ Nueva")
                    .on_hover_text("Nueva nota (Ctrl+N)")
                    .clicked()
                {
                    self.new_note();
                }
            });
        });
    }

    fn notes_panel(&mut self, ui: &mut egui::Ui) {
        let mut open = None;
        let mut delete = None;
        let mut hovered = self.hovered_row;

        egui::Panel::left("notas")
            .exact_size(280.0)
            .resizable(false)
            .show(ui, |ui| {
                let search = ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Buscar…  (Ctrl+F)")
                        .desired_width(f32::INFINITY),
                );
                if std::mem::take(&mut self.focus_search) {
                    search.request_focus();
                }
                ui.separator();

                let query = store::fold(&self.search);
                let mut order: Vec<usize> = (0..self.notes.len()).collect();
                order.sort_by_cached_key(|&idx| store::fold(&self.notes[idx].title));

                egui::ScrollArea::vertical()
                    .id_salt("lista-notas")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        hovered = None;
                        let mut visibles = 0;

                        for idx in order {
                            let note = &self.notes[idx];
                            if !query.is_empty()
                                && !store::fold(&note.title).contains(&query)
                                && !store::fold(&note.body).contains(&query)
                            {
                                continue;
                            }
                            visibles += 1;

                            let dirty = self.dirty.contains(&note.dir);
                            let title = if dirty {
                                format!("{} *", note.title)
                            } else {
                                note.title.clone()
                            };
                            let snippet = snippet(&note.body);
                            let is_active = self.tabs.get(self.active).copied() == Some(idx);
                            let show_close = hovered == Some(idx);

                            let bg = ui.painter().add(egui::Shape::Noop);
                            let row = ui
                                .scope_builder(
                                    egui::UiBuilder::new().sense(egui::Sense::click()),
                                    |ui| {
                                        ui.set_width(ui.available_width());
                                        egui::Sides::new().shrink_left().show(
                                            ui,
                                            |ui| {
                                                ui.vertical(|ui| {
                                                    ui.spacing_mut().item_spacing.y = 2.0;
                                                    let mut text =
                                                        egui::RichText::new(&title).size(14.0);
                                                    if is_active {
                                                        text = text.strong();
                                                    }
                                                    ui.add(egui::Label::new(text).truncate());
                                                    ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(&snippet)
                                                                .small()
                                                                .weak(),
                                                        )
                                                        .truncate(),
                                                    );
                                                });
                                            },
                                            |ui| {
                                                let close = ui.add_visible(
                                                    show_close,
                                                    egui::Button::new("✕").frame(false),
                                                );
                                                if close.clicked() {
                                                    delete = Some(idx);
                                                }
                                            },
                                        );
                                    },
                                )
                                .response;

                            if row.hovered() {
                                hovered = Some(idx);
                            }
                            if show_close || is_active {
                                let fill = if is_active {
                                    ui.visuals().selection.bg_fill.gamma_multiply(0.35)
                                } else {
                                    ui.visuals().widgets.hovered.weak_bg_fill
                                };
                                ui.painter()
                                    .set(bg, egui::Shape::rect_filled(row.rect, 4.0, fill));
                            }
                            if row.clicked() {
                                open = Some(idx);
                            }
                        }

                        if visibles == 0 {
                            ui.weak(if self.notes.is_empty() {
                                "Todavía no hay notas."
                            } else {
                                "Sin resultados."
                            });
                        }
                    });
            });

        self.hovered_row = hovered;
        if let Some(idx) = open {
            self.open_tab(idx);
        }
        if let Some(idx) = delete {
            self.trash_note(idx);
        }
    }

    fn tabs_bar(&mut self, ui: &mut egui::Ui) {
        let mut close = None;

        egui::ScrollArea::horizontal()
            .id_salt("tabs")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for pos in 0..self.tabs.len() {
                        let idx = self.tabs[pos];
                        let Some(note) = self.notes.get(idx) else {
                            continue;
                        };
                        let mut label = note.title.clone();
                        if self.dirty.contains(&note.dir) {
                            label.push_str(" *");
                        }
                        if ui.selectable_label(self.active == pos, label).clicked() {
                            self.active = pos;
                        }
                        if ui.add(egui::Button::new("✕").frame(false)).clicked() {
                            close = Some(idx);
                        }
                    }
                });
            });

        if let Some(idx) = close {
            self.request_close_tab(idx);
        }
    }

    fn editor_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            self.tabs_bar(ui);

            if let Some(error) = self.error.clone() {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }

            let Some(idx) = self.active_note() else {
                ui.centered_and_justified(|ui| {
                    ui.weak("Ninguna nota abierta. Usá «+ Nueva» o elegí una de la lista.");
                });
                return;
            };

            let request_focus = std::mem::take(&mut self.focus_title);

            let title = ui.add(
                egui::TextEdit::singleline(&mut self.notes[idx].title)
                    .hint_text("Título")
                    .desired_width(f32::INFINITY),
            );
            if title.changed() {
                self.mark_dirty(idx);
            }
            if request_focus {
                title.request_focus();
            }

            ui.separator();

            let size = egui::vec2(ui.available_width(), ui.available_height());
            let body = ui.add_sized(
                size,
                egui::TextEdit::multiline(&mut self.notes[idx].body)
                    .hint_text("Escribí acá…")
                    .code_editor(),
            );
            if body.changed() {
                self.mark_dirty(idx);
            }
        });
    }

    fn unsaved_modal(&mut self, ctx: &egui::Context) {
        let Some(target) = self.pending else {
            return;
        };
        let idx = match target {
            CloseTarget::Tab(idx) | CloseTarget::Window(idx) => idx,
        };
        if idx >= self.notes.len() {
            self.pending = None;
            return;
        }

        let title = self.notes[idx].title.clone();
        let error = self.error.clone();
        let mut choice = None;

        let response = egui::Modal::new(egui::Id::unique("cambios-sin-guardar")).show(ctx, |ui| {
            ui.set_max_width(380.0);
            ui.heading("Cambios sin guardar");
            ui.label(format!("«{title}» tiene cambios sin guardar."));
            if let Some(error) = error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Guardar").clicked() {
                    choice = Some(Choice::Save);
                }
                if ui.button("Descartar").clicked() {
                    choice = Some(Choice::Discard);
                }
                if ui.button("Cancelar").clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        });

        if choice.is_none() && response.should_close() {
            choice = Some(Choice::Cancel);
        }

        match choice {
            Some(Choice::Save) => match self.save_note(idx) {
                Ok(()) => {
                    self.error = None;
                    self.advance_close(target);
                }
                Err(err) => {
                    // Queda sucia y el modal sigue abierto para reintentar.
                    self.error = Some(format!("No se pudo guardar: {err}"));
                    self.pending = Some(target);
                }
            },
            Some(Choice::Discard) => {
                self.clear_dirty(idx);
                self.advance_close(target);
            }
            Some(Choice::Cancel) => {
                self.pending = None;
                self.close_now = false;
            }
            None => self.pending = Some(target),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        self.rescan_on_focus(&ctx);
        self.handle_close_request(&ctx);

        if self.close_now {
            self.close_now = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        self.handle_shortcuts(&ctx);
        self.toolbar(ui);
        self.notes_panel(ui);
        self.editor_panel(ui);
        self.unsaved_modal(&ctx);

        // Asegura el frame siguiente, donde se manda el `Close` real.
        if self.close_now {
            ctx.request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let persisted = Persisted {
            tabs: self
                .tabs
                .iter()
                .filter_map(|&idx| self.notes.get(idx).map(|note| note.title.clone()))
                .collect(),
            active: self
                .tabs
                .get(self.active)
                .and_then(|&idx| self.notes.get(idx))
                .map(|note| note.title.clone()),
        };
        eframe::set_value(storage, STORAGE_KEY, &persisted);
    }
}

/// Primeras dos líneas del cuerpo, con espacios colapsados, recortadas para la fila.
fn snippet(body: &str) -> String {
    let text = body
        .lines()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if text.chars().count() > 90 {
        let mut short: String = text.chars().take(90).collect();
        short.push('…');
        short
    } else {
        text
    }
}
