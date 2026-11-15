// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

#[path = "../src/codegen.rs"]
mod codegen;

use codegen::{OUTPUTS, Result, Scratch, Tree, check_drift, copy_inputs, generate, snapshot};
use std::fs;
use std::path::Path;

fn fixture() -> Result<Scratch> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let scratch = Scratch::new()?;
    copy_inputs(&source, scratch.path())?;
    for output in OUTPUTS {
        let tree = snapshot(&source, output)?;
        assert!(!tree.is_empty(), "checked-in output is missing: {output}");
        for (relative, bytes) in tree {
            let file = scratch.path().join(output).join(relative);
            fs::create_dir_all(file.parent().unwrap())?;
            fs::write(file, bytes)?;
        }
    }
    Ok(scratch)
}

fn outputs(root: &Path) -> Result<Vec<Tree>> {
    OUTPUTS
        .iter()
        .map(|output| snapshot(root, output))
        .collect()
}

fn contains(tree: &Tree, needle: &str) -> bool {
    tree.values()
        .any(|bytes| String::from_utf8_lossy(bytes).contains(needle))
}

#[test]
fn unchanged_generation_is_deterministic() -> Result<()> {
    let scratch = fixture()?;
    let committed = outputs(scratch.path())?;
    generate(scratch.path())?;
    assert_eq!(committed, outputs(scratch.path())?);
    generate(scratch.path())?;
    assert_eq!(committed, outputs(scratch.path())?);
    assert!(check_drift(scratch.path())?.is_empty());
    for bytes in snapshot(scratch.path(), OUTPUTS[2])?.values() {
        let text = String::from_utf8_lossy(bytes);
        for line in text.lines().filter(|line| line.starts_with("import ")) {
            if let Some((_, import)) = line.split_once("from \"")
                && import.starts_with('.')
            {
                assert!(import.ends_with(".js\";"), "extensionless import: {line}");
            }
        }
    }
    Ok(())
}

#[test]
fn detects_changed_missing_and_additional_files_without_repair() -> Result<()> {
    for output in OUTPUTS {
        for mutation in ["changed", "missing", "additional"] {
            let scratch = fixture()?;
            let tree = snapshot(scratch.path(), output)?;
            let relative = tree.keys().next().unwrap();
            let file = scratch.path().join(output).join(relative);
            match mutation {
                "changed" => fs::write(&file, b"deliberate drift")?,
                "missing" => fs::remove_file(&file)?,
                "additional" => {
                    fs::write(scratch.path().join(output).join("unexpected.txt"), b"extra")?;
                }
                _ => unreachable!(),
            }
            let before = outputs(scratch.path())?;
            let issues = check_drift(scratch.path())?;
            assert!(
                issues.iter().any(|issue| {
                    issue.starts_with(output) && issue.ends_with(&format!(": {mutation}"))
                }),
                "unexpected drift report: {issues:?}"
            );
            assert_eq!(before, outputs(scratch.path())?);
        }
    }
    Ok(())
}

#[test]
fn field_change_reaches_rust_messages_and_typescript() -> Result<()> {
    let scratch = fixture()?;
    let file = scratch
        .path()
        .join("engine/protocol/proto/impluvium/v2/auth.proto");
    let source = fs::read_to_string(&file)?;
    let marker = "  string device_name = 1;";
    assert_eq!(source.matches(marker).count(), 1);
    fs::write(
        &file,
        source.replace(marker, &format!("{marker}\n  string codegen_probe = 100;")),
    )?;
    let before = outputs(scratch.path())?;
    assert!(!check_drift(scratch.path())?.is_empty());
    assert_eq!(before, outputs(scratch.path())?);
    generate(scratch.path())?;
    assert!(contains(
        &snapshot(scratch.path(), OUTPUTS[0])?,
        "codegen_probe"
    ));
    assert!(contains(
        &snapshot(scratch.path(), OUTPUTS[2])?,
        "codegenProbe"
    ));
    Ok(())
}

#[test]
fn service_change_replaces_rust_service_and_typescript_descriptor() -> Result<()> {
    let scratch = fixture()?;
    let file = scratch
        .path()
        .join("engine/protocol/proto/impluvium/v2/auth.proto");
    let source = fs::read_to_string(&file)?;
    assert_eq!(source.matches("service AuthService {").count(), 1);
    fs::write(
        &file,
        source.replace("service AuthService {", "service CodegenProbeService {"),
    )?;
    let before = outputs(scratch.path())?;
    assert!(!check_drift(scratch.path())?.is_empty());
    assert_eq!(before, outputs(scratch.path())?);
    generate(scratch.path())?;
    let connect = snapshot(scratch.path(), OUTPUTS[1])?;
    assert!(contains(&connect, "CodegenProbeServiceClient"));
    assert!(contains(
        &connect,
        "/impluvium.v2.CodegenProbeService/StartPairing"
    ));
    assert!(!contains(&connect, "AuthServiceClient"));
    let typescript = snapshot(scratch.path(), OUTPUTS[2])?;
    assert!(contains(&typescript, "CodegenProbeService"));
    assert!(!contains(&typescript, "export const AuthService"));
    Ok(())
}
