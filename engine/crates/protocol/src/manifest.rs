// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Manifest v3 semantics in manifest.md §10 order and deterministic production.
//! Decoded-model validation does not establish authentication. `verify` applies
//! the envelope, bounded payload parser and semantic checks in their full order.

pub use crate::proto::impluvium::manifest::v3::{
    Chunk, Chunking, ChunkingAlgorithm, Entry, EntryType, HashAlgorithm, Header, Manifest,
};
use crate::{
    decode::{self, Diagnostic, InputLimit, Reason, error},
    envelope, fastcdc,
    hash::Algorithm,
    keys::{Content, TrustedKeySet},
    limits,
    path::{self, WindowsVariant},
};
use buffa::Message;
use std::collections::{BTreeMap, HashMap, HashSet};

pub const MAX_ENTRIES: usize = 2_000_000;
pub const MAX_CHUNKS: usize = 4_000_000;
pub const MAX_CHUNK_BYTES: u64 = 16_777_216;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Profile {
    #[default]
    Base,
    WindowsPublication,
    WindowsInstallation,
}

/// The manifest is owned by the caller; successful validation returns warnings only.
pub fn validate(manifest: &Manifest, profile: Profile) -> Result<Vec<Diagnostic>, Diagnostic> {
    use Reason::*;
    // Raw parsers check these before decoding. Repeat model bounds before allocating
    // reference/ancestor sets, also for callers constructing generated types directly.
    model_limits(manifest)?;
    if manifest.created_at.is_set() {
        check_timestamp(&manifest.created_at)?;
    }
    if manifest.format == 0 {
        return Err(error(missing_field, "Manifest.format"));
    }
    if manifest.format != 3 {
        return Err(error(unsupported_manifest_format, "Manifest.format"));
    }
    for (value, field) in [
        (&manifest.game_id, "Manifest.game_id"),
        (&manifest.version_id, "Manifest.version_id"),
    ] {
        if !uuid(value) {
            return Err(error(invalid_uuid, field));
        }
    }
    if !manifest.created_at.is_set() {
        return Err(error(missing_field, "Manifest.created_at"));
    }
    if !manifest.header.is_set() {
        return Err(error(missing_header, "Manifest.header"));
    }
    let header = &manifest.header;
    let algorithm = Algorithm::from_wire(header.hash_algorithm.to_i32())?;
    if !header.chunking.is_set() {
        return Err(error(missing_chunking, "Header.chunking"));
    }
    if header.chunking.algorithm.to_i32() == 0 {
        return Err(error(missing_field, "Chunking.algorithm"));
    }
    for feature in &manifest.required_features {
        if !feature_name(feature) {
            return Err(error(invalid_name, "Manifest.required_features"));
        }
    }
    if manifest.required_features.windows(2).any(|w| w[0] >= w[1]) {
        return Err(error(
            unsorted_required_features,
            "Manifest.required_features",
        ));
    }
    if !manifest.required_features.is_empty() {
        return Err(error(
            unknown_required_feature,
            "Manifest.required_features",
        ));
    }
    for (i, chunk) in manifest.chunks.iter().enumerate() {
        if chunk.id.len() != 32 {
            return Err(error(invalid_chunk_id, "Chunk.id"));
        }
        if chunk.size == 0 {
            return Err(error(chunk_size_zero, "Chunk.size"));
        }
        if chunk.size > MAX_CHUNK_BYTES {
            return Err(error(chunk_size_too_large, "Chunk.size"));
        }
        if i > 0 && manifest.chunks[i - 1].id >= chunk.id {
            return Err(error(unsorted_chunks, "Manifest.chunks"));
        }
    }
    let mut referenced = vec![false; manifest.chunks.len()];
    let mut paths = HashSet::new();
    // Total overflow belongs to step 15, after every entry's own checks.
    let mut total = Some(0u64);
    for (i, entry) in manifest.entries.iter().enumerate() {
        path::validate_base(&entry.path, "Entry.path")?;
        if i > 0 && manifest.entries[i - 1].path >= entry.path {
            return Err(error(unsorted_entries, "Manifest.entries"));
        }
        for (end, _) in entry.path.match_indices('/') {
            if paths.contains(&entry.path[..end]) {
                return Err(error(path_parent_conflict, "Entry.path"));
            }
        }
        paths.insert(entry.path.as_str());
        let kind = entry.r#type.to_i32();
        if !(1..=3).contains(&kind) {
            return Err(error(invalid_entry_type, "Entry.type"));
        }
        if kind != 1 && entry.size != 0 {
            return Err(error(invalid_entry_fields, "Entry.size"));
        }
        if kind == 1 {
            if entry.hash.len() != 32 {
                return Err(error(invalid_hash_length, "Entry.hash"));
            }
        } else if !entry.hash.is_empty() {
            return Err(error(invalid_entry_fields, "Entry.hash"));
        }
        if kind != 2 && !entry.symlink_target.is_empty() {
            return Err(error(invalid_entry_fields, "Entry.symlink_target"));
        }
        if kind != 1 || entry.size == 0 {
            if !entry.chunk_indexes.is_empty() {
                return Err(error(invalid_entry_fields, "Entry.chunk_indexes"));
            }
        } else if entry.chunk_indexes.is_empty() {
            return Err(error(invalid_entry_fields, "Entry.chunk_indexes"));
        }
        if entry.mode > 0o777 || (kind == 2 && entry.mode != 0) {
            return Err(error(invalid_mode, "Entry.mode"));
        }
        if kind == 1 {
            if entry.size == 0 {
                if entry.hash.as_slice() != algorithm.digest(b"") {
                    return Err(error(empty_file_hash_mismatch, "Entry.hash"));
                }
            } else {
                // All indexes precede the sum, including a later invalid index.
                for &index in &entry.chunk_indexes {
                    if index as usize >= manifest.chunks.len() {
                        return Err(error(chunk_index_out_of_range, "Entry.chunk_indexes"));
                    }
                }
                let mut sum = 0u64;
                for &index in &entry.chunk_indexes {
                    let index = index as usize;
                    sum = checked_size_add(sum, manifest.chunks[index].size)?;
                    referenced[index] = true;
                }
                if sum != entry.size {
                    return Err(error(chunk_size_mismatch, "Entry.size"));
                }
                if entry.chunk_indexes.len() == 1
                    && manifest.chunks[entry.chunk_indexes[0] as usize].id != entry.hash
                {
                    return Err(error(single_chunk_hash_mismatch, "Entry.hash"));
                }
            }
            total = total.and_then(|sum| sum.checked_add(entry.size));
        }
        if kind == 2 {
            path::validate_symlink(&entry.path, &entry.symlink_target)?;
        }
    }
    let total = total.ok_or(error(size_overflow, ""))?;
    if total != manifest.total_size {
        return Err(error(total_size_mismatch, "Manifest.total_size"));
    }
    if referenced.contains(&false) {
        return Err(error(unreferenced_chunk, "Manifest.chunks"));
    }
    match profile {
        Profile::Base => Ok(Vec::new()),
        Profile::WindowsPublication => {
            path::validate_windows(&manifest.entries, WindowsVariant::Publication)
        }
        Profile::WindowsInstallation => {
            path::validate_windows(&manifest.entries, WindowsVariant::Installation)
        }
    }
}

