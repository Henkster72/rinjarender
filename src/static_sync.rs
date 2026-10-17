use crate::manifest::{ArtifactState, FileRecord, StaticFileState};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub type SyncState = BTreeMap<String, StaticFileState>;

pub fn sync(
    source_root: &Path,
    output_root: &Path,
) -> Result<Vec<FileRecord>, Box<dyn std::error::Error>> {
    let (records, _) = sync_with_state(source_root, output_root, None, true)?;
    Ok(records)
}

pub fn sync_with_state(
    source_root: &Path,
    output_root: &Path,
    previous: Option<&BTreeMap<String, StaticFileState>>,
    verify: bool,
) -> Result<(Vec<FileRecord>, SyncState), Box<dyn std::error::Error>> {
    let mut records = Vec::new();
    let mut state = BTreeMap::new();
    if !source_root.is_dir() {
        return Ok((records, state));
    }
    visit(
        source_root,
        source_root,
        output_root,
        previous,
        verify,
        &mut records,
        &mut state,
    )?;
    Ok((records, state))
}

pub fn stale_candidates(
    source_root: &Path,
    _output_root: &Path,
    previous: Option<&BTreeMap<String, StaticFileState>>,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let Some(previous) = previous else {
        return Ok(Vec::new());
    };
    let mut source_paths = std::collections::BTreeSet::new();
    if source_root.is_dir() {
        collect_relative(source_root, source_root, &mut source_paths)?;
    }
    let mut stale = Vec::new();
    for relative in previous.keys() {
        if !source_paths.contains(relative) {
            stale.push(
                PathBuf::from("static")
                    .join(relative)
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    stale.sort();
    Ok(stale)
}

fn collect_relative(
    root: &Path,
    dir: &Path,
    paths: &mut std::collections::BTreeSet<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for item in fs::read_dir(dir)? {
        let path = item?.path();
        if path.is_dir() {
            collect_relative(root, &path, paths)?;
        } else {
            paths.insert(
                path.strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}
fn visit(
    root: &Path,
    dir: &Path,
    output_root: &Path,
    previous: Option<&BTreeMap<String, StaticFileState>>,
    verify: bool,
    records: &mut Vec<FileRecord>,
    state: &mut BTreeMap<String, StaticFileState>,
) -> Result<(), Box<dyn std::error::Error>> {
    for item in fs::read_dir(dir)? {
        let path = item?.path();
        if path.is_dir() {
            visit(root, &path, output_root, previous, verify, records, state)?;
            continue;
        }
        let rel = path.strip_prefix(root)?;
        let target = output_root.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let source_meta = fs::metadata(&path)?;
        let source_size = source_meta.len();
        let source_mtime_ns = mtime_ns(&source_meta)?;
        let output_meta = fs::metadata(&target).ok();
        let rel_key = rel.to_string_lossy().replace('\\', "/");
        let trusted = if verify {
            None
        } else {
            previous
                .and_then(|items| items.get(&rel_key))
                .filter(|old| {
                    old.size == source_size
                        && old.mtime_ns == source_mtime_ns
                        && output_meta.as_ref().is_some_and(|meta| {
                            meta.len() == old.output_size
                                && mtime_ns(meta).ok() == Some(old.output_mtime_ns)
                        })
                })
        };
        let (digest, state_kind) = if let Some(old) = trusted {
            (old.sha256.clone(), ArtifactState::Unchanged)
        } else {
            let digest = hash_file(&path)?;
            let state_kind = match output_meta {
                None => {
                    fs::copy(&path, &target)?;
                    ArtifactState::New
                }
                Some(_) if hash_file(&target)? == digest => ArtifactState::Unchanged,
                Some(_) => {
                    fs::copy(&path, &target)?;
                    ArtifactState::Changed
                }
            };
            (digest, state_kind)
        };
        let output_meta = fs::metadata(&target)?;
        records.push(FileRecord {
            path: PathBuf::from("static")
                .join(rel)
                .to_string_lossy()
                .replace('\\', "/"),
            source: Some(path.to_string_lossy().into_owned()),
            kind: "static".into(),
            bytes: source_size,
            sha256: digest,
            state: state_kind,
        });
        state.insert(
            rel_key.clone(),
            StaticFileState {
                source: path.to_string_lossy().into_owned(),
                output: target.to_string_lossy().into_owned(),
                size: source_size,
                mtime_ns: source_mtime_ns,
                sha256: records.last().unwrap().sha256.clone(),
                output_size: output_meta.len(),
                output_mtime_ns: mtime_ns(&output_meta)?,
            },
        );
    }
    Ok(())
}

fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut reader = BufReader::new(fs::File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn mtime_ns(metadata: &fs::Metadata) -> std::io::Result<u128> {
    Ok(metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn roots() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "jnja-static-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (root.join("source"), root.join("output"))
    }

    #[test]
    fn detects_new_changed_same_size_and_unchanged_files() {
        let (source, output) = roots();
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("new.txt"), "new").unwrap();
        fs::write(source.join("same.txt"), "abc").unwrap();
        fs::write(source.join("different.txt"), "long").unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("same.txt"), "abc").unwrap();
        fs::write(output.join("different.txt"), "xxxx").unwrap();
        let first = sync(&source, &output).unwrap();
        assert_eq!(
            first
                .iter()
                .find(|x| x.path.ends_with("new.txt"))
                .unwrap()
                .state,
            ArtifactState::New
        );
        assert_eq!(
            first
                .iter()
                .find(|x| x.path.ends_with("same.txt"))
                .unwrap()
                .state,
            ArtifactState::Unchanged
        );
        assert_eq!(
            first
                .iter()
                .find(|x| x.path.ends_with("different.txt"))
                .unwrap()
                .state,
            ArtifactState::Changed
        );
        fs::write(source.join("same.txt"), "xyz").unwrap();
        let second = sync(&source, &output).unwrap();
        assert_eq!(
            second
                .iter()
                .find(|x| x.path.ends_with("same.txt"))
                .unwrap()
                .state,
            ArtifactState::Changed
        );
        fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    #[test]
    fn trusted_state_avoids_content_reads_but_detects_output_changes() {
        let (source, output) = roots();
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(source.join("asset.txt"), "abc").unwrap();
        let (first, state) = sync_with_state(&source, &output, None, true).unwrap();
        assert_eq!(first[0].state, ArtifactState::New);
        let (second, _) = sync_with_state(&source, &output, Some(&state), false).unwrap();
        assert_eq!(second[0].state, ArtifactState::Unchanged);
        std::thread::sleep(std::time::Duration::from_secs(1));
        fs::write(output.join("asset.txt"), "xyz").unwrap();
        let (third, _) = sync_with_state(&source, &output, Some(&state), false).unwrap();
        assert_eq!(third[0].state, ArtifactState::Changed);
        fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    #[test]
    fn unknown_legacy_output_is_not_stale_without_previous_ownership() {
        let (source, output) = roots();
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("legacy.jpg"), "legacy").unwrap();
        assert!(stale_candidates(&source, &output, None).unwrap().is_empty());
        fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    #[test]
    fn removed_previous_owned_static_file_is_stale() {
        let (source, output) = roots();
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("foo.css"), "old").unwrap();
        let mut previous = BTreeMap::new();
        previous.insert(
            "foo.css".into(),
            StaticFileState {
                output: output.join("foo.css").to_string_lossy().into_owned(),
                ..StaticFileState::default()
            },
        );
        assert_eq!(
            stale_candidates(&source, &output, Some(&previous)).unwrap(),
            vec!["static/foo.css"]
        );
        fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    #[test]
    fn previously_owned_missing_local_output_is_still_stale() {
        let (source, output) = roots();
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&output).unwrap();
        let mut previous = BTreeMap::new();
        previous.insert("remote-only.css".into(), StaticFileState::default());
        assert_eq!(
            stale_candidates(&source, &output, Some(&previous)).unwrap(),
            vec!["static/remote-only.css"]
        );
        fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    #[test]
    fn mixed_owned_and_unknown_static_files_only_stale_owned_file() {
        let (source, output) = roots();
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("foo.css"), "old").unwrap();
        fs::write(output.join("legacy.jpg"), "legacy").unwrap();
        let mut previous = BTreeMap::new();
        previous.insert("foo.css".into(), StaticFileState::default());
        assert_eq!(
            stale_candidates(&source, &output, Some(&previous)).unwrap(),
            vec!["static/foo.css"]
        );
        fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }
}
