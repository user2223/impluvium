#![cfg(test)]
extern crate test_generator;

use std::path::{Path, PathBuf};

use test_generator::test_resources;
use tokio::io::SimplexStream;

use crate::manifest::{generate_manifest_rusty, ManifestWriterFactory};

/// This crate's directory, relative to the workspace root.
const CRATE_DIR: &str = "libraries/droplet";

/// `test_resources` expands its glob at compile time, relative to the directory
/// rustc runs in (the workspace root), while the tests run in the crate
/// directory. Resolve the generated path against the crate instead. The prefix
/// is removed component by component, so Windows separators work as well.
fn resource_path(resource: &str) -> PathBuf {
    let relative = Path::new(resource)
        .strip_prefix(CRATE_DIR)
        .unwrap_or_else(|_| panic!("test resource {resource} is not below {CRATE_DIR}"));
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test_resources("libraries/droplet/testfiles/**/*.7z")]
fn manifest_gen(resource: &str) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");

    runtime.block_on(async move {
        let filepath = resource_path(resource);
        let filepath = filepath.as_path();
        let manifest = generate_manifest_rusty(
            filepath,
            |_| {},
            |message| {
                println!("({}) {}", filepath.display(), message);
            },
            None::<&dyn ManifestWriterFactory<Writer = SimplexStream>>, // Dummy type signature, not actually used
            None,
        )
        .await
        .unwrap_or_else(|err| {
            panic!(
                "failed to generate manifest for {}: {:?}",
                filepath.display(),
                err
            )
        });

        let first_chunk = manifest
            .chunks
            .values()
            .next()
            .expect("no chunks generated");
        let first_chunk_length = first_chunk.files.len();
        if first_chunk_length == 0 {
            panic!("{} has no files in manifest", filepath.display());
        }
    });
}