/// Check arithmetic before adding either semantic size sum.
pub fn checked_size_add(sum: u64, size: u64) -> Result<u64, Diagnostic> {
    sum.checked_add(size)
        .ok_or(error(Reason::size_overflow, ""))
}

/// The enclosing input must already have passed authentication if acceptance is intended.
pub fn decode_payload(
    bytes: &[u8],
    profile: Profile,
) -> Result<(Manifest, Vec<Diagnostic>), Diagnostic> {
    let manifest =
        decode::decode_binary::<Manifest>(bytes, InputLimit::payload(limits::MANIFEST_BYTES))?;
    let warnings = validate(&manifest, profile)?;
    Ok((manifest, warnings))
}

/// Full envelope.md §7 verification for a manifest under pinned content keys.
/// IDs must still be computed over the original payload, never a re-encoding.
pub fn verify(
    bytes: &[u8],
    trusted: &TrustedKeySet<Content>,
    profile: Profile,
) -> Result<(Manifest, Vec<Diagnostic>), Diagnostic> {
    let authenticated = envelope::authenticate(bytes, envelope::MANIFEST, trusted)?;
    decode_payload(authenticated.payload(), profile)
}

pub(crate) fn uuid(value: &str) -> bool {
    value.len() == 36
        && value != "00000000-0000-0000-0000-000000000000"
        && value.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        })
}
pub(crate) fn feature_name(value: &str) -> bool {
    let Some((segments, version)) = value.rsplit_once('.') else {
        return false;
    };
    let Some(digits) = version.strip_prefix('v') else {
        return false;
    };
    !digits.is_empty()
        && (b'1'..=b'9').contains(&digits.as_bytes()[0])
        && digits.bytes().all(|c| c.is_ascii_digit())
        && !value.is_empty()
        && value.len() <= 128
        && segments.split('.').all(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
        })
}
fn check_timestamp(timestamp: &buffa_types::google::protobuf::Timestamp) -> Result<(), Diagnostic> {
    if !(-62_135_596_800..=253_402_300_799).contains(&timestamp.seconds)
        || !(0..1_000_000_000).contains(&timestamp.nanos)
    {
        Err(error(Reason::invalid_timestamp, "Manifest.created_at"))
    } else {
        Ok(())
    }
}
fn value_limit(size: usize, max: usize, field: &'static str) -> Result<(), Diagnostic> {
    if size > max {
        Err(error(Reason::byte_limit_exceeded, field))
    } else {
        Ok(())
    }
}
fn element_limit(size: usize, max: usize, field: &'static str) -> Result<(), Diagnostic> {
    if size > max {
        Err(error(Reason::element_limit_exceeded, field))
    } else {
        Ok(())
    }
}
fn model_limits(manifest: &Manifest) -> Result<(), Diagnostic> {
    model_limits_with_size(manifest, |model| model.try_encoded_len())
}

