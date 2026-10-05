<sub>Alternativa a JPEGmini</sub>

# Fino

**Fotos más livianas que se ven exactamente igual.**

[English](README.en.md) · Español

Fino es una app para macOS que recomprime JPEG con un criterio *perceptual*. Prueba versiones cada vez más pequeñas de cada foto, compara cada candidata con el original zona por zona y se queda con la más pequeña en la que no aparece ningún artefacto visible. Todo con piezas abiertas, en tu Mac y sin conexión.

```
89 fotos de cámara (24 MP)   511 MB → 156 MB   (−69,4 %)   ~0,6 s por foto
misma resolución · mismos metadatos · SSIMULACRA 2 ≈ 85
```

![Fino optimizando un lote de 1248 fotos de cámara](docs/screenshots/running-dark.png)

<table>
  <tr>
    <td><img src="docs/screenshots/compare.jpg" alt="Comparador antes/después con divisor"></td>
    <td><img src="docs/screenshots/done-light.png" alt="Sesión terminada: ahorro por foto"></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/history-light.png" alt="Historial de sesiones"></td>
    <td><img src="docs/screenshots/settings-light.png" alt="Ajustes"></td>
  </tr>
</table>

<sub>Capturas con datos de ejemplo: fotos reales de cámara y cifras simuladas con el ahorro medido (~67 %).</sub>

## Qué hace

| | |
|---|---|
| **Optimizar originales** | Reemplaza cada foto. Antes guarda un respaldo para que puedas **deshacer la sesión** (retención de 1, 7 o 30 días). |
| **Respaldos** | Viven en `~/Library/Application Support/app.fino.desktop/backups/`. Se borran solos al vencer (Fino lo revisa al abrirse y al volver a la ventana). En Ajustes ves cuánto ocupan y puedes **liberarlos**; en Historial, descartar el de una sesión. Si los borras a mano, Fino lo detecta. |
| **HEIC → JPEG (opcional)** | Desactivado de fábrica: el HEIC ya es el formato más liviano y Fino lo deja como está. Si lo activas, cada HEIC pasa a JPEG en intensidad *Compacta* — unos 25 % más liviano que el HEIC en fotos de iPhone — con EXIF, XMP, perfil de color y orientación correctos; se quitan el HDR, la profundidad y los datos de retrato. Las Live Photos siguen emparejadas con su video. |
| **Ventana mini** | Un dial pequeño que flota sobre las demás apps: arrastras fotos o carpetas y ves pasar cada foto mientras se optimiza. |
| **Exportar copia** | Deja los originales intactos. Guarda las copias en una carpeta `Fino` junto a cada foto o en un destino fijo, y respeta la estructura de carpetas. |
| **Varios tamaños** | Hasta 4 tamaños por exportación (lado largo, ancho máx. o alto máx.). Respeta la orientación EXIF y nunca amplía. |
| **Intensidad** | *Impecable* (ni con lupa notarás la diferencia), *Idéntica* (recomendada: se ve igual que el original) y *Compacta* (un poco más liviana, para web y redes). |
| **Comparar** | Vista antes/después con divisor deslizable y **lupa al 100 %** que sigue al cursor. Solo se ofrece si el original y la versión optimizada siguen donde estaban. |
| **Historial** | Ahorro total, sesiones sin límite (base de datos SQLite local), deshacer y exportación del log a CSV con el motivo de cada archivo omitido. |
| **Tema claro y oscuro** | Sigue a macOS o se fija desde Ajustes; también la barra de título y los diálogos. |
| **Privacidad** | Opción para quitar la ubicación GPS de EXIF y XMP; el resto de metadatos no se toca. |
| **Respeta tu archivo** | Conserva byte a byte EXIF, XMP, IPTC y perfiles ICC, además de fechas de creación y modificación, etiquetas de Finder y permisos. Escribe de forma atómica. |
| **Sin pérdida cuando conviene** | Si recomprimir no compensa (fotos ya comprimidas), reescribe solo la codificación: −3 a −7 % con píxeles **idénticos**. |
| **Nunca empeora** | Si no ahorra al menos un 3 %, deja el archivo como estaba. También salta las fotos que ya pasaron por Fino y los HDR con *gain map*. |
| **Finder** | Arrastra fotos o carpetas al icono del Dock, o usa «Abrir con → Fino». |
| **Apple Silicon e Intel** | Binario universal. |
| **CLI** | `fino` usa el mismo motor desde la terminal. |

