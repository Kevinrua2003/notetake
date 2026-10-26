//! Espacio de trabajo: notas, tabs, foco, cambios sin guardar y renombre.
//! Los llamadores usan [`NoteId`]; los índices no salen de este módulo.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use crate::store::{self, Note};

/// Identificador estable de una nota durante la sesión: sobrevive al renombre,
/// al borrado de otras notas y a la reconciliación con el disco.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NoteId(u64);

/// Orden de la lista de notas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Order {
    #[default]
    Alphabetical,
    Recent,
}

impl Order {
    pub const ALL: [Self; 2] = [Self::Alphabetical, Self::Recent];

    pub fn label(self) -> &'static str {
        match self {
            Self::Alphabetical => "Alfabético",
            Self::Recent => "Reciente",
        }
    }
}

impl NoteId {
    /// Id de prueba: el campo es privado para que no se inventen ids en producción.
    #[cfg(test)]
    pub(crate) fn for_test(n: u64) -> NoteId {
        NoteId(n)
    }
}

struct Entry {
    id: NoteId,
    note: Note,
}

pub struct Workspace {
    data_dir: PathBuf,
    /// Cuadernos en disco, ordenados por título plegado.
    notebooks: Vec<String>,
    /// Orden de carga, nunca se reordena: el id no depende de la posición.
    entries: Vec<Entry>,
    tabs: Vec<NoteId>,
    /// Índice dentro de `tabs`, no dentro de `entries`.
    active: usize,
    /// Notas con cambios sin guardar, por id: el renombre no las toca.
    dirty: HashSet<NoteId>,
    /// Nota cuya fila está en edición inline.
    editing: Option<NoteId>,
    next_id: u64,
}

impl Workspace {
    pub fn load(data_dir: PathBuf) -> Self {
        let mut workspace = Self {
            notebooks: store::discover_notebooks(&data_dir),
            data_dir,
            entries: Vec::new(),
            tabs: Vec::new(),
            active: 0,
            dirty: HashSet::new(),
            editing: None,
            next_id: 0,
        };
        for note in store::discover(&workspace.data_dir) {
            let id = workspace.new_id();
            workspace.entries.push(Entry { id, note });
        }
        workspace
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn notebooks(&self) -> &[String] {
        &self.notebooks
    }

    pub fn tabs(&self) -> &[NoteId] {
        &self.tabs
    }

    /// Identidad de las tabs y de la activa, para persistir la sesión.
    pub fn session(&self) -> (Vec<String>, Option<String>) {
        let tabs = self
            .tabs
            .iter()
            .filter_map(|&id| self.note(id))
            .map(|note| format!("{}/{}", note.notebook, note.title))
            .collect();
        let active = self
            .active()
            .and_then(|id| self.note(id))
            .map(|note| format!("{}/{}", note.notebook, note.title));
        (tabs, active)
    }

    pub fn active(&self) -> Option<NoteId> {
        self.tabs.get(self.active).copied()
    }

    pub fn note(&self, id: NoteId) -> Option<&Note> {
        self.entry(id).map(|entry| &entry.note)
    }

    /// Muta la nota y la marca sucia si `f` devuelve `true`.
    pub fn edit(&mut self, id: NoteId, f: impl FnOnce(&mut Note) -> bool) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        let changed = f(&mut entry.note);
        if changed {
            self.dirty.insert(id);
        }
        changed
    }

    pub fn is_dirty(&self, id: NoteId) -> bool {
        self.dirty.contains(&id)
    }

    pub fn clear_dirty(&mut self, id: NoteId) {
        self.dirty.remove(&id);
    }

    /// Primer tab abierto con cambios sin guardar, en orden de tabs.
    pub fn dirty_tab(&self) -> Option<NoteId> {
        self.tabs
            .iter()
            .copied()
            .find(|&id| self.dirty.contains(&id))
    }

    pub fn editing(&self) -> Option<NoteId> {
        self.editing
    }

    /// Abre la nota en un tab, o la enfoca si ya estaba abierta.
    pub fn open(&mut self, id: NoteId) {
        match self.tabs.iter().position(|&tab| tab == id) {
            Some(pos) => self.active = pos,
            None => {
                self.tabs.push(id);
                self.active = self.tabs.len() - 1;
            }
        }
    }

