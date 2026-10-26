# Contexto

Vocabulario del dominio. Es el que usan el código, los tests y la UI; los términos
nuevos se agregan acá antes de nombrar un módulo.

- **Nota**: una carpeta con un `note.md` adentro. La unidad de todo.
- **Carpeta**: ubicación de una nota en el data dir. Su nombre es el título.
- **Título**: nombre de carpeta ya pasado por `slugify` (`Mi Nota` → `mi-nota`).
  Único por data dir: una colisión suma `-2`, `-3`.
- **Cuerpo**: contenido de `note.md`. Texto plano, sin frontmatter.
- **Tab**: nota abierta en la barra del editor, en orden de apertura.
- **Activa**: la nota del tab enfocado. Hay una sola.
- **Sucio**: nota con cambios en memoria que todavía no están en disco.
- **Renombre inline**: edición del título en la fila de la lista; confirmar escribe
  en disco al instante.
- **Papelera**: `_trash/`. Borrar es mover; restaurar es mover de vuelta a mano.
- **Reconciliación**: releer el disco al recuperar el foco. Las notas sucias no se
  tocan.
- **Orden**: alfabético (título plegado) o reciente (mtime descendente).
- **Sesión**: tabs abiertas y activa, persistidas por título en `app.ron`.
- **`NoteId`**: identidad de una nota durante el proceso. No se persiste.

Las decisiones de arquitectura están en [`docs/adr/`](docs/adr/).
