// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use crate::{
    decode::{Diagnostic, Reason, error},
    ids::ManifestId,
};
use std::collections::{BTreeMap, HashMap, VecDeque};

/// Dependency projection of a structurally checked descriptor. The caller supplies
/// one current descriptor per version, from one consistent publication snapshot.
#[derive(Clone, Copy, Debug)]
pub struct PublishedRelease<'a> {
    pub manifest_id: ManifestId,
    pub requires: &'a [String],
}
#[derive(Debug, Eq, PartialEq)]
pub struct PublicationFailure<'a> {
    pub version_id: &'a str,
    pub diagnostic: Diagnostic,
}

/// Check the hypothetical replacement and all reverse-reachable published roots.
/// Storage and atomic publication are the caller's responsibility.
///
/// Work is O(V + E) after ordering the snapshot. Queues contain each version at
/// most once, there is no recursion, and retained manifest sets saturate at nine.
/// The supplied snapshot bounds all queues and graph storage. Versions may share
/// a manifest ID, so eight IDs need not mean eight versions.
pub fn validate_publication<'a>(
    version_id: &'a str,
    next: PublishedRelease<'a>,
    published: &BTreeMap<&'a str, PublishedRelease<'a>>,
) -> Result<Vec<&'a str>, PublicationFailure<'a>> {
    let mut snapshot = published.clone();
    snapshot.insert(version_id, next);
    let names: Vec<_> = snapshot.keys().copied().collect();
    let releases: Vec<_> = snapshot.values().copied().collect();
    let indexes: HashMap<_, _> = names.iter().enumerate().map(|(i, &n)| (n, i)).collect();
    let n = names.len();
    let mut reverse = vec![Vec::new(); n];
    let mut degree = vec![0usize; n];
    let mut missing = vec![false; n];
    for (i, release) in releases.iter().enumerate() {
        for required in release.requires {
            if let Some(&j) = indexes.get(required.as_str()) {
                reverse[j].push(i);
                degree[i] += 1;
            } else {
                missing[i] = true;
            }
        }
    }
    // Missing descendants take precedence over cycles and ticket-set size.
    let mut queue: VecDeque<_> = (0..n).filter(|&i| missing[i]).collect();
    while let Some(i) = queue.pop_front() {
        for &parent in &reverse[i] {
            if !missing[parent] {
                missing[parent] = true;
                queue.push_back(parent);
            }
        }
    }
    let root = indexes[version_id];
    let mut affected = vec![false; n];
    affected[root] = true;
    queue.push_back(root);
    while let Some(i) = queue.pop_front() {
        for &parent in &reverse[i] {
            if !affected[parent] {
                affected[parent] = true;
                queue.push_back(parent);
            }
        }
    }
    // Removing leaves leaves exactly cycles and their ancestors. During removal,
    // each acyclic closure is summarized once; nine IDs suffice to prove > eight.
    let mut manifests: Vec<Vec<ManifestId>> =
        releases.iter().map(|r| vec![r.manifest_id]).collect();
    queue.extend((0..n).filter(|&i| degree[i] == 0));
    while let Some(i) = queue.pop_front() {
        for &parent in &reverse[i] {
            // Distinct indexes: an edge to itself cannot be a removed leaf.
            for k in 0..manifests[i].len() {
                let id = manifests[i][k];
                if manifests[parent].len() < 9 && !manifests[parent].contains(&id) {
                    manifests[parent].push(id);
                }
            }
            degree[parent] -= 1;
            if degree[parent] == 0 {
                queue.push_back(parent);
            }
        }
    }
    let mut checked = Vec::new();
    for i in std::iter::once(root).chain((0..n).filter(|&i| i != root && affected[i])) {
        checked.push(names[i]);
        let reason = if missing[i] {
            Some(Reason::dependency_not_found)
        } else if degree[i] != 0 {
            Some(Reason::dependency_cycle)
        } else if manifests[i].len() > 8 {
            Some(Reason::ticket_set_too_large)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(PublicationFailure {
                version_id: names[i],
                diagnostic: error(reason, ""),
            });
        }
    }
    Ok(checked)
}
