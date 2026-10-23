# notetake — especificación

Aplicación de notas de escritorio, personal, minimalista. Rust + egui/eframe.
Objetivo: organizarse y escribir notas rápido, sin features que no se usan.

## Alcance

Incluido en el MVP:

- Lista plana de notas, orden alfabético por título.
- Búsqueda en vivo por título y cuerpo.
- Editor de texto plano.
- Tabs de notas abiertas, persistentes entre sesiones.
- Alta, renombrado y borrado (a papelera interna).
- Recarga de cambios hechos fuera de la app al recuperar foco.

Fuera de alcance (explícito):

- Recordatorios, fechas de vencimiento y notificaciones.
- Adjuntos (la estructura de archivos ya los deja listos).
- Tags, carpetas jerárquicas, notas anidadas.
- Markdown renderizado o resaltado de sintaxis.
- Sincronización, cifrado, multi-dispositivo.
- Notificaciones de escritorio y daemons.

## Stack

```toml
[package]
name = "notetake"
edition = "2024"
rust-version = "1.95"

[dependencies]
eframe = { version = "0.37", default-features = false,
           features = ["glow", "wayland", "x11", "accesskit", "default_fonts", "persistence"] }
serde  = { version = "1", features = ["derive"] }

[dev-dependencies]
tempfile = "3"
```

Decisiones:

- `glow` (OpenGL) en vez de `wgpu`: árbol de dependencias más chico y builds más rápidos. Mesa GL/EGL ya está instalado en el sistema.
- `accesskit` se mantiene: soporte de lectores de pantalla.
- `persistence` guarda tamaño de ventana, tabs abiertas y nota activa.

## Almacenamiento

Directorio de datos, en este orden:

1. `$NOTETAKE_DIR`
2. `$XDG_DATA_HOME/notetake`
3. `$HOME/.local/share/notetake`

Se crea al arrancar si no existe.

Cada nota es una carpeta con un archivo `note.md`:

```
<data>/2026-10-23-mi-nota/
              note.md     # solo el cuerpo del texto
<data>/_trash/              # notas borradas, recuperables a mano
```

- El **título es el nombre de la carpeta**. El `note.md` contiene solo el cuerpo, sin título ni frontmatter.
- Los adjuntos futuros vivirán dentro de la carpeta de la nota y se referenciarán con rutas relativas.
- Carpetas dentro del data dir que no contengan `note.md` se ignoran.
- Nombres que empiezan con `_` o `.` se ignoran (reservados: `_trash`).

### Slug del título

Reglas, aplicadas al guardar:

- Minúsculas, sin acentos (á→a, é→e, í→i, ó→o, ú→u, ü→u, ñ→n).
- Solo `[a-z0-9-]`; cualquier otro carácter se convierte en `-`.
- Guiones repetidos se colapsan; se recortan los de los extremos.
- Truncado a 80 caracteres (recortando guiones finales).
- Resultado vacío → `sin-titulo`.
- Colisión con una carpeta existente → sufijo `-2`, `-3`, …
- Sin prefijo de fecha: el orden es alfabético por título.

### Escritura

- Solo se guarda con `Ctrl+S` (o el botón equivalente si existe).
- Escritura atómica: escribir `note.md.tmp` y luego `rename` sobre `note.md`.
- El renombrado de la carpeta se aplica en el mismo `Ctrl+S`, después de escribir el cuerpo. Si falla, la nota sigue apuntando a la carpeta anterior y el error se muestra en la UI.
- `Ctrl+S` sobre una nota sin cambios no hace nada (no toca el mtime).

### Borrado

- La × de la fila mueve la carpeta a `_trash/<nombre>` (con sufijo si colisiona).
- Sin diálogo de confirmación: es recuperable.
- Si la nota estaba abierta, su tab se cierra.

## Interfaz

Ventana única, maestro-detalle, tamaño inicial 1000x700.

```
┌──────────────────────────────────────────────────────┐
│ + Nueva   [buscar…]                                  │  toolbar
├───────────────┬──────────────────────────────────────┤
│ nota a        │ [nota a ×] [nota b ×]                │  tabs
│ snippet…   ×  ├──────────────────────────────────────┤
│ nota b        │ título (editable)                    │
│ snippet…   ×  ├──────────────────────────────────────┤
│               │ cuerpo en texto plano, monoespaciado │
└───────────────┴──────────────────────────────────────┘
```