fn model_limits_with_size(
    manifest: &Manifest,
    encoded_len: impl FnOnce(&Manifest) -> Result<u32, buffa::EncodeError>,
) -> Result<(), Diagnostic> {
    value_limit(manifest.game_id.len(), 128, "Manifest.game_id")?;
    value_limit(manifest.version_id.len(), 128, "Manifest.version_id")?;
    element_limit(
        manifest.required_features.len(),
        64,
        "Manifest.required_features",
    )?;
    for feature in &manifest.required_features {
        value_limit(feature.len(), 128, "Manifest.required_features")?;
    }
    element_limit(manifest.entries.len(), MAX_ENTRIES, "Manifest.entries")?;
    element_limit(manifest.chunks.len(), MAX_CHUNKS, "Manifest.chunks")?;
    entry_model_limits(&manifest.entries)?;
    for chunk in &manifest.chunks {
        value_limit(chunk.id.len(), 128, "Chunk.id")?;
    }
    // Measure without serializing the payload, before validation builds its
    // reference and ancestor sets. Producer and consumer models share this limit.
    let encoded = encoded_len(manifest).map_err(|_| error(Reason::byte_limit_exceeded, ""))?;
    value_limit(encoded as usize, limits::MANIFEST_BYTES, "")
}

/// Bounds for entry-only profile checks, including their enclosing payload budget.
pub(crate) fn entry_model_limits(entries: &[Entry]) -> Result<(), Diagnostic> {
    element_limit(entries.len(), MAX_ENTRIES, "Manifest.entries")?;
    let mut remaining = limits::CHUNK_INDEXES;
    for entry in entries {
        value_limit(entry.path.len(), path::MAX_PATH_BYTES, "Entry.path")?;
        value_limit(entry.hash.len(), 128, "Entry.hash")?;
        value_limit(
            entry.symlink_target.len(),
            path::MAX_PATH_BYTES,
            "Entry.symlink_target",
        )?;
        if entry.chunk_indexes.len() > remaining {
            return Err(error(Reason::element_limit_exceeded, ""));
        }
        remaining -= entry.chunk_indexes.len();
    }
    let budget = InputLimit::payload(limits::MANIFEST_BYTES);
    let mut encoded = 0u64;
    for entry in entries {
        let size = entry
            .try_encoded_len()
            .map_err(|_| error(Reason::byte_limit_exceeded, ""))?;
        let framed = 1 + buffa::encoding::varint_len(u64::from(size)) as u64 + u64::from(size);
        budget.check_add(encoded, framed)?;
        encoded += framed;
    }
    Ok(())
}