    pub fn close(&mut self, id: NoteId) {
        if let Some(pos) = self.tabs.iter().position(|&tab| tab == id) {
            self.tabs.remove(pos);
            if pos < self.active {
                self.active -= 1;
            }
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
    }

    /// Ids de un cuaderno en el orden pedido, filtrados por título y cuerpo.
    pub fn visible(&self, order: Order, query: &str, notebook: &str) -> Vec<NoteId> {
        let query = store::fold(query);
        let mut ids: Vec<NoteId> = self
            .entries
            .iter()
            .filter(|entry| entry.note.notebook == notebook)
            .filter(|entry| {
                query.is_empty()
                    || store::fold(&entry.note.title).contains(&query)
                    || store::fold(&entry.note.body).contains(&query)
            })
            .map(|entry| entry.id)
            .collect();
        match order {
            Order::Alphabetical => ids.sort_by_cached_key(|&id| self.title_fold(id)),
            Order::Recent => ids.sort_by(|&a, &b| {
                self.mtime(b)
                    .cmp(&self.mtime(a))
                    .then_with(|| self.title_fold(a).cmp(&self.title_fold(b)))
            }),
        }
        ids
    }

    /// Cantidad de notas de un cuaderno.
    pub fn notebook_len(&self, notebook: &str) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.note.notebook == notebook)
            .count()
    }

    pub fn start_rename(&mut self, id: NoteId) {
        if self.entry(id).is_some() {
            self.editing = Some(id);
        }
    }

    pub fn cancel_rename(&mut self) {
        self.editing = None;
    }

    /// Aplica el renombre de la nota en edición. Vacío o sin cambios no hace nada.
    pub fn commit_rename(&mut self, title: &str) -> io::Result<()> {
        let Some(id) = self.editing.take() else {
            return Ok(());
        };
        let Some(entry) = self.entry(id) else {
            return Ok(());
        };
        let title = title.trim();
        if title.is_empty() || store::slugify(title) == entry.note.title {
            return Ok(());
        }
        let dir = entry.note.dir.clone();
        // Renombrar no guarda: el cuerpo en memoria puede tener cambios sueltos.
        let body = entry.note.body.clone();
        let mut note = store::rename(&dir, title)?;
        note.body = body;
        if let Some(entry) = self.entry_mut(id) {
            entry.note = note;
        }
        Ok(())
    }

    /// Crea `sin-titulo` (o `sin-titulo-N`) en el cuaderno, sin abrirla.
    pub fn create(&mut self, notebook: &str) -> io::Result<NoteId> {
        let note = store::create(&self.data_dir, notebook)?;
        let id = self.new_id();
        self.entries.push(Entry { id, note });
        Ok(id)
    }

    pub fn create_notebook(&mut self, title: &str) -> io::Result<String> {
        let notebook = store::create_notebook(&self.data_dir, title)?;
        self.notebooks.push(notebook.clone());
        self.notebooks.sort_by_key(|name| store::fold(name));
        Ok(notebook)
    }

    /// Renombra el cuaderno y actualiza en memoria las notas que contiene.
    pub fn rename_notebook(&mut self, current: &str, title: &str) -> io::Result<String> {
        let renamed = store::rename_notebook(&self.data_dir, current, title)?;
        for entry in &mut self.entries {
            if entry.note.notebook == current {
                entry.note.notebook = renamed.clone();
                entry.note.dir = self.data_dir.join(&renamed).join(&entry.note.title);
            }
        }
        if let Some(name) = self.notebooks.iter_mut().find(|name| *name == current) {
            *name = renamed.clone();
        }
        self.notebooks.sort_by_key(|name| store::fold(name));
        Ok(renamed)
    }

    pub fn trash_notebook(&mut self, notebook: &str) -> io::Result<()> {
        store::trash_notebook(&self.data_dir, notebook)?;
        let ids: Vec<NoteId> = self
            .entries
            .iter()
            .filter(|entry| entry.note.notebook == notebook)
            .map(|entry| entry.id)
            .collect();
        for id in ids {
            self.remove(id);
        }
        self.notebooks.retain(|name| name != notebook);
        Ok(())
    }

    /// Mueve una nota a otro cuaderno sin perder el cuerpo sin guardar ni el id.
    pub fn move_note(&mut self, id: NoteId, notebook: &str) -> io::Result<()> {
        let Some(entry) = self.entry(id) else {
            return Ok(());
        };
        if entry.note.notebook == notebook {
            return Ok(());
        }
        let body = entry.note.body.clone();
        let dir = entry.note.dir.clone();
        let mut note = store::move_note(&self.data_dir, &dir, notebook)?;
        note.body = body;
        if let Some(entry) = self.entry_mut(id) {
            entry.note = note;
        }
        Ok(())
    }

    pub fn save(&mut self, id: NoteId) -> io::Result<()> {
        let Some(entry) = self.entry(id) else {
            return Ok(());
        };
        let note = store::save(&entry.note.dir, &entry.note.title, &entry.note.body)?;
        if let Some(entry) = self.entry_mut(id) {
            entry.note = note;
        }
        self.dirty.remove(&id);
        Ok(())
    }

    pub fn trash(&mut self, id: NoteId) -> io::Result<()> {
        let Some(entry) = self.entry(id) else {
            return Ok(());
        };
        store::trash(&self.data_dir, &entry.note.dir)?;
        self.remove(id);
        Ok(())
    }

    /// Relee el disco: actualiza lo cambiado, suma lo nuevo y saca lo borrado.
    /// Las notas sucias tienen prioridad: no se tocan.
    pub fn reconcile(&mut self) {
        let disk = store::discover(&self.data_dir);
        self.notebooks = store::discover_notebooks(&self.data_dir);
        let mut gone = Vec::new();
        for entry in &mut self.entries {
            if self.dirty.contains(&entry.id) {
                continue;
            }
            match disk.iter().find(|found| found.dir == entry.note.dir) {
                Some(found) => {
                    if found.mtime != entry.note.mtime {
                        entry.note = found.clone();
                    }
                }
                None => gone.push(entry.id),
            }
        }
        for id in gone {
            self.remove(id);
        }
        for found in disk {
            if !self.entries.iter().any(|entry| entry.note.dir == found.dir) {
                let id = self.new_id();
                self.entries.push(Entry { id, note: found });
            }
        }
    }

    /// Reabre las tabs de la sesión anterior, por `cuaderno/título`.
    pub fn restore(&mut self, tabs: Vec<String>, active: Option<String>) {
        for title in tabs {
            if let Some(id) = self.id_of(&title)
                && !self.tabs.contains(&id)
            {
                self.tabs.push(id);
            }
        }
        self.active = active
            .as_deref()
            .and_then(|title| self.id_of(title))
            .and_then(|id| self.tabs.iter().position(|&tab| tab == id))
            .unwrap_or(0);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
    }

    fn remove(&mut self, id: NoteId) {
        self.close(id);
        self.dirty.remove(&id);
        if self.editing == Some(id) {
            self.editing = None;
        }
        self.entries.retain(|entry| entry.id != id);
    }

    fn id_of(&self, reference: &str) -> Option<NoteId> {
        let (notebook, title) = reference.split_once('/')?;
        self.entries
            .iter()
            .find(|entry| entry.note.notebook == notebook && entry.note.title == title)
            .map(|entry| entry.id)
    }

    fn entry(&self, id: NoteId) -> Option<&Entry> {
        // shortcut: búsqueda lineal por id; con miles de notas conviene un índice.
        self.entries.iter().find(|entry| entry.id == id)
    }

    fn entry_mut(&mut self, id: NoteId) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|entry| entry.id == id)
    }

    fn title_fold(&self, id: NoteId) -> String {
        self.entry(id)
            .map(|entry| store::fold(&entry.note.title))
            .unwrap_or_default()
    }

    fn mtime(&self, id: NoteId) -> std::time::SystemTime {
        self.entry(id)
            .map(|entry| entry.note.mtime)
            .unwrap_or(std::time::UNIX_EPOCH)
    }

    fn new_id(&mut self) -> NoteId {
        self.next_id += 1;
        NoteId(self.next_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn workspace(dir: &Path) -> Workspace {
        Workspace::load(dir.to_path_buf())
    }

    fn seed(dir: &Path, notes: &[(&str, &str)]) {
        let notebook = dir.join("general");
        std::fs::create_dir_all(&notebook).unwrap();
        std::fs::write(notebook.join(".notebook"), "").unwrap();
        for (name, body) in notes {
            let path = notebook.join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("note.md"), body).unwrap();
        }
    }

    /// Marca la nota sucia sin tocar el cuerpo: algunos tests solo necesitan el estado.
    fn marcar(ws: &mut Workspace, id: NoteId) {
        ws.edit(id, |_| true);
    }

    fn titles(ws: &Workspace, order: Order, query: &str) -> Vec<String> {
        ws.visible(order, query, "general")
            .into_iter()
            .filter_map(|id| ws.note(id))
            .map(|note| note.title.clone())
            .collect()
    }

    #[test]
    fn renombrar_conserva_el_id_y_el_sucio() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());

        let id = ws.create("general").unwrap();
        ws.open(id);
        ws.edit(id, |note| {
            note.body = "cuerpo".into();
            true
        });
        ws.start_rename(id);
        ws.commit_rename("comprar pan").unwrap();

        assert_eq!(ws.active(), Some(id));
        assert!(ws.is_dirty(id), "el renombre no debe limpiar el sucio");
        assert!(ws.editing().is_none());
        assert_eq!(ws.note(id).unwrap().title, "comprar-pan");
        assert_eq!(ws.note(id).unwrap().body, "cuerpo");
        assert!(tmp.path().join("general/comprar-pan/note.md").is_file());
    }

    #[test]
    fn borrar_otra_nota_no_mueve_el_renombre() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());

        let primera = ws.create("general").unwrap();
        let segunda = ws.create("general").unwrap();
        ws.start_rename(segunda);
        ws.trash(primera).unwrap();

        assert_eq!(ws.editing(), Some(segunda));
        ws.commit_rename("segunda").unwrap();
        assert_eq!(ws.note(segunda).unwrap().title, "segunda");
        assert!(ws.note(primera).is_none());
    }

    #[test]
    fn borrar_la_nota_en_edicion_cancela_el_renombre() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());

        let id = ws.create("general").unwrap();
        ws.start_rename(id);
        ws.trash(id).unwrap();

        assert!(ws.editing().is_none());
        assert!(ws.note(id).is_none());
    }

    #[test]
    fn cerrar_tab_ajusta_el_activo() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());
        let a = ws.create("general").unwrap();
        let b = ws.create("general").unwrap();
        let c = ws.create("general").unwrap();
        for id in [a, b, c] {
            ws.open(id);
        }
        assert_eq!(ws.active(), Some(c));

        ws.close(a);
        assert_eq!(ws.active(), Some(c), "cerrar otro tab no mueve el foco");
        ws.close(c);
        assert_eq!(ws.active(), Some(b));
        ws.close(b);
        assert_eq!(ws.active(), None);
    }

    #[test]
    fn trash_quita_tab_sucio_y_entrada() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());
        let id = ws.create("general").unwrap();
        ws.open(id);
        marcar(&mut ws, id);

        ws.trash(id).unwrap();

        assert!(ws.tabs().is_empty());
        assert!(!ws.is_dirty(id));
        assert!(ws.note(id).is_none());
        assert!(tmp.path().join("_trash").is_dir());
    }

    #[test]
    fn save_actualiza_titulo_mtime_y_limpia_el_sucio() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());
        let id = ws.create("general").unwrap();
        ws.edit(id, |note| {
            note.body = "hola".into();
            true
        });

        ws.save(id).unwrap();

        let note = ws.note(id).unwrap();
        assert!(!ws.is_dirty(id));
        assert_eq!(note.body, "hola");
        assert_ne!(note.mtime, UNIX_EPOCH);
        assert!(note.dir.join("note.md").is_file());
    }

    #[test]
    fn reconcile_conserva_ids_y_sucio_y_suma_lo_nuevo() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("uno", "original"), ("dos", "original")]);
        let mut ws = workspace(tmp.path());
        let ids = ws.visible(Order::Alphabetical, "", "general");
        assert_eq!(ids.len(), 2);
        marcar(&mut ws, ids[0]);

        std::fs::write(tmp.path().join("general/uno/note.md"), "cambiado").unwrap();
        std::fs::write(tmp.path().join("general/dos/note.md"), "cambiado").unwrap();
        seed(tmp.path(), &[("tres", "nueva")]);
        ws.reconcile();

        assert_eq!(ws.visible(Order::Alphabetical, "", "general").len(), 3);
        assert_eq!(
            ws.note(ids[0]).unwrap().body,
            "original",
            "la nota sucia no se pisa"
        );
        assert!(ws.is_dirty(ids[0]));
        assert_eq!(
            ws.note(ids[1]).unwrap().body,
            "cambiado",
            "la nota limpia se relee"
        );
    }

    #[test]
    fn reconcile_saca_tabs_y_edicion_de_lo_borrado() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());
        let a = ws.create("general").unwrap();
        let b = ws.create("general").unwrap();
        ws.open(a);
        ws.open(b);
        ws.start_rename(b);
        let dir = ws.note(b).unwrap().dir.clone();

        std::fs::remove_dir_all(dir).unwrap();
        ws.reconcile();

        assert!(ws.note(b).is_none());
        assert_eq!(ws.tabs(), &[a]);
        assert!(ws.editing().is_none());
    }

    #[test]
    fn visible_ordena_y_filtra() {
        let tmp = tempfile::tempdir().unwrap();
        seed(
            tmp.path(),
            &[("zeta", "z"), ("Alfa", "a"), ("beta", "canción de otoño")],
        );
        let ws = workspace(tmp.path());

        assert_eq!(
            titles(&ws, Order::Alphabetical, ""),
            ["Alfa", "beta", "zeta"]
        );
        assert_eq!(titles(&ws, Order::Alphabetical, "CANCION"), ["beta"]);
        assert_eq!(titles(&ws, Order::Alphabetical, "alfa"), ["Alfa"]);
        assert!(titles(&ws, Order::Alphabetical, "zzz").is_empty());
    }

    #[test]
    fn visible_reciente_usa_el_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("vieja", "")]);
        std::thread::sleep(Duration::from_millis(20));
        seed(tmp.path(), &[("nueva", "")]);
        let ws = workspace(tmp.path());

        assert_eq!(titles(&ws, Order::Recent, ""), ["nueva", "vieja"]);
    }

    #[test]
    fn dirty_tab_respeta_el_orden_de_los_tabs() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());
        let a = ws.create("general").unwrap();
        let b = ws.create("general").unwrap();
        ws.open(a);
        ws.open(b);
        marcar(&mut ws, b);
        marcar(&mut ws, a);

        assert_eq!(ws.dirty_tab(), Some(a));
        ws.clear_dirty(a);
        assert_eq!(ws.dirty_tab(), Some(b));
    }

    #[test]
    fn restore_abre_tabs_por_cuaderno_y_titulo_e_ignora_desconocidas() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("uno", ""), ("dos", "")]);
        let mut ws = workspace(tmp.path());

        ws.restore(
            vec![
                "general/dos".into(),
                "general/fantasma".into(),
                "general/uno".into(),
            ],
            Some("general/uno".into()),
        );

        assert_eq!(ws.tabs().len(), 2, "la desconocida se ignora");
        assert_eq!(ws.note(ws.tabs()[0]).unwrap().title, "dos");
        assert_eq!(
            ws.active(),
            Some(ws.tabs()[1]),
            "el activo va por cuaderno/título"
        );
        assert_eq!(ws.note(ws.tabs()[1]).unwrap().title, "uno");
    }

    #[test]
    fn restore_ignora_referencias_sin_cuaderno() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("uno", "")]);
        let mut ws = workspace(tmp.path());

        ws.restore(vec!["uno".into()], Some("uno".into()));

        assert!(
            ws.tabs().is_empty(),
            "las tabs viejas sin cuaderno no se restauran"
        );
        assert_eq!(ws.active(), None);
    }

    #[test]
    fn visible_filtra_por_cuaderno() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());
        ws.create_notebook("general").unwrap();
        ws.create_notebook("trabajo").unwrap();
        let general = ws.create("general").unwrap();
        let trabajo = ws.create("trabajo").unwrap();

        assert_eq!(
            ws.visible(Order::Alphabetical, "", "general"),
            vec![general]
        );
        assert_eq!(
            ws.visible(Order::Alphabetical, "", "trabajo"),
            vec![trabajo]
        );
        assert_eq!(ws.notebook_len("general"), 1);
        assert_eq!(ws.notebook_len("trabajo"), 1);
    }

    #[test]
    fn mover_nota_conserva_id_y_cuerpo_sucio() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());
        ws.create_notebook("general").unwrap();
        ws.create_notebook("trabajo").unwrap();
        let id = ws.create("general").unwrap();
        ws.edit(id, |note| {
            note.body = "sin guardar".into();
            true
        });

        ws.move_note(id, "trabajo").unwrap();

        let note = ws.note(id).unwrap();
        assert_eq!(note.notebook, "trabajo");
        assert_eq!(note.body, "sin guardar");
        assert_eq!(note.dir.parent().unwrap(), tmp.path().join("trabajo"));
        assert!(ws.is_dirty(id));
        assert!(ws.visible(Order::Alphabetical, "", "general").is_empty());
    }

    #[test]
    fn renombrar_cuaderno_conserva_ids_y_tabs() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());
        ws.create_notebook("general").unwrap();
        let id = ws.create("general").unwrap();
        ws.open(id);

        let renamed = ws.rename_notebook("general", "Personal").unwrap();

        assert_eq!(renamed, "personal");
        assert_eq!(ws.notebooks(), ["personal"]);
        assert_eq!(ws.tabs(), &[id]);
        let note = ws.note(id).unwrap();
        assert_eq!(note.notebook, "personal");
        assert_eq!(note.dir, tmp.path().join("personal/sin-titulo"));
        assert!(note.dir.join("note.md").is_file());
    }

    #[test]
    fn trash_cuaderno_quita_sus_tabs_y_edicion() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());
        ws.create_notebook("general").unwrap();
        let a = ws.create("general").unwrap();
        let b = ws.create("general").unwrap();
        ws.open(a);
        ws.open(b);
        ws.start_rename(b);

        ws.trash_notebook("general").unwrap();

        assert!(ws.notebooks().is_empty());
        assert!(ws.tabs().is_empty());
        assert!(ws.editing().is_none());
        assert!(ws.note(a).is_none());
        assert!(ws.note(b).is_none());
        assert!(tmp.path().join("_trash/general").is_dir());
    }

    #[test]
    fn session_devuelve_las_tabs_con_cuaderno() {
        let tmp = tempfile::tempdir().unwrap();
        seed(tmp.path(), &[("uno", ""), ("dos", "")]);
        let mut ws = workspace(tmp.path());

        let ids = ws.visible(Order::Alphabetical, "", "general");
        let uno = ids
            .iter()
            .copied()
            .find(|&id| ws.note(id).unwrap().title == "uno")
            .unwrap();
        let dos = ids
            .iter()
            .copied()
            .find(|&id| ws.note(id).unwrap().title == "dos")
            .unwrap();
        ws.open(uno);
        ws.open(dos);

        assert_eq!(
            ws.session(),
            (
                vec!["general/uno".to_owned(), "general/dos".to_owned()],
                Some("general/dos".to_owned())
            )
        );
    }

    #[test]
    fn edit_marca_sucio_solo_si_cambio() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());
        let id = ws.create("general").unwrap();

        assert!(!ws.edit(id, |note| {
            note.body = "igual".into();
            false
        }));
        assert!(!ws.is_dirty(id));

        assert!(ws.edit(id, |note| {
            note.body.push('!');
            true
        }));
        assert!(ws.is_dirty(id));
        assert_eq!(ws.note(id).unwrap().body, "igual!");
    }

    #[test]
    fn edit_de_un_id_ajeno_no_hace_nada() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = workspace(tmp.path());
        assert!(!ws.edit(NoteId::for_test(99), |_| true));
    }
}
