# Fino — identidad y sistema de diseño

## Nombre

**Fino** — dos sílabas, se dice igual en español, italiano e inglés.
*Fino* es delgado y es refinado: archivos más livianos, calidad intacta.
Corto, amigable, fácil de recordar y de escribir; funciona como verbo implícito
("pásalo por Fino").

Alternativas consideradas: *Plume* (choca con Plume Design, marca grande de redes),
*Wisp* (confusión con Wispr Flow, app Mac popular), *Tuck*, *Pico*, *Mochi*.

## Dirección

**Instrumento de cuarto oscuro.** Superficie grafito silenciosa que nunca compite con las
fotos, números grandes y seguros, y un único color de señal que solo significa una cosa:
*bytes ahorrados*. Iconos para acciones conocidas, texto mínimo.

## Color (OKLCH, `src/styles/tokens.css`)

| Token | Oscuro | Claro | Uso |
|---|---|---|---|
| `--color-bg` | 15,5 % 0.006 255 | 97,4 % 0.004 95 | Fondo |
| `--color-panel` | 19,5 % | 99,2 % | Paneles |
| `--color-text` | 95 % | 20 % | Texto |
| `--color-display` | 72 % 0.018 260 | 30 % | Titular de entrada |
| `--color-signal` | **88 % 0.17 128** (lima) | 62 % 0.17 135 | Solo ahorro / estado activo |
| `--color-warn` | 82 % 0.14 78 | | Sin respaldo |
| `--color-danger` | 70 % 0.17 25 | | Errores |

El comparador antes/después es siempre oscuro (neutral para juzgar color).

## Tipografía (2 familias, OFL, empaquetadas — la app funciona offline)

- **Bricolage Grotesque** (variable, ejes `wght` + `wdth`) — titulares y cifras.
  Condensada (`wdth` 78–90) para el titular "Suelta tus fotos aquí." y los números de ahorro;
  tiene carácter sin ser decorativa.
- **Geist** (variable) — interfaz, etiquetas, tablas. Neutra, precisa, excelente a tamaños pequeños.

Números siempre con `tabular-nums` para que no "bailen" al contar.

## Iconos

[Lucide](https://lucide.dev) — trazo 1.75, consistente. Logo propio: una hoja esbelta en lima
(`src/components/shell/Logo.tsx`, `assets/brand/icon.svg`).

## Movimiento

Solo `transform` / `opacity` / `clip-path`. Curva `--ease-out-expo` para entradas,
`--ease-spring` para toggles y cifras finales. Las fotos se "reparten" en una pila al terminar
cada una. Todo se desactiva con `prefers-reduced-motion`.

## Pantallas

1. **Optimizar** — soltar → pila de fotos con progreso → resultado (cifra ahorrada, comparar,
   deshacer, lista por archivo). Panel derecho: medidor, fotos, % y ahorro total.
2. **Comparar** — divisor deslizable, lupa 100 % que sigue al cursor, ←/→ entre fotos, Z para zoom.
3. **Historial** — bento con el ahorro total como protagonista; tabla de sesiones con
   comparar, exportar CSV y deshacer.
4. **Ajustes** — salida (originales/exportar), respaldo, destino, tamaños, intensidad, privacidad.
