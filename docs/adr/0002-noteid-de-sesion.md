# 0002 — `NoteId` de sesión, tabs persistidas por título

## Contexto

`tabs`, el foco, las notas sucias y el renombre inline se indexaban por posición
(`usize`) en cuatro estructuras paralelas. Borrar una nota o reconciliar con el
disco obligaba a reindexar todo; un olvido dejaba el renombre apuntando a otra
nota.

## Decisión

Cada nota recibe un `NoteId(u64)` opaco al cargar o crear, y no se reindexa nunca.
`workspace` es el único módulo que lo conoce y lo usa. La sesión (tabs abiertas y
activa) se persiste por título en `app.ron`, porque el id no sobrevive al proceso.

## Consecuencias

- Borrar o renombrar otra nota no toca los ids: el renombre y el sucio siguen
  apuntando a la nota correcta.
- `restore` ignora títulos que ya no existen.
- El título sigue siendo la identidad en disco; el id solo ordena la sesión.
- `app.ron` no cambió de formato.

## Alternativas rechazadas

- Persistir el id: cambia el formato y no aporta nada; el disco ya identifica por
  carpeta.
- Usar la ruta como id: cambia al renombrar, que es el caso que había que resolver.
