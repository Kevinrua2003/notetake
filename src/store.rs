//! Almacenamiento en disco: una nota es una carpeta con un `note.md` adentro.
//! Sin dependencias: solo `std`.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const NOTE_FILE: &str = "note.md";
const NOTE_TMP: &str = "note.md.tmp";
const NOTEBOOK_MARKER: &str = ".notebook";
const TRASH_DIR: &str = "_trash";
const FALLBACK_TITLE: &str = "sin-titulo";
const TITLE_MAX_BYTES: usize = 200;

#[derive(Clone)]
pub struct Note {
    /// Cuaderno que contiene la nota.
    pub notebook: String,
    /// Carpeta de la nota.
    pub dir: PathBuf,
    /// = nombre de carpeta.
    pub title: String,
    /// Contenido de `note.md`.
    pub body: String,
    /// mtime de `note.md`.
    pub mtime: SystemTime,
}

/// `$NOTETAKE_DIR` > `$XDG_DATA_HOME/notetake` > `$HOME/.local/share/notetake`.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = env::var_os("NOTETAKE_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(xdg) = env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(xdg).join("notetake");
    }
    if let Some(home) = env::var_os("HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(home).join(".local/share/notetake");
    }
    // Sin HOME no hay ubicación estándar: último recurso, un dir temporal.
    env::temp_dir().join("notetake")
}

/// Lee las carpetas que son cuadernos, ordenadas por título con `fold`.
/// Un cuaderno es un directorio directo con `.notebook` o con al menos una nota.
/// Saltea nombres reservados (`_`, `.`), archivos sueltos y dirs vacíos sin marcador.
pub fn discover_notebooks(data_dir: &Path) -> Vec<String> {
    let mut notebooks = Vec::new();
    let Ok(entries) = fs::read_dir(data_dir) else {
        return notebooks;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.starts_with('_') || name.starts_with('.') {
            continue;
        }
        let dir = entry.path();
        if dir.is_dir() && is_notebook(&dir) {
            notebooks.push(name);
        }
    }
    notebooks.sort_by_key(|name| fold(name));
    notebooks
}

/// Lee todas las notas de todos los cuadernos, ordenadas por título con `fold`.
/// Saltea nombres reservados (`_`, `.`), archivos sueltos, dirs sin `note.md` y notas ilegibles.
pub fn discover(data_dir: &Path) -> Vec<Note> {
    let mut notes = Vec::new();
    for notebook in discover_notebooks(data_dir) {
        let Ok(entries) = fs::read_dir(data_dir.join(&notebook)) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(title) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if title.starts_with('_') || title.starts_with('.') {
                continue;
            }
            let dir = entry.path();
            if is_note_dir(&dir)
                && let Ok(note) = read(&dir)
            {
                notes.push(note);
            }
        }
    }
    notes.sort_by_key(|note| fold(&note.title));
    notes
}

/// Lee una nota desde su carpeta.
pub fn read(dir: &Path) -> io::Result<Note> {
    let file = dir.join(NOTE_FILE);
    let body = fs::read(&file)?;
    Ok(Note {
        notebook: notebook_of(dir),
        dir: dir.to_path_buf(),
        title: title_of(dir),
        body: String::from_utf8_lossy(&body).into_owned(),
        mtime: mtime_of(&file),
    })
}

/// Crea `sin-titulo` (o `sin-titulo-N`) dentro del cuaderno, con `note.md` vacío.
pub fn create(data_dir: &Path, notebook: &str) -> io::Result<Note> {
    let parent = data_dir.join(notebook);
    fs::create_dir_all(&parent)?;
    let mut n = 1;
    let dir = loop {
        let name = if n == 1 {
            FALLBACK_TITLE.to_string()
        } else {
            format!("{FALLBACK_TITLE}-{n}")
        };
        let path = parent.join(name);
        match fs::create_dir(&path) {
            Ok(()) => break path,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(err) => return Err(err),
        }
    };
    let file = dir.join(NOTE_FILE);
    fs::write(&file, "")?;
    Ok(Note {
        notebook: notebook.to_owned(),
        title: title_of(&dir),
        dir,
        body: String::new(),
        mtime: mtime_of(&file),
    })
}

