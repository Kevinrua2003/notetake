# 0002 — `NoteId` de sesión, tabs persistidas por título

## Contexto

`tabs`, el foco, las notas sucias y el renombre inline se indexaban por posición
(`usize`) en cuatro estructuras paralelas. Borrar una nota o reconciliar con el
disco obligaba a reindexar todo; un olvido dejaba el renombre apuntando a otra
nota.

## Decisión

Cada nota recibe un `NoteId(u64)` opaco al cargar o crear, y no se reindexa nunca.
`workspace` es el único módulo que lo conoce y lo usa. La sesión (tabs abiertas,
activa y cuaderno elegido) se persiste en `app.ron`; las tabs usan
`cuaderno/título` porque el id no sobrevive al proceso y el título solo no
alcanza cuando dos cuadernos tienen una nota con el mismo nombre.

## Consecuencias

- Borrar o renombrar otra nota no toca los ids: el renombre y el sucio siguen
  apuntando a la nota correcta.
- `restore` ignora referencias sin cuaderno y títulos que ya no existen.
- Renombrar o mover una nota o un cuaderno cambia su referencia de sesión, pero no
  su `NoteId` durante el proceso.
- El título sigue siendo la identidad en disco; el id solo ordena la sesión.
- `app.ron` suma el campo `notebook`; un `app.ron` viejo arranca sin tabs.

## Alternativas rechazadas

- Persistir el id: cambia el formato y no aporta nada; el disco ya identifica por
  carpeta.
- Usar la ruta como id: cambia al renombrar, que es el caso que había que resolver.
