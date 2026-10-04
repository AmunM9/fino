//! File-system side of optimization: discovering photos, writing atomically and
//! keeping everything a photographer relies on — dates, Finder tags, permissions.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

const JPEG_EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "jpe", "jfif"];
const HEIC_EXTENSIONS: [&str; 3] = ["heic", "heif", "hif"];
const VIDEO_EXTENSIONS: [&str; 1] = ["mov"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub path: PathBuf,
    /// The folder the user dropped, used to mirror structure when exporting.
    pub root: Option<PathBuf>,
}

pub fn has_jpeg_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| JPEG_EXTENSIONS.iter().any(|j| e.eq_ignore_ascii_case(j)))
}

fn extension_in(path: &Path, list: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| list.iter().any(|j| e.eq_ignore_ascii_case(j)))
}

pub fn has_heic_extension(path: &Path) -> bool {
    extension_in(path, &HEIC_EXTENSIONS)
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

/// Expands dropped paths into files. Files dropped directly are always included (so the user
/// learns why a non-JPEG was skipped); inside folders only JPEGs and HEICs are picked up. Symlinked
/// folders are not followed. Each file appears once even if dropped twice (e.g. a folder and
/// a photo inside it) — two workers on one file would race on the same bytes.
pub fn collect(paths: &[PathBuf]) -> Vec<Job> {
    let mut jobs = Vec::new();
    for path in paths {
        if path.is_dir() {
            walk(path, path, &mut jobs);
        } else if path.is_file() {
            jobs.push(Job {
                path: path.clone(),
                root: None,
            });
        }
    }
    let mut seen = HashSet::new();
    jobs.retain(|job| {
        seen.insert(fs::canonicalize(&job.path).unwrap_or_else(|_| job.path.clone()))
    });
    jobs
}

fn walk(dir: &Path, root: &Path, jobs: &mut Vec<Job>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if is_hidden(&path) || kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            walk(&path, root, jobs);
        } else if kind.is_file() && (has_jpeg_extension(&path) || has_heic_extension(&path)) {
            jobs.push(Job {
                path,
                root: Some(root.to_path_buf()),
            });
        }
    }
}

/// Everything about a file we restore after replacing its contents.
#[derive(Debug, Clone)]
pub struct Attributes {
    modified: SystemTime,
    accessed: SystemTime,
    created: Option<SystemTime>,
    permissions: fs::Permissions,
    xattrs: Vec<(OsString, Vec<u8>)>,
}

impl Attributes {
    pub fn read(path: &Path) -> std::io::Result<Self> {
        let meta = fs::metadata(path)?;
        let xattrs = xattr::list(path)
            .map(|names| {
                names
                    .filter(|n| n != "com.apple.quarantine")
                    .filter_map(|n| xattr::get(path, &n).ok().flatten().map(|v| (n, v)))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            modified: meta.modified()?,
            accessed: meta.accessed()?,
            created: meta.created().ok(),
            permissions: meta.permissions(),
            xattrs,
        })
    }