/// Crea un cuaderno vacío con su marcador. Devuelve el nombre final (puede sumar sufijo).
pub fn create_notebook(data_dir: &Path, title: &str) -> io::Result<String> {
    fs::create_dir_all(data_dir)?;
    let name = unique_name(data_dir, &sanitize_title(title), None);
    let dir = data_dir.join(&name);
    fs::create_dir(&dir)?;
    fs::write(dir.join(NOTEBOOK_MARKER), "")?;
    Ok(name)
}

/// Renombra la carpeta del cuaderno. Devuelve el nombre final.
pub fn rename_notebook(data_dir: &Path, current: &str, title: &str) -> io::Result<String> {
    let name = sanitize_title(title);
    if current == name {
        return Ok(name);
    }
    let current_dir = data_dir.join(current);
    let target = data_dir.join(unique_name(data_dir, &name, Some(&current_dir)));
    fs::rename(current_dir, &target)?;
    Ok(title_of(&target))
}

/// Escribe el cuerpo de forma atómica y, si el título cambia el nombre de la
/// carpeta, la renombra. Devuelve la nota ya releída, con título y mtime finales.
pub fn save(current_dir: &Path, title: &str, body: &str) -> io::Result<Note> {
    write_atomic(current_dir, body)?;

    let title = sanitize_title(title);
    if current_dir
        .file_name()
        .is_some_and(|name| name == title.as_str())
    {
        return read(current_dir);
    }
    let parent = parent_of(current_dir)?;
    let target = parent.join(unique_name(parent, &title, Some(current_dir)));
    fs::rename(current_dir, &target)?;
    read(&target)
}

/// Renombra la carpeta según el título, sin tocar `note.md`.
pub fn rename(current_dir: &Path, title: &str) -> io::Result<Note> {
    let title = sanitize_title(title);
    if current_dir
        .file_name()
        .is_some_and(|name| name == title.as_str())
    {
        return read(current_dir);
    }
    let parent = parent_of(current_dir)?;
    let target = parent.join(unique_name(parent, &title, Some(current_dir)));
    fs::rename(current_dir, &target)?;
    read(&target)
}

/// Mueve una nota a otro cuaderno, resolviendo colisiones de título.
pub fn move_note(data_dir: &Path, note_dir: &Path, notebook: &str) -> io::Result<Note> {
    if note_dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == notebook)
    {
        return read(note_dir);
    }
    let parent = data_dir.join(notebook);
    if !parent.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "el cuaderno destino no existe",
        ));
    }
    let Some(name) = note_dir.file_name() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "la ruta de la nota no tiene nombre de carpeta",
        ));
    };
    let name = name.to_string_lossy();
    let target = parent.join(unique_name(&parent, &name, None));
    fs::rename(note_dir, &target)?;
    read(&target)
}

/// Carpeta donde van las notas borradas (puede no existir todavía).
pub fn trash_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(TRASH_DIR)
}

/// Mueve la carpeta a `<data>/_trash/<nombre>`, con sufijo si ya existe.
pub fn trash(data_dir: &Path, note_dir: &Path) -> io::Result<()> {
    let Some(name) = note_dir.file_name() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "la ruta de la nota no tiene nombre de carpeta",
        ));
    };
    let target_dir = trash_dir(data_dir);
    fs::create_dir_all(&target_dir)?;
    let name = name.to_string_lossy();
    let target = target_dir.join(unique_name(&target_dir, &name, None));
    fs::rename(note_dir, target)
}

/// Mueve un cuaderno completo (con sus notas) a `_trash/`.
pub fn trash_notebook(data_dir: &Path, notebook: &str) -> io::Result<()> {
    trash(data_dir, &data_dir.join(notebook))
}

/// Minúsculas y sin acentos, para buscar.
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' => 'u',
            'ü' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

/// Título exacto como nombre de carpeta: conserva lo escrito salvo lo que el
/// sistema de archivos no admite. Descarta prefijos reservados (`_`, `.`) y
/// recorta a 200 bytes para dejar lugar al sufijo de colisión.
pub fn sanitize_title(title: &str) -> String {
    let mut out = String::new();
    for c in title.trim().trim_start_matches(['.', '_']).chars() {
        let c = if matches!(c, '/' | '\\' | '\0') {
            '-'
        } else {
            c
        };
        if out.len() + c.len_utf8() > TITLE_MAX_BYTES {
            break;
        }
        out.push(c);
    }
    let out = out.trim_end_matches([' ', '.']);
    if out.is_empty() {
        FALLBACK_TITLE.to_string()
    } else {
        out.to_string()
    }
}

