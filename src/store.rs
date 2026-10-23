//! Almacenamiento en disco: una nota es una carpeta con un `note.md` adentro.
//! Sin dependencias: solo `std`.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const NOTE_FILE: &str = "note.md";
const NOTE_TMP: &str = "note.md.tmp";
const TRASH_DIR: &str = "_trash";
const FALLBACK_TITLE: &str = "sin-titulo";
const SLUG_MAX: usize = 80;

pub struct Note {
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

/// Lee todas las notas válidas del data dir, ordenadas por título con `fold`.
/// Saltea nombres reservados (`_`, `.`), archivos sueltos, dirs sin `note.md` y notas ilegibles.
pub fn discover(data_dir: &Path) -> Vec<Note> {
    let mut notes = Vec::new();
    let Ok(entries) = fs::read_dir(data_dir) else {
        return notes;
    };
    for entry in entries.flatten() {
        let Some(title) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if title.starts_with('_') || title.starts_with('.') {
            continue;
        }
        let dir = entry.path();
        if !dir.is_dir() || !dir.join(NOTE_FILE).is_file() {
            continue;
        }
        if let Ok(note) = read(&dir) {
            notes.push(note);
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
        dir: dir.to_path_buf(),
        title: title_of(dir),
        body: String::from_utf8_lossy(&body).into_owned(),
        mtime: mtime_of(&file),
    })
}

/// Crea `sin-titulo` (o `sin-titulo-N`) con `note.md` vacío.
pub fn create(data_dir: &Path) -> io::Result<Note> {
    fs::create_dir_all(data_dir)?;
    let mut n = 1;
    let dir = loop {
        let name = if n == 1 {
            FALLBACK_TITLE.to_string()
        } else {
            format!("{FALLBACK_TITLE}-{n}")
        };
        let path = data_dir.join(name);
        match fs::create_dir(&path) {
            Ok(()) => break path,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(err) => return Err(err),
        }
    };
    let file = dir.join(NOTE_FILE);
    fs::write(&file, "")?;
    Ok(Note {
        title: title_of(&dir),
        dir,
        body: String::new(),
        mtime: mtime_of(&file),
    })
}

/// Escribe el cuerpo de forma atómica y, si el slug del título cambia el nombre,
/// renombra la carpeta. Devuelve la carpeta final.
pub fn save(data_dir: &Path, current_dir: &Path, title: &str, body: &str) -> io::Result<PathBuf> {
    write_atomic(current_dir, body)?;

    let slug = slugify(title);
    if current_dir
        .file_name()
        .is_some_and(|name| name == slug.as_str())
    {
        return Ok(current_dir.to_path_buf());
    }
    let target = data_dir.join(unique_name(data_dir, &slug, Some(current_dir)));
    fs::rename(current_dir, &target)?;
    Ok(target)
}

/// Mueve la carpeta a `<data>/_trash/<nombre>`, con sufijo si ya existe.
pub fn trash(data_dir: &Path, note_dir: &Path) -> io::Result<()> {
    let Some(name) = note_dir.file_name() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "la ruta de la nota no tiene nombre de carpeta",
        ));
    };
    let trash_dir = data_dir.join(TRASH_DIR);
    fs::create_dir_all(&trash_dir)?;
    let name = name.to_string_lossy();
    let target = trash_dir.join(unique_name(&trash_dir, &name, None));
    fs::rename(note_dir, target)
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

/// Slug del título: `[a-z0-9-]`, guiones colapsados y recortados, máximo 80.
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    for c in fold(title).chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut slug: String = out.trim_end_matches('-').chars().take(SLUG_MAX).collect();
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        FALLBACK_TITLE.to_string()
    } else {
        slug
    }
}