    /// Best effort: a missing Finder tag must never fail an otherwise good write.
    pub fn apply(&self, path: &Path) -> std::io::Result<()> {
        fs::set_permissions(path, self.permissions.clone())?;
        for (name, value) in &self.xattrs {
            let _ = xattr::set(path, name, value);
        }
        filetime::set_file_times(
            path,
            filetime::FileTime::from_system_time(self.accessed),
            filetime::FileTime::from_system_time(self.modified),
        )?;
        if let Some(created) = self.created {
            let _ = set_creation_time(path, created);
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn set_creation_time(path: &Path, time: SystemTime) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let since = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(std::io::Error::other)?;
    let c_path =
        std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
    let mut list = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_CRTIME,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut spec = libc::timespec {
        tv_sec: since.as_secs() as libc::time_t,
        tv_nsec: since.subsec_nanos() as libc::c_long,
    };
    // SAFETY: `c_path` is a valid NUL-terminated path; `list` requests exactly one
    // ATTR_CMN_CRTIME attribute, whose payload is the single `timespec` we pass with its size.
    let rc = unsafe {
        libc::setattrlist(
            c_path.as_ptr(),
            (&mut list as *mut libc::attrlist).cast(),
            (&mut spec as *mut libc::timespec).cast(),
            std::mem::size_of::<libc::timespec>(),
            0,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
fn set_creation_time(_path: &Path, _time: SystemTime) -> std::io::Result<()> {
    Ok(())
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    target.with_file_name(format!(".{name}.fino-{}-{n}.tmp", std::process::id()))
}

/// Writes `bytes` to `target` via a sibling temp file + rename, so a crash never leaves
/// a half-written photo behind.
pub fn write_atomic(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = temp_path(target);
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Keeps the original's bytes at `backup`: a hard link when on the same volume (instant,
/// no extra space until the original is replaced), otherwise a copy.
pub fn backup(original: &Path, backup: &Path) -> std::io::Result<()> {
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::hard_link(original, backup).or_else(|_| fs::copy(original, backup).map(|_| ()))
}

/// Replaces `path` with `bytes`, keeping its dates, tags and permissions. If `backup_to` is
/// given the previous contents are preserved there first.
///
/// Once the new bytes are in place the replacement has happened; restoring attributes is
/// best effort so a stubborn timestamp never turns a done file into a "failed" one.
pub fn replace(path: &Path, bytes: &[u8], backup_to: Option<&Path>) -> std::io::Result<()> {
    let attrs = Attributes::read(path)?;
    if let Some(dest) = backup_to {
        backup(path, dest)?;
    }
    write_atomic(path, bytes)?;
    let _ = attrs.apply(path);
    Ok(())
}

/// Where a converted photo goes next to its source: same name, `.jpg` (`.JPG` when the
/// source's extension is upper case, as iPhones write), never over an existing file.
pub fn converted_path(source: &Path) -> PathBuf {
    unique_path(&converted_name(source))
}

/// `IMG_0001.HEIC` → `IMG_0001.JPG`, `photo.heic` → `photo.jpg`.
pub fn converted_name(source: &Path) -> PathBuf {
    let upper = source
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.chars().all(|c| c.is_ascii_uppercase()));
    source.with_extension(if upper { "JPG" } else { "jpg" })
}

/// Moves a file, across volumes if needed (copy, then delete the source).
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let attrs = Attributes::read(from)?;
    fs::copy(from, to)?;
    let _ = attrs.apply(to);
    fs::remove_file(from)
}

/// Replace mode for a conversion (HEIC → JPEG): writes `bytes` as a new JPEG next to
/// `source` with the source's dates and tags, then moves the source into `backup_to` (or
/// deletes it when backups are off). If the source cannot be retired, the new JPEG is
/// removed again so nothing is left half done. Returns the JPEG's path.
pub fn convert_in_place(
    source: &Path,
    bytes: &[u8],
    backup_to: Option<&Path>,
) -> std::io::Result<PathBuf> {
    let attrs = Attributes::read(source)?;
    let out = converted_path(source);
    write_atomic(&out, bytes)?;
    let _ = attrs.apply(&out);
    let retired = match backup_to {
        Some(dest) => move_file(source, dest),
        None => fs::remove_file(source),
    };
    if let Err(e) = retired {
        let _ = fs::remove_file(&out);
        return Err(e);
    }
    Ok(out)
}

/// Undo of `convert_in_place`: puts the original back and removes the JPEG — unless the
/// JPEG changed since Fino wrote it, or something new took the original's name.
pub fn restore_converted(
    backup: &Path,
    original: &Path,
    output: &Path,
    expected: Option<Fingerprint>,
) -> std::io::Result<()> {
    if let Some(expected) = expected {
        if fingerprint(output).ok() != Some(expected) {
            return Err(std::io::Error::other(
                "the converted photo changed after it was written; left as is",
            ));
        }
    }
    if original.exists() {
        return Err(std::io::Error::other(
            "a file with the original's name exists again; left as is",
        ));
    }
    move_file(backup, original)?;
    match fs::remove_file(output) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// The video half of a Live Photo: a `.mov` with the same name next to the photo.
pub fn live_photo_video(photo: &Path) -> Option<PathBuf> {
    let stem = photo.file_stem()?;
    let dir = photo.parent()?;
    fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.file_stem() == Some(stem) && extension_in(p, &VIDEO_EXTENSIONS))
}

/// Size and modification time, used to tell whether a file changed since Fino wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub bytes: u64,
    pub modified_ms: u64,
}

pub fn fingerprint(path: &Path) -> std::io::Result<Fingerprint> {
    let meta = fs::metadata(path)?;
    let modified_ms = meta
        .modified()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Ok(Fingerprint {
        bytes: meta.len(),
        modified_ms,
    })
}

/// Writes an exported copy next to nothing it would clobber, carrying the source's dates.
/// Returns the path actually written.
pub fn export(source: &Path, dest: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let dest = unique_path(dest);
    write_atomic(&dest, bytes)?;
    let attrs = Attributes::read(source)?;
    attrs.apply(&dest)?;
    Ok(dest)
}

/// Puts the original back over `path` (undo). Uses rename when possible.
///
/// If `expected` is given and the file no longer matches what Fino wrote — the user edited
/// it since — nothing is touched: undo must never destroy newer work.
pub fn restore(backup: &Path, path: &Path, expected: Option<Fingerprint>) -> std::io::Result<()> {
    if let Some(expected) = expected {
        if fingerprint(path).ok() != Some(expected) {
            return Err(std::io::Error::other(
                "the file changed after it was optimized; left as is",
            ));
        }
    }
    let attrs = Attributes::read(backup)?;
    if fs::rename(backup, path).is_err() {
        let bytes = fs::read(backup)?;
        write_atomic(path, &bytes)?;
        fs::remove_file(backup)?;
    }
    let _ = attrs.apply(path);
    Ok(())
}

/// `photo.jpg` → `photo 2.jpg`, `photo 3.jpg`… like Finder.
pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| path.with_file_name(format!("{stem} {n}{ext}")))
        .find(|p| !p.exists())
        .expect("an unused name always exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_jpegs_recursively_and_skips_hidden_and_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("a/b")).unwrap();
        for f in [
            "one.JPG",
            "a/two.jpeg",
            "a/b/three.jpg",
            "a/notes.txt",
            ".hidden.jpg",
        ] {
            fs::write(root.join(f), b"x").unwrap();
        }
        let loose = root.join("a/notes.txt");
        let jobs = collect(&[root.to_path_buf(), loose.clone()]);
        let names: Vec<_> = jobs
            .iter()
            .map(|j| j.path.strip_prefix(root).unwrap().to_path_buf())
            .collect();
        assert_eq!(
            names,
            vec![
                PathBuf::from("a/b/three.jpg"),
                "a/two.jpeg".into(),
                "one.JPG".into(),
                "a/notes.txt".into()
            ]
        );
        assert_eq!(jobs[0].root.as_deref(), Some(root));
        assert_eq!(jobs[3].root, None);
    }

    #[test]
    fn replace_keeps_dates_and_backup_holds_old_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("p.jpg");
        fs::write(&photo, b"old").unwrap();
        let past = filetime::FileTime::from_unix_time(1_600_000_000, 0);
        filetime::set_file_mtime(&photo, past).unwrap();
        let backup_path = dir.path().join("backup/p.jpg");

        replace(&photo, b"new", Some(&backup_path)).unwrap();

        assert_eq!(fs::read(&photo).unwrap(), b"new");
        assert_eq!(fs::read(&backup_path).unwrap(), b"old");
        let mtime = filetime::FileTime::from_last_modification_time(&fs::metadata(&photo).unwrap());
        assert_eq!(mtime, past);

        restore(&backup_path, &photo, Some(fingerprint(&photo).unwrap())).unwrap();
        assert_eq!(fs::read(&photo).unwrap(), b"old");
        assert!(!backup_path.exists());
    }

