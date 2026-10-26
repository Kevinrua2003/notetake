# 0004 — Cuadernos de un solo nivel

## Contexto

Las notas vivían sueltas en la raíz del data dir. Hacía falta agruparlas sin
perder el modelo de carpetas, la sincronización por archivos ni el borrado
recuperable.

## Decisión

Cada cuaderno es una carpeta directa de `<data>`. Las notas viven un nivel más
adentro: `<data>/<cuaderno>/<nota>/note.md`. No hay anidamiento.

Un cuaderno existe si tiene el marcador `.notebook` (cuaderno vacío) o al menos
una nota. La app no crea un cuaderno inicial: sin cuadernos muestra un estado
vacío.

El sidebar tiene un cuaderno elegido que filtra la lista, la búsqueda y `Ctrl+N`.
Las tabs y la nota activa son globales: cambiar de cuaderno no cierra nada.
Borrar un cuaderno mueve la carpeta completa a `_trash/` y la confirmación avisa
que se eliminan todas sus notas.

## Consecuencias

- Los títulos son únicos por cuaderno; mover una nota puede sumarle un sufijo.
- El `app.ron` guarda el cuaderno elegido y las tabs como `cuaderno/título`.
- No hay migración automática: las notas viejas sueltas en la raíz se mueven a un
  cuaderno a mano.
- La reconciliación sigue comparando por ruta; mover una nota fuera de la app la
  saca de su tab.

## Alternativas rechazadas

- Dos niveles o árbol libre: complica la navegación y la reconciliación sin un
  caso de uso claro.
- Un archivo de metadatos por cuaderno: duplica el estado que ya dice el disco.
- Crear «General» automáticamente: el data dir vacío tiene que verse vacío.