fn title_of(dir: &Path) -> String {
    dir.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
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

    #[test]
    fn slugify_reglas() {
        assert_eq!(slugify("Hola Mundo"), "hola-mundo");
        assert_eq!(slugify("Canción de Otoño"), "cancion-de-otono");
        assert_eq!(slugify("ÁÉÍÓÚ Ü Ñ"), "aeiou-u-n");
        assert_eq!(slugify("  ¡Hola, mundo!  "), "hola-mundo");
        assert_eq!(slugify("a---b"), "a-b");
        assert_eq!(slugify("-a-"), "a");
        assert_eq!(slugify(""), "sin-titulo");
        assert_eq!(slugify("!!! ??? ..."), "sin-titulo");
        assert_eq!(slugify("nota_2026.md"), "nota-2026-md");
        assert_eq!(slugify("ÑANDÚ"), "nandu");
    }

    #[test]
    fn slugify_trunca_a_80_sin_guion_final() {
        let largo = "abc ".repeat(40);
        let slug = slugify(&largo);
        assert_eq!(slug.len(), 79);
        assert!(!slug.ends_with('-'));

        let justo = "a".repeat(200);
        assert_eq!(slugify(&justo).len(), SLUG_MAX);
    }

    #[test]
    fn create_save_read_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();

        let note = create(data).unwrap();
        assert_eq!(note.title, "sin-titulo");
        assert!(note.dir.join(NOTE_FILE).is_file());
        assert_eq!(body_of(&note.dir), "");

        let dir = save(data, &note.dir, "Mi Nota", "hola\nmundo").unwrap();
        assert_eq!(dir.file_name().unwrap(), "mi-nota");
        assert!(!note.dir.exists());

        let leida = read(&dir).unwrap();
        assert_eq!(leida.title, "mi-nota");
        assert_eq!(leida.body, "hola\nmundo");
        assert_eq!(leida.dir, dir);
        assert_ne!(leida.mtime, UNIX_EPOCH);
    }

    #[test]
    fn save_no_deja_tmp() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        let note = create(data).unwrap();

        let dir = save(data, &note.dir, "atomic", "cuerpo").unwrap();
        assert!(!dir.join(NOTE_TMP).exists());
        assert!(!note.dir.join(NOTE_TMP).exists());
        assert_eq!(body_of(&dir), "cuerpo");
    }

    #[test]
    fn save_renombra_y_resuelve_colision() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();

        let a = create(data).unwrap();
        let a = save(data, &a.dir, "primera", "uno").unwrap();
        assert_eq!(a.file_name().unwrap(), "primera");

        let b = create(data).unwrap();
        let b = save(data, &b.dir, "primera", "dos").unwrap();
        assert_eq!(b.file_name().unwrap(), "primera-2");
        assert_eq!(body_of(&b), "dos");

        let renombrada = save(data, &a, "segunda", "uno bis").unwrap();
        assert_eq!(renombrada.file_name().unwrap(), "segunda");
        assert!(!a.exists());
        assert_eq!(body_of(&renombrada), "uno bis");
        assert_eq!(body_of(&b), "dos");
    }

    #[test]
    fn save_mismo_titulo_no_renombra() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        let note = create(data).unwrap();
        let dir = save(data, &note.dir, "sin-titulo", "algo").unwrap();
        assert_eq!(dir, note.dir);
        assert!(dir.is_dir());
        assert_eq!(body_of(&dir), "algo");
    }

    #[test]
    fn trash_mueve_y_no_pisa() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();

        let a = create(data).unwrap();
        let a = save(data, &a.dir, "nota", "original").unwrap();
        trash(data, &a).unwrap();
        assert!(!a.exists());
        let trash_dir = data.join(TRASH_DIR);
        assert_eq!(body_of(&trash_dir.join("nota")), "original");

        let b = create(data).unwrap();
        let b = save(data, &b.dir, "nota", "segunda").unwrap();
        assert_eq!(b.file_name().unwrap(), "nota");
        trash(data, &b).unwrap();
        assert!(!b.exists());
        assert_eq!(body_of(&trash_dir.join("nota")), "original");
        assert_eq!(body_of(&trash_dir.join("nota-2")), "segunda");
    }

    #[test]
    fn discover_ignora_y_ordena() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();

        for (name, body) in [("zeta", "z"), ("Alfa", "a"), ("beta", "b")] {
            let dir = data.join(name);
            fs::create_dir(&dir).unwrap();
            fs::write(dir.join(NOTE_FILE), body).unwrap();
        }
        let basura = data.join(TRASH_DIR);
        fs::create_dir(&basura).unwrap();
        fs::write(basura.join(NOTE_FILE), "t").unwrap();
        let oculta = data.join(".oculta");
        fs::create_dir(&oculta).unwrap();
        fs::write(oculta.join(NOTE_FILE), "h").unwrap();
        fs::create_dir(data.join("vacia")).unwrap();
        fs::write(data.join("suelta.md"), "x").unwrap();

        let notes = discover(data);
        let titulos: Vec<&str> = notes.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titulos, ["Alfa", "beta", "zeta"]);
        assert_eq!(notes[0].body, "a");
        assert_eq!(notes[2].body, "z");
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

        let a = create(data).unwrap();
        let b = create(data).unwrap();
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