    #[test]
    fn restore_refuses_to_clobber_a_file_edited_after_optimizing() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("p.jpg");
        fs::write(&photo, b"old").unwrap();
        let backup_path = dir.path().join("b/p.jpg");
        replace(&photo, b"new", Some(&backup_path)).unwrap();
        let written = fingerprint(&photo).unwrap();
        fs::write(&photo, b"edited in Lightroom").unwrap();

        assert!(restore(&backup_path, &photo, Some(written)).is_err());
        assert_eq!(fs::read(&photo).unwrap(), b"edited in Lightroom");
        assert!(backup_path.exists(), "backup kept for a later decision");
    }

    #[test]
    fn collect_deduplicates_a_file_dropped_twice() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("a.jpg");
        fs::write(&photo, b"x").unwrap();
        let jobs = collect(&[dir.path().to_path_buf(), photo]);
        assert_eq!(jobs.len(), 1);
    }

    #[test]
    fn export_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("p.jpg");
        fs::write(&src, b"src").unwrap();
        let dest = dir.path().join("out/p.jpg");
        let first = export(&src, &dest, b"a").unwrap();
        let second = export(&src, &dest, b"b").unwrap();
        assert_eq!(first, dest);
        assert_eq!(second, dir.path().join("out/p 2.jpg"));
    }

    #[test]
    fn conversion_writes_a_jpeg_beside_and_undo_brings_the_heic_back() {
        let dir = tempfile::tempdir().unwrap();
        let heic = dir.path().join("IMG_0001.HEIC");
        fs::write(&heic, b"heic bytes").unwrap();
        fs::write(dir.path().join("IMG_0001.JPG"), b"compatible copy").unwrap();
        let backup = dir.path().join("backups/00000-IMG_0001.HEIC");

        let out = convert_in_place(&heic, b"jpeg bytes", Some(&backup)).unwrap();
        assert_eq!(
            out,
            dir.path().join("IMG_0001 2.JPG"),
            "never clobbers, keeps case"
        );
        assert!(!heic.exists() && backup.exists());
        assert_eq!(fs::read(&out).unwrap(), b"jpeg bytes");

        let written = fingerprint(&out).ok();
        restore_converted(&backup, &heic, &out, written).unwrap();
        assert_eq!(fs::read(&heic).unwrap(), b"heic bytes");
        assert!(!out.exists() && !backup.exists());
    }

    #[test]
    fn undo_of_a_conversion_refuses_to_lose_newer_work() {
        let dir = tempfile::tempdir().unwrap();
        let heic = dir.path().join("a.heic");
        fs::write(&heic, b"heic").unwrap();
        let backup = dir.path().join("b/a.heic");
        let out = convert_in_place(&heic, b"jpeg", Some(&backup)).unwrap();
        let written = fingerprint(&out).ok();
        fs::write(&out, b"edited in Lightroom afterwards").unwrap();
        assert!(restore_converted(&backup, &heic, &out, written).is_err());
        assert!(backup.exists() && out.exists(), "nothing touched");
    }

    #[test]
    fn finds_the_video_half_of_a_live_photo() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("IMG_0002.HEIC");
        fs::write(&photo, b"x").unwrap();
        assert_eq!(live_photo_video(&photo), None);
        fs::write(dir.path().join("IMG_0002.MOV"), b"v").unwrap();
        assert_eq!(
            live_photo_video(&photo),
            Some(dir.path().join("IMG_0002.MOV"))
        );
    }
}