## Cómo funciona el motor

```
JPEG ─► inspección ─► decodificación ─► [resize Lanczos3]
     ─► búsqueda de calidad con el encoder rápido (pista del lote → galope → bisección)
           cada prueba: score global ½ res → tiles más difíciles a resolución completa
           ganadora: verificada en TODOS los tiles
     ─► ganadora escrita progresiva + Huffman óptimo (mismos píxeles)
     ─► ¿poca ganancia? → pasada sin pérdida (coeficientes DCT intactos)
     ─► metadatos originales + marcador «Fino/» ─► escritura atómica
```

- **Encoder**: libjpeg-turbo (vía mozjpeg en modo rápido) con la tabla de cuantización de
  N. Robidoux; salida progresiva con Huffman óptimo. Conserva el submuestreo de croma original.
- **Métrica**: [zensim](https://crates.io/crates/zensim), aproximación rápida de SSIMULACRA 2 en
  XYB: score global (banding, color) + **peor tile** a resolución completa (bloques, ringing),
  porque el ojo se va directo a la peor zona de la foto.
- **Umbrales** (calibrados contra SSIMULACRA 2 real con 89 fotos de cámara):

| Intensidad | Global (½ res) | Peor tile | SSIMULACRA 2 real |
|---|---|---|---|
| Impecable | ≥ 94,0 | ≥ 91,0 | ≈ 89 |
| Idéntica | ≥ 91,5 | ≥ 86,5 | ≈ 85,5 |
| Compacta | ≥ 89,0 | ≥ 82,5 | ≈ 82 |

## Estructura

```
crates/fino-core   motor: inspección, codec, métrica, búsqueda, metadatos, archivos
crates/fino-cli    binario `fino`
src-tauri          app de escritorio: sesiones, historial, respaldos, comandos IPC
src                interfaz (React + TypeScript)
docs               diseño y capturas
```

## Desarrollo

Requisitos: Rust estable, Node 18 o superior y Xcode Command Line Tools.

```bash
npm install
```

```bash
npm run tauri dev
```

```bash
cargo test --workspace
```

```bash
npm run tauri build
```

Binario universal (Apple Silicon + Intel; requiere `nasm` para el SIMD de Intel y el target `x86_64-apple-darwin`):

```bash
npm run build:universal
```

Para ver la interfaz en el navegador con datos de ejemplo, ejecuta `npm run dev` y abre `http://localhost:1420/?state=done` (parámetros: `state=idle|running|done`, `view=history|settings|compare`, `theme=light|dark`).

CLI:

```bash
cargo run --release -p fino-cli -- ~/Pictures/Viaje
```

```bash
cargo run --release -p fino-cli -- --in-place --long-edge 2048 --strip-location fotos/
```

Para calibrar los umbrales contra SSIMULACRA 2 real:

```bash
cargo run --release -p fino-core --example calibrate -- carpeta/con/jpegs
```

## Licencias de terceros

mozjpeg (IJG/BSD), zune-jpeg (MIT/Apache-2.0/Zlib), zensim (MIT/Apache-2.0), fast_image_resize (MIT/Apache-2.0) y Tauri (MIT/Apache-2.0). Fuentes: Bricolage Grotesque y Geist (SIL OFL 1.1). La foto del comparador es de la Kodak Lossless True Color Image Suite; las demás, del autor.
