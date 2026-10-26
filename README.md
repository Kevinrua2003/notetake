# notetake

App de notas de escritorio, personal y minimalista. Rust + egui/eframe.

Una nota = una carpeta con un `note.md` adentro, dentro de un cuaderno. El título
es el nombre de la carpeta y el cuaderno agrupa notas en un solo nivel. Búsqueda
en vivo, tabs y guardado manual. Nada más.

## Uso

```bash
cargo run --release
```

Ventana inicial 1000x700. Al arrancar abre las mismas tabs de la sesión anterior
y el último cuaderno elegido.

La barra de título y los botones de ventana los dibuja la app: arrastrá la barra
para mover la ventana, doble clic para maximizar y arrastrá un borde para
redimensionar. La hamburguesa de la izquierda abre los ajustes: tema
(claro/oscuro/sistema), orden de la lista (alfabético/reciente) y acceso a la
carpeta de datos y a la papelera.

Arriba de la lista está el selector de cuaderno. La lista, la búsqueda y `Ctrl+N`
trabajan sobre el cuaderno elegido. Cambiar de cuaderno solo filtra el sidebar:
la nota abierta sigue en su tab aunque sea de otro cuaderno. Si todavía no hay
cuadernos, la app muestra un estado vacío con «Crear cuaderno».

El menú del selector permite crear, renombrar y eliminar cuadernos. Eliminar un
cuaderno mueve su carpeta completa a `_trash/` y el modal avisa que se eliminan
todas sus notas. Borrar una nota también pide confirmación; si tiene cambios sin
guardar, ofrece «Cancelar / Guardar y mover / Descartar y mover».

La interfaz usa Inter (incrustada, sin dependencias del sistema), grises neutros
y un único acento teal. El cuerpo se escribe en una columna de 700 px centrada,
las filas de la lista muestran título y primera línea, y el punto teal junto al
nombre indica cambios sin guardar. La nota abierta se marca con una regla teal
en el margen izquierdo; en el modal, «Guardar» es la acción primaria.

| Atajo | Acción |
|---|---|
| `Ctrl+N` | Nueva nota en el cuaderno elegido y renombre inline de la fila |
| `Ctrl+S` | Guardar el cuerpo de la nota abierta |
| `Ctrl+F` | Foco al buscador del cuaderno elegido |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | Copiar, cortar y pegar texto |
| `Esc` | Cancela el renombre inline; si no hay, limpia la búsqueda |
| `Ctrl+W` | Cerrar el tab actual |

Click en una nota de la lista la abre en un tab. El lápiz de la fila renombra
inline (`Enter` confirma, `Esc` cancela, perder foco confirma) y el renombre se
aplica en disco al instante. El `×` de la fila aparece al pasar el mouse y pide
confirmación antes de mandar la nota a `_trash/`.

Click derecho abre un menú contextual: en el editor, Cortar / Copiar / Pegar /
Seleccionar todo; en una fila de la lista, Abrir / Renombrar / Mover a cuaderno /
Mover a papelera.

Si hay cambios sin guardar, al cerrar el tab (o la ventana) aparece un modal
Guardar / Descartar / Cancelar.

## Dónde se guardan las notas

En este orden:

1. `$NOTETAKE_DIR`
2. `$XDG_DATA_HOME/notetake`
3. `$HOME/.local/share/notetake`

```
<data>/general/
          .notebook       # marcador de cuaderno vacío
          comprar-cafe/
                    note.md     # solo el cuerpo, texto plano
<data>/_trash/        # notas y cuadernos borrados, recuperables a mano
```

- Cada nota vive dentro de un cuaderno; las notas sueltas en la raíz no se leen.
- Un cuaderno es una carpeta directa con `.notebook` o con al menos una nota.
- El título es el nombre de la carpeta: `Mi Nota` se guarda como `mi-nota`.
  La unicidad es por cuaderno.
- `note.md` guarda solo el cuerpo; no hay frontmatter ni título duplicado.
- Carpetas sin `note.md`, cuadernos vacíos sin marcador y nombres que empiezan con
  `_` o `.` se ignoran.
