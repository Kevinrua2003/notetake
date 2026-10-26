use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use eframe::egui;

use crate::closing::{Action, Closing, Intent};
use crate::store;
use crate::style;
use crate::ui::{Icon, accent_button, icon_button, paint_icon, snippet, title_job};
use crate::update::{self, InstallKind};
use crate::workspace::{NoteId, Order, Workspace};

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
    Failed(String),
}

/// Lo que el modal necesita mostrar, sin arrastrar los canales.
#[derive(Clone)]
enum UpdateView {
    Idle,
    Checking,
    UpToDate,
    Available(String),
    Installing,
    Installed(String),
    Failed(String),
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
            Self::Failed(err) => UpdateView::Failed(err.clone()),
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
    workspace: Workspace,
    search: String,
    error: Option<String>,
    hovered_row: Option<NoteId>,
    hovered_tab: Option<NoteId>,
    focus_search: bool,
    /// Texto del renombre inline; el id en edición vive en el workspace.
    rename_text: String,
    rename_focus: bool,
    theme: Theme,
    order: Order,
    /// Último tema aplicado al contexto, para no tocar las opciones cada frame.
    applied_theme: Option<Theme>,
    /// Momento del último guardado, para el destello del tab.
    saved_flash: Option<std::time::Instant>,
    maximized: bool,
    closing: Closing,
    was_focused: bool,
    update: UpdateState,
    /// El modal de actualizaciones está abierto.
    update_modal: bool,
    /// Detectado una vez: `cargo run` no se puede reemplazar solo.
    install_kind: InstallKind,
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
            workspace: Workspace::load(data_dir),
            search: String::new(),
            error: None,
            hovered_row: None,
            hovered_tab: None,
            focus_search: false,
            rename_text: String::new(),
            rename_focus: false,
            theme: Theme::default(),
            order: Order::default(),
            applied_theme: None,
            saved_flash: None,
            maximized: false,
            closing: Closing::Idle,
            was_focused: true,
            update: UpdateState::Idle,
            update_modal: false,
            install_kind: update::install_kind(),
        }
    }

    fn restore(&mut self, persisted: Persisted) {
        self.theme = persisted.theme;
        self.order = persisted.order;
        self.workspace.restore(persisted.tabs, persisted.active);
    }

    fn request_close_tab(&mut self, id: NoteId) {
        if self.workspace.is_dirty(id) {
            self.closing.ask(id, Intent::Tab);
        } else {
            self.workspace.close(id);
        }
    }

    fn new_note(&mut self) {
        self.commit_rename();
        match self.workspace.create() {
            Ok(id) => {
                self.workspace.open(id);
                self.start_rename(id);
                self.error = None;
            }
            Err(err) => self.error = Some(format!("No se pudo crear la nota: {err}")),
        }
    }

    fn start_rename(&mut self, id: NoteId) {
        self.workspace.start_rename(id);
        if let Some(note) = self.workspace.note(id) {
            self.rename_text = note.title.clone();
        }
        self.rename_focus = true;
    }

    /// Confirma el renombre inline. Vacío o sin cambios no hace nada.
    fn commit_rename(&mut self) {
        let title = std::mem::take(&mut self.rename_text).trim().to_owned();
        match self.workspace.commit_rename(&title) {
            Ok(()) => self.error = None,
            Err(err) => self.error = Some(format!("No se pudo renombrar: {err}")),
        }
    }

    fn request_close_window(&mut self, ctx: &egui::Context) {
        match self.workspace.dirty_tab() {
            Some(id) => self.closing.ask(id, Intent::Window),
            None => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn toggle_maximized(&mut self, ctx: &egui::Context) {
        self.maximized = !self.maximized;
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.maximized));
    }

    fn save_note(&mut self, id: NoteId) -> Result<(), String> {
        self.workspace.save(id).map_err(|err| err.to_string())?;
        self.saved_flash = Some(std::time::Instant::now());
        Ok(())
    }

    fn save_current(&mut self, id: NoteId) {
        if !self.workspace.is_dirty(id) {
            return;
        }
        match self.save_note(id) {
            Ok(()) => self.error = None,
            Err(err) => self.error = Some(format!("No se pudo guardar: {err}")),
        }
    }

    fn trash_note(&mut self, id: NoteId) {
        if let Err(err) = self.workspace.trash(id) {
            self.error = Some(format!("No se pudo mover a la papelera: {err}"));
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.closing.is_asking() {
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
            clear_search = (self.workspace.editing().is_some() || !self.search.is_empty())
                && input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
        });

        if new_note {
            self.new_note();
        }
        if save && let Some(id) = self.workspace.active() {
            self.save_current(id);
        }
        if focus_search {
            self.focus_search = true;
        }
        if clear_search {
            if self.workspace.editing().is_some() {
                // Esc cancela la edición inline antes que la búsqueda.
                self.workspace.cancel_rename();
                self.rename_text.clear();
            } else {
                self.search.clear();
            }
        }
        if close_tab && let Some(id) = self.workspace.active() {
            self.request_close_tab(id);
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.viewport().close_requested()) {
            return;
        }
        if self.closing.is_asking() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            return;
        }
        if let Some(id) = self.workspace.dirty_tab() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing.ask(id, Intent::Window);
        }
        // Sin tabs sucios: no se cancela, eframe cierra la ventana.
    }

    fn rescan_on_focus(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|input| input.viewport().focused).unwrap_or(false);
        if focused && !self.was_focused {
            self.workspace.reconcile();
        }
        self.was_focused = focused;
    }

    /// Después del modal: cierra el tab o sigue con la próxima nota sucia.
    fn advance(&mut self, id: NoteId, intent: Intent) {
        match intent {
            Intent::Tab => self.workspace.close(id),
            Intent::Window | Intent::Restart => match self.workspace.dirty_tab() {
                Some(next) => self.closing.ask(next, intent),
                None if intent == Intent::Restart => self.closing.finish(Action::Relaunch),
                None => self.closing.finish(Action::CloseWindow),
            },
        }
    }

    /// Toma la respuesta del modal y sigue el cierre.
    fn advance_resolved(&mut self) {
        if let Some((id, intent)) = self.closing.resolved() {
            self.advance(id, intent);
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
        let kind = self.install_kind;
        let mut check = false;

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
            match kind {
                InstallKind::Unsupported => {
                    ui.add_enabled(false, egui::Button::new("Buscar actualizaciones"))
                        .on_disabled_hover_text("Build de desarrollo o ruta no reconocida");
                }
                _ => {
                    if ui.button("Buscar actualizaciones").clicked() {
                        check = true;
                        ui.close();
                    }
                }
            }
        });

        self.theme = theme;
        self.order = order;
        if open_data {
            let dir = self.workspace.data_dir().to_path_buf();
            self.open_dir(&dir);
        }
        if open_trash {
            let trash = store::trash_dir(self.workspace.data_dir());
            match std::fs::create_dir_all(&trash) {
                Ok(()) => self.open_dir(&trash),
                Err(err) => self.error = Some(format!("No se pudo crear la papelera: {err}")),
            }
        }
        if check {
            self.update_modal = true;
            self.start_check(ui.ctx());
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
                Ok(Err(err)) => UpdateState::Failed(err),
                Err(TryRecvError::Empty) => UpdateState::Checking(rx),
                Err(TryRecvError::Disconnected) => {
                    UpdateState::Failed("El chequeo de actualizaciones se cortó.".into())
                }
            },
            UpdateState::Installing(rx) => match rx.try_recv() {
                Ok(Ok(tag)) => UpdateState::Installed(tag),
                Ok(Err(err)) => UpdateState::Failed(err),
                Err(TryRecvError::Empty) => UpdateState::Installing(rx),
                Err(TryRecvError::Disconnected) => {
                    UpdateState::Failed("La instalación se cortó.".into())
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
    fn request_restart(&mut self) {
        match self.workspace.dirty_tab() {
            Some(id) => self.closing.ask(id, Intent::Restart),
            None => self.closing.finish(Action::Relaunch),
        }
    }

    /// Abre el binario nuevo y deja que eframe cierre y guarde esta instancia.
    fn relaunch(&mut self) {
        let launched = update::exe_path()
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
            Ok(()) => self.closing.finish(Action::CloseWindow),
            Err(err) => {
                self.closing.cancel();
                self.error = Some(format!("No se pudo reiniciar: {err}"));
            }
        }
    }

    fn tabs_bar(&mut self, ui: &mut egui::Ui) {
        if self.workspace.tabs().is_empty() {
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
                    for id in self.workspace.tabs().to_vec() {
                        let (close, hovered) = self.tab_pill(ui, id, prev_hover);
                        close_tab = close_tab.or(close);
                        if hovered {
                            hovered_tab = Some(id);
                        }
                    }
                });
            });
        ui.add_space(2.0);

        self.hovered_tab = hovered_tab;
        if let Some(id) = close_tab {
            self.request_close_tab(id);
        }
    }

    /// Un tab: nombre y `×` dentro del mismo recuadro. Devuelve (cerrar, hovered).
    fn tab_pill(
        &mut self,
        ui: &mut egui::Ui,
        id: NoteId,
        prev_hover: Option<NoteId>,
    ) -> (Option<NoteId>, bool) {
        let Some(note) = self.workspace.note(id) else {
            return (None, false);
        };
        let active = self.workspace.active() == Some(id);
        let title = note.title.clone();
        let dirty = self.workspace.is_dirty(id);
        let show_close = active || prev_hover == Some(id);

        let fill = if active {
            style::active_fill(ui.visuals())
        } else if prev_hover == Some(id) {
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
                        self.workspace.open(id);
                    }
                    if show_close
                        && icon_button(ui, Icon::Close, 16.0, "Cerrar tab", false).clicked()
                    {
                        close = Some(id);
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

        (close, response.container_contains_pointer())
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

                let ids = self.workspace.visible(self.order, &self.search);

                // La regla de la nota activa vive en el margen, no sobre el texto.
                let rail_x = ui.max_rect().left() - 5.0;

                egui::ScrollArea::vertical()
                    .id_salt("lista-notas")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        hovered = None;
                        let mut visibles = 0;

                        for id in ids {
                            visibles += 1;

                            if self.workspace.editing() == Some(id) {
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut self.rename_text)
                                        .id(ui.make_persistent_id(("renombrar", id)))
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

                            let Some(note) = self.workspace.note(id) else {
                                continue;
                            };
                            let dirty = self.workspace.is_dirty(id);
                            let snippet = snippet(&note.body);
                            let is_active = self.workspace.active() == Some(id);
                            let show_close = hovered == Some(id);

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
                                                    pencil = Some(id);
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
                                                        delete = Some(id);
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
                                hovered = Some(id);
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
                                open = Some(id);
                            }
                            row.container_context_menu(|ui| {
                                if ui.button("Abrir").clicked() {
                                    open = Some(id);
                                    ui.close();
                                }
                                if ui.button("Renombrar").clicked() {
                                    pencil = Some(id);
                                    ui.close();
                                }
                                if ui.button("Mover a papelera").clicked() {
                                    delete = Some(id);
                                    ui.close();
                                }
                            });
                        }

                        if visibles == 0 {
                            ui.add_space(8.0);
                            if self.workspace.is_empty() {
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
        if let Some(id) = open {
            self.workspace.open(id);
        }
        if let Some(id) = delete {
            self.trash_note(id);
        }
        if let Some(id) = pencil {
            self.commit_rename();
            self.start_rename(id);
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

            let Some(id) = self.workspace.active() else {
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
                        self.workspace.edit(id, |note| {
                            let body = ui.add(
                                egui::TextEdit::multiline(&mut note.body)
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
                                && let Some(mut state) =
                                    egui::TextEdit::load_state(ui.ctx(), editor_id)
                            {
                                state.cursor.set_char_range(Some(selection));
                                state.store(ui.ctx(), editor_id);
                            }
                            let changed = body.changed();
                            let menu_changed = editor_context_menu(&body, &mut note.body, menu_key);
                            ui.add_space(32.0);
                            changed || menu_changed
                        });
                    });
                });
        });
    }

    fn unsaved_modal(&mut self, ctx: &egui::Context) {
        let Some((id, _)) = self.closing.question() else {
            return;
        };
        let Some(note) = self.workspace.note(id) else {
            self.closing.cancel();
            return;
        };

        let title = note.title.clone();
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
                if accent_button(ui, "Guardar").clicked() {
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
            Some(Choice::Save) => match self.save_note(id) {
                Ok(()) => {
                    self.error = None;
                    self.advance_resolved();
                }
                Err(err) => {
                    // Queda sucia y el modal sigue abierto para reintentar.
                    self.error = Some(format!("No se pudo guardar: {err}"));
                }
            },
            Some(Choice::Discard) => {
                self.workspace.clear_dirty(id);
                self.advance_resolved();
            }
            Some(Choice::Cancel) => self.closing.cancel(),
            None => {}
        }
    }

    /// Modal de actualizaciones: buscar, instalar y reiniciar en un solo lugar.
    /// Mientras hay trabajo de red no se puede cerrar.
    fn update_modal(&mut self, ctx: &egui::Context) {
        if !self.update_modal {
            return;
        }
        let view = self.update.view();
        let busy = matches!(view, UpdateView::Checking | UpdateView::Installing);
        let current = update::CURRENT;
        let mut action = None;
        let mut close = false;

        let response = egui::Modal::new(egui::Id::unique("actualizaciones")).show(ctx, |ui| {
            ui.set_max_width(380.0);
            ui.heading("Actualizaciones");
            match &view {
                UpdateView::Idle => {}
                UpdateView::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Buscando actualizaciones…");
                    });
                }
                UpdateView::UpToDate => {
                    ui.label(format!("Ya tenés la última versión (v{current})."));
                }
                UpdateView::Available(tag) => {
                    ui.label(format!(
                        "Hay una versión nueva: {tag}. Tenés la v{current}."
                    ));
                }
                UpdateView::Installing => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Instalando actualización…");
                    });
                }
                UpdateView::Installed(tag) => {
                    ui.label(format!("Actualizado a {tag}."));
                }
                UpdateView::Failed(err) => {
                    ui.colored_label(ui.visuals().error_fg_color, err);
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let primary = match &view {
                    UpdateView::Available(_) => Some(("Actualizar", UpdateAction::Install)),
                    UpdateView::Installed(_) => Some(("Reiniciar ahora", UpdateAction::Restart)),
                    _ => None,
                };
                if let Some((label, choice)) = primary
                    && accent_button(ui, label).clicked()
                {
                    action = Some(choice);
                }
                if matches!(view, UpdateView::Failed(_)) && ui.button("Reintentar").clicked() {
                    action = Some(UpdateAction::Check);
                }
                if !busy && ui.button("Cerrar").clicked() {
                    close = true;
                }
            });
        });

        // Se consulta siempre para consumir Esc; mientras trabaja, no cierra.
        if response.should_close() && !busy {
            close = true;
        }

        if close {
            self.update_modal = false;
            self.update = UpdateState::Idle;
        }
        match action {
            Some(UpdateAction::Check) => self.start_check(ctx),
            Some(UpdateAction::Install) => self.start_install(ctx),
            Some(UpdateAction::Restart) => {
                self.update_modal = false;
                self.request_restart();
            }
            None => {}
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

        match self.closing.take() {
            Some(Action::CloseWindow) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Some(Action::Relaunch) => self.relaunch(),
            None => {}
        }

        self.handle_shortcuts(&ctx);
        self.title_bar(ui);
        self.notes_panel(ui);
        self.editor_panel(ui);
        self.paint_hairlines(ui);
        self.unsaved_modal(&ctx);
        self.update_modal(&ctx);

        // Asegura el frame siguiente, donde se manda el `Close` real.
        if self.closing.is_pending() {
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
        let (tabs, active) = self.workspace.session();
        let persisted = Persisted {
            tabs,
            active,
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

    /// Id de la única nota del seed: los tests trabajan de a una.
    fn only_id(app: &App) -> NoteId {
        let ids = app.workspace.visible(Order::Alphabetical, "");
        assert_eq!(ids.len(), 1, "el test espera una sola nota");
        ids[0]
    }

    fn only_note(app: &App) -> &store::Note {
        app.workspace.note(only_id(app)).unwrap()
    }

    /// Títulos visibles en orden alfabético.
    fn titles(app: &App) -> Vec<String> {
        app.workspace
            .visible(Order::Alphabetical, "")
            .into_iter()
            .filter_map(|id| app.workspace.note(id))
            .map(|note| note.title.clone())
            .collect()
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

        assert_eq!(
            h.state().workspace.tabs().len(),
            1,
            "la nota debería abrirse en un tab"
        );
        assert_eq!(titles(h.state()), ["nota-a"]);
    }

    #[test]
    fn cierra_un_tab_inactivo_desde_su_x() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\n"), ("nota-b", "dos\n")]);
        let mut h = harness(tmp.path());

        h.get_by_label("nota-a").click();
        h.run();
        h.get_by_label("nota-b").click();
        h.run();
        assert_eq!(h.state().workspace.tabs().len(), 2);

        // El label de la tab es el de más arriba; el de la lista está debajo.
        let tab = h
            .get_all_by_label("nota-a")
            .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .unwrap();
        h.hover_at(tab.rect().center());
        h.run();

        let closes = h.get_all_by_label("Cerrar tab").collect::<Vec<_>>();
        assert_eq!(
            closes.len(),
            2,
            "la X del tab inactivo debería verse al pasar el mouse"
        );
        let close = closes
            .iter()
            .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
            .unwrap()
            .rect()
            .center();
        click(&mut h, close);

        assert_eq!(
            h.state().workspace.tabs().len(),
            1,
            "el tab inactivo debería cerrarse desde su X"
        );
    }

    #[test]
    fn primera_frame_con_nota_restaurada() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\n")]);

        // Sin `run()` previo: reproduce una sesión restaurada, donde el primer
        // frame ya dibuja el editor.
        let mut app = App::load(tmp.path().to_path_buf());
        let id = only_id(&app);
        app.workspace.open(id);
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 700.0))
            .build_ui_state(|ui, app: &mut App| app.root_ui(ui), app);
        h.run();

        assert_eq!(h.state().workspace.tabs().len(), 1);
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
            h.state().workspace.is_dirty(only_id(h.state())),
            "escribir debería marcar la nota sucia"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("nota-a/note.md")).unwrap(),
            "uno\n",
            "no debería guardarse antes de Ctrl+S"
        );

        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        h.run();

        assert!(!h.state().workspace.is_dirty(only_id(h.state())));
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
        assert_eq!(h.state().workspace.editing(), Some(only_id(h.state())));

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
        assert!(h.state().workspace.editing().is_none());
        assert_eq!(titles(h.state()), ["comprar-pan"]);
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
        assert!(!h.state().update_modal, "ni abrir el modal");
    }

    #[test]
    fn el_modal_de_actualizaciones_muestra_el_estado() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "")]);
        let mut h = harness(tmp.path());

        h.state_mut().update_modal = true;
        h.state_mut().update = UpdateState::Available(update::Release {
            tag: "v9.9.9".into(),
            deb_url: "https://example.com/notetake.deb".into(),
        });
        h.run();
        h.get_by_label("Actualizar");

        h.state_mut().update = UpdateState::Installed("v9.9.9".into());
        h.run();
        h.get_by_label("Reiniciar ahora");

        h.state_mut().update = UpdateState::Failed("sin red".into());
        h.run();
        h.get_by_label("Reintentar");

        h.state_mut().update = UpdateState::UpToDate;
        h.run();
        h.key_press(egui::Key::Escape);
        h.run();
        assert!(!h.state().update_modal, "Esc oculta el modal");
        assert!(matches!(h.state().update, UpdateState::Idle));
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
        assert_eq!(titles(h.state()), ["nota-a"]);
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
        assert!(h.state().workspace.is_dirty(only_id(h.state())));

        h.get_by_label("Renombrar").click();
        h.run();
        h.event(egui::Event::Text("nueva".to_owned()));
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();

        assert!(
            h.state().workspace.is_dirty(only_id(h.state())),
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
        assert_eq!(h.state().workspace.editing(), Some(only_id(h.state())));
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
            .workspace
            .visible(Order::Alphabetical, &h.state().search);
        assert_eq!(visibles.len(), 2);

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

        assert!(
            h.state().workspace.tabs().is_empty(),
            "el tab debería cerrarse"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("nota-a/note.md")).unwrap(),
            "uno\n",
            "descartar no debe escribir en disco"
        );
    }

    #[test]
    fn modal_al_cerrar_tab_sucio_y_guardar() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\n")]);
        let mut h = harness(tmp.path());

        h.get_by_label("nota-a").click();
        h.run();
        click(&mut h, Pos2::new(600.0, 300.0));
        h.event(egui::Event::Text("dos".to_owned()));
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::W);
        h.run();

        h.get_by_label("Guardar").click();
        h.run();

        assert!(
            h.state().workspace.tabs().is_empty(),
            "guardar debería cerrar el tab"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("nota-a/note.md")).unwrap(),
            "uno\ndos"
        );
    }

    #[test]
    fn modal_cancelar_deja_el_tab_abierto_y_sucio() {
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

        h.get_by_label("Cancelar").click();
        h.run();

        assert_eq!(h.state().workspace.tabs().len(), 1);
        assert!(h.state().workspace.is_dirty(only_id(h.state())));
    }

    #[test]
    fn modal_de_ventana_recorre_las_notas_sucias() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("nota-a", "uno\n"), ("nota-b", "dos\n")]);
        let mut h = harness(tmp.path());

        for nota in ["nota-a", "nota-b"] {
            h.get_by_label(nota).click();
            h.run();
            click(&mut h, Pos2::new(600.0, 300.0));
            h.event(egui::Event::Text("cambio".to_owned()));
            h.run();
        }

        click(&mut h, Pos2::new(983.0, 19.0));
        assert!(
            h.query_by_label("«nota-a» tiene cambios sin guardar.")
                .is_some(),
            "el modal debería empezar por el primer tab sucio"
        );

        h.get_by_label("Descartar").click();
        h.run();
        h.run();
        assert!(
            h.query_by_label("«nota-b» tiene cambios sin guardar.")
                .is_some(),
            "el modal debería seguir con la otra nota sucia"
        );

        h.get_by_label("Descartar").click();
        h.run();
        h.run();
        assert!(h.state().workspace.dirty_tab().is_none());
        assert!(h.query_by_label("Cambios sin guardar").is_none());
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
            only_note(h.state()).body.is_empty(),
            "cortar debería vaciar la selección completa"
        );
        assert!(
            h.state().workspace.is_dirty(only_id(h.state())),
            "cortar debería marcar la nota sucia"
        );
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

        assert_eq!(
            h.state().workspace.tabs().len(),
            1,
            "el menú debería abrir la nota"
        );
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
