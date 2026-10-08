import { platform } from "../platform";
import type { Dictionary } from "./types";

/** What the system file manager is called, for "show in …". */
const fileManager = platform === "windows" ? "File Explorer" : platform === "macos" ? "Finder" : "the folder";

/** How the computer is referred to in copy. */
const thisComputer = platform === "macos" ? "this Mac" : "this PC";

const items = (n: number) => (n === 1 ? "1 item" : `${n} items`);

export const en: Dictionary = {
  locale: "en",
  t: {
    nav: { label: "Main navigation", optimize: "Optimize", history: "History", settings: "Settings", about: "About Fino" },
    window: { controls: "Window", minimize: "Minimize", maximize: "Maximize", restore: "Restore", close: "Close" },

    drop: {
      headline: ["Drop", "your photos", "here."],
      hovering: "Let go.",
      or: "or",
      choosePhotos: "Choose photos",
      chooseFolder: "Folder",
      replaceHint: (days: number | null) =>
        days ? `Optimizes the originals · ${days}-day backup` : "Optimizes the originals · no backup",
      exportHint: (where: string) => `Exports copies to ${where}`,
      nextToEach: "a “Fino” folder next to each photo",
      busyQueued: (n: number) => `${n} queued`,
      photosFilter: "Photos",
    },

    results: { label: "Results", done: "Optimized", skipped: "Skipped", failed: "Failed" },
    dismiss: "Dismiss",

    session: {
      working: "Optimizing",
      of: "of",
      cancel: "Stop",
      lighter: "lighter",
      photos: (n: number) => (n === 1 ? "1 photo" : `${n} photos`),
      skipped: (n: number) => `${n} skipped`,
      failed: (n: number) => (n === 1 ? "1 failed" : `${n} failed`),
      nothingSaved: "Nothing to save",
      nothingSavedHint: "These photos were already as light as they can be without losing quality.",
      compare: "Compare",
      undo: "Undo",
      undone: "Originals restored",
      reveal: `Show in ${fileManager}`,
      newSession: "Drop more",
      confirmReplaceTitle: "Replace the originals?",
      confirmReplace: (n: number, backups: boolean) =>
        `Fino will optimize ${items(n)} in place.` +
        (backups ? " It keeps a copy so you can undo it." : " No copy of the originals will be kept."),
      confirmOk: "Optimize",
      confirmCancel: "Cancel",
    },

    panel: {
      label: "Session summary",
      progress: "Batch progress",
      saved: "saved",
      photos: "Photos",
      savings: "Saved",
      savingsHint: "reduction / original size",
      allTime: "Saved all time",
      since: (when: string) => `since ${when}`,
    },

    mini: {
      enter: "Mini window",
      enterHint: "Mini window: stays on top of other windows, so you can drop photos from anywhere",
      expand: "Full window",
      add: "Choose photos",
      drop: "Drop photos or folders here",
      progress: (done: string, total: string) => `${done} of ${total}`,
      ready: (n: number) => (n === 1 ? "photo ready" : "photos ready"),
      lighter: "lighter",
      replaceTitle: "Files will be replaced",
      replaceBody: (n: number, backups: boolean) =>
        `Fino will overwrite ${items(n)} with the optimized version.` +
        (backups ? " It keeps a copy so you can undo it." : " No copy will be kept."),
      continue: "Continue",
      dontAskAgain: "Don’t show again",
    },

    convert: {
      badge: "HEIC → JPEG",
      hint: "Converted from HEIC to a Compact JPEG: size change compared with the HEIC.",
      count: (n: number) => `${n} converted from HEIC`,
      skippedMini: (n: number) => `${n} HEIC not converted · conversion is off in Settings`,
    },

    lossless: "Lossless",
    losslessHint: "Exactly the same pixels: only the encoding was rearranged.",

    compare: {
      original: "Original",
      fino: "Fino",
      similarity: "Similarity",
      divider: "Before / after divider",
      zoom: "100%",
      fit: "Fit",
      close: "Close",
      previous: "Previous",
      next: "Next",
      unavailable: "The original or the optimized version can no longer be found: it was moved or deleted.",
    },

    history: {
      title: "History",
      subtitle: `Everything Fino has saved on ${thisComputer}.`,
      totalSaved: "Saved all time",
      photos: "Photos optimized",
      sessions: "Sessions",
      avgReduction: "Average reduction",
      date: "Date",
      files: "Photos",
      saved: "Saved",
      origin: "Source",
      originMore: (n: number) => `and ${n} more`,
      originFolders: (n: number) => `${n} folders`,
      originScattered: "Several locations",
      originReveal: (path: string) => `${path} · Show in ${fileManager}`,
      replaced: "Originals",
      exported: "Exported",
      exportLog: "Export CSV",
      actions: "Actions",
      empty: "No sessions yet. Drop some photos to start.",
      undoConfirm: "Restore this session’s originals?",
      loadMore: (hidden: number) => `Show earlier sessions · ${hidden} more`,
      nothingToCompare: "Nothing to compare: the files were moved or deleted",
      discard: "Discard backup",
      discardWithSize: (size: string) => `Discard backup · ${size}`,
      discardTitle: "Discard the backup?",
      discardConfirm: (size: string) =>
        `This frees ${size}. The optimized photos don’t change, but you won’t be able to undo this session.`,
      discardOk: "Discard",
    },

    settings: {
      title: "Settings",
      subtitle: "How Fino works when you drop photos.",
      output: "Output",
      replace: "Optimize originals",
      replaceHint: "Replaces each photo with its optimized version.",
      export: "Export a copy",
      exportHint: "Leaves the originals untouched and saves copies elsewhere.",
      warn: "Ask before replacing",
      backups: "Keep a backup to undo",
      retention: "Keep backups for",
      retentionHint: "They’re deleted automatically when the time is up. Fino checks when it opens and whenever you come back to it.",
      backupSpace: "Backup space",
      backupSpaceHint: (sessions: number) =>
        sessions === 0
          ? "Copies from sessions no longer shown in History"
          : `${sessions === 1 ? "1 session" : `${sessions} sessions`} you can still undo`,
      noBackups: "No backups kept",
      free: "Free up",
      freeTitle: "Delete all backups?",
      freeConfirm: (size: string) =>
        `This frees ${size}. The optimized photos don’t change, but you won’t be able to undo those sessions.`,
      freeOk: "Free up space",
      days: (n: number) => (n === 1 ? "1 day" : `${n} days`),
      destination: "Destination",
      nextToEach: "“Fino” folder next to each photo",
      change: "Change",
      reset: "Use a folder next to each photo",
      sizes: "Sizes",
      sizesHint: "Up to 4 sizes per export, each in its own folder.",
      addSize: "Add size",
      removeSize: "Remove size",
      originalSize: "Original size",
      longEdge: "Long edge",
      maxWidth: "Max width",
      maxHeight: "Max height",
      widthFolder: "Width",
      heightFolder: "Height",
      strength: "Strength",
      privacy: "Files and privacy",
      stripLocation: "Remove GPS location",
      stripLocationHint: "Erases the coordinates from EXIF and XMP. All other metadata is kept.",
      skipOptimized: "Skip photos already optimized",
      skipOptimizedHint: "Fino marks its files so it never compresses them twice.",
      convertHeic: "Convert HEIC to JPEG",
      convertHeicHint: "They become Compact JPEGs. HDR, depth and portrait data are removed.",
      theme: "Theme",
      themes: { system: "System theme", light: "Light theme", dark: "Dark theme" },
      general: "General",
      language: "Language",
      languageHint: "System follows your computer’s language, or English if it isn’t Spanish.",
      systemLanguage: "System",
    },

    about: {
      close: "Close",
      version: (v: string) => `Version ${v}`,
      madeBy: "Made by",
    },

    presets: [
      { label: "4K", pixels: 3840 },
      { label: "Gallery", pixels: 2048 },
      { label: "Web", pixels: 1600 },
      { label: "Social", pixels: 1080 },
    ],
  },

  strengths: {
    pristine: {
      name: "Flawless",
      promise: "No difference even under a loupe, flipping between original and result.",
      typical: "≈ 60%",
    },
    identical: {
      name: "Identical",
      promise: "Looks the same as the original. The recommended one.",
      typical: "≈ 70%",
    },
    compact: {
      name: "Compact",
      promise: "A little lighter. Ideal for the web, social media and sending.",
      typical: "≈ 72%",
    },
  },

  skips: {
    alreadyOptimized: "Already optimized",
    noGain: "Already as small as it gets",
    unsupported: "Unsupported format",
    cmyk: "CMYK JPEG",
    exoticJpeg: "Unsupported JPEG variant",
    tooLarge: "Too large",
    hdrGainMap: "HDR photo (kept)",
    embeddedMedia: "Contains video (Motion Photo)",
    hdrPhoto: "HDR a JPEG can’t store",
    spatialPhoto: "Spatial photo (3D)",
    conversionOff: "Not converted: HEIC conversion is off in Settings",
  },
};
