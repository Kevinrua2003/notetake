# Contexto

Vocabulario del dominio. Es el que usan el código, los tests y la UI; los términos
nuevos se agregan acá antes de nombrar un módulo.

- **Nota**: una carpeta con un `note.md`, dentro de un cuaderno. La unidad de todo.
- **Cuaderno**: carpeta directa del data dir que agrupa notas. Un solo nivel, sin
  anidarse. Existe por su marcador `.notebook` o por tener al menos una nota.
- **Carpeta**: ubicación de una nota dentro de un cuaderno. Su nombre es el título.
- **Título**: nombre de carpeta, tal cual se escribe (`Mi Nota` → `Mi Nota`).
  `sanitize_title` solo limpia lo que el sistema de archivos no admite (`/`,
  prefijos `_`/`.`, espacios o puntos finales). Único por cuaderno: una colisión
  suma `-2`, `-3`.
- **Cuerpo**: contenido de `note.md`. Texto plano, sin frontmatter.
- **Tab**: nota abierta en la barra del editor, en orden de apertura. Las tabs son
  globales: cambiar de cuaderno no las cierra.
- **Activa**: la nota del tab enfocado. Hay una sola.
- **Sucio**: nota con cambios en memoria que todavía no están en disco.
- **Renombre inline**: edición del título en la fila de la lista; confirmar escribe
  en disco al instante.
- **Papelera**: `_trash/`. Borrar una nota mueve su carpeta; borrar un cuaderno
  mueve el cuaderno completo con sus notas. Restaurar es mover de vuelta a mano.
- **Reconciliación**: releer el disco al recuperar el foco. Las notas sucias no se
  tocan.
- **Orden**: alfabético (título plegado) o reciente (mtime descendente).
- **Cuaderno elegido**: el que filtra lista, búsqueda y `Ctrl+N`. Es independiente
  de la nota activa.
- **Sesión**: tabs abiertas, activa y cuaderno elegido, persistidos en `app.ron`.
  Las tabs usan `cuaderno/título`.
- **`NoteId`**: identidad de una nota durante el proceso. No se persiste.

Las decisiones de arquitectura están en [`docs/adr/`](docs/adr/).
