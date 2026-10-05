//! Where a session's photos came from, so a batch can be told apart in History days later.
//! Derived from the source paths already stored for every file: older sessions get one too.

use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Up to this many loose files from one folder are named one by one.
const FEW_FILES: usize = 3;
/// Shallower than this (`/`, `/Users`, `/Volumes`) a shared ancestor says nothing useful.
const MIN_COMMON_DEPTH: usize = 3;
/// Existence checks per session when looking for something to reveal in Finder.
const REVEAL_PROBES: usize = 20;

/// How the origin reads; the UI words it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OriginLabel {
    /// A few files from one folder: the first one's name and how many more.
    Files { first: String, more: usize },
    /// Everything came from one folder: its name and its parent's, since names like "JPG"
    /// or "Exportadas" mean little alone.
    Folder {
        parent: Option<String>,
        name: String,
    },
    /// Several folders under one that they share.
    Folders { name: String, count: usize },
    /// Nothing meaningful in common (different disks, or only `/Users`).
    Scattered,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Origin {
    #[serde(flatten)]
    pub label: OriginLabel,
    /// The folder in full, home shortened to `~`, for the tooltip.
    pub path: Option<String>,
    /// What "show in Finder" opens: a photo still in place, else the folder if it exists.
    pub reveal: Option<PathBuf>,
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Deepest folder containing every one of `dirs`.
fn common_ancestor<'a>(mut dirs: impl Iterator<Item = &'a Path>) -> PathBuf {
    let Some(first) = dirs.next() else {
        return PathBuf::new();
    };
    dirs.fold(first.to_path_buf(), |common, dir| {
        common
            .components()
            .zip(dir.components())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a)
            .collect()
    })
}

fn display(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.to_string_lossy()),
        None => path.to_string_lossy().into_owned(),
    }
}

/// Origin of a session from its files' source paths; `None` when it has none.
pub fn describe(paths: &[PathBuf], home: Option<&Path>) -> Option<Origin> {
    let first = paths.first()?;
    let dirs: BTreeSet<&Path> = paths.iter().filter_map(|p| p.parent()).collect();
    let common = common_ancestor(dirs.iter().copied());

    let (label, folder) = if dirs.len() == 1 && paths.len() <= FEW_FILES {
        let label = OriginLabel::Files {
            first: name_of(first),
            more: paths.len() - 1,
        };
        (label, Some(common.clone()))
    } else if dirs.len() == 1 {
        let parent = common
            .parent()
            .filter(|p| p.file_name().is_some())
            .map(name_of);
        let label = OriginLabel::Folder {
            parent,
            name: name_of(&common),
        };
        (label, Some(common.clone()))
    } else if common.components().count() < MIN_COMMON_DEPTH {
        (OriginLabel::Scattered, None)
    } else {
        let label = OriginLabel::Folders {
            name: name_of(&common),
            count: dirs.len(),
        };
        (label, Some(common.clone()))
    };

    let reveal = paths
        .iter()
        .take(REVEAL_PROBES)
        .find(|p| p.is_file())
        .cloned()
        .or_else(|| folder.as_ref().filter(|f| f.is_dir()).cloned());
    Some(Origin {
        label,
        path: folder.as_deref().map(|f| display(f, home)),
        reveal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    fn label(list: &[&str]) -> OriginLabel {
        describe(&paths(list), None).unwrap().label
    }

    #[test]
    fn a_few_loose_files_are_named() {
        assert_eq!(
            label(&["/Users/a/Desktop/IMG_1.HEIC", "/Users/a/Desktop/IMG_2.HEIC"]),
            OriginLabel::Files {
                first: "IMG_1.HEIC".into(),
                more: 1
            }
        );
        assert_eq!(
            label(&["/Users/a/Desktop/IMG_1.HEIC"]),
            OriginLabel::Files {
                first: "IMG_1.HEIC".into(),
                more: 0
            }
        );
    }

    #[test]
    fn one_folder_is_named_with_its_parent() {
        let list: Vec<String> = (0..10)
            .map(|i| format!("/Users/a/Fotos/Boda Ana/JPG/{i}.jpg"))
            .collect();
        let list: Vec<&str> = list.iter().map(String::as_str).collect();
        assert_eq!(
            label(&list),
            OriginLabel::Folder {
                parent: Some("Boda Ana".into()),
                name: "JPG".into()
            }
        );
    }

    #[test]
    fn several_folders_are_counted_under_the_one_they_share() {
        assert_eq!(
            label(&[
                "/Users/a/Proyectos/X/1.jpg",
                "/Users/a/Proyectos/Y/2.jpg",
                "/Users/a/Proyectos/Y/Z/3.jpg"
            ]),
            OriginLabel::Folders {
                name: "Proyectos".into(),
                count: 3
            }
        );
    }

    #[test]
    fn folders_sharing_only_a_shallow_ancestor_are_scattered() {
        assert_eq!(
            label(&["/Volumes/SD/1.jpg", "/Users/a/2.jpg"]),
            OriginLabel::Scattered
        );
        assert_eq!(
            label(&["/Users/a/1.jpg", "/Users/b/2.jpg"]),
            OriginLabel::Scattered
        );
        let origin = describe(&paths(&["/Volumes/SD/1.jpg", "/Users/a/2.jpg"]), None).unwrap();
        assert_eq!((origin.path, origin.reveal), (None, None));
    }

    #[test]
    fn no_files_no_origin() {
        assert_eq!(describe(&[], None), None);
    }

    #[test]
    fn tooltip_shortens_home_and_reveal_prefers_a_photo_still_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Boda");
        std::fs::create_dir(&folder).unwrap();
        let gone = folder.join("a.jpg");
        let kept = folder.join("b.jpg");
        std::fs::write(&kept, b"x").unwrap();

        let origin = describe(&[gone.clone(), kept.clone()], Some(dir.path())).unwrap();
        assert_eq!(origin.path.as_deref(), Some("~/Boda"));
        assert_eq!(origin.reveal, Some(kept.clone()));

        std::fs::remove_file(&kept).unwrap();
        let origin = describe(&[gone.clone(), kept], None).unwrap();
        assert_eq!(
            origin.reveal,
            Some(folder.clone()),
            "falls back to the folder"
        );

        std::fs::remove_dir(&folder).unwrap();
        assert_eq!(
            describe(&[gone], None).unwrap().reveal,
            None,
            "nothing left to show"
        );
    }

    #[test]
    fn wire_format_is_flat_and_tagged() {
        let origin = describe(&paths(&["/Users/a/Fotos/1.jpg"]), None).unwrap();
        let wire = serde_json::to_value(origin).unwrap();
        assert_eq!(wire["kind"], "files");
        assert_eq!(wire["first"], "1.jpg");
        assert_eq!(wire["more"], 0);
        assert_eq!(wire["path"], "/Users/a/Fotos");
    }
}
