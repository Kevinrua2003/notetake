# notetake

App de notas de escritorio, personal y minimalista. Rust + egui/eframe.

Una nota = una carpeta con un `note.md` adentro. El título es el nombre de la carpeta.
Lista plana, búsqueda en vivo, tabs y guardado manual. Nada más.

## Uso

```bash
cargo run --release
```

Ventana inicial 1000x700. Al arrancar abre las mismas tabs de la sesión anterior.

La barra de título y los botones de ventana los dibuja la app: arrastrá la barra
para mover la ventana, doble clic para maximizar y arrastrá un borde para
redimensionar. La hamburguesa de la izquierda abre los ajustes: tema
(claro/oscuro/sistema), orden de la lista (alfabético/reciente) y acceso a la
carpeta de datos y a la papelera.

| Atajo | Acción |
|---|---|
| `Ctrl+N` | Nueva nota y renombre inline de la fila |
| `Ctrl+S` | Guardar el cuerpo de la nota abierta |
| `Ctrl+F` | Foco al buscador |
| `Esc` | Cancela el renombre inline; si no hay, limpia la búsqueda |
| `Ctrl+W` | Cerrar el tab actual |

Click en una nota de la lista la abre en un tab. El lápiz de la fila renombra
inline (`Enter` confirma, `Esc` cancela, perder foco confirma) y el renombre se
aplica en disco al instante. El `×` de la fila aparece al pasar el mouse y manda
la nota a `_trash/`.

Si hay cambios sin guardar, al cerrar el tab (o la ventana) aparece un modal
Guardar / Descartar / Cancelar.

## Dónde se guardan las notas

En este orden:

1. `$NOTETAKE_DIR`
2. `$XDG_DATA_HOME/notetake`
3. `$HOME/.local/share/notetake`

```
<data>/comprar-cafe/
          note.md     # solo el cuerpo, texto plano
<data>/_trash/        # notas borradas, recuperables a mano
```

- El título es el nombre de la carpeta: `Mi Nota` se guarda como `mi-nota`.
- `note.md` guarda solo el cuerpo; no hay frontmatter ni título duplicado.
- Carpetas sin `note.md` y nombres que empiezan con `_` o `.` se ignoran.
- Borrar es recuperable: mover la carpeta fuera de `_trash/` la devuelve a la lista.

Para usar una carpeta sincronizada (Dropbox, Nextcloud, etc.):

```bash
NOTETAKE_DIR=~/Nextcloud/notas cargo run --release
```

## Qué no hace

Recordatorios, notificaciones, adjuntos, tags, carpetas jerárquicas, markdown
renderizado, sincronización propia, cifrado. El layout de carpeta-por-nota deja
lugar para adjuntos más adelante.

## Desarrollo

```bash
cargo test            # 12 tests de storage + 11 de UI
cargo run             # datos reales
NOTETAKE_DIR=/tmp/notetake-dev cargo run   # sandbox
```

- `src/store.rs` — acceso a disco sin dependencias: descubrir, leer, guardar
  atómico (`note.md.tmp` + `rename`), renombrar, mover a `_trash`, slug y fold
  de acentos.
- `src/app.rs` — estado y UI egui: barra de título propia, ajustes (tema, orden
  y carpetas), lista, búsqueda, tabs, renombre inline, modal y rescan al
  recuperar foco.
- `src/main.rs` — arranque, resolución del data dir, ventana.

Los tests de UI usan `egui_kittest`: corren la UI real sin compositor y uno de
ellos renderiza snapshots con wgpu.

## Límites conocidos

- Guardado solo con `Ctrl+S`: si el proceso muere, se pierde el texto sin guardar.
- Renombrar una carpeta rompe enlaces externos a esa ruta (los adjuntos internos
  seguirán funcionando, porque son relativos).
- Sin adjuntos todavía: la estructura ya los soporta, la UI no.
- La barra de título es propia, así que no sigue el tema del escritorio y en
  Wayland depende de que el compositor acepte la ventana sin decoración nativa.
- Tema y orden se guardan en el `app.ron` de eframe; si matás el proceso a mano
  justo después de cambiarlos, se pierde la preferencia (la UI igual la aplica
  en esa sesión).
