# 0003 — Sin trait para el almacenamiento

## Contexto

La revisión de arquitectura sugirió un `trait Store` con un fake en memoria para
testear `workspace` sin tocar disco.

## Decisión

`store` son funciones sobre `std::fs`. Los tests de `store` y `workspace` usan
`tempfile` y cruzan la interfaz real.

## Razón

Un solo adapter (el sistema de archivos) es un seam hipotético. El fake tendría que
reimplementar rename, colisiones de título, escritura atómica y mtimes: justo la
semántica que hay que probar. Un test contra el fake no diría nada sobre el disco.

## Consecuencias

- Los tests de `workspace` escriben en un directorio temporal; siguen siendo
  rápidos (la suite entera corre en menos de un segundo).
- Si aparece un segundo backend real (sincronización propia, por ejemplo), ahí sí
  se define el puerto con dos adapters.
