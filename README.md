# vijeo-mcp

Servidor MCP (stdio, Rust + `rmcp` 3.8) para pilotar **Vijeo-Designer** como lo haría un usuario. Cada proyecto abierto es una instancia normal de Vijeo-Frame, que el MCP maneja con sus menús, asistentes, diálogos y Navigator. No usa la API COM (en modo SoMachine añadía un driver sin dirección a cada target). Las pantallas se leen, editan y crean con el portapapeles de Vijeo, como Copiar/Pegar. La documentación de diseño y de ingeniería inversa se guarda aparte, en local (`docs/`, fuera del repositorio).

## Compilar

```bash
cargo build --release -p vijeo-mcp
```

Binario: `target\release\vijeo-mcp.exe`. Requiere Windows con Vijeo-Designer instalado (6.3.0.103 verificado) y una sesión de escritorio: cada proyecto se abre en una ventana visible de Vijeo. Se enlaza con el CRT estático (`.cargo\config.toml`), así que no necesita el redistribuible de Visual C++ x64.

### Releases

Cada release publicado en GitHub lleva `vijeo-mcp-<versión>.zip` (exe x64 + README) y su `.sha256`. Los genera el workflow [`release.yml`](.github/workflows/release.yml) al publicar el release, también si se crea desde la web. El tag tiene que ser `v<versión>` de `Cargo.toml`. Desde Actions › Release assets › Run workflow se rehacen los assets de un tag que ya tenga release.

```powershell
.\scripts\release.ps1                 # pruebas + compilación + dist\vijeo-mcp-<versión>.zip, en local
.\scripts\release.ps1 -Publish        # además crea el release v<versión>; GitHub Actions adjunta los assets
```

Antes de publicar hay que subir la versión en `[workspace.package]` de `Cargo.toml`. Si el MCP está usando `target\release\vijeo-mcp.exe`, el script lo renombra a `vijeo-mcp.run-HHMMSS.exe` antes de compilar.

## Registrar en Claude Code

```bash
claude mcp add vijeo -- C:\mcp\vijeo-mcp\target\release\vijeo-mcp.exe
```

Opciones (`vijeo-mcp --help`): `--vijeo-version 6.3`, `--max-instances 3`, `--start-timeout 30`, `--call-timeout 60`, `--build-timeout 600`, `--dialog-grace 1500`, `--protect <dir>` (repetible), `--cache-dir <dir>`, `--log-file <ruta>`. Nivel de log con `RUST_LOG`.

Qué modelos se pueden crear: la regla del catálogo supone la licencia estándar. Para comprobarlo en esta instalación, `vijeo-mcp --probe-models` (sin servidor; unos 15 minutos, abre y cierra Vijeo) crea y borra un target de cada modelo y guarda el resultado en `%LOCALAPPDATA%\vijeo-mcp`; el MCP lo usa desde el siguiente arranque. `--probe-only <texto>` prueba sólo algunos modelos.

- `--start-timeout`: al abrir o crear un proyecto se esperan al menos 180 s, porque la actualización de versión puede tardar.
- `--call-timeout`: guardar, los asistentes y borrar targets. `--build-timeout`: compilar y exportar el `.vxml`.
- `--dialog-grace` (ms): tiempo que se tolera un aviso desconocido antes de fallar.

## Requisitos

- Vijeo-Designer 6.3 instalado, con su catálogo de modelos (`TargetInfo`) y sus drivers (`HMI IO`).
- Para las lecturas del XML (`project_export_xml`, `variable_list`, `variable_lookup`, `panel_list`, `panel_report`, `action_list` y `project_cross_references`): `HKLM\SOFTWARE\WOW6432Node\Schneider Electric\Vijeo-Designer63\Vijeo-Frame\Options\SupportsXMLImportExport` = 1 (DWORD). Sin esa clave, Vijeo no ofrece el formato `.vxml` en Export Project y la herramienta devuelve `unsupported`, indicando la clave que falta.

## Herramientas

