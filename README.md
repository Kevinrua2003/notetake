# notetake

App de notas de escritorio, personal y minimalista. Rust + egui/eframe.

Una nota = una carpeta con un `note.md` adentro. El título es el nombre de la carpeta.
Lista plana, búsqueda en vivo, tabs y guardado manual. Nada más.

## Uso

```bash
cargo run --release
```

Ventana inicial 1000x700. Al arrancar abre las mismas tabs de la sesión anterior.

| Atajo | Acción |
|---|---|
| `Ctrl+N` | Nueva nota (`sin-titulo`), abre tab con el título enfocado |
| `Ctrl+S` | Guardar; si cambió el título, renombra la carpeta |
| `Ctrl+F` | Foco al buscador |
| `Esc` | Limpiar la búsqueda |
| `Ctrl+W` | Cerrar el tab actual |

Click en una nota de la lista la abre en un tab. El `×` de la fila la manda a `_trash/`.
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
cargo test            # 11 tests de storage + 6 de UI
cargo run             # datos reales
NOTETAKE_DIR=/tmp/notetake-dev cargo run   # sandbox
```

- `src/store.rs` — acceso a disco sin dependencias: descubrir, leer, guardar
  atómico (`note.md.tmp` + `rename`), renombrar, mover a `_trash`, slug y fold
  de acentos.
- `src/app.rs` — estado y UI egui: lista ordenada, búsqueda, tabs, modal y
  rescan al recuperar foco.
- `src/main.rs` — arranque, resolución del data dir, ventana.

Los tests de UI usan `egui_kittest`: corren la UI real sin compositor y uno de
ellos renderiza snapshots con wgpu.

## Límites conocidos

- Guardado solo con `Ctrl+S`: si el proceso muere, se pierde el texto sin guardar.
- Renombrar una carpeta rompe enlaces externos a esa ruta (los adjuntos internos
  seguirán funcionando, porque son relativos).
- Sin adjuntos todavía: la estructura ya los soporta, la UI no.
