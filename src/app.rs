use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, SystemTime};

use eframe::egui;

use crate::store::{self, Note};
use crate::style;
use crate::update::{self, InstallKind};

/// Clave usada en el storage de eframe.
const STORAGE_KEY: &str = "notetake";

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Persisted {
    #[serde(default)]
    tabs: Vec<String>,
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    theme: Theme,
    #[serde(default)]
    order: Order,
}

/// Tema de la app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    fn label(self) -> &'static str {
        match self {
            Self::System => "Sistema",
            Self::Light => "Claro",
            Self::Dark => "Oscuro",
        }
    }

    fn preference(self) -> egui::ThemePreference {
        match self {
            Self::System => egui::ThemePreference::System,
            Self::Light => egui::ThemePreference::Light,
            Self::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// Orden de la lista de notas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Order {
    #[default]
    Alphabetical,
    Recent,
}

impl Order {
    const ALL: [Self; 2] = [Self::Alphabetical, Self::Recent];

    fn label(self) -> &'static str {
        match self {
            Self::Alphabetical => "Alfabético",
            Self::Recent => "Reciente",
        }
    }
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

/// Chequeo de actualizaciones. El trabajo de red corre en un thread aparte y
/// deja el resultado en el canal.
enum UpdateState {
    Idle,
    Checking(Receiver<Result<Option<update::Release>, String>>),
    UpToDate,
    Available(update::Release),
    Installing(Receiver<Result<String, String>>),
    Installed(String),
}

/// Lo que el popup de ajustes necesita mostrar, sin arrastrar los canales.
#[derive(Clone)]
enum UpdateView {
    Idle,
    Checking,
    UpToDate,
    Available(String),
    Installing,
    Installed(String),
}

impl UpdateState {
    fn view(&self) -> UpdateView {
        match self {
            Self::Idle => UpdateView::Idle,
            Self::Checking(_) => UpdateView::Checking,
            Self::UpToDate => UpdateView::UpToDate,
            Self::Available(release) => UpdateView::Available(release.tag.clone()),
            Self::Installing(_) => UpdateView::Installing,
            Self::Installed(tag) => UpdateView::Installed(tag.clone()),
        }
    }
}

#[derive(Clone, Copy)]
enum UpdateAction {
    Check,
    Install,
    Restart,
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
    hovered_tab: Option<usize>,
    focus_search: bool,
    /// Nota cuya fila de la lista está en edición inline.
    renaming: Option<usize>,
    rename_text: String,
    rename_focus: bool,
    theme: Theme,
    order: Order,
    /// Último tema aplicado al contexto, para no tocar las opciones cada frame.
    applied_theme: Option<Theme>,
    /// Momento del último guardado, para el destello del tab.
    saved_flash: Option<std::time::Instant>,
    maximized: bool,
    pending: Option<CloseTarget>,
    close_now: bool,
    was_focused: bool,
    update: UpdateState,
    /// Detectado una vez: `cargo run` no se puede reemplazar solo.
    install_kind: InstallKind,
    /// Hay que relanzar el binario nuevo cuando termine el cierre.
    restart: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, data_dir: PathBuf) -> Self {
        let mut app = Self::load(data_dir);

        if let Some(storage) = cc.storage
            && let Some(persisted) = eframe::get_value::<Persisted>(storage, STORAGE_KEY)
        {
            app.restore(persisted);
        }

        // El estilo debe estar listo antes del primer frame: el Ui raíz se crea
        // con el estilo vigente y una nota restaurada ya pide el estilo "editor".
        style::install(&cc.egui_ctx);
        cc.egui_ctx.set_theme(app.theme.preference());
        app.applied_theme = Some(app.theme);

        app
    }

    fn load(data_dir: PathBuf) -> Self {
        Self {
            notes: store::discover(&data_dir),
            data_dir,
            tabs: Vec::new(),
            active: 0,
            search: String::new(),
            dirty: HashSet::new(),
            error: None,
            hovered_row: None,
            hovered_tab: None,
            focus_search: false,
            renaming: None,
            rename_text: String::new(),
            rename_focus: false,
            theme: Theme::default(),
            order: Order::default(),
            applied_theme: None,
            saved_flash: None,
            maximized: false,
            pending: None,
            close_now: false,
            was_focused: true,
            update: UpdateState::Idle,
            install_kind: update::install_kind(),
            restart: false,
        }
    }

    fn restore(&mut self, persisted: Persisted) {
        self.theme = persisted.theme;
        self.order = persisted.order;
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
        self.commit_rename();
        match store::create(&self.data_dir) {
            Ok(note) => {
                self.notes.push(note);
                let idx = self.notes.len() - 1;
                self.open_tab(idx);
                self.start_rename(idx);
                self.error = None;
            }
            Err(err) => self.error = Some(format!("No se pudo crear la nota: {err}")),
        }
    }

    fn start_rename(&mut self, idx: usize) {
        let Some(note) = self.notes.get(idx) else {
            return;
        };
        self.renaming = Some(idx);
        self.rename_text = note.title.clone();
        self.rename_focus = true;
    }

    /// Confirma el renombre inline. Vacío o sin cambios no hace nada.
    fn commit_rename(&mut self) {
        let Some(idx) = self.renaming.take() else {
            return;
        };
        let title = self.rename_text.trim().to_owned();
        self.rename_text.clear();
        let Some(note) = self.notes.get(idx) else {
            return;
        };
        if title.is_empty() || store::slugify(&title) == note.title {
            return;
        }

        let old_dir = note.dir.clone();
        match store::rename(&self.data_dir, &old_dir, &title) {
            Ok(dir) => {
                let title = dir
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mtime = std::fs::metadata(dir.join("note.md"))
                    .and_then(|meta| meta.modified())
                    .unwrap_or_else(|_| SystemTime::now());
                if let Some(note) = self.notes.get_mut(idx) {
                    note.dir = dir.clone();
                    note.title = title;
                    note.mtime = mtime;
                }
                if self.dirty.remove(&old_dir) {
                    self.dirty.insert(dir);
                }
                self.error = None;
            }
            Err(err) => self.error = Some(format!("No se pudo renombrar: {err}")),
        }
    }

    fn request_close_window(&mut self, ctx: &egui::Context) {
        if let Some(idx) = self.next_dirty_tab() {
            self.pending = Some(CloseTarget::Window(idx));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn toggle_maximized(&mut self, ctx: &egui::Context) {
        self.maximized = !self.maximized;
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.maximized));
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
        self.saved_flash = Some(std::time::Instant::now());
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
            // Con búsqueda vacía el Esc queda para que egui cierre menús y popups.
            clear_search = (self.renaming.is_some() || !self.search.is_empty())
                && input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
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
            if self.renaming.is_some() {
                // Esc cancela la edición inline antes que la búsqueda.
                self.renaming = None;
                self.rename_text.clear();
            } else {
                self.search.clear();
            }
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

    /// Líneas de 1 px bajo la barra de título y a la derecha del sidebar.
    fn paint_hairlines(&self, ui: &egui::Ui) {
        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        let rect = ui.max_rect();
        let top = rect.top() + style::TITLE_HEIGHT;
        ui.painter().hline(rect.x_range(), top, stroke);
        ui.painter().vline(
            rect.left() + style::SIDEBAR_WIDTH,
            egui::Rangef::new(top, rect.bottom()),
            stroke,
        );
    }

    fn title_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("barra-titulo")
            .exact_size(style::TITLE_HEIGHT)
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().faint_bg_color)
                    .inner_margin(egui::Margin::symmetric(2, 0)),
            )
            .show(ui, |ui| {
                let bar = ui.max_rect();
                // El fondo arrastra la ventana; los botones se dibujan encima.
                let drag = ui.interact(
                    bar,
                    egui::Id::unique("arrastre-ventana"),
                    egui::Sense::click_and_drag(),
                );

                ui.horizontal_centered(|ui| {
                    ui.add_space(4.0);
                    self.settings_button(ui);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if icon_button(ui, Icon::Close, 30.0, "Cerrar", true).clicked() {
                            self.request_close_window(ui.ctx());
                        }
                        let (icon, tip) = if self.maximized {
                            (Icon::Restore, "Restaurar")
                        } else {
                            (Icon::Maximize, "Maximizar")
                        };
                        if icon_button(ui, icon, 30.0, tip, false).clicked() {
                            self.toggle_maximized(ui.ctx());
                        }
                        if icon_button(ui, Icon::Minimize, 30.0, "Minimizar", false).clicked() {
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        ui.add_space(4.0);
                    });
                });

                if drag.drag_started_by(egui::PointerButton::Primary) {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if drag.double_clicked() {
                    self.toggle_maximized(ui.ctx());
                }
            });
    }

    fn settings_button(&mut self, ui: &mut egui::Ui) {
        let response = icon_button(ui, Icon::Menu, 30.0, "Ajustes", false);
        if egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response)) {
            ui.painter().rect_filled(
                response.rect,
                4.0,
                ui.visuals().widgets.hovered.weak_bg_fill,
            );
            paint_icon(
                ui.painter(),
                Icon::Menu,
                response.rect,
                ui.visuals().strong_text_color(),
            );
        }

        let mut theme = self.theme;
        let mut order = self.order;
        let mut open_data = false;
        let mut open_trash = false;
        let update_view = self.update.view();
        let kind = self.install_kind;
        let mut update_action = None;

        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(190.0);
            ui.menu_button("Tema", |ui| {
                for option in Theme::ALL {
                    if ui.radio(theme == option, option.label()).clicked() {
                        theme = option;
                        ui.close();
                    }
                }
            });
            ui.menu_button("Orden", |ui| {
                for option in Order::ALL {
                    if ui.radio(order == option, option.label()).clicked() {
                        order = option;
                        ui.close();
                    }
                }
            });
            ui.separator();
            if ui.button("Abrir carpeta de datos").clicked() {
                open_data = true;
                ui.close();
            }
            if ui.button("Abrir papelera").clicked() {
                open_trash = true;
                ui.close();
            }
            ui.separator();
            match update_view {
                UpdateView::Idle if kind == InstallKind::Unsupported => {
                    ui.add_enabled(false, egui::Button::new("Buscar actualizaciones"))
                        .on_disabled_hover_text("Build de desarrollo o ruta no reconocida");
                }
                UpdateView::Idle => {
                    if ui.button("Buscar actualizaciones").clicked() {
                        update_action = Some(UpdateAction::Check);
                        ui.close();
                    }
                }
                UpdateView::Checking => {
                    ui.add_enabled(false, egui::Button::new("Buscando…"));
                }
                UpdateView::UpToDate => {
                    let version = format!("Última versión (v{})", update::CURRENT);
                    ui.add_enabled(false, egui::Button::new(version));
                }
                UpdateView::Available(tag) => {
                    if ui.button(format!("Instalar {tag}")).clicked() {
                        update_action = Some(UpdateAction::Install);
                        ui.close();
                    }
                }
                UpdateView::Installing => {
                    ui.add_enabled(false, egui::Button::new("Descargando…"));
                }
                UpdateView::Installed(tag) => {
                    ui.weak(format!("Actualizado a {tag}"));
                    if ui.button("Reiniciar ahora").clicked() {
                        update_action = Some(UpdateAction::Restart);
                        ui.close();
                    }
                }
            }
        });

        self.theme = theme;
        self.order = order;
        if open_data {
            let dir = self.data_dir.clone();
            self.open_dir(&dir);
        }
        if open_trash {
            let trash = store::trash_dir(&self.data_dir);
            match std::fs::create_dir_all(&trash) {
                Ok(()) => self.open_dir(&trash),
                Err(err) => self.error = Some(format!("No se pudo crear la papelera: {err}")),
            }
        }
        match update_action {
            Some(UpdateAction::Check) => self.start_check(ui.ctx()),
            Some(UpdateAction::Install) => self.start_install(ui.ctx()),
            Some(UpdateAction::Restart) => self.request_restart(ui.ctx()),
            None => {}
        }
    }

    fn open_dir(&mut self, path: &Path) {
        if let Err(err) = open_folder(path) {
            self.error = Some(format!("No se pudo abrir «{}»: {err}", path.display()));
        }
    }

    /// Recoge el resultado del thread de red y repinta mientras espera.
    fn poll_update(&mut self, ctx: &egui::Context) {
        let state = std::mem::replace(&mut self.update, UpdateState::Idle);
        self.update = match state {
            UpdateState::Checking(rx) => match rx.try_recv() {
                Ok(Ok(Some(release))) => UpdateState::Available(release),
                Ok(Ok(None)) => UpdateState::UpToDate,
                Ok(Err(err)) => {
                    self.error = Some(err);
                    UpdateState::Idle
                }
                Err(TryRecvError::Empty) => UpdateState::Checking(rx),
                Err(TryRecvError::Disconnected) => {
                    self.error = Some("El chequeo de actualizaciones se cortó.".into());
                    UpdateState::Idle
                }
            },
            UpdateState::Installing(rx) => match rx.try_recv() {
                Ok(Ok(tag)) => UpdateState::Installed(tag),
                Ok(Err(err)) => {
                    self.error = Some(err);
                    UpdateState::Idle
                }
                Err(TryRecvError::Empty) => UpdateState::Installing(rx),
                Err(TryRecvError::Disconnected) => {
                    self.error = Some("La instalación se cortó.".into());
                    UpdateState::Idle
                }
            },
            other => other,
        };

        if matches!(
            self.update,
            UpdateState::Checking(_) | UpdateState::Installing(_)
        ) {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn start_check(&mut self, ctx: &egui::Context) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::check());
            ctx.request_repaint();
        });
        self.error = None;
        self.update = UpdateState::Checking(rx);
    }

    fn start_install(&mut self, ctx: &egui::Context) {
        let UpdateState::Available(release) = &self.update else {
            return;
        };
        let (tag, url, kind) = (
            release.tag.clone(),
            release.deb_url.clone(),
            self.install_kind,
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::install(&url, kind).map(|()| tag));
            ctx.request_repaint();
        });
        self.error = None;
        self.update = UpdateState::Installing(rx);
    }

    /// Reinicia con el binario nuevo; si hay cambios sin guardar, primero pasa
    /// por el modal de cierre.
    fn request_restart(&mut self, ctx: &egui::Context) {
        self.restart = true;
        if let Some(idx) = self.next_dirty_tab() {
            self.pending = Some(CloseTarget::Window(idx));
        } else {
            self.relaunch();
            ctx.request_repaint();
        }
    }

    /// Abre el binario nuevo y deja que eframe cierre y guarde esta instancia.
    fn relaunch(&mut self) {
        self.restart = false;
        let launched = std::env::current_exe()
            .map_err(|err| err.to_string())
            .and_then(|exe| {
                // La espera deja que eframe guarde app.ron antes de abrir la nueva
                // instancia; sin ella las dos leen y escriben el mismo estado.
                let script = format!("sleep 0.5; exec '{}'", exe.display());
                std::process::Command::new("sh")
                    .args(["-c", &script])
                    .spawn()
                    .map(|_| ())
                    .map_err(|err| err.to_string())
            });
        match launched {
            Ok(()) => self.close_now = true,
            Err(err) => self.error = Some(format!("No se pudo reiniciar: {err}")),
        }
    }

    fn tabs_bar(&mut self, ui: &mut egui::Ui) {
        if self.tabs.is_empty() {
            self.hovered_tab = None;
            return;
        }

        let mut close_tab = None;
        let mut hovered_tab = None;
        let prev_hover = self.hovered_tab;

        ui.add_space(2.0);
        egui::ScrollArea::horizontal()
            .id_salt("tabs")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for pos in 0..self.tabs.len() {
                        let (close, hovered) = self.tab_pill(ui, pos, prev_hover);
                        close_tab = close_tab.or(close);
                        if hovered {
                            hovered_tab = Some(self.tabs[pos]);
                        }
                    }
                });
            });
        ui.add_space(2.0);

        self.hovered_tab = hovered_tab;
        if let Some(idx) = close_tab {
            self.request_close_tab(idx);
        }
    }

    /// Un tab: nombre y `×` dentro del mismo recuadro. Devuelve (cerrar, hovered).
    fn tab_pill(
        &mut self,
        ui: &mut egui::Ui,
        pos: usize,
        prev_hover: Option<usize>,
    ) -> (Option<usize>, bool) {
        let Some(&idx) = self.tabs.get(pos) else {
            return (None, false);
        };
        let Some(note) = self.notes.get(idx) else {
            return (None, false);
        };
        let active = self.active == pos;
        let title = note.title.clone();
        let dirty = self.dirty.contains(&note.dir);
        let show_close = active || prev_hover == Some(idx);

        let fill = if active {
            style::active_fill(ui.visuals())
        } else if prev_hover == Some(idx) {
            ui.visuals().widgets.hovered.weak_bg_fill
        } else {
            egui::Color32::TRANSPARENT
        };
        let mut close = None;

        let response = egui::Frame::new()
            .fill(fill)
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 4))
            .show(ui, |ui| {
                // 200 = 180 de texto + márgenes + el `×`.
                ui.set_max_width(200.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let color = if active {
                        ui.visuals().text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    let name = ui.add(
                        egui::Label::new(title_job(
                            &title,
                            color,
                            12.5,
                            dirty.then(|| style::accent(ui.visuals())),
                        ))
                        .truncate()
                        .sense(egui::Sense::click()),
                    );
                    if name.clicked() {
                        self.active = pos;
                    }
                    if show_close
                        && icon_button(ui, Icon::Close, 16.0, "Cerrar tab", false).clicked()
                    {
                        close = Some(idx);
                    }
                });
            })
            .response;

        if active && let Some(start) = self.saved_flash {
            let elapsed = start.elapsed().as_millis() as u64;
            if elapsed < style::SAVE_FLASH_MS {
                let alpha = 1.0 - elapsed as f32 / style::SAVE_FLASH_MS as f32;
                ui.painter().rect_filled(
                    response.rect,
                    6.0,
                    style::accent(ui.visuals()).gamma_multiply(0.3 * alpha),
                );
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(16));
            }
        }

        (close, response.hovered())
    }

    fn notes_panel(&mut self, ui: &mut egui::Ui) {
        let mut open = None;
        let mut delete = None;
        let mut pencil = None;
        let mut commit = false;
        let mut hovered = self.hovered_row;

        egui::Panel::left("notas")
            .exact_size(style::SIDEBAR_WIDTH)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().faint_bg_color)
                    .inner_margin(egui::Margin::symmetric(8, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let button_w = 26.0;
                    let search_w = (ui.available_width() - button_w - 6.0).max(60.0);
                    let search = ui.add_sized(
                        egui::vec2(search_w, 24.0),
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Buscar…  (Ctrl+F)")
                            .desired_width(search_w),
                    );
                    if std::mem::take(&mut self.focus_search) {
                        search.request_focus();
                    }
                    if icon_button(ui, Icon::Plus, 24.0, "Nueva nota (Ctrl+N)", false).clicked() {
                        self.new_note();
                    }
                });
                ui.add_space(4.0);

                let query = store::fold(&self.search);
                let mut indices: Vec<usize> = (0..self.notes.len()).collect();
                match self.order {
                    Order::Alphabetical => {
                        indices.sort_by_cached_key(|&idx| store::fold(&self.notes[idx].title));
                    }
                    Order::Recent => indices.sort_by(|&a, &b| {
                        self.notes[b].mtime.cmp(&self.notes[a].mtime).then_with(|| {
                            store::fold(&self.notes[a].title)
                                .cmp(&store::fold(&self.notes[b].title))
                        })
                    }),
                }

                // La regla de la nota activa vive en el margen, no sobre el texto.
                let rail_x = ui.max_rect().left() - 5.0;

                egui::ScrollArea::vertical()
                    .id_salt("lista-notas")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        hovered = None;
                        let mut visibles = 0;

                        for idx in indices {
                            let note = &self.notes[idx];
                            if !query.is_empty()
                                && !store::fold(&note.title).contains(&query)
                                && !store::fold(&note.body).contains(&query)
                            {
                                continue;
                            }
                            visibles += 1;

                            if self.renaming == Some(idx) {
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut self.rename_text)
                                        .id(ui.make_persistent_id(("renombrar", idx)))
                                        .hint_text("Nombre de la nota")
                                        .desired_width(f32::INFINITY),
                                );
                                if self.rename_focus {
                                    response.request_focus();
                                    // Texto seleccionado: escribir encima reemplaza.
                                    if let Some(mut state) =
                                        egui::text_edit::TextEditState::load(ui.ctx(), response.id)
                                    {
                                        let end = egui::text::CCursor::new(
                                            self.rename_text.chars().count(),
                                        );
                                        state.cursor.set_char_range(Some(
                                            egui::text_selection::CCursorRange::two(
                                                egui::text::CCursor::new(0),
                                                end,
                                            ),
                                        ));
                                        state.store(ui.ctx(), response.id);
                                    }
                                    self.rename_focus = false;
                                } else if response.lost_focus() {
                                    commit = true;
                                }
                                continue;
                            }

                            let dirty = self.dirty.contains(&note.dir);
                            let snippet = snippet(&note.body);
                            let is_active = self.tabs.get(self.active).copied() == Some(idx);
                            let show_close = hovered == Some(idx);

                            let bg = ui.painter().add(egui::Shape::Noop);
                            let row = ui
                                .scope_builder(
                                    egui::UiBuilder::new().sense(egui::Sense::click()),
                                    |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.set_min_height(44.0);
                                        ui.add_space(4.0);
                                        egui::Sides::new().shrink_left().show(
                                            ui,
                                            |ui| {
                                                ui.vertical(|ui| {
                                                    ui.spacing_mut().item_spacing.y = 2.0;
                                                    let color = ui.visuals().text_color();
                                                    ui.add(
                                                        egui::Label::new(title_job(
                                                            &note.title,
                                                            color,
                                                            13.5,
                                                            dirty.then(|| {
                                                                style::accent(ui.visuals())
                                                            }),
                                                        ))
                                                        .truncate()
                                                        .selectable(false),
                                                    );
                                                    ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(&snippet)
                                                                .small()
                                                                .weak(),
                                                        )
                                                        .truncate()
                                                        .selectable(false),
                                                    );
                                                });
                                            },
                                            |ui| {
                                                if icon_button(
                                                    ui,
                                                    Icon::Pencil,
                                                    20.0,
                                                    "Renombrar",
                                                    false,
                                                )
                                                .clicked()
                                                {
                                                    pencil = Some(idx);
                                                }
                                                if show_close {
                                                    if icon_button(
                                                        ui,
                                                        Icon::Close,
                                                        20.0,
                                                        "Mover a papelera",
                                                        true,
                                                    )
                                                    .clicked()
                                                    {
                                                        delete = Some(idx);
                                                    }
                                                } else {
                                                    // Reserva el lugar del `×` para que el lápiz no salte.
                                                    ui.add_space(20.0);
                                                }
                                            },
                                        );
                                    },
                                )
                                .response;

                            if row.hovered() {
                                hovered = Some(idx);
                            }
                            if row.hovered() || is_active {
                                let fill = if is_active {
                                    style::active_fill(ui.visuals())
                                } else {
                                    ui.visuals().widgets.hovered.weak_bg_fill
                                };
                                ui.painter()
                                    .set(bg, egui::Shape::rect_filled(row.rect, 6.0, fill));
                                if is_active {
                                    let rule = egui::Rect::from_min_size(
                                        egui::pos2(rail_x, row.rect.center().y - 9.0),
                                        egui::vec2(2.0, 18.0),
                                    );
                                    ui.painter().rect_filled(
                                        rule,
                                        1.0,
                                        style::accent(ui.visuals()),
                                    );
                                }
                            }
                            if row.clicked() {
                                open = Some(idx);
                            }
                            row.container_context_menu(|ui| {
                                if ui.button("Abrir").clicked() {
                                    open = Some(idx);
                                    ui.close();
                                }
                                if ui.button("Renombrar").clicked() {
                                    pencil = Some(idx);
                                    ui.close();
                                }
                                if ui.button("Mover a papelera").clicked() {
                                    delete = Some(idx);
                                    ui.close();
                                }
                            });
                        }

                        if visibles == 0 {
                            ui.add_space(8.0);
                            if self.notes.is_empty() {
                                ui.weak("Todavía no hay notas.");
                                ui.add_space(2.0);
                                ui.weak(egui::RichText::new("Ctrl+N crea la primera").small());
                            } else {
                                ui.weak(format!("Sin resultados para «{}».", self.search.trim()));
                            }
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
        if let Some(idx) = pencil {
            self.commit_rename();
            self.start_rename(idx);
        } else if commit {
            self.commit_rename();
        }
    }

    fn editor_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            self.tabs_bar(ui);
            ui.separator();

            if let Some(error) = self.error.clone() {
                ui.add_space(6.0);
                ui.colored_label(ui.visuals().error_fg_color, error);
            }

            let Some(idx) = self.active_note() else {
                ui.allocate_ui_with_layout(
                    ui.available_size(),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.add_space(ui.available_height() * 0.38);
                        ui.label(
                            egui::RichText::new("Ninguna nota abierta")
                                .size(15.0)
                                .color(ui.visuals().weak_text_color()),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new("Elegí una nota de la lista o creá una con Ctrl+N")
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                    },
                );
                return;
            };

            let width = style::READING_WIDTH.min(ui.available_width() - 16.0);
            ui.add_space(18.0);
            let editor_id = egui::Id::unique("cuerpo-editor");
            let menu_key = editor_id.with("seleccion-menu");
            // egui colapsa la selección al presionar el botón secundario: la
            // guardamos antes de que el TextEdit procese el click.
            if ui.input(|input| input.pointer.secondary_pressed())
                && let Some(state) = egui::TextEdit::load_state(ui.ctx(), editor_id)
            {
                let selection = state.cursor.char_range().filter(|range| !range.is_empty());
                ui.ctx()
                    .data_mut(|data| data.insert_temp(menu_key, selection));
            }
            // El TextEdit crece con el contenido; el ScrollArea se encarga del resto.
            egui::ScrollArea::vertical()
                .id_salt("cuerpo")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Mínimo: alto del viewport, para que un click en cualquier
                    // parte del área vacía caiga dentro del editor.
                    // shortcut: fila estimada en 22 px (15 px + 4 de aire);
                    // si cambia la escala del editor, recalcular.
                    let rows = (((ui.available_height() - 40.0) / 22.0).floor() as usize).max(1);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        let body = ui.add(
                            egui::TextEdit::multiline(&mut self.notes[idx].body)
                                .id(editor_id)
                                .desired_rows(rows)
                                .hint_text("Escribí acá…")
                                .font(egui::TextStyle::Name("editor".into()))
                                .frame(egui::Frame::NONE)
                                .margin(egui::Margin::same(0))
                                .text_color(ui.visuals().text_color())
                                .desired_width(width),
                        );
                        // El click secundario colapsa la selección dentro del
                        // TextEdit: la restauramos para que siga resaltada
                        // mientras el menú está abierto.
                        if ui.input(|input| input.pointer.secondary_pressed())
                            && let Some(selection) = ui
                                .ctx()
                                .data(|data| {
                                    data.get_temp::<Option<egui::text_selection::CCursorRange>>(
                                        menu_key,
                                    )
                                })
                                .flatten()
                            && let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), editor_id)
                        {
                            state.cursor.set_char_range(Some(selection));
                            state.store(ui.ctx(), editor_id);
                        }
                        if body.changed() {
                            self.mark_dirty(idx);
                        }
                        if editor_context_menu(&body, &mut self.notes[idx].body, menu_key) {
                            self.mark_dirty(idx);
                        }
                        ui.add_space(32.0);
                    });
                });
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
                ui.scope(|ui| {
                    let widgets = &mut ui.visuals_mut().widgets;
                    let text = egui::Color32::from_rgb(0xf7, 0xfb, 0xfb);
                    widgets.inactive.weak_bg_fill = style::ACCENT;
                    widgets.hovered.weak_bg_fill = style::ACCENT.gamma_multiply(1.15);
                    widgets.active.weak_bg_fill = style::ACCENT.gamma_multiply(0.9);
                    for widget in [
                        &mut widgets.inactive,
                        &mut widgets.hovered,
                        &mut widgets.active,
                    ] {
                        widget.fg_stroke = egui::Stroke::new(1.0, text);
                        widget.bg_stroke = egui::Stroke::NONE;
                    }
                    if ui.button("Guardar").clicked() {
                        choice = Some(Choice::Save);
                    }
                });
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
                self.restart = false;
            }
            None => self.pending = Some(target),
        }
    }

    /// Cuerpo de la UI, compartido entre eframe y los tests de `egui_kittest`.
    fn root_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        if self.applied_theme != Some(self.theme) {
            style::install(&ctx);
            ctx.set_theme(self.theme.preference());
            // El Ui raíz ya se creó con el estilo anterior; este frame también lo usa.
            *ui.style_mut() = (*ctx.global_style()).clone();
            self.applied_theme = Some(self.theme);
        }
        if let Some(maximized) = ctx.input(|input| input.viewport().maximized) {
            self.maximized = maximized;
        }
        if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
            self.resize_edges(&ctx, ui.max_rect(), pos);
        }

        self.rescan_on_focus(&ctx);
        self.handle_close_request(&ctx);
        self.poll_update(&ctx);

        if self.close_now {
            self.close_now = false;
            if self.restart {
                self.relaunch();
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        self.handle_shortcuts(&ctx);
        self.title_bar(ui);
        self.notes_panel(ui);
        self.editor_panel(ui);
        self.paint_hairlines(ui);
        self.unsaved_modal(&ctx);

        // Asegura el frame siguiente, donde se manda el `Close` real.
        if self.close_now {
            ctx.request_repaint();
        }
    }

    /// Redimensionado por bordes: sin decoración nativa no hay manijas del compositor.
    fn resize_edges(&self, ctx: &egui::Context, rect: egui::Rect, pos: egui::Pos2) {
        if self.maximized {
            return;
        }
        const T: f32 = 6.0;
        let left = pos.x <= rect.left() + T;
        let right = pos.x >= rect.right() - T;
        let top = pos.y <= rect.top() + T;
        let bottom = pos.y >= rect.bottom() - T;

        use egui::{CursorIcon as Cursor, ResizeDirection as Dir};
        let (dir, cursor) = match (left, right, top, bottom) {
            (true, _, true, _) => (Dir::NorthWest, Cursor::ResizeNwSe),
            (_, true, true, _) => (Dir::NorthEast, Cursor::ResizeNeSw),
            (true, _, _, true) => (Dir::SouthWest, Cursor::ResizeNeSw),
            (_, true, _, true) => (Dir::SouthEast, Cursor::ResizeNwSe),
            (true, _, _, _) => (Dir::West, Cursor::ResizeHorizontal),
            (_, true, _, _) => (Dir::East, Cursor::ResizeHorizontal),
            (_, _, true, _) => (Dir::North, Cursor::ResizeVertical),
            (_, _, _, true) => (Dir::South, Cursor::ResizeVertical),
            _ => return,
        };

        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.root_ui(ui);
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
            theme: self.theme,
            order: self.order,
        };
        eframe::set_value(storage, STORAGE_KEY, &persisted);
    }
}