| Herramienta | Qué hace |
|---|---|
| `vijeo_installations`, `vijeo_status` | Instalaciones, backend elegido, capacidades, instancias de Vijeo (arrancando, lista, ocupada o caída), proyectos abiertos y carpetas temporales huérfanas |
| `catalog_list_models`, `catalog_list_families` | Catálogo de HMI descifrado de la instalación. `creatable` = se puede crear con el asistente (familia activa y licencia estándar) |
| `catalog_list_drivers` | Drivers de comunicación instalados (`HMI IO\*_PDL.dll`), con filtro por fabricante, medio y texto |
| `project_new` | Crea un proyecto con su primer target (asistente New Project; `target` es obligatorio), lo guarda como `.vdz` en `save_as` y lo deja abierto |
| `project_open`, `project_info`, `project_close` | Abre un `.vdz` en su propia instancia de Vijeo (trabaja sobre una copia), da su nombre, ruta, `dirty` y targets, y lo cierra |
| `project_save`, `project_rename` | Guarda la copia de trabajo y empaqueta el `.vdz` en su ruta o en `save_as`. `project_rename` cambia el nombre que se usa al empaquetar |
| `target_list`, `target_add`, `target_rename`, `target_delete`, `panel_list` | Targets (añadir con el asistente New Target; renombrar y borrar en el Navigator) y paneles base (`BP:\...`) |
| `project_build`, `target_build` | Build All: nº de errores y warnings y el texto de cada uno, de la Feedback Zone. Con `project` compila el estado actual del proyecto abierto; con `path` compila una copia de un `.vdz` sin tocarlo (~25 s con un proyecto real) |
| `project_messages` | Contenido actual de la Feedback Zone de un proyecto abierto |
| `target_config` | Drivers, equipos (IP, puerto, Unit ID, parámetros) y propiedades de descarga de cada target, de un proyecto abierto (su copia de trabajo) o de cualquier `.vdz` sin abrir Vijeo |
| `target_config_set` | Cambia en un `.vdz`, sin abrir Vijeo, la IP, la máscara y la puerta de enlace de descarga del HMI, y la IP, el puerto y el Unit ID de los equipos (puerto y Unit ID sólo en Modbus TCP). Reescribe `path` o, con `save_as`, escribe otro `.vdz`; `dry_run` sólo calcula los cambios. Rechaza un destino abierto con `project_open` |
| `project_export_xml` | Export Project: escribe `<dir>\<proyecto>.vxml` (public.xsd) y la carpeta `<dir>\<proyecto>\` con las imágenes |
| `variable_list`, `variable_lookup`, `project_cross_references` | Variables (nombre completo, tipo, origen, dirección, descripción), existencia de una variable y referencias cruzadas (dónde se usa cada una y cuáles no se usan), leídas del `.vxml` |
| `panel_report`, `action_list` | Paneles con sus acciones de panel, y acciones globales (disparador, enclavamiento, script u operaciones), del `.vxml` |
| `script_lint` | Revisa los scripts del proyecto (o un texto suelto) con el compilador de scripts de Vijeo (`ScriptEngine.dll`: sintaxis y tipos con línea y columna) y contra sus variables y la API de scripts: variables que no existen, mayúsculas, métodos. Los que usan objetos que el MCP aún no describe al compilador salen como «sin comprobar» |
| `project_goto` | Abre un panel base en la ventana de Vijeo del proyecto (Navigator) |
| `variables_import` | Variables › Import Variables en un target abierto: un CSV o una lista de variables (se genera el CSV). Las que ya existen se conservan salvo con `replace_existing`; el `.vdz` cambia con `project_save` |
| `xml_validate` | Sin Vijeo: valida un `.vxml` contra el `public.xsd` de la instalación (con .NET `System.Xml`) |
| `variables_generate_csv` | Sin Vijeo: escribe un CSV de variables en el formato de Export/Import Variables, rechazando palabras reservadas; con `project` avisa de nombres repetidos y grupos de escaneo desconocidos |
| `data_convert` | Sin Vijeo: recetas `.rcp` y registros `.dat` a CSV, y CSV a `.rcp`, con `DataConverter.exe` |
| `panel_objects` | Objetos de un panel base de un proyecto abierto: ruta (`Grupo/Objeto`), tipo, posición y tamaño absolutos, textos y variables, con lo que contiene cada grupo |
| `panel_edit` | Edita los objetos de un panel base: renombrar, mover, redimensionar, cambiar un texto, sustituir texto o variables dentro de las cadenas, borrar, poner o quitar animaciones, cambiar las acciones de un botón y los colores. `dry_run` sólo devuelve cómo quedaría |
| `panel_insert` | Crea objetos desde cero (botones con acciones —bits, palabras, scripts compilados con el compilador de Vijeo, cambiar de panel, abrir y cerrar ventanas emergentes—, lámparas, displays numéricos y de texto, textos, rectángulos, elipses y líneas, con animaciones de visibilidad, parpadeo, color, relleno y posición) o añade piezas de la Toolchest u objetos copiados de otro panel, en la posición indicada, con otro nombre y sustituciones de texto o variables |
| `toolchest_list` | Sin Vijeo: bibliotecas de la Toolchest de la instalación y, con filtro, sus piezas (id para `panel_insert`, tipos y tamaño) |

`project_goto`, `variables_import` y las tres herramientas de objetos de panel no funcionan después de un `project_build` en la misma instancia (Vijeo deja de atender el Navigator): úsalos antes de compilar, o guarda, cierra y vuelve a abrir.

Las herramientas de objetos trabajan como Copiar y Pegar en el editor del panel, con el portapapeles de Windows (inventario §13.2):
- `panel_edit` comprueba las operaciones sobre una copia; luego corta todos los objetos del panel, los edita y los pega. Al final compara lo que quedó con lo esperado; si el panel quedara vacío, vuelve a pegar el original.
- Vijeo vuelve a enlazar las variables por nombre al pegar y renumera los `ObjectID`.
- Las piezas de la Toolchest llegan sin configurar, igual que si se arrastraran: `project_build` dice qué variables les faltan.
- El texto que tuviera el portapapeles se devuelve al terminar; el resto de formatos se pierde.
- Las piezas de `Image Library` y algunas plantillas antiguas no se pueden insertar.

Cada proyecto abierto es una instancia de Vijeo-Frame que trabaja sobre una copia del `.vdz` en `Vijeo-Manager\MCP_TMP_<pid>_<n>`. El `.vdz` sólo cambia con `project_save` o con `target_config_set` (sin abrir Vijeo), y la copia se borra al cerrar el proyecto. Abrir tarda unos 10-30 s.

`project_close` falla si hay cambios hechos por el MCP sin guardar, salvo con `discard=true`. `dirty` sólo refleja esos cambios: lo que el usuario edite a mano en la ventana no se detecta, aunque `project_save` lo guarda.

`target_list` y `target_config` (con `project`) leen la copia de trabajo, así que incluyen los cambios del MCP que aún no están en el `.vdz`. Las lecturas del `.vxml` usan la última exportación, que se reutiliza mientras no cambie el `.SwxCF` de la copia (es decir, hasta el siguiente guardado): lo que se edite a mano sin guardar no aparece en ellas.

Las herramientas cuya capacidad no existe en la versión instalada no aparecen en `tools/list`.

## Seguridad

- Nunca abre ni guarda dentro de `Vijeo-Manager` (proyectos reales), de la carpeta de instalación ni de las carpetas `--protect`. La única excepción son las carpetas `MCP_TMP_<pid>_<n>` que Vijeo crea allí al abrir una copia (`project_open`, `project_new`, `project_build` y `target_build` con `path`). El MCP comprueba que el nombre no exista antes y sólo borra carpetas con ese prefijo.
- Sólo `.vdz`. No hay herramientas de descarga a HMI.
- `target_config_set` escribe un `.vdz` sin Vijeo, con la misma validación de ruta que `project_save`. Escribe `<destino>.mcp-tmp` junto al destino, relee el fichero y compara la configuración de todos los targets con la esperada. Sólo si coincide renombra el temporal sobre el destino; si no, lo borra y falla. Rechaza un destino abierto con `project_open`.
- Los diálogos de Vijeo se responden así: actualizar la versión del proyecto → Sí; modelo no soportado → No (lo convierte a un modelo equivalente); tres o más instancias de Vijeo → Aceptar. Los asistentes y el diálogo de fichero los maneja el MCP. Un aviso desconocido se cierra con Aceptar o Cancelar tras `--dialog-grace` y la operación falla con `modal_dialog`; otro diálogo con botones falla tras 30 s y queda abierto.
- Si la instancia de un proyecto se cae, sus herramientas devuelven `server_crashed` hasta que se cierra con `project_close`. Los cambios sin guardar se pierden: guarda a menudo.
- Al salir (stdin cerrado o Ctrl+C) cierra sin guardar todos los proyectos (`Close Project`, responde No) y sus instancias. Si una no sale en 20 s, se termina.

## Pruebas

```bash
cargo test --workspace
```

Con Vijeo real (lanzan instancias visibles y trabajan sobre copias en `%TEMP%`):

```bash
cargo test --workspace --features vijeo-backend-vd63/live-vijeo,vijeo-mcp/live-vijeo -- --test-threads=1
```