- Panel izquierdo fijo ~280 px: buscador arriba, lista debajo. Cada fila: título, 2 líneas de snippet, indicador de cambios sin guardar, × al hover.
- Click en una fila abre la nota en un tab; si ya está abierta, enfoca ese tab.
- Panel derecho: tira de tabs (scroll horizontal, sin límite) + campo de título + editor multilínea que ocupa el resto.
- Fuente monoespaciada en el cuerpo.

## Comportamiento

| Situación | Resultado |
|---|---|
| `Ctrl+N` | Crea nota `sin-titulo` (o `sin-titulo-N`), la abre en un tab con el título enfocado |
| `Ctrl+S` | Guarda cuerpo, aplica slug/rename, limpia `dirty` |
| `Ctrl+F` | Foco al buscador |
| `Esc` | Limpia la búsqueda |
| `Ctrl+W` / × del tab | Cierra el tab; si está sucio, abre el modal |
| Cambiar de tab o de nota | No guarda ni pregunta; la nota queda abierta y sucia |
| Cerrar ventana con tabs sucios | Mismo modal, una vez por cada tab sucio; Cancelar aborta el cierre |
| Recuperar foco | Rescan del data dir |

### Modal de cambios sin guardar

Un único modal reusado: «Cambios sin guardar — Guardar / Descartar / Cancelar».

- Guardar: guarda esa nota y continúa la acción.
- Descartar: descarta los cambios y continúa.
- Cancelar: aborta la acción completa (no cierra el tab o cancela el cierre de la ventana).

### Rescan al recuperar foco

Al pasar de ventana desenfocada a enfocada:

- Notas en disco cuyo mtime cambió y que **no** están sucias: se recargan desde disco.
- Notas sucias: no se tocan (el usuario tiene prioridad).
- Carpetas desaparecidas: se quita la nota de la lista y se cierra su tab si estaba abierta.
- Carpetas nuevas con `note.md`: se agregan.

### Búsqueda

- Filtra en vivo mientras se escribe.
- Busca en título y cuerpo.
- Insensible a mayúsculas y acentos.
- Todo en memoria (las notas se cargan al arrancar).

## Arquitectura

```
src/
  main.rs    NativeOptions, resolución del data dir, run_native
  store.rs   fs puro: descubrir, leer, guardar, renombrar, trash, slug, fold
  app.rs     estado + UI egui
```

Modelo en memoria, una sola fuente de verdad:

```rust
struct Note {
    dir: PathBuf,       // carpeta actual (cambia al renombrar)
    title: String,      // = nombre de carpeta, editable
    body: String,
    mtime: SystemTime,
}

struct App {
    notes: Vec<Note>,           // orden de carga, nunca se reordena
    tabs: Vec<usize>,           // índices a notes
    active: usize,
    dirty: HashSet<PathBuf>,    // carpetas con cambios sin guardar
    search: String,
}
```

- La vista ordenada alfabéticamente se calcula por frame (índices al maestro), así un rename no desordena ni rompe tabs.
- Al borrar una nota se ajustan los índices de los tabs abiertos.
- Persistencia de sesión: lista de títulos de tabs abiertos + título activo, resueltos a índices al arrancar.

## Tests

`store.rs` con `tempfile`:

- Slug: acentos, espacios, símbolos, vacío, truncado a 80, colisión con sufijo.
- Roundtrip guardar/leer.
- Escritura atómica: no queda `note.md.tmp`.
- Rename de carpeta con colisión.
- Movimiento a `_trash` con colisión.
- Fold de acentos para búsqueda.

Verificación manual: `NOTETAKE_DIR=/tmp/opencode/nt-test cargo run`, nunca contra el directorio real.

## Riesgos aceptados

- Renombrar una carpeta rompe enlaces externos a esa ruta (los adjuntos internos siguen funcionando).
- Carpetas creadas fuera de la app sin `note.md` se ignoran.
- Sin adjuntos en el MVP; solo se deja el layout listo.
