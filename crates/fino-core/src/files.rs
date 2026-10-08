//! File-system side of optimization: discovering photos, writing atomically and
//! keeping everything a photographer relies on — dates, permissions and, on macOS, Finder
//! tags.

use std::collections::HashSet;
#[cfg(unix)]
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

/// Windows marks hidden and system files with attributes rather than a leading dot.
#[cfg(windows)]
fn hidden_by_attribute(entry: &fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    entry
        .metadata()
        .is_ok_and(|m| m.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0)
}

#[cfg(not(windows))]
fn hidden_by_attribute(_entry: &fs::DirEntry) -> bool {
    false
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
        if is_hidden(&path) || hidden_by_attribute(&entry) || kind.is_symlink() {
            continue;
        }
        let heic = crate::heic::CONVERSION_AVAILABLE && has_heic_extension(&path);
        if kind.is_dir() {
            walk(&path, root, jobs);
        } else if kind.is_file() && (has_jpeg_extension(&path) || heic) {
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
    /// Extended attributes: Finder tags, colour labels, "where from"…
    #[cfg(unix)]
    xattrs: Vec<(OsString, Vec<u8>)>,
}

impl Attributes {
    pub fn read(path: &Path) -> std::io::Result<Self> {
        let meta = fs::metadata(path)?;
        Ok(Self {
            modified: meta.modified()?,
            accessed: meta.accessed()?,
            created: meta.created().ok(),
            permissions: meta.permissions(),
            #[cfg(unix)]
            xattrs: xattr::list(path)
                .map(|names| {
                    names
                        .filter(|n| n != "com.apple.quarantine")
                        .filter_map(|n| xattr::get(path, &n).ok().flatten().map(|v| (n, v)))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    /// Best effort: a missing Finder tag must never fail an otherwise good write.
    /// Permissions go last: a read-only file still takes its dates first.
    pub fn apply(&self, path: &Path) -> std::io::Result<()> {
        #[cfg(unix)]
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
        fs::set_permissions(path, self.permissions.clone())
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

#[cfg(windows)]
fn set_creation_time(path: &Path, time: SystemTime) -> std::io::Result<()> {
    use std::os::windows::fs::{FileTimesExt, OpenOptionsExt};
    // Attribute access only: works on read-only files too.
    const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
    let file = fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .open(path)?;
    file.set_times(fs::FileTimes::new().set_created(time))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn set_creation_time(_path: &Path, _time: SystemTime) -> std::io::Result<()> {
    Ok(())
}

/// Moves `from` onto `to`, replacing it.
///
/// On Windows a replace fails where macOS succeeds: on a read-only target (made writable
/// first — callers restore its attributes afterwards) and while another process briefly
/// holds the file (antivirus, search indexer, thumbnail cache), which is retried.
#[cfg(windows)]
fn rename_over(from: &Path, to: &Path) -> std::io::Result<()> {
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_LOCK_VIOLATION: i32 = 33;
    let was_read_only = make_writable(to);
    let mut attempt = 0;
    let result = loop {
        // A file held open by another process clears up in moments; access denied may be a
        // real permission problem, so it gets fewer tries.
        let (limit, error) = match fs::rename(from, to) {
            Ok(()) => break Ok(()),
            Err(e) => match e.raw_os_error() {
                Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION) => (8, e),
                Some(ERROR_ACCESS_DENIED) => (2, e),
                _ => break Err(e),
            },
        };
        if attempt >= limit {
            break Err(error);
        }
        attempt += 1;
        std::thread::sleep(std::time::Duration::from_millis(40 * attempt));
    };
    if result.is_err() && was_read_only {
        set_read_only(to);
    }
    result
}

#[cfg(not(windows))]
fn rename_over(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

/// Clears Windows' read-only flag, which blocks replacing or deleting a file. Returns
/// whether it was set.
#[cfg(windows)]
#[allow(clippy::permissions_set_readonly_false)] // Windows: only the read-only attribute.
fn make_writable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let mut permissions = meta.permissions();
    if !permissions.readonly() {
        return false;
    }
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).is_ok()
}

/// Puts back a read-only flag `make_writable` cleared, when the replace did not happen.
#[cfg(windows)]
fn set_read_only(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let mut permissions = meta.permissions();
        permissions.set_readonly(true);
        let _ = fs::set_permissions(path, permissions);
    }
}

#[cfg(not(windows))]
fn make_writable(_path: &Path) -> bool {
    false
}

/// `fs::remove_file`, also for files Windows marks read-only.
pub fn remove_file(path: &Path) -> std::io::Result<()> {
    make_writable(path);
    fs::remove_file(path)
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
        drop(file);
        rename_over(&tmp, target)
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
    fs::hard_link(original, backup).or_else(|_| fs::copy(original, backup).map(|_| ()))?;
    // A backup must stay removable. (A hard link shares the original's flag — and the
    // original is about to be replaced and get its attributes back anyway.)
    make_writable(backup);
    Ok(())
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

/// `IMG_0001.HEIC` → `IMG_0001.JPG`, `photo.heic` → `photo.jpg`.
pub fn converted_name(source: &Path) -> PathBuf {
    let upper = source
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.chars().all(|c| c.is_ascii_uppercase()));
    source.with_extension(if upper { "JPG" } else { "jpg" })
}

/// Claims `path` — or `path 2`, `path 3`… — by creating an empty placeholder with
/// `create_new`, so two workers can never pick the same name. The caller writes over it.
pub fn reserve_unique(path: &Path) -> std::io::Result<PathBuf> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let candidates = std::iter::once(path.to_path_buf())
        .chain((2..).map(|n| path.with_file_name(format!("{stem} {n}{ext}"))));
    for candidate in candidates {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!("an unused name always exists")
}

/// Copies `from` to `to` through a temp file + rename, carrying dates and tags: a crash or
/// a full disk never leaves a truncated file under the final name.
pub fn copy_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    let attrs = Attributes::read(from)?;
    let tmp = temp_path(to);
    let copied = fs::copy(from, &tmp).and_then(|_| {
        make_writable(&tmp);
        rename_over(&tmp, to)
    });
    if copied.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    copied?;
    let _ = attrs.apply(to);
    Ok(())
}

/// Moves a file, across volumes if needed (atomic copy, then delete the source).
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_file(from, to)?;
    remove_file(from)
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
    let out = reserve_unique(&converted_name(source))?;
    if let Err(e) = write_atomic(&out, bytes) {
        let _ = fs::remove_file(&out);
        return Err(e);
    }
    let _ = attrs.apply(&out);
    let retired = match backup_to {
        Some(dest) => move_file(source, dest),
        None => remove_file(source),
    };
    if let Err(e) = retired {
        let _ = fs::remove_file(&out);
        return Err(e);
    }
    Ok(out)
}

/// Undo of `convert_in_place`: puts the original back and removes the JPEG — unless the
/// JPEG changed since Fino wrote it, or something new took the original's name. A JPEG the
/// user already deleted is fine: there is nothing left to protect.
pub fn restore_converted(
    backup: &Path,
    original: &Path,
    output: &Path,
    expected: Option<Fingerprint>,
) -> std::io::Result<()> {
    let output_present = output.exists();
    if let (Some(expected), true) = (expected, output_present) {
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
    if output_present {
        // The original is back, which is what undo promises; a JPEG that cannot be removed
        // is left beside it rather than turning a successful undo into a failure.
        let _ = remove_file(output);
    }
    Ok(())
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
    let dest = reserve_unique(dest)?;
    let written = write_atomic(&dest, bytes)
        .and_then(|_| Attributes::read(source))
        .and_then(|attrs| attrs.apply(&dest));
    if let Err(e) = written {
        let _ = fs::remove_file(&dest);
        return Err(e);
    }
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
    if rename_over(backup, path).is_err() {
        let bytes = fs::read(backup)?;
        write_atomic(path, &bytes)?;
        remove_file(backup)?;
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

    #[test]
    fn undo_still_works_after_the_user_deleted_the_converted_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let heic = dir.path().join("a.heic");
        fs::write(&heic, b"heic").unwrap();
        let backup = dir.path().join("b/a.heic");
        let out = convert_in_place(&heic, b"jpeg", Some(&backup)).unwrap();
        let written = fingerprint(&out).ok();
        fs::remove_file(&out).unwrap();
        restore_converted(&backup, &heic, &out, written).unwrap();
        assert_eq!(fs::read(&heic).unwrap(), b"heic");
    }

    #[test]
    fn concurrent_writers_never_share_a_name() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("IMG.JPG");
        let names: Vec<PathBuf> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| reserve_unique(&target).unwrap()))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let unique: HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), 8);
    }
}
