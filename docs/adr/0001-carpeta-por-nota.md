# 0001 — Una nota es una carpeta con un `note.md`

## Contexto

App personal de notas, sin base de datos. Las notas tienen que poder sincronizarse
por archivos (Dropbox, Nextcloud), borrarse sin perderlas y editarse a mano.

## Decisión

Cada nota es `<data>/<cuaderno>/<nota>/note.md`. El título es el nombre de la
carpeta de la nota, tal cual se escribe; `sanitize_title` solo resuelve lo que el
sistema de archivos no admite. El cuerpo es el contenido de `note.md`. Cada
cuaderno es una carpeta directa del data dir y existe por su marcador `.notebook`
o por tener al menos una nota. Borrar mueve la carpeta de la nota a `_trash/`;
borrar un cuaderno mueve la carpeta completa.

## Consecuencias

- Renombrar mueve la carpeta: los enlaces externos a esa ruta se rompen.
- Los títulos son únicos por cuaderno, no por data dir.
- Renombrar o borrar un cuaderno mueve todas sus notas con él.
- El listado es un `readdir` por cuaderno; no hay índice ni caché que mantener.
- Los adjuntos futuros viven dentro de la carpeta de la nota, con rutas relativas.
- Los nombres `_*` y `.*` quedan reservados (papelera, temporales, marcador).
- Las notas sueltas en la raíz no se leen: cada nota necesita un cuaderno.

## Alternativas rechazadas

- Un archivo único (JSON o Markdown con frontmatter): el título se duplica, no se
  puede sincronizar por nota y borrar deja de ser mover una carpeta.
- SQLite: agrega dependencia y migraciones; se pierde poder abrir las notas a mano.
