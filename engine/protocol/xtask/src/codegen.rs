// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub type Tree = BTreeMap<PathBuf, Vec<u8>>;

pub const PROTOCOL: &str = "engine/protocol";
pub const OUTPUTS: [&str; 3] = [
    "engine/crates/protocol/src/generated/buffa",
    "engine/crates/protocol/src/generated/connect",
    "engine/protocol/ts/src/generated",
];
const CONFIG_OUTPUTS: [&str; 5] = [
    "../crates/protocol/src/generated/buffa",
    "../crates/protocol/src/generated/buffa",
    "../crates/protocol/src/generated/connect",
    "../crates/protocol/src/generated/connect",
    "ts/src/generated",
];
const PLUGINS: [&str; 5] = [
    "protoc-gen-buffa",
    "protoc-gen-buffa-packaging",
    "protoc-gen-connect-rust",
    "protoc-gen-buffa-packaging",
    "protoc-gen-es",
];
static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Result<Self> {
        loop {
            let sequence = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "impluvium-codegen-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Reject links in ancestors as well as leaves before any output can be replaced.
pub fn checked_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut path = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(format!("unsafe relative path: {}", relative.display()).into());
        };
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(
                    format!("symlink is not an input or output: {}", path.display()).into(),
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

pub fn snapshot(root: &Path, relative: &str) -> Result<Tree> {
    let directory = checked_path(root, Path::new(relative))?;
    match fs::symlink_metadata(&directory) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Tree::new()),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.is_dir() => {
            return Err(format!("expected directory: {}", directory.display()).into());
        }
        Ok(_) => {}
    }
    let mut tree = Tree::new();
    collect(&directory, &directory, &mut tree)?;
    Ok(tree)
}

fn collect(base: &Path, directory: &Path, tree: &mut Tree) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(base, &path, tree)?;
        } else if kind.is_file() {
            tree.insert(path.strip_prefix(base)?.to_path_buf(), fs::read(&path)?);
        } else {
            return Err(format!("unsupported file type: {}", path.display()).into());
        }
    }
    Ok(())
}

fn write_tree(directory: &Path, tree: &Tree) -> Result<()> {
    fs::create_dir_all(directory)?;
    for (relative, bytes) in tree {
        let file = checked_path(directory, relative)?;
        fs::create_dir_all(file.parent().ok_or("file has no parent")?)?;
        fs::write(file, bytes)?;
    }
    Ok(())
}

pub fn copy_inputs(source: &Path, destination: &Path) -> Result<()> {
    let protocol = destination.join(PROTOCOL);
    fs::create_dir_all(&protocol)?;
    for name in ["buf.yaml", "buf.gen.yaml"] {
        let relative = Path::new(PROTOCOL).join(name);
        let file = checked_path(source, &relative)?;
        if !fs::symlink_metadata(&file)?.is_file() {
            return Err(format!("expected regular file: {}", file.display()).into());
        }
        fs::copy(file, protocol.join(name))?;
    }
    let schema = snapshot(source, "engine/protocol/proto")?;
    if schema.is_empty() {
        return Err("schema tree is empty".into());
    }
    write_tree(&protocol.join("proto"), &schema)
}

// This deliberately small configuration dialect keeps buf's cleanup inside the
// three owned trees. Extend it explicitly when the generation contract changes.
fn validate_config(source: &str) -> Result<()> {
    let actual: Vec<_> = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let mut expected = vec![
        "version: v2".into(),
        "clean: true".into(),
        "plugins:".into(),
    ];
    for (index, (plugin, output)) in PLUGINS.iter().zip(CONFIG_OUTPUTS).enumerate() {
        expected.push(format!("- local: {plugin}"));
        expected.push(format!("out: {output}"));
        let options: &[&str] = match index {
            0 => &["opt: [views=true, json=true]"],
            1 => &["strategy: all"],
            2 => &["opt: [buffa_module=crate::proto, gate_client_feature=client]"],
            3 => &["strategy: all", "opt: [filter=services]"],
            4 => &["opt: [target=ts, import_extension=js]"],
            _ => unreachable!(),
        };
        expected.extend(options.iter().map(|line| (*line).to_owned()));
    }
    expected.extend(["inputs:".into(), "- directory: proto".into()]);
    if actual != expected {
        return Err(
            "generation plugins, options, outputs or schema input differ from the contract".into(),
        );
    }
    Ok(())
}

pub fn validate_configs(module: &str, generation: &str) -> Result<()> {
    let lines: Vec<_> = module
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    if lines
        != [
            "version: v2",
            "modules:",
            "- path: proto",
            "lint:",
            "use:",
            "- STANDARD",
            "breaking:",
            "use:",
            "- WIRE_JSON",
        ]
    {
        return Err(
            "unsupported module configuration; expected local proto module without dependencies"
                .into(),
        );
    }
    validate_config(generation)
}

pub fn differences(output: &str, committed: &Tree, generated: &Tree) -> Vec<String> {
    let names: BTreeSet<_> = committed.keys().chain(generated.keys()).collect();
    names
        .into_iter()
        .filter_map(|name| {
            let reason = match (committed.get(name), generated.get(name)) {
                (None, Some(_)) => "missing",
                (Some(_), None) => "additional",
                (Some(left), Some(right)) if left != right => "changed",
                _ => return None,
            };
            Some(format!("{output}/{}: {reason}", name.display()))
        })
        .collect()
}

fn fresh_outputs(root: &Path) -> Result<Scratch> {
    let scratch = Scratch::new()?;
    copy_inputs(root, scratch.path())?;
    let protocol = scratch.path().join(PROTOCOL);
    validate_configs(
        &fs::read_to_string(protocol.join("buf.yaml"))?,
        &fs::read_to_string(protocol.join("buf.gen.yaml"))?,
    )?;
    let status = Command::new("buf")
        .args(["generate", "--template", "buf.gen.yaml"])
        .current_dir(&protocol)
        .status()?;
    if !status.success() {
        return Err(format!("buf generate failed: {status}").into());
    }
    for (index, output) in OUTPUTS.iter().enumerate() {
        let tree = snapshot(scratch.path(), output)?;
        let has_entry = if index < 2 {
            tree.contains_key(Path::new("mod.rs"))
        } else {
            tree.keys()
                .any(|path| path.extension().is_some_and(|ext| ext == "ts"))
        };
        if !has_entry {
            return Err(format!("generator produced no entry point in {output}").into());
        }
    }
    Ok(scratch)
}

pub fn check_drift(root: &Path) -> Result<Vec<String>> {
    let root = root.canonicalize()?;
    let scratch = fresh_outputs(&root)?;
    let mut issues = Vec::new();
    for output in OUTPUTS {
        issues.extend(differences(
            output,
            &snapshot(&root, output)?,
            &snapshot(scratch.path(), output)?,
        ));
    }
    Ok(issues)
}

pub fn generate(root: &Path) -> Result<()> {
    let root = root.canonicalize()?;
    // Preflight all destinations before invoking any tool or changing a tree.
    for output in OUTPUTS {
        snapshot(&root, output)?;
    }
    let scratch = fresh_outputs(&root)?;
    for output in OUTPUTS {
        let tree = snapshot(scratch.path(), output)?;
        let destination = checked_path(&root, Path::new(output))?;
        match fs::remove_dir_all(&destination) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        write_tree(&destination, &tree)?;
    }
    Ok(())
}
