//! Data shared with the web UI. Field names are camelCase on the wire; keep in sync
//! with `src/lib/types.ts`.

use fino_core::{ResizeMode, SkipReason, Strength};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputMode {
    Replace,
    Export,
}

/// Light or dark UI. `System` follows macOS and switches live with it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizePreset {
    pub id: String,
    pub label: String,
    /// `None` keeps the original dimensions.
    pub mode: Option<ResizeMode>,
    pub pixels: Option<u32>,
}

impl SizePreset {
    pub fn original() -> Self {
        Self {
            id: "original".into(),
            label: "Original".into(),
            mode: None,
            pixels: None,
        }
    }

    pub fn resize(&self) -> Option<fino_core::Resize> {
        match (self.mode, self.pixels) {
            (Some(mode), Some(pixels)) if pixels > 0 => Some(fino_core::Resize { mode, pixels }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub output_mode: OutputMode,
    pub warn_before_replace: bool,
    pub keep_backups: bool,
    pub backup_retention_days: u32,
    /// `None` = a `Fino` folder next to each original.
    pub export_dir: Option<PathBuf>,
    pub sizes: Vec<SizePreset>,
    pub strength: Strength,
    pub strip_location: bool,
    pub skip_optimized: bool,
    pub appearance: Appearance,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_mode: OutputMode::Replace,
            warn_before_replace: true,
            keep_backups: true,
            backup_retention_days: 7,
            export_dir: None,
            sizes: vec![SizePreset::original()],
            strength: Strength::Identical,
            strip_location: false,
            skip_optimized: true,
            appearance: Appearance::System,
        }
    }
}

impl Settings {
    /// Repairs values a hand-edited or older settings file could carry.
    pub fn sanitized(self) -> Self {
        let sizes: Vec<SizePreset> = self.sizes.into_iter().take(4).collect();
        Self {
            sizes: if sizes.is_empty() {
                vec![SizePreset::original()]
            } else {
                sizes
            },
            backup_retention_days: self.backup_retention_days.clamp(1, 90),
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileStatus {
    Done,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputFile {
    pub path: PathBuf,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub size_label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileResult {
    pub id: usize,
    pub path: PathBuf,
    pub name: String,
    pub status: FileStatus,
    pub original_bytes: u64,
    /// Bytes of the primary output (equal to `original_bytes` when skipped).
    pub output_bytes: u64,
    pub outputs: Vec<OutputFile>,
    /// Where the untouched original can be viewed now (the file itself, or its backup).
    pub original_path: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub score: Option<f64>,
    pub quality: Option<u8>,
    pub skip_reason: Option<SkipReason>,
    pub error: Option<String>,
    pub millis: u64,
    /// Small upright JPEG for UI thumbnails (session cache; may be gone later).
    #[serde(default)]
    pub preview_path: Option<PathBuf>,
    /// Output decodes to exactly the original pixels (entropy coding only).
    #[serde(default)]
    pub lossless: bool,
    /// The source was another format (HEIC) and the output is a new JPEG next to it.
    #[serde(default)]
    pub converted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub started_at: u64,
    pub finished_at: u64,
    pub photos: usize,
    pub optimized: usize,
    pub skipped: usize,
    pub failed: usize,
    pub original_bytes: u64,
    pub output_bytes: u64,
    pub saved_bytes: u64,
    pub output_mode: OutputMode,
    pub strength: Strength,
    pub can_undo: bool,
    pub undone: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionEvent {
    #[serde(rename_all = "camelCase")]
    Started { session_id: String, total: usize },
    #[serde(rename_all = "camelCase")]
    File { result: FileResult },
    #[serde(rename_all = "camelCase")]
    Finished { summary: SessionSummary },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub saved_bytes: u64,
    pub original_bytes: u64,
    pub photos: u64,
    pub sessions: u64,
    pub since: Option<u64>,
}

/// A session as the History screen shows it: the stored summary plus what is on disk now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    #[serde(flatten)]
    pub summary: SessionSummary,
    /// At least one photo still has both its original and its optimized file in place.
    pub comparable: bool,
    /// Disk space this session's undo backups take right now.
    pub backup_bytes: u64,
}

/// History plus live disk state; never persisted.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub totals: Totals,
    pub sessions: Vec<SessionView>,
    /// All undo backups, including folders of sessions no longer listed.
    pub backup_bytes: u64,
}

/// One replaced original kept for undo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub original: PathBuf,
    pub backup: PathBuf,
    /// What Fino wrote; undo refuses to overwrite a file that no longer matches.
    #[serde(default)]
    pub written: Option<fino_core::files::Fingerprint>,
    /// Where Fino wrote when it differs from `original` (a HEIC converted to `.jpg`): undo
    /// removes this file and puts the original back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<PathBuf>,
}

/// Everything stored about a finished session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub summary: SessionSummary,
    pub results: Vec<FileResult>,
    pub backups: Vec<BackupEntry>,
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
