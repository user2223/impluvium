// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

mod codegen;

use std::path::Path;

fn run() -> codegen::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    match args.as_slice() {
        [command] if command == "generate" => codegen::generate(&root),
        [command] if command == "check-drift" => {
            let issues = codegen::check_drift(&root)?;
            if issues.is_empty() {
                println!("Protocol outputs match the schema.");
                Ok(())
            } else {
                Err(format!("protocol output drift:\n{}", issues.join("\n")).into())
            }
        }
        _ => Err("usage: impluvium-protocol-xtask <generate|check-drift>".into()),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::codegen::{Scratch, Tree, checked_path, differences, snapshot, validate_configs};
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn compares_file_sets_and_bytes() {
        let before = Tree::from([
            (PathBuf::from("changed.rs"), b"old".to_vec()),
            (PathBuf::from("extra.rs"), Vec::new()),
        ]);
        let after = Tree::from([
            (PathBuf::from("changed.rs"), b"new".to_vec()),
            (PathBuf::from("missing.rs"), Vec::new()),
        ]);
        assert_eq!(
            differences("output", &before, &after),
            [
                "output/changed.rs: changed",
                "output/extra.rs: additional",
                "output/missing.rs: missing",
            ]
        );
        assert!(differences("output", &before, &before).is_empty());
    }

    #[test]
    fn snapshot_preserves_unknown_files_and_bytes() {
        let scratch = Scratch::new().unwrap();
        fs::create_dir(scratch.path().join("output")).unwrap();
        fs::write(scratch.path().join("output/unexpected.bin"), [0, 255]).unwrap();
        let before = snapshot(scratch.path(), "output").unwrap();
        assert_eq!(before[Path::new("unexpected.bin")], [0, 255]);
        assert_eq!(before, snapshot(scratch.path(), "output").unwrap());
        assert!(snapshot(scratch.path(), "absent").unwrap().is_empty());
    }

    #[test]
    fn rejects_paths_outside_the_root() {
        let scratch = Scratch::new().unwrap();
        assert!(checked_path(scratch.path(), Path::new("../outside")).is_err());
        assert!(checked_path(scratch.path(), Path::new("/outside")).is_err());
    }

    #[test]
    fn rejects_escaping_outputs_remote_plugins_and_module_dependencies() {
        let module = include_str!("../../buf.yaml");
        let generation = include_str!("../../buf.gen.yaml");
        assert!(validate_configs(module, generation).is_ok());
        assert!(
            validate_configs(
                module,
                &generation.replace("ts/src/generated", "../../outside")
            )
            .is_err()
        );
        assert!(
            validate_configs(
                module,
                &generation.replace("local: protoc-gen-es", "remote: buf.build/bufbuild/es")
            )
            .is_err()
        );
        assert!(
            validate_configs(
                module,
                &generation.replace("    opt: [filter=services]\n", "")
            )
            .is_err()
        );
        assert!(
            validate_configs(
                &format!("{module}\ndeps:\n  - buf.build/example/types\n"),
                generation
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_output_symlinks_and_linked_ancestors() {
        use std::os::unix::fs::symlink;

        let scratch = Scratch::new().unwrap();
        fs::create_dir(scratch.path().join("output")).unwrap();
        symlink("../absent", scratch.path().join("output/link")).unwrap();
        assert!(snapshot(scratch.path(), "output").is_err());
        assert!(checked_path(scratch.path(), Path::new("output/link/file")).is_err());
    }
}
