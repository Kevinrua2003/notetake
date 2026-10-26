# 0001 — Una nota es una carpeta con un `note.md`

## Contexto

App personal de notas, sin base de datos. Las notas tienen que poder sincronizarse
por archivos (Dropbox, Nextcloud), borrarse sin perderlas y editarse a mano.

## Decisión

Cada nota es `<data>/<slug>/note.md`. El título es el nombre de la carpeta, ya
pasado por `slugify`; el cuerpo es el contenido de `note.md`. Borrar mueve la
carpeta a `_trash/`.

## Consecuencias

- Renombrar mueve la carpeta: los enlaces externos a esa ruta se rompen.
- El listado es un `readdir`; no hay índice ni caché que mantener.
- Los adjuntos futuros viven dentro de la carpeta de la nota, con rutas relativas.
- Los nombres `_*` y `.*` quedan reservados (papelera, temporales).

## Alternativas rechazadas

- Un archivo único (JSON o Markdown con frontmatter): el título se duplica, no se
  puede sincronizar por nota y borrar deja de ser mover una carpeta.
- SQLite: agrega dependencia y migraciones; se pierde poder abrir las notas a mano.
