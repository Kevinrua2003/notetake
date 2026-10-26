//! Buscar e instalar actualizaciones desde GitHub Releases (solo Linux).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const REPO: &str = "Kevinrua2003/notetake";
/// Versión compilada. `Cargo.toml` es la única fuente de verdad.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// GitHub corta una consulta ociosa bastante antes; la descarga tiene su margen.
const TIMEOUT: Duration = Duration::from_secs(15);
/// El `.deb` ronda los 10 MB; en una conexión lenta 15 s no alcanzan.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// El `.deb` ronda los 10 MB; el tope solo evita llenar la RAM con basura.
const MAX_DEB: u64 = 64 * 1024 * 1024;

/// Cómo está instalado el binario que está corriendo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    /// Paquete `.deb` en `/usr/bin`: actualizar pide contraseña con `pkexec`.
    Deb,
    /// `cargo install --root ~/.local`: el binario se reemplaza directo.
    Local,
    /// `cargo run` u otra ruta: no hay nada que la app pueda reemplazar.
    Unsupported,
}

/// Release nueva, tal como la publica GitHub.
#[derive(Clone, Debug)]
pub struct Release {
    /// Tag sin normalizar, como viene de la API: «v0.2.0».
    pub tag: String,
    /// URL del `.deb` adjunto.
    pub deb_url: String,
}

