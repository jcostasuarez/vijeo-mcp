# Formato de los objetos de pantalla de Vijeo-Designer 6.3

Notas de trabajo de la decimotercera tanda (2026-11-27): la gramática, campo a campo, de los datos de cada clase en
el portapapeles de Vijeo (lo que va detrás de la cabecera común, inventario §13.2) y de las animaciones.

- [switch.md](switch.md): el botón (Switch v6), `CExpression`, `CNativeAction` y los bloques compartidos de
  HMIManagers (bibliotecas de colores, etiquetas y fuentes, imágenes, estilo nativo).
- [animaciones.md](animaciones.md): la extensión de la cabecera común (gestor de animaciones) y cada control.
- [formas.md](formas.md): Text, Rectangle, Line y Ellipse.
- Lámparas y displays: comentarios de cabecera de `crates/vijeo-backend-vd63/src/widgets/lamp.rs` y `display.rs`.

Las rutas `S\clips`, `S\xml`, `probe` y similares se refieren al corpus y a los prototipos de aquella sesión, en una
carpeta temporal que no se conserva. El corpus eran portapapeles copiados de los proyectos de ejemplo de Machine
Expert Basic (no se incluyen en el repositorio): para repetir la prueba de ida y vuelta, copia paneles con Select All
+ Copy, guarda el formato `63INDE_VIJEO_ObjectClipboard` como `.bin` (`tools/clip_ole_dump.ps1`) y ejecuta
`cargo test -p vijeo-backend-vd63 corpus` con `VIJEO_MCP_CORPUS=<carpeta>`.

Los nombres de función y direcciones son de las DLL de Vijeo-Designer 6.3.0.103 analizadas con Ghidra; no se
incluye código descompilado.