/// Stable version metadata is supplied explicitly; no build clock is consulted.
pub struct VersionMetadata<'a> {
    pub game_id: &'a str,
    pub version_id: &'a str,
    pub created_at: buffa_types::google::protobuf::Timestamp,
}

pub enum SourceKind<'a> {
    File(&'a [u8]),
    Symlink(&'a str),
    Directory,
}
pub struct SourceEntry<'a> {
    pub path: &'a str,
    pub mode: u32,
    pub kind: SourceKind<'a>,
}

/// Borrowed chunk bytes in exactly the sorted order of the manifest chunk table.
pub struct ProducedManifest<'a> {
    pub manifest: Manifest,
    pub chunk_bytes: Vec<&'a [u8]>,
}

/// Build the fixed SHA-256/FastCDC policy. Source directory entries may include
/// nonempty directories; those entries are omitted after path and duplicate checks.
/// Input order has no effect on payload bytes or assigned chunk indexes.
pub fn produce<'a>(
    metadata: VersionMetadata<'_>,
    source: &[SourceEntry<'a>],
) -> Result<ProducedManifest<'a>, Diagnostic> {
    element_limit(source.len(), MAX_ENTRIES, "Manifest.entries")?;
    for (value, field) in [
        (metadata.game_id, "Manifest.game_id"),
        (metadata.version_id, "Manifest.version_id"),
    ] {
        value_limit(value.len(), 128, field)?;
        if !uuid(value) {
            return Err(error(Reason::invalid_uuid, field));
        }
    }
    check_timestamp(&metadata.created_at)?;
    let mut ordered: Vec<_> = source.iter().collect();
    ordered.sort_unstable_by_key(|entry| entry.path.as_bytes());
    for (i, entry) in ordered.iter().enumerate() {
        path::validate_base(entry.path, "Entry.path")?;
        if i > 0 && ordered[i - 1].path == entry.path {
            return Err(error(Reason::unsorted_entries, "Manifest.entries"));
        }
        if let SourceKind::Symlink(target) = &entry.kind {
            path::validate_symlink(entry.path, target)?;
        }
    }
    let mut chunks: BTreeMap<[u8; 32], &'a [u8]> = BTreeMap::new();
    let mut entries = Vec::new();
    let mut indexes_left = limits::CHUNK_INDEXES;
    let mut total_size = 0u64;
    for source in &ordered {
        if matches!(&source.kind, SourceKind::Directory) && {
            let prefix = format!("{}/", source.path);
            let next = ordered.partition_point(|entry| entry.path.as_bytes() < prefix.as_bytes());
            ordered
                .get(next)
                .is_some_and(|entry| entry.path.starts_with(&prefix))
        } {
            continue;
        }
        let mut entry = Entry {
            path: source.path.into(),
            mode: source.mode & 0o777,
            ..Default::default()
        };
        match &source.kind {
            SourceKind::File(bytes) => {
                let bytes = *bytes;
                entry.r#type = EntryType::Regular.into();
                entry.size = bytes.len() as u64;
                entry.hash = Algorithm::Sha256.digest(bytes).to_vec();
                total_size = checked_size_add(total_size, entry.size)?;
                for span in fastcdc::spans(bytes) {
                    if indexes_left == 0 {
                        return Err(error(Reason::element_limit_exceeded, ""));
                    }
                    indexes_left -= 1;
                    let chunk = &bytes[span];
                    let id = Algorithm::Sha256.digest(chunk);
                    let count = chunks.len();
                    if let std::collections::btree_map::Entry::Vacant(slot) = chunks.entry(id) {
                        element_limit(count + 1, MAX_CHUNKS, "Manifest.chunks")?;
                        slot.insert(chunk);
                    }
                }
            }
            SourceKind::Symlink(target) => {
                entry.r#type = EntryType::Symlink.into();
                entry.symlink_target = (*target).into();
                entry.mode = 0;
            }
            SourceKind::Directory => entry.r#type = EntryType::Directory.into(),
        }
        entries.push(entry);
    }
    let positions: HashMap<_, _> = chunks
        .keys()
        .enumerate()
        .map(|(i, id)| (*id, i as u32))
        .collect();
    // Assign references only after deduplication and final chunk sorting.
    let mut files = ordered.iter().filter_map(|source| {
        if let SourceKind::File(bytes) = &source.kind {
            Some(*bytes)
        } else {
            None
        }
    });
    for entry in &mut entries {
        if entry.r#type.to_i32() == 1 {
            let bytes = files.next().expect("one source for each regular entry");
            entry.chunk_indexes = fastcdc::spans(bytes)
                .map(|span| positions[&Algorithm::Sha256.digest(&bytes[span])])
                .collect();
        }
    }
    let manifest = Manifest {
        format: 3,
        game_id: metadata.game_id.into(),
        version_id: metadata.version_id.into(),
        created_at: buffa_types::google::protobuf::Timestamp {
            seconds: metadata.created_at.seconds,
            nanos: metadata.created_at.nanos,
            ..Default::default()
        }
        .into(),
        header: Header {
            hash_algorithm: 1.into(),
            chunking: Chunking {
                algorithm: 1.into(),
                min_size: fastcdc::MIN as u64,
                avg_size: fastcdc::AVG as u64,
                max_size: fastcdc::MAX as u64,
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }
        .into(),
        total_size,
        entries,
        chunks: chunks
            .iter()
            .map(|(id, bytes)| Chunk {
                id: id.to_vec(),
                size: bytes.len() as u64,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    validate(&manifest, Profile::Base)?;
    Ok(ProducedManifest {
        manifest,
        chunk_bytes: chunks.into_values().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_model_payload_budget_includes_protobuf_framing() {
        fn framed_size(entry: &Entry) -> u32 {
            let size = entry.encoded_len();
            1 + buffa::encoding::varint_len(u64::from(size)) as u32 + size
        }

        let mut model = produce(
            VersionMetadata {
                game_id: "11111111-1111-1111-1111-111111111111",
                version_id: "22222222-2222-2222-2222-222222222222",
                created_at: Default::default(),
            },
            &[],
        )
        .unwrap()
        .manifest;
        let base_size = model.encoded_len();
        let suffix = format!("/{}", "b".repeat(255)).repeat(15);
        let full = Entry {
            path: format!("000000{}{suffix}/c", "a".repeat(248)),
            r#type: EntryType::Directory.into(),
            ..Default::default()
        };
        assert_eq!(full.path.len(), path::MAX_PATH_BYTES);
        let limit = limits::MANIFEST_BYTES as u32;
        let copies = (limit - base_size) / framed_size(&full);
        let remainder = limit - base_size - copies * framed_size(&full);
        let mut tail = full.clone();
        tail.path.replace_range(..6, &format!("{copies:06}"));
        // Both nested length prefixes occupy two bytes at this tail size.
        tail.path.truncate((remainder - 8) as usize);
        model.entries = vec![full, tail];
        assert!(validate(&model, Profile::Base).is_ok());

        // Generated Manifest owns every path String. Compute the repeated table's
        // size from two real entries instead of allocating over 512 MiB of paths.
        // The common check uses the same generated size function in production.
        let measured =
            |model: &Manifest| {
                Ok(base_size
                    + copies * framed_size(&model.entries[0])
                    + framed_size(&model.entries[1]))
            };
        assert_eq!(measured(&model).unwrap(), limit);
        assert_eq!(model_limits_with_size(&model, measured), Ok(()));
        model.entries[1].path.push('b');
        assert_eq!(measured(&model).unwrap(), limit + 1);
        assert_eq!(
            model_limits_with_size(&model, measured),
            Err(error(Reason::byte_limit_exceeded, ""))
        );
    }

    #[test]
    fn semantic_size_arithmetic_detects_overflow() {
        // Valid chunk sizes and the reference limit bound accepted manifests
        // below u64::MAX. Exercise the sum guard itself at the integer boundary.
        assert_eq!(checked_size_add(u64::MAX - 1, 1), Ok(u64::MAX));
        assert_eq!(
            checked_size_add(u64::MAX, 1),
            Err(error(Reason::size_overflow, ""))
        );
    }
}