#[derive(serde::Deserialize)]
struct Latest {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(serde::Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn install_kind() -> InstallKind {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    exe_path().map_or(InstallKind::Unsupported, |exe| {
        kind_for(&exe, home.as_deref())
    })
}

/// Ruta del binario en ejecución. Después de instalar, el `.deb` ya reemplazó el
/// archivo, así que `/proc/self/exe` la reporta con « (deleted)» al final: esa
/// ruta sigue siendo la del binario nuevo.
pub fn exe_path() -> std::io::Result<PathBuf> {
    Ok(clean_deleted(std::env::current_exe()?))
}

fn clean_deleted(exe: PathBuf) -> PathBuf {
    match exe
        .to_str()
        .and_then(|path| path.strip_suffix(" (deleted)"))
    {
        Some(path) => PathBuf::from(path),
        None => exe,
    }
}

fn kind_for(exe: &Path, home: Option<&Path>) -> InstallKind {
    if exe.starts_with("/usr/bin") {
        InstallKind::Deb
    } else if home.is_some_and(|home| exe.starts_with(home.join(".local"))) {
        InstallKind::Local
    } else {
        InstallKind::Unsupported
    }
}

/// Consulta la última release. `Ok(None)` si ya estamos al día.
pub fn check() -> Result<Option<Release>, String> {
    let body = agent(TIMEOUT)
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "notetake")
        .call()
        .map_err(request_error)?
        .body_mut()
        .read_to_string()
        .map_err(|err| format!("Respuesta inválida de GitHub: {err}"))?;

    let latest: Latest = serde_json::from_str(&body)
        .map_err(|err| format!("Respuesta inválida de GitHub: {err}"))?;
    if !is_newer(&latest.tag_name, CURRENT)? {
        return Ok(None);
    }

    let deb_url = latest
        .assets
        .into_iter()
        .find(|asset| asset.name.ends_with(".deb"))
        .map(|asset| asset.browser_download_url)
        .ok_or("La release no tiene un .deb adjunto")?;

    Ok(Some(Release {
        tag: latest.tag_name,
        deb_url,
    }))
}

/// Descarga y aplica la release. Si falla, el `.deb` queda en el temporal para
/// instalarlo a mano.
pub fn install(deb_url: &str, kind: InstallKind) -> Result<(), String> {
    let deb = download(deb_url)?;
    match kind {
        InstallKind::Deb => install_deb(&deb)?,
        InstallKind::Local => install_local(&deb)?,
        InstallKind::Unsupported => return Err("Esta copia no se puede actualizar sola".into()),
    }
    let _ = std::fs::remove_file(&deb);
    Ok(())
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .new_agent()
}

fn request_error(err: ureq::Error) -> String {
    match err {
        ureq::Error::StatusCode(403) => {
            "GitHub limitó los pedidos (403); probá de nuevo en un rato.".into()
        }
        ureq::Error::StatusCode(404) => "GitHub no tiene releases para este repo.".into(),
        ureq::Error::Timeout(_) => "GitHub no respondió a tiempo.".into(),
        other => format!("No se pudo consultar GitHub: {other}"),
    }
}

fn download(url: &str) -> Result<PathBuf, String> {
    // shortcut: sin checksum; HTTPS de GitHub alcanza para una app personal.
    // Si se distribuye a más gente, publicar SHA256 en el release y compararlo acá.
    let deb = agent(DOWNLOAD_TIMEOUT)
        .get(url)
        .header("User-Agent", "notetake")
        .call()
        .map_err(request_error)?
        .body_mut()
        .with_config()
        .limit(MAX_DEB)
        .read_to_vec()
        .map_err(|err| format!("Descarga incompleta: {err}"))?;

    let path = std::env::temp_dir().join("notetake-update.deb");
    std::fs::write(&path, deb)
        .map_err(|err| format!("No se pudo escribir {}: {err}", path.display()))?;
    Ok(path)
}

fn install_deb(deb: &Path) -> Result<(), String> {
    let status = Command::new("pkexec")
        .arg("apt-get")
        .arg("install")
        .arg("-y")
        .arg(deb)
        .status()
        .map_err(|err| format!("No se pudo ejecutar pkexec: {err}"))?;
    match status.code() {
        Some(0) => Ok(()),
        // pkexec: 126 = diálogo cancelado, 127 = no autorizado.
        Some(126 | 127) => Err("Instalación cancelada".into()),
        Some(code) => Err(format!(
            "apt-get falló (código {code}); el .deb quedó en {}",
            deb.display()
        )),
        None => Err("apt-get terminó de forma anormal".into()),
    }
}

fn install_local(deb: &Path) -> Result<(), String> {
    let exe = exe_path().map_err(|err| format!("No se pudo ubicar el ejecutable: {err}"))?;
    let extracted = std::env::temp_dir().join("notetake-update");
    let _ = std::fs::remove_dir_all(&extracted);

    let status = Command::new("dpkg-deb")
        .arg("-x")
        .arg(deb)
        .arg(&extracted)
        .status()
        .map_err(|err| format!("No se pudo extraer el .deb con dpkg-deb: {err}"))?;
    if !status.success() {
        return Err(format!(
            "dpkg-deb no pudo extraer el .deb; quedó en {}",
            deb.display()
        ));
    }

    // Renombrar dentro del mismo directorio: `rename` cruzando filesystems falla.
    let staged = exe.with_file_name(".notetake.new");
    let copy = std::fs::copy(extracted.join("usr/bin/notetake"), &staged)
        .and_then(|_| std::fs::rename(&staged, &exe));
    let _ = std::fs::remove_dir_all(&extracted);
    copy.map_err(|err| {
        let _ = std::fs::remove_file(&staged);
        format!("No se pudo reemplazar {}: {err}", exe.display())
    })
}

fn is_newer(latest: &str, current: &str) -> Result<bool, String> {
    let latest = parse(latest).ok_or_else(|| format!("Versión ilegible: «{latest}»"))?;
    let current = parse(current).ok_or_else(|| format!("Versión ilegible: «{current}»"))?;
    Ok(latest > current)
}

/// `v0.2.0` y `0.2.0` → `(0, 2, 0)`. Sin prereleases: el tag se valida en CI.
fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim().trim_start_matches('v').split('.');
    let major: u64 = parts.next()?.parse().ok()?;
    let minor: u64 = parts.next()?.parse().ok()?;
    let patch: u64 = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_tags_de_tres_numeros() {
        assert_eq!(parse("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse("1.10.3"), Some((1, 10, 3)));
        assert_eq!(parse("v0.2"), None);
        assert_eq!(parse("v0.2.0-rc1"), None);
        assert_eq!(parse("v0.2.x"), None);
        assert_eq!(parse("1.2.3.4"), None);
    }

    #[test]
    fn compara_versiones() {
        assert_eq!(is_newer("v0.2.0", "0.1.0"), Ok(true));
        assert_eq!(is_newer("v0.1.0", "0.1.0"), Ok(false));
        assert_eq!(is_newer("v0.1.9", "0.1.10"), Ok(false));
        assert!(is_newer("v0.2.0-rc1", "0.1.0").is_err());
    }

    #[test]
    fn limpia_la_marca_de_archivo_borrado() {
        let exe = PathBuf::from("/usr/bin/notetake");
        assert_eq!(
            clean_deleted(PathBuf::from("/usr/bin/notetake (deleted)")),
            exe
        );
        assert_eq!(clean_deleted(exe.clone()), exe);
        assert_eq!(
            clean_deleted(PathBuf::from("/home/kevin/.local/bin/notetake (deleted)")),
            PathBuf::from("/home/kevin/.local/bin/notetake")
        );
    }

    #[test]
    fn detecta_el_tipo_de_instalacion() {
        let home = Path::new("/home/kevin");
        assert_eq!(
            kind_for(Path::new("/usr/bin/notetake"), Some(home)),
            InstallKind::Deb
        );
        assert_eq!(
            kind_for(Path::new("/home/kevin/.local/bin/notetake"), Some(home)),
            InstallKind::Local
        );
        assert_eq!(
            kind_for(Path::new("/repo/target/release/notetake"), Some(home)),
            InstallKind::Unsupported
        );
        assert_eq!(
            kind_for(Path::new("/usr/bin/notetake"), None),
            InstallKind::Deb
        );
    }
}
