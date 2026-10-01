use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

fn io(what: &str, path: &Path, e: &std::io::Error) -> Error {
    Error::new(format!("{what} {}: {e}", path.display()))
}

/// Writes `bytes` to `path` through a temp file in the same directory and a rename. A symlink
/// at `path` is followed: the file it points to is replaced, the link stays.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let real = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = real.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).map_err(|e| io("can't create", dir, &e))?;
    let name = real
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.warpify-{}.tmp", std::process::id()));
    fs::write(&tmp, bytes).map_err(|e| io("can't write", &tmp, &e))?;
    fs::rename(&tmp, &real).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io("can't replace", &real, &e)
    })
}

/// The file's text, `None` when it does not exist.
pub(crate) fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io("can't read", path, &e)),
    }
}

/// `config.kdl` -> `config.kdl.warpify.bak`
pub(crate) fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".warpify.bak");
    path.with_file_name(name)
}

/// Copies `path` to its backup unless a backup already exists.
pub(crate) fn backup_once(path: &Path) -> Result<Option<PathBuf>> {
    let bak = backup_path(path);
    if !path.exists() || bak.exists() {
        return Ok(None);
    }
    fs::copy(path, &bak).map_err(|e| io("can't back up to", &bak, &e))?;
    Ok(Some(bak))
}

pub(crate) fn remove_if_exists(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io("can't remove", path, &e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_dirs_replaces_and_leaves_no_temp() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a/b/f.txt");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "two");
        assert_eq!(fs::read_dir(p.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn atomic_write_keeps_a_symlink() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("real.txt");
        fs::write(&real, "x").unwrap();
        let link = t.path().join("link.txt");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomic(&link, b"y").unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "y");
    }

    #[test]
    fn backup_is_made_once() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("config.kdl");
        fs::write(&p, "orig").unwrap();
        assert!(backup_once(&p).unwrap().is_some());
        fs::write(&p, "changed").unwrap();
        assert!(backup_once(&p).unwrap().is_none());
        assert_eq!(
            fs::read_to_string(t.path().join("config.kdl.warpify.bak")).unwrap(),
            "orig"
        );
    }
}