/// Abre una carpeta en el gestor de archivos; `gio` es el respaldo de `xdg-open`.
fn open_folder(path: &Path) -> std::io::Result<()> {
    if matches!(
        std::process::Command::new("xdg-open").arg(path).status(),
        Ok(status) if status.success()
    ) {
        return Ok(());
    }
    match std::process::Command::new("gio")
        .arg("open")
        .arg(path)
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(std::io::Error::other(format!(
            "gio open terminó con {status}"
        ))),
        Err(err) => Err(err),
    }
}

/// Texto con el punto de «sin guardar» al final, truncable como un solo bloque.
fn title_job(
    text: &str,
    color: egui::Color32,
    size: f32,
    dot: Option<egui::Color32>,
) -> egui::text::LayoutJob {
    let format = egui::TextFormat {
        font_id: egui::FontId::proportional(size),
        color,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(text, 0.0, format.clone());
    if let Some(dot) = dot {
        job.append(
            "  •",
            0.0,
            egui::TextFormat {
                color: dot,
                ..format
            },
        );
    }
    job
}

/// Menú contextual del editor. Devuelve `true` si el cuerpo cambió.
fn editor_context_menu(response: &egui::Response, body: &mut String, menu_key: egui::Id) -> bool {
    use egui::TextBuffer as _;

    let mut changed = false;
    let mut acted = false;

    response.context_menu(|ui| {
        let ctx = ui.ctx().clone();
        let id = response.id;
        let selection = ctx
            .data(|data| data.get_temp::<Option<egui::text_selection::CCursorRange>>(menu_key))
            .flatten();

        let Some(mut state) = egui::TextEdit::load_state(&ctx, id) else {
            return;
        };
        let cursor = state.cursor.char_range().unwrap_or_default();

        if ui
            .add_enabled(selection.is_some(), egui::Button::new("Cortar"))
            .clicked()
            && let Some(selection) = selection
        {
            ctx.copy_text(selection.slice_str(body).to_owned());
            let cursor = body.delete_selected(&selection);
            state
                .cursor
                .set_char_range(Some(egui::text_selection::CCursorRange::one(cursor)));
            changed = true;
            acted = true;
        }

        if ui
            .add_enabled(selection.is_some(), egui::Button::new("Copiar"))
            .clicked()
            && let Some(selection) = selection
        {
            ctx.copy_text(selection.slice_str(body).to_owned());
            state.cursor.set_char_range(Some(selection));
            acted = true;
        }

        if ui.button("Pegar").clicked()
            && let Some(text) = pasted_text()
        {
            let mut cursor = body.delete_selected(&selection.unwrap_or(cursor));
            body.insert_text_at(&mut cursor, &text, usize::MAX);
            state
                .cursor
                .set_char_range(Some(egui::text_selection::CCursorRange::one(cursor)));
            changed = true;
            acted = true;
        }

        if ui.button("Seleccionar todo").clicked() {
            state
                .cursor
                .set_char_range(Some(egui::text_selection::CCursorRange::select_all_str(
                    body,
                )));
            acted = true;
        }

        if acted {
            state.store(&ctx, id);
            ui.close();
        }
    });

    if acted {
        response.request_focus();
    }
    changed
}

fn pasted_text() -> Option<String> {
    arboard::Clipboard::new()
        .ok()?
        .get_text()
        .ok()
        .filter(|text| !text.is_empty())
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

#[derive(Clone, Copy)]
enum Icon {
    Menu,
    Plus,
    Pencil,
    Close,
    Minimize,
    Maximize,
    Restore,
}

/// Botón cuadrado con ícono dibujado (la fuente por defecto no tiene estos glifos).
fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    tooltip: &str,
    danger: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let hovered = response.hovered();
    let color = if danger && hovered {
        egui::Color32::WHITE
    } else if hovered {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    if hovered {
        let fill = if danger {
            egui::Color32::from_rgb(190, 60, 55)
        } else {
            ui.visuals().widgets.hovered.weak_bg_fill
        };
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    paint_icon(ui.painter(), icon, rect, color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tooltip));
    response.on_hover_text(tooltip)
}

fn paint_icon(painter: &egui::Painter, icon: Icon, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.5, color);
    let center = rect.center();
    let s = rect.width();

    match icon {
        Icon::Menu => {
            for i in -1..=1 {
                let y = center.y + i as f32 * s * 0.16;
                painter.line_segment(
                    [
                        egui::pos2(center.x - s * 0.22, y),
                        egui::pos2(center.x + s * 0.22, y),
                    ],
                    stroke,
                );
            }
        }
        Icon::Close => {
            let r = s * 0.22;
            painter.line_segment(
                [center + egui::vec2(-r, -r), center + egui::vec2(r, r)],
                stroke,
            );
            painter.line_segment(
                [center + egui::vec2(r, -r), center + egui::vec2(-r, r)],
                stroke,
            );
        }
        Icon::Minimize => {
            painter.line_segment(
                [
                    center + egui::vec2(-s * 0.22, s * 0.06),
                    center + egui::vec2(s * 0.22, s * 0.06),
                ],
                stroke,
            );
        }
        Icon::Maximize => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(s * 0.42, s * 0.42)),
                1,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Restore => {
            let size = egui::vec2(s * 0.34, s * 0.34);
            painter.rect_stroke(
                egui::Rect::from_center_size(center + egui::vec2(s * 0.09, s * 0.09), size),
                1,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.rect_stroke(
                egui::Rect::from_center_size(center - egui::vec2(s * 0.09, s * 0.09), size),
                1,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Plus => {
            painter.line_segment(
                [
                    center + egui::vec2(-s * 0.18, 0.0),
                    center + egui::vec2(s * 0.18, 0.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    center + egui::vec2(0.0, -s * 0.18),
                    center + egui::vec2(0.0, s * 0.18),
                ],
                stroke,
            );
        }
        Icon::Pencil => {
            let start = center + egui::vec2(-s * 0.28, s * 0.28);
            let end = center + egui::vec2(s * 0.26, -s * 0.26);
            painter.line_segment([start, end], egui::Stroke::new(2.0, color));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    start + egui::vec2(-s * 0.07, -s * 0.01),
                    start + egui::vec2(s * 0.01, s * 0.07),
                    start + egui::vec2(-s * 0.10, s * 0.10),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    use egui::Pos2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn seed(dir: &std::path::Path, notes: &[(&str, &str)]) {
        for (name, body) in notes {
            let path = dir.join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("note.md"), body).unwrap();
        }
    }

    fn harness(dir: &std::path::Path) -> Harness<'static, App> {
        let app = App::load(dir.to_path_buf());
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1000.0, 700.0))
            .build_ui_state(|ui, app: &mut App| app.root_ui(ui), app);
        harness.run();
        harness
    }

    fn click(harness: &mut Harness<'static, App>, pos: Pos2) {
        harness.hover_at(pos);
        harness.drag_at(pos);
        harness.drop_at(pos);
        harness.run();
    }

    fn right_click(harness: &mut Harness<'static, App>, pos: Pos2) {
        harness.hover_at(pos);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::default(),
            });
        }
        harness.run();
    }

    #[test]
    fn abre_tab_al_clickear_la_fila() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\n")]);
        let mut h = harness(tmp.path());

        h.get_by_label("nota-a").click();
        h.run();

        assert_eq!(h.state().tabs.len(), 1, "la nota debería abrirse en un tab");
        assert_eq!(h.state().notes[0].title, "nota-a");
    }

    #[test]
    fn primera_frame_con_nota_restaurada() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\n")]);

        // Sin `run()` previo: reproduce una sesión restaurada, donde el primer
        // frame ya dibuja el editor.
        let mut app = App::load(tmp.path().to_path_buf());
        app.open_tab(0);
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 700.0))
            .build_ui_state(|ui, app: &mut App| app.root_ui(ui), app);
        h.run();

        assert_eq!(h.state().tabs.len(), 1);
    }

    #[test]
    fn edita_y_guarda_con_ctrl_s() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\n")]);
        let mut h = harness(tmp.path());

        h.get_by_label("nota-a").click();
        h.run();
        click(&mut h, Pos2::new(600.0, 300.0));
        h.event(egui::Event::Text("dos".to_owned()));
        h.run();

        assert!(
            h.state().is_dirty(0),
            "escribir debería marcar la nota sucia"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("nota-a/note.md")).unwrap(),
            "uno\n",
            "no debería guardarse antes de Ctrl+S"
        );

        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        h.run();

        assert!(!h.state().is_dirty(0));
        assert!(
            h.state().saved_flash.is_some(),
            "guardar debería disparar el destello"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("nota-a/note.md")).unwrap(),
            "uno\ndos"
        );
    }

    #[test]
    fn renombra_inline_desde_la_lista() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("sin-titulo", "cuerpo\n")]);
        let mut h = harness(tmp.path());

        h.get_by_label("Renombrar").click();
        h.run();
        assert_eq!(h.state().renaming, Some(0));

        h.event(egui::Event::Text("comprar pan".to_owned()));
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();

        assert!(tmp.path().join("comprar-pan/note.md").is_file());
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("comprar-pan/note.md")).unwrap(),
            "cuerpo\n",
            "renombrar no debe reescribir el cuerpo"
        );
        assert!(h.state().renaming.is_none());
        assert_eq!(h.state().notes[0].title, "comprar-pan");
    }

    #[test]
    fn ajustes_cambia_el_tema() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "")]);
        let mut h = harness(tmp.path());

        h.get_by_label("Ajustes").click();
        h.run();
        h.get_by_label("Tema").click();
        h.run();
        h.get_by_label("Oscuro").click();
        h.run();

        assert_eq!(h.state().theme, Theme::Dark);
        assert_eq!(
            h.ctx.options(|options| options.theme_preference),
            egui::ThemePreference::Dark
        );
    }

    #[test]
    fn ajustes_cambia_el_orden_de_la_lista() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "")]);
        std::thread::sleep(std::time::Duration::from_millis(20));
        seed(tmp.path(), &[("nota-b", "")]);
        let mut h = harness(tmp.path());

        assert!(
            h.get_by_label("nota-a").rect().top() < h.get_by_label("nota-b").rect().top(),
            "por defecto va alfabético"
        );

        h.get_by_label("Ajustes").click();
        h.run();
        h.get_by_label("Orden").click();
        h.run();
        h.get_by_label("Reciente").click();
        h.run();

        assert_eq!(h.state().order, Order::Recent);
        assert!(
            h.get_by_label("nota-b").rect().top() < h.get_by_label("nota-a").rect().top(),
            "la más reciente va arriba"
        );
    }

    #[test]
    fn ajustes_no_deja_actualizar_desde_un_build_de_desarrollo() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "")]);
        let mut h = harness(tmp.path());

        h.get_by_label("Ajustes").click();
        h.run();

        // Los tests corren desde `target/`: no hay binario instalado que reemplazar.
        assert_eq!(h.state().install_kind, InstallKind::Unsupported);
        h.get_by_label("Buscar actualizaciones").click();
        h.run();
        assert!(
            matches!(h.state().update, UpdateState::Idle),
            "el ítem deshabilitado no debe arrancar el chequeo"
        );
    }

    #[test]
    fn esc_cancela_el_renombre() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "")]);
        let mut h = harness(tmp.path());

        h.get_by_label("Renombrar").click();
        h.run();
        h.event(egui::Event::Text("otro".to_owned()));
        h.run();
        h.key_press(egui::Key::Escape);
        h.run();

        assert!(tmp.path().join("nota-a").is_dir());
        assert!(!tmp.path().join("otro").exists());
        assert_eq!(h.state().notes[0].title, "nota-a");
    }

    #[test]
    fn renombrar_conserva_los_cambios_sin_guardar() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\n")]);
        let mut h = harness(tmp.path());

        h.get_by_label("nota-a").click();
        h.run();
        click(&mut h, Pos2::new(600.0, 300.0));
        h.event(egui::Event::Text("dos".to_owned()));
        h.run();
        assert!(h.state().is_dirty(0));

        h.get_by_label("Renombrar").click();
        h.run();
        h.event(egui::Event::Text("nueva".to_owned()));
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();

        assert!(
            h.state().is_dirty(0),
            "el renombre no debe limpiar el sucio"
        );
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        h.run();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("nueva/note.md")).unwrap(),
            "uno\ndos"
        );
    }

    #[test]
    fn nueva_nota_arranca_renombre() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path()).unwrap();
        let mut h = harness(tmp.path());

        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
        h.run();
        assert_eq!(h.state().renaming, Some(0));
        assert_eq!(h.state().rename_text, "sin-titulo");

        h.event(egui::Event::Text("mi nota".to_owned()));
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();

        assert!(tmp.path().join("mi-nota/note.md").is_file());
        assert!(!tmp.path().join("sin-titulo").exists());
    }

    #[test]
    fn busqueda_filtra_titulo_y_cuerpo() {
        let tmp = tempfile::tempdir().unwrap();
        seed(
            tmp.path(),
            &[
                ("comprar-cafe", "leche\n"),
                ("ideas", "una nota sobre cafe\n"),
            ],
        );
        let mut h = harness(tmp.path());

        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
        h.run();
        h.event(egui::Event::Text("cafe".to_owned()));
        h.run();

        assert_eq!(h.state().search, "cafe");
        // Ambos matchean: uno por título, otro por cuerpo.
        let visibles = h
            .state()
            .notes
            .iter()
            .filter(|note| {
                let q = store::fold(&h.state().search);
                store::fold(&note.title).contains(&q) || store::fold(&note.body).contains(&q)
            })
            .count();
        assert_eq!(visibles, 2);

        h.key_press(egui::Key::Escape);
        h.run();
        assert!(h.state().search.is_empty());
    }

    #[test]
    fn modal_al_cerrar_tab_sucio_y_descartar() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\n")]);
        let mut h = harness(tmp.path());

        h.get_by_label("nota-a").click();
        h.run();
        click(&mut h, Pos2::new(600.0, 300.0));
        h.event(egui::Event::Text("cambio".to_owned()));
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::W);
        h.run();

        assert!(
            h.query_by_label("Cambios sin guardar").is_some(),
            "el modal debería estar visible"
        );

        h.get_by_label("Descartar").click();
        h.run();

        assert!(h.state().tabs.is_empty(), "el tab debería cerrarse");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("nota-a/note.md")).unwrap(),
            "uno\n",
            "descartar no debe escribir en disco"
        );
    }

    #[test]
    fn estado_vacio_sin_nota_abierta() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\n")]);
        let mut h = harness(tmp.path());

        assert!(h.query_by_label("Ninguna nota abierta").is_some());

        h.get_by_label("nota-a").click();
        h.run();

        assert!(h.query_by_label("Ninguna nota abierta").is_none());
    }

    #[test]
    fn estado_vacio_del_buscador() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\n")]);
        let mut h = harness(tmp.path());

        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
        h.run();
        h.event(egui::Event::Text("zzz".to_owned()));
        h.run();

        assert!(h.query_by_label("Sin resultados para «zzz».").is_some());
    }

    #[test]
    fn menu_contextual_del_editor_corta_la_seleccion() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\ndos\n")]);
        let mut h = harness(tmp.path());

        h.get_by_label("nota-a").click();
        h.run();
        let editor = Pos2::new(600.0, 300.0);
        click(&mut h, editor);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run();

        right_click(&mut h, editor);
        let state = egui::TextEdit::load_state(&h.ctx, egui::Id::unique("cuerpo-editor")).unwrap();
        assert_eq!(
            state.cursor.char_range(),
            Some(egui::text_selection::CCursorRange::select_all_str(
                "uno\ndos\n"
            )),
            "el click derecho no debería perder la selección"
        );
        h.get_by_label("Cortar").click();
        h.run();

        assert!(
            h.state().notes[0].body.is_empty(),
            "cortar debería vaciar la selección completa"
        );
        assert!(h.state().is_dirty(0), "cortar debería marcar la nota sucia");
    }

    #[test]
    fn menu_contextual_de_la_lista_abre_la_nota() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\n")]);
        let mut h = harness(tmp.path());

        h.get_by_label("nota-a").click_secondary();
        h.run();
        h.get_by_label("Abrir").click();
        h.run();

        assert_eq!(h.state().tabs.len(), 1, "el menú debería abrir la nota");
    }

    #[test]
    fn snapshot_tema_claro() {
        let tmp = tempfile::tempdir().unwrap();
        seed(
            tmp.path(),
            &[
                ("comprar-cafe", "Leche\nPan\n"),
                ("idea-app", "App de notas en Rust con egui.\n"),
                ("reunion-lunes", "Lunes 10:00\nMetricas del sprint\n"),
            ],
        );
        let app = App::load(tmp.path().to_path_buf());
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 700.0))
            .with_pixels_per_point(1.0)
            .wgpu()
            .build_ui_state(|ui, app: &mut App| app.root_ui(ui), app);
        h.run();

        h.get_by_label("comprar-cafe").click();
        h.run();
        h.get_by_label("Ajustes").click();
        h.run();
        h.get_by_label("Tema").click();
        h.run();
        h.get_by_label("Claro").click();
        h.run();

        let claro = h.render().expect("render wgpu");
        claro
            .save(std::env::temp_dir().join("kt-light.png"))
            .unwrap();
    }

    #[test]
    fn snapshot_estados_renderizados() {
        let tmp = tempfile::tempdir().unwrap();
        seed(
            tmp.path(),
            &[
                ("comprar-cafe", "Leche\nPan\n"),
                ("idea-app", "App de notas en Rust con egui.\n"),
                ("reunion-lunes", "Lunes 10:00\nMetricas del sprint\n"),
            ],
        );
        let app = App::load(tmp.path().to_path_buf());
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 700.0))
            .with_pixels_per_point(1.0)
            .wgpu()
            .build_ui_state(|ui, app: &mut App| app.root_ui(ui), app);
        h.run();

        h.get_by_label("comprar-cafe").click();
        h.run();
        click(&mut h, Pos2::new(600.0, 300.0));
        h.event(egui::Event::Text("texto sucio".to_owned()));
        h.run();
        let limpio = h.render().expect("render wgpu");
        limpio
            .save(std::env::temp_dir().join("kt-dirty.png"))
            .unwrap();

        let primero = h.get_all_by_label("Renombrar").next().unwrap();
        primero.click();
        h.run();
        let editando = h.render().expect("render wgpu");
        editando
            .save(std::env::temp_dir().join("kt-rename.png"))
            .unwrap();
        h.key_press(egui::Key::Escape);
        h.run();

        h.get_by_label("Ajustes").click();
        h.run();
        h.get_by_label("Tema").click();
        h.run();
        let menu = h.render().expect("render wgpu");
        menu.save(std::env::temp_dir().join("kt-menu.png")).unwrap();
        h.key_press(egui::Key::Escape);
        h.run();

        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::W);
        h.run();
        let modal = h.render().expect("render wgpu");
        modal
            .save(std::env::temp_dir().join("kt-modal.png"))
            .unwrap();
    }
}