fn title_of(dir: &Path) -> String {
    dir.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn notebook_of(dir: &Path) -> String {
    dir.parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn parent_of(dir: &Path) -> io::Result<&Path> {
    dir.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "la ruta de la nota no tiene carpeta padre",
        )
    })
}

fn is_note_dir(dir: &Path) -> bool {
    dir.is_dir() && dir.join(NOTE_FILE).is_file()
}

fn is_notebook(dir: &Path) -> bool {
    if dir.join(NOTEBOOK_MARKER).is_file() {
        return true;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return false;
        };
        !name.starts_with('_') && !name.starts_with('.') && is_note_dir(&entry.path())
    })
}

fn mtime_of(file: &Path) -> SystemTime {
    fs::metadata(file)
        .and_then(|meta| meta.modified())
        .unwrap_or(UNIX_EPOCH)
}

fn write_atomic(dir: &Path, body: &str) -> io::Result<()> {
    let tmp = dir.join(NOTE_TMP);
    fs::write(&tmp, body)?;
    if let Err(err) = fs::rename(&tmp, dir.join(NOTE_FILE)) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

fn is_taken(parent: &Path, name: &str, skip: Option<&Path>) -> bool {
    let path = parent.join(name);
    if let Some(skip) = skip
        && (path == skip || skip.file_name().is_some_and(|own| own == name))
    {
        return false;
    }
    path.exists()
}

fn unique_name(parent: &Path, base: &str, skip: Option<&Path>) -> String {
    if !is_taken(parent, base, skip) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let name = format!("{base}-{n}");
        if !is_taken(parent, &name, skip) {
            return name;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_of(dir: &Path) -> String {
        fs::read_to_string(dir.join(NOTE_FILE)).unwrap()
    }

    fn seed_note(data: &Path, notebook: &str, title: &str, body: &str) {
        let dir = data.join(notebook).join(title);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(NOTE_FILE), body).unwrap();
    }

    #[test]
    fn sanitize_title_conserva_lo_escrito() {
        assert_eq!(sanitize_title("nota nueva #1"), "nota nueva #1");
        assert_eq!(sanitize_title("Comprar Café"), "Comprar Café");
        assert_eq!(sanitize_title("¡Ojo! ¿Qué?"), "¡Ojo! ¿Qué?");
        assert_eq!(sanitize_title("!nota"), "!nota");
        assert_eq!(sanitize_title("a---b"), "a---b");
        assert_eq!(sanitize_title("nota_2026.md"), "nota_2026.md");
        assert_eq!(sanitize_title("  hola  "), "hola");
        assert_eq!(sanitize_title("a/b\\c"), "a-b-c");
        assert_eq!(sanitize_title("_oculta"), "oculta");
        assert_eq!(sanitize_title("nota."), "nota");
        assert_eq!(sanitize_title(""), "sin-titulo");
        assert_eq!(sanitize_title("..."), "sin-titulo");
        assert_eq!(sanitize_title("emoji 🚀"), "emoji 🚀");
    }

    #[test]
    fn sanitize_title_recorta_a_200_bytes_sin_partir_un_char() {
        let title = sanitize_title(&"á".repeat(150));
        assert_eq!(title.len(), TITLE_MAX_BYTES);
        assert_eq!(title.chars().count(), TITLE_MAX_BYTES / 2);

        let justo = "a".repeat(200);
        assert_eq!(sanitize_title(&justo).len(), TITLE_MAX_BYTES);
    }

    #[test]
    fn create_save_read_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();

        let note = create(data, "general").unwrap();
        assert_eq!(note.title, "sin-titulo");
        assert_eq!(note.notebook, "general");
        assert!(note.dir.join(NOTE_FILE).is_file());
        assert_eq!(body_of(&note.dir), "");

        let dir = save(&note.dir, "Mi Nota", "hola\nmundo").unwrap();
        assert_eq!(dir.dir.file_name().unwrap(), "Mi Nota");
        assert_eq!(dir.title, "Mi Nota");
        assert_eq!(dir.body, "hola\nmundo");
        assert!(!note.dir.exists());

        let leida = read(&dir.dir).unwrap();
        assert_eq!(leida.notebook, "general");
        assert_eq!(leida.title, "Mi Nota");
        assert_eq!(leida.body, "hola\nmundo");
        assert_eq!(leida.dir, dir.dir);
        assert_ne!(leida.mtime, UNIX_EPOCH);
    }

    #[test]
    fn save_no_deja_tmp() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        let note = create(data, "general").unwrap();

        let dir = save(&note.dir, "atomic", "cuerpo").unwrap();
        assert!(!dir.dir.join(NOTE_TMP).exists());
        assert!(!note.dir.join(NOTE_TMP).exists());
        assert_eq!(body_of(&dir.dir), "cuerpo");
    }

    #[test]
    fn save_renombra_y_resuelve_colision() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();

        let a = create(data, "general").unwrap();
        let a = save(&a.dir, "primera", "uno").unwrap();
        assert_eq!(a.dir.file_name().unwrap(), "primera");

        let b = create(data, "general").unwrap();
        let b = save(&b.dir, "primera", "dos").unwrap();
        assert_eq!(b.dir.file_name().unwrap(), "primera-2");
        assert_eq!(body_of(&b.dir), "dos");

        let renombrada = save(&a.dir, "segunda", "uno bis").unwrap();
        assert_eq!(renombrada.dir.file_name().unwrap(), "segunda");
        assert!(!a.dir.exists());
        assert_eq!(body_of(&renombrada.dir), "uno bis");
        assert_eq!(body_of(&b.dir), "dos");
    }

    #[test]
    fn save_mismo_titulo_no_renombra() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        let note = create(data, "general").unwrap();
        let dir = save(&note.dir, "sin-titulo", "algo").unwrap();
        assert_eq!(dir.dir, note.dir);
        assert!(dir.dir.is_dir());
        assert_eq!(body_of(&dir.dir), "algo");
    }

    #[test]
    fn rename_no_toca_el_cuerpo_y_resuelve_colision() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();

        let a = create(data, "general").unwrap();
        let a = save(&a.dir, "primera", "uno").unwrap();
        let b = create(data, "general").unwrap();
        let b = save(&b.dir, "segunda", "dos").unwrap();

        let movida = rename(&a.dir, "Comprar Café").unwrap();
        assert_eq!(movida.dir.file_name().unwrap(), "Comprar Café");
        assert!(!a.dir.exists());
        assert_eq!(body_of(&movida.dir), "uno");

        let otra = rename(&b.dir, "Comprar Café").unwrap();
        assert_eq!(otra.dir.file_name().unwrap(), "Comprar Café-2");
        assert_eq!(body_of(&otra.dir), "dos");

        assert_eq!(rename(&otra.dir, "Comprar Café").unwrap().dir, otra.dir);
    }

    #[test]
    fn trash_mueve_y_no_pisa() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();

        let a = create(data, "general").unwrap();
        let a = save(&a.dir, "nota", "original").unwrap();
        trash(data, &a.dir).unwrap();
        assert!(!a.dir.exists());
        let trash_dir = data.join(TRASH_DIR);
        assert_eq!(body_of(&trash_dir.join("nota")), "original");

        let b = create(data, "general").unwrap();
        let b = save(&b.dir, "nota", "segunda").unwrap();
        assert_eq!(b.dir.file_name().unwrap(), "nota");
        trash(data, &b.dir).unwrap();
        assert!(!b.dir.exists());
        assert_eq!(body_of(&trash_dir.join("nota")), "original");
        assert_eq!(body_of(&trash_dir.join("nota-2")), "segunda");
    }

    #[test]
    fn discover_ignora_y_ordena() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();

        for (notebook, name, body) in [
            ("general", "zeta", "z"),
            ("general", "Alfa", "a"),
            ("trabajo", "beta", "b"),
        ] {
            seed_note(data, notebook, name, body);
        }
        let basura = data.join(TRASH_DIR);
        fs::create_dir(&basura).unwrap();
        fs::write(basura.join(NOTE_FILE), "t").unwrap();
        let oculta = data.join(".oculta");
        fs::create_dir_all(oculta.join("nota")).unwrap();
        fs::write(oculta.join("nota").join(NOTE_FILE), "h").unwrap();
        fs::create_dir(data.join("vacia")).unwrap();
        fs::create_dir_all(data.join("notas-sin-marcador")).unwrap();
        create_notebook(data, "marcado").unwrap();
        fs::create_dir_all(data.join("raiz")).unwrap();
        fs::write(data.join("raiz").join(NOTE_FILE), "r").unwrap();
        fs::write(data.join("suelta.md"), "x").unwrap();

        assert_eq!(discover_notebooks(data), ["general", "marcado", "trabajo"]);

        let notes = discover(data);
        let titulos: Vec<&str> = notes.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titulos, ["Alfa", "beta", "zeta"]);
        assert_eq!(notes[0].body, "a");
        assert_eq!(notes[0].notebook, "general");
        assert_eq!(notes[1].notebook, "trabajo");
        assert_eq!(notes[2].body, "z");
    }

    #[test]
    fn create_notebook_deja_marcador_y_resuelve_colision() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();

        let first = create_notebook(data, "Mi Cuaderno").unwrap();
        let second = create_notebook(data, "Mi Cuaderno").unwrap();

        assert_eq!(first, "Mi Cuaderno");
        assert_eq!(second, "Mi Cuaderno-2");
        assert!(data.join(&first).join(NOTEBOOK_MARKER).is_file());
        assert_eq!(discover_notebooks(data), [first, second]);
    }

    #[test]
    fn rename_notebook_mueve_sus_notas() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        let notebook = create_notebook(data, "general").unwrap();
        seed_note(data, &notebook, "nota", "cuerpo");

        let renamed = rename_notebook(data, &notebook, "Personal").unwrap();

        assert_eq!(renamed, "Personal");
        assert!(!data.join(&notebook).exists());
        assert!(data.join(&renamed).join(NOTEBOOK_MARKER).is_file());
        assert_eq!(body_of(&data.join(&renamed).join("nota")), "cuerpo");
        assert_eq!(
            read(&data.join(&renamed).join("nota")).unwrap().notebook,
            "Personal"
        );
    }

    #[test]
    fn move_note_cambia_cuaderno_y_resuelve_colision() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        create_notebook(data, "general").unwrap();
        create_notebook(data, "trabajo").unwrap();
        let a = create(data, "general").unwrap();
        let a = save(&a.dir, "reunion", "uno").unwrap();
        seed_note(data, "trabajo", "reunion", "dos");

        let movida = move_note(data, &a.dir, "trabajo").unwrap();

        assert_eq!(movida.notebook, "trabajo");
        assert_eq!(movida.title, "reunion-2");
        assert!(!a.dir.exists());
        assert_eq!(body_of(&movida.dir), "uno");
        assert_eq!(
            body_of(&data.join("trabajo").join("reunion")),
            "dos",
            "la nota existente no se pisa"
        );
    }

    #[test]
    fn move_note_al_mismo_cuaderno_no_hace_nada() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        let notebook = create_notebook(data, "general").unwrap();
        let note = create(data, &notebook).unwrap();

        let movida = move_note(data, &note.dir, &notebook).unwrap();

        assert_eq!(movida.dir, note.dir);
        assert!(note.dir.is_dir());
    }

    #[test]
    fn trash_notebook_mueve_todo_el_cuaderno() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        let notebook = create_notebook(data, "general").unwrap();
        seed_note(data, &notebook, "nota", "cuerpo");

        trash_notebook(data, &notebook).unwrap();

        assert!(!data.join(&notebook).exists());
        assert_eq!(
            body_of(&data.join(TRASH_DIR).join(&notebook).join("nota")),
            "cuerpo"
        );
    }

    #[test]
    fn fold_sin_acentos_ni_mayusculas() {
        assert_eq!(fold("ÁÉÍÓÚ Ü Ñ"), "aeiou u n");
        assert_eq!(fold("Hola Mundo"), "hola mundo");
        assert_eq!(fold("Canción de Otoño"), "cancion de otono");
    }

    #[test]
    fn create_reutiliza_sufijo_libre() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();

        let a = create(data, "general").unwrap();
        let b = create(data, "general").unwrap();
        assert_eq!(a.dir.file_name().unwrap(), "sin-titulo");
        assert_eq!(b.dir.file_name().unwrap(), "sin-titulo-2");
        assert_ne!(a.dir, b.dir);
    }

    #[test]
    fn discover_dir_inexistente_no_falla() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(discover(&tmp.path().join("no-existe")).is_empty());
    }
}
