import { platform } from "../platform";
import type { SkipReason, Strength } from "../types";
import type { StrengthCopy } from "./types";

/** What the system file manager is called, for "show in …". */
const fileManager = platform === "windows" ? "el Explorador" : platform === "macos" ? "Finder" : "la carpeta";

/** How the computer is referred to in copy. */
const thisComputer = platform === "macos" ? "este Mac" : "este equipo";

const t = {
  nav: { label: "Navegación principal", optimize: "Optimizar", history: "Historial", settings: "Ajustes", about: "Acerca de Fino" },
  window: { controls: "Ventana", minimize: "Minimizar", maximize: "Maximizar", restore: "Restaurar", close: "Cerrar" },

  drop: {
    headline: ["Suelta", "tus fotos", "aquí."],
    hovering: "Suéltalas.",
    or: "o",
    choosePhotos: "Elegir fotos",
    chooseFolder: "Carpeta",
    replaceHint: (days: number | null) =>
      days ? `Optimiza los originales · respaldo ${days} días` : "Optimiza los originales · sin respaldo",
    exportHint: (where: string) => `Exporta copias a ${where}`,
    nextToEach: "una carpeta «Fino» junto a cada foto",
    busyQueued: (n: number) => `${n} en cola`,
    /** File-type name in the photo picker. */
    photosFilter: "Fotos",
  },

  results: { label: "Resultados", done: "Optimizada", skipped: "Omitida", failed: "Error" },
  dismiss: "Cerrar",

  session: {
    working: "Optimizando",
    of: "de",
    cancel: "Detener",
    lighter: "más livianas",
    photos: (n: number) => (n === 1 ? "1 foto" : `${n} fotos`),
    skipped: (n: number) => (n === 1 ? "1 omitida" : `${n} omitidas`),
    failed: (n: number) => (n === 1 ? "1 con error" : `${n} con error`),
    nothingSaved: "Nada que ahorrar",
    nothingSavedHint: "Estas fotos ya estaban tan livianas como pueden estar sin perder calidad.",
    compare: "Comparar",
    undo: "Deshacer",
    undone: "Originales restaurados",
    reveal: `Mostrar en ${fileManager}`,
    newSession: "Soltar más",
    confirmReplaceTitle: "¿Reemplazar los originales?",
    confirmReplace: (n: number, backups: boolean) =>
      `Fino va a optimizar ${n === 1 ? "1 elemento" : `${n} elementos`} en su lugar.` +
      (backups ? " Guardará una copia para que puedas deshacerlo." : " No se guardará copia de los originales."),
    confirmOk: "Optimizar",
    confirmCancel: "Cancelar",
  },

  panel: {
    label: "Resumen de la sesión",
    progress: "Progreso del lote",
    saved: "ahorrados",
    photos: "Fotos",
    savings: "Ahorro",
    savingsHint: "reducción / tamaño original",
    allTime: "Ahorro total",
    since: (when: string) => `desde ${when}`,
  },

  mini: {
    enter: "Ventana mini",
    enterHint: "Ventana mini: siempre encima de las demás, para soltar fotos desde cualquier lugar",
    expand: "Ventana completa",
    add: "Elegir fotos",
    drop: "Suelta fotos o carpetas aquí",
    progress: (done: string, total: string) => `${done} de ${total}`,
    ready: (n: number): string => (n === 1 ? "foto lista" : "fotos listas"),
    lighter: "más livianas",
    replaceTitle: "Se reemplazarán los archivos",
    replaceBody: (n: number, backups: boolean) =>
      `Fino va a sobrescribir ${n === 1 ? "1 elemento" : `${n} elementos`} con su versión optimizada.` +
      (backups ? " Guardará una copia para que puedas deshacerlo." : " No se guardará copia."),
    continue: "Continuar",
    dontAskAgain: "No volver a mostrar",
  },

  convert: {
    badge: "HEIC → JPEG",
    hint: "Convertida de HEIC a JPEG Compacta: cambio de tamaño frente al HEIC.",
    count: (n: number) => (n === 1 ? "1 convertida de HEIC" : `${n} convertidas de HEIC`),
    skippedMini: (n: number) => `${n} HEIC sin convertir · conversión desactivada en Ajustes`,
  },

  lossless: "Sin pérdida",
  losslessHint: "Mismos píxeles exactos: solo se reordenó la codificación.",

  compare: {
    original: "Original",
    fino: "Fino",
    similarity: "Similitud",
    divider: "División antes / después",
    zoom: "100 %",
    fit: "Ajustar",
    close: "Cerrar",
    previous: "Anterior",
    next: "Siguiente",
    unavailable: "Ya no se encuentran el original o la versión optimizada: se movieron o se borraron.",
  },

  history: {
    title: "Historial",
    subtitle: `Todo lo que Fino ha ahorrado en ${thisComputer}.`,
    totalSaved: "Ahorro total",
    photos: "Fotos optimizadas",
    sessions: "Sesiones",
    avgReduction: "Reducción media",
    date: "Fecha",
    files: "Fotos",
    saved: "Ahorro",
    origin: "Origen",
    originMore: (n: number) => (n === 1 ? "y 1 más" : `y ${n} más`),
    originFolders: (n: number) => `${n} carpetas`,
    originScattered: "Varias ubicaciones",
    originReveal: (path: string) => `${path} · Mostrar en ${fileManager}`,
    replaced: "Originales",
    exported: "Exportadas",
    exportLog: "Exportar CSV",
    actions: "Acciones",
    empty: "Aún no hay sesiones. Suelta unas fotos para empezar.",
    undoConfirm: "¿Restaurar los originales de esta sesión?",
    loadMore: (hidden: number) => `Ver sesiones anteriores · ${hidden === 1 ? "1 más" : `${hidden} más`}`,
    nothingToCompare: "Nada que comparar: los archivos se movieron o se borraron",
    discard: "Descartar respaldo",
    discardWithSize: (size: string) => `Descartar respaldo · ${size}`,
    discardTitle: "¿Descartar el respaldo?",
    discardConfirm: (size: string) =>
      `Liberarás ${size}. Las fotos optimizadas no cambian, pero ya no podrás deshacer esta sesión.`,
    discardOk: "Descartar",
  },

  settings: {
    title: "Ajustes",
    subtitle: "Cómo trabaja Fino cuando sueltas fotos.",
    output: "Salida",
    replace: "Optimizar originales",
    replaceHint: "Reemplaza cada foto por su versión optimizada.",
    export: "Exportar copia",
    exportHint: "Deja los originales intactos y guarda copias aparte.",
    warn: "Preguntar antes de reemplazar",
    backups: "Guardar respaldo para deshacer",
    retention: "Conservar respaldos",
    retentionHint: "Al cumplir el plazo se borran solos. Fino lo revisa al abrirse y cada vez que vuelves a él.",
    backupSpace: "Espacio en respaldos",
    backupSpaceHint: (sessions: number) =>
      sessions === 0
        ? "Copias de sesiones que ya no aparecen en el historial"
        : `${sessions === 1 ? "1 sesión" : `${sessions} sesiones`} que aún puedes deshacer`,
    noBackups: "No hay respaldos guardados",
    free: "Liberar",
    freeTitle: "¿Borrar todos los respaldos?",
    freeConfirm: (size: string) =>
      `Liberarás ${size}. Las fotos optimizadas no cambian, pero ya no podrás deshacer esas sesiones.`,
    freeOk: "Liberar espacio",
    days: (n: number) => (n === 1 ? "1 día" : `${n} días`),
    destination: "Destino",
    nextToEach: "Carpeta «Fino» junto a cada foto",
    change: "Cambiar",
    reset: "Usar carpeta junto a cada foto",
    sizes: "Tamaños",
    sizesHint: "Hasta 4 tamaños por exportación; cada uno en su carpeta.",
    addSize: "Añadir tamaño",
    removeSize: "Quitar tamaño",
    originalSize: "Tamaño original",
    longEdge: "Lado largo",
    maxWidth: "Ancho máx.",
    maxHeight: "Alto máx.",
    /** Start of an export folder's name, e.g. "Ancho 1600 px". */
    widthFolder: "Ancho",
    heightFolder: "Alto",
    strength: "Intensidad",
    privacy: "Archivos y privacidad",
    stripLocation: "Quitar ubicación GPS",
    stripLocationHint: "Borra las coordenadas de EXIF y XMP. El resto de metadatos se conserva.",
    skipOptimized: "Omitir fotos ya optimizadas",
    skipOptimizedHint: "Fino marca sus archivos para no recomprimirlos dos veces.",
    convertHeic: "Convertir HEIC a JPEG",
    convertHeicHint:
      "Se convierten en JPEG en intensidad Compacta. Se quitan el HDR, la profundidad y los datos de retrato.",
    theme: "Tema",
    themes: { system: "Tema del sistema", light: "Tema claro", dark: "Tema oscuro" },
    general: "General",
    language: "Idioma",
    languageHint: "Del sistema: sigue el idioma de tu equipo; inglés si no es español.",
    systemLanguage: "Del sistema",
  },

  about: {
    title: "Acerca de",
    version: (v: string) => `Versión ${v}`,
    madeBy: "Hecho por",
  },

  presets: [
    { label: "4K", pixels: 3840 },
    { label: "Galería", pixels: 2048 },
    { label: "Web", pixels: 1600 },
    { label: "Redes", pixels: 1080 },
  ],
};

const strengths: Record<Strength, StrengthCopy> = {
  pristine: {
    name: "Impecable",
    promise: "Ni con lupa notarás la diferencia, aunque alternes original y resultado.",
    typical: "≈ 60 %",
  },
  identical: {
    name: "Idéntica",
    promise: "Se ve igual que el original. La recomendada.",
    typical: "≈ 70 %",
  },
  compact: {
    name: "Compacta",
    promise: "Un poco más liviana. Ideal para web, redes y envíos.",
    typical: "≈ 72 %",
  },
};

const skips: Record<SkipReason, string> = {
  alreadyOptimized: "Ya optimizada",
  noGain: "Ya estaba al mínimo",
  unsupported: "Formato no compatible",
  cmyk: "JPEG CMYK",
  exoticJpeg: "Variante JPEG no compatible",
  tooLarge: "Demasiado grande",
  hdrGainMap: "Foto HDR (se conserva)",
  embeddedMedia: "Contiene video (Motion Photo)",
  hdrPhoto: "HDR que un JPEG no puede guardar",
  spatialPhoto: "Foto espacial (3D)",
  conversionOff: "Sin convertir: conversión HEIC desactivada en Ajustes",
};

export const es = { locale: "es", t, strengths, skips };