- Borrar una nota mueve su carpeta a `_trash/<nota>`. Borrar un cuaderno mueve la
  carpeta completa a `_trash/<cuaderno>/`. Mover la carpeta de vuelta la recupera.
- Los cuadernos no se anidan. Para migrar notas viejas, creá un cuaderno y mové
  las carpetas de notas adentro a mano; la app no migra la raíz.

Para usar una carpeta sincronizada (Dropbox, Nextcloud, etc.):

```bash
NOTETAKE_DIR=~/Nextcloud/notas cargo run --release
```

## Qué no hace

Recordatorios, notificaciones, adjuntos, tags, cuadernos anidados, markdown
renderizado, sincronización propia, cifrado. El layout de carpeta-por-nota deja
lugar para adjuntos más adelante.

## Desarrollo

```bash
cargo test            # 82: 17 storage + 20 workspace + 6 cierre + 4 widgets + 30 UI + 4 actualizaciones + 1 contraste
cargo run             # datos reales
NOTETAKE_DIR=/tmp/notetake-dev cargo run   # sandbox
```

- `src/style.rs` — identidad visual: fuente Inter, escala, neutros claro/oscuro,
  acento y métricas (ancho de lectura, sidebar, barra de título).
- `src/store.rs` — acceso a disco sin dependencias: descubrir cuadernos y notas,
  leer, guardar atómico (`note.md.tmp` + `rename`), renombrar, mover entre
  cuadernos, mover a `_trash`, títulos exactos y fold de acentos.
- `src/workspace.rs` — notas, cuadernos, tabs, foco, cambios sin guardar y renombre
  con `NoteId` estable: los índices no salen del módulo. Es dueño del disco
  (crear, guardar, renombrar, mover, papelera y reconciliación al recuperar foco).
- `src/closing.rs` — máquina del cierre: un solo estado (preguntando, cerrando o
  relanzando) en vez de tres banderas sueltas.
- `src/ui.rs` — widgets dibujados a mano: íconos, botón de acento, título con el
  punto de «sin guardar» y recorte del snippet de la fila.
- `src/app.rs` — UI egui y estado de vista: barra de título propia, ajustes
  (tema, orden y carpetas), selector de cuaderno, lista, búsqueda, menús
  contextuales y modales (incluidas las confirmaciones de borrado).
- `src/update.rs` — chequeo e instalación desde GitHub Releases (solo Linux).
- `src/main.rs` — arranque, resolución del data dir, ventana e icono.
- `CONTEXT.md` — vocabulario del dominio; las decisiones están en `docs/adr/`.
- `assets/icon.png` — icono de la app (512 px, esquinas redondeadas).
- `assets/notetake.desktop` — entrada de escritorio para el icono del dock.
- `assets/InterVariable.ttf` — fuente incrustada; licencia OFL en
  `assets/Inter-OFL.txt`.

Los tests de UI usan `egui_kittest`: corren la UI real sin compositor y uno de
ellos renderiza snapshots con wgpu.

### Icono en el dock (Linux/Wayland)

`with_icon` cubre X11 y Windows. En Wayland el compositor ignora el icono de
ventana y busca `notetake.desktop` por el `app_id` (`notetake`).

GNOME descarta las entradas cuyo `Exec` no exista en `PATH`, y ahí vuelve al
icono genérico: con solo `cargo run` el binario no está instalado. Instalalo una
vez:

```bash
cargo install --path . --root ~/.local
mkdir -p ~/.local/share/icons/hicolor/512x512/apps
cp assets/icon.png ~/.local/share/icons/hicolor/512x512/apps/notetake.png
cp assets/notetake.desktop ~/.local/share/applications/
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor
update-desktop-database ~/.local/share/applications
```

Con eso el icono aparece también al correr `cargo run`, porque la asociación es
por `app_id` y no por el ejecutable. Si el dock sigue mostrando el genérico,
cerrar sesión una vez: GNOME cachea las entradas al iniciar.

