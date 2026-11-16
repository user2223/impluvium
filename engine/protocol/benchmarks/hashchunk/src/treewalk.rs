// SPDX-License-Identifier: 0BSD
//! Directory walk: regular files sorted by raw path bytes; symlinks are counted, never followed.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct FileEntry {
    pub rel: String,
    pub abs: PathBuf,
    pub size: u64,
    pub mode: u32,
}

#[derive(Default)]
pub struct Tree {
    pub files: Vec<FileEntry>,
    pub symlinks: Vec<(String, String)>,
}

fn rec(dir: &Path, prefix: &str, t: &mut Tree) -> io::Result<()> {
    for e in fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let md = fs::symlink_metadata(e.path())?;
        let ft = md.file_type();
        if ft.is_dir() {
            rec(&e.path(), &rel, t)?;
        } else if ft.is_symlink() {
            let target = fs::read_link(e.path())?.to_string_lossy().into_owned();
            t.symlinks.push((rel, target));
        } else if ft.is_file() {
            #[cfg(unix)]
            let mode = std::os::unix::fs::PermissionsExt::mode(&md.permissions()) & 0o7777;
            #[cfg(not(unix))]
            let mode = 0o644;
            t.files.push(FileEntry {
                rel,
                abs: e.path(),
                size: md.len(),
                mode,
            });
        }
    }
    Ok(())
}

/// Walks a directory, or treats a single regular file as a one-entry tree with the relative name "file".
pub fn walk(root: &Path) -> io::Result<Tree> {
    let md = fs::symlink_metadata(root)?;
    let mut t = Tree::default();
    if md.is_file() {
        t.files.push(FileEntry {
            rel: "file".into(),
            abs: root.to_path_buf(),
            size: md.len(),
            mode: 0o644,
        });
        return Ok(t);
    }
    rec(root, "", &mut t)?;
    t.files
        .sort_by(|a, b| a.rel.as_bytes().cmp(b.rel.as_bytes()));
    t.symlinks.sort();
    Ok(t)
}