### Versionado y releases

SemVer con una sola fuente de verdad: `version` en `Cargo.toml`. El tag es `v`
más esa misma versión.

| Cambio | Bump | Tag |
|---|---|---|
| Fix compatible | patch (`0.1.0` → `0.1.1`) | `v0.1.1` |
| Feature compatible | minor (`0.1.1` → `0.2.0`) | `v0.2.0` |
| Breaking change | major (`0.2.0` → `1.0.0`) | `v1.0.0` |

Antes de 1.0.0 un breaking change puede ir en el minor.

El release es automático: al cambiar `version` en `Cargo.toml` y pushear a
`main`, el workflow corre los tests, construye el `.deb` con cargo-deb y crea
el tag `vX.Y.Z` y el Release (con las notas que genera GitHub y el `.deb` como
asset), siempre que ese tag todavía no exista. No hace falta taggear a mano.

```bash
cargo set-version 0.3.0   # cargo-edit; o editar Cargo.toml y correr cargo build
git commit -am "chore: release v0.3.0"
git push origin main      # el workflow crea v0.3.0 y publica el Release
```

Los push a `main` con una versión ya publicada no hacen nada. Si un release
falla, se reintenta en el próximo push (el tag recién se crea cuando el build
termina bien). También se puede taggear y pushear a mano: el workflow valida
que el tag coincida con `Cargo.toml`.

El tag lo crea el workflow con `GITHUB_TOKEN`, que no dispara otros workflows;
por eso el build y la publicación viven en la misma corrida.

`.github/workflows/ci.yml` corre los tests en cada push a `main` y en PRs.

### Actualizaciones desde la app

El último ítem de los ajustes es «Buscar actualizaciones». Abre un modal que
consulta `releases/latest` en GitHub y compara el tag con `version` de
`Cargo.toml`, mostrando un spinner mientras busca. Si ya está al día, lo dice.
Si hay una release nueva, el mismo modal ofrece «Actualizar»: descarga el
`.deb`, lo aplica (spinner durante la instalación) y después ofrece «Reiniciar
ahora» (con el modal de cambios sin guardar de por medio). Mientras busca o
instala, el modal no se puede cerrar.

- Instalado como `.deb` (`/usr/bin/notetake`): `pkexec apt-get install` y el
  diálogo de contraseña del sistema.
- Instalado con `cargo install --root ~/.local`: extrae el binario del `.deb`
  con `dpkg-deb` y reemplaza `~/.local/bin/notetake`.
- Corriendo con `cargo run`: el ítem queda deshabilitado.

El chequeo es manual: nada de red al arrancar. Si una instalación falla, el
`.deb` queda en el temporal para instalarlo a mano y el error se muestra dentro
del modal, con «Reintentar».

## Licencia

MIT (ver [`LICENSE`](LICENSE)). La tipografía Inter es de la SIL Open Font
License, ver `assets/Inter-OFL.txt`.

## Límites conocidos

- Guardado solo con `Ctrl+S`: si el proceso muere, se pierde el texto sin guardar.
- Renombrar una carpeta rompe enlaces externos a esa ruta (los adjuntos internos
  seguirán funcionando, porque son relativos).
- Sin adjuntos todavía: la estructura ya los soporta, la UI no.
- Los cuadernos no se anidan. Las notas sueltas en la raíz del data dir no se
  migran solas: hay que moverlas a un cuaderno a mano.
- La barra de título es propia, así que no sigue el tema del escritorio y en
  Wayland depende de que el compositor acepte la ventana sin decoración nativa.
- Tema, orden, cuaderno elegido y sesión se guardan en el `app.ron` de eframe; si
  matás el proceso a mano justo después de cambiarlos, se pierde el cambio (la UI
  igual lo aplica en esa sesión).
- Las actualizaciones confían en HTTPS: no se verifica checksum del `.deb`.
- Actualizar un `.deb` depende de `pkexec` y `apt-get`; un `cargo install` en
  `~/.local`, de `dpkg-deb`.
