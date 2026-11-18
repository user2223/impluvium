// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Release v1 validation in descriptor.md order. Commands are data and are never run.

pub use crate::proto::impluvium::manifest::v3::{
    Command, EmulatorRef, LaunchOption, Platform, ReleaseDescriptor, ReleaseKind, SetupStep,
};
use crate::{
    decode::{self, Diagnostic, InputLimit, Projection, Reason, Value, error},
    envelope, ids,
    keys::{Content, TrustedKeySet},
    limits::{self, MessageKind},
    manifest::{self, Manifest},
    path::{self, WindowsVariant},
};
use std::collections::HashSet;

mod publication;
pub use publication::{PublicationFailure, PublishedRelease, validate_publication};

pub const MAX_COMMAND_BYTES: usize = 262_144;

/// Structurally checked, with the exact original bytes retained for revision identity.
/// This type alone establishes neither authentication nor manifest/context binding.
#[derive(Debug)]
pub struct Descriptor {
    model: ReleaseDescriptor,
    payload: Vec<u8>,
}
impl Descriptor {
    pub fn model(&self) -> &ReleaseDescriptor {
        &self.model
    }
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// Bounded binary profile followed by descriptor.md §5. Map semantics use the
/// projection's wire order, before the generated HashMaps can erase that order.
pub fn decode_payload(bytes: &[u8]) -> Result<Descriptor, Diagnostic> {
    let projection = decode::project_binary(
        MessageKind::ReleaseDescriptor,
        bytes,
        InputLimit::payload(limits::DESCRIPTOR_BYTES),
    )?;
    let model = decode::decode_projection(&projection)?;
    validate_structure(&model, &projection)?;
    Ok(Descriptor {
        model,
        payload: bytes.to_vec(),
    })
}

/// Full release-envelope verification, including structural and context semantics.
/// The supplied manifest must already have passed independent verification.
pub fn verify(
    bytes: &[u8],
    trusted: &TrustedKeySet<Content>,
    context: &Context<'_>,
) -> Result<(Descriptor, Vec<Diagnostic>), Diagnostic> {
    let authenticated = envelope::authenticate(bytes, envelope::RELEASE, trusted)?;
    let descriptor = decode_payload(authenticated.payload())?;
    let warnings = validate_context(&descriptor, context)?;
    Ok((descriptor, warnings))
}

fn display(value: &str) -> bool {
    !value
        .chars()
        .any(|c| c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c))
}
fn key(value: &str, field: &'static str) -> Result<(), Diagnostic> {
    if value.is_empty() || value.len() > 128 || !display(value) {
        Err(error(Reason::invalid_key, field))
    } else {
        Ok(())
    }
}
fn text(value: &str, field: &'static str) -> Result<(), Diagnostic> {
    if display(value) {
        Ok(())
    } else {
        Err(error(Reason::invalid_text, field))
    }
}
fn children(p: &Projection, number: u32) -> impl Iterator<Item = &Projection> {
    p.fields.iter().filter_map(move |(n, v)| match v {
        Value::Message(m) if *n == number => Some(m),
        _ => None,
    })
}
fn map(p: &Projection, number: u32) -> impl Iterator<Item = (&str, &str)> {
    p.fields.iter().filter_map(move |(n, v)| match v {
        Value::MapEntry(k, v) if *n == number => Some((k.as_str(), v.as_str())),
        _ => None,
    })
}
fn validate_structure(d: &ReleaseDescriptor, p: &Projection) -> Result<(), Diagnostic> {
    use Reason::*;
    for (value, field) in [
        (&d.game_id, "ReleaseDescriptor.game_id"),
        (&d.version_id, "ReleaseDescriptor.version_id"),
    ] {
        if !manifest::uuid(value) {
            return Err(error(invalid_uuid, field));
        }
    }
    if d.revision == 0 {
        return Err(error(invalid_revision, "ReleaseDescriptor.revision"));
    }
    if d.manifest_id.len() != 32 {
        return Err(error(invalid_manifest_id, "ReleaseDescriptor.manifest_id"));
    }
    if !(1..=limits::MANIFEST_BYTES as u64).contains(&d.manifest_size) {
        return Err(error(
            invalid_manifest_size,
            "ReleaseDescriptor.manifest_size",
        ));
    }
    if d.kind.to_i32() == 0 {
        return Err(error(missing_field, "ReleaseDescriptor.kind"));
    }
    if ![1, 2].contains(&d.kind.to_i32()) {
        return Err(error(unsupported_release_kind, "ReleaseDescriptor.kind"));
    }
    let mut requirements = HashSet::new();
    for r in &d.requires {
        if !manifest::uuid(r) {
            return Err(error(invalid_uuid, "ReleaseDescriptor.requires"));
        }
        if !requirements.insert(r) {
            return Err(error(duplicate_requirement, "ReleaseDescriptor.requires"));
        }
        if r == &d.version_id {
            return Err(error(self_requirement, "ReleaseDescriptor.requires"));
        }
    }
    for f in &d.required_features {
        if !manifest::feature_name(f) {
            return Err(error(invalid_name, "ReleaseDescriptor.required_features"));
        }
    }
    if d.required_features.windows(2).any(|w| w[0] >= w[1]) {
        return Err(error(
            unsorted_required_features,
            "ReleaseDescriptor.required_features",
        ));
    }
    if !d.required_features.is_empty() {
        return Err(error(
            unknown_required_feature,
            "ReleaseDescriptor.required_features",
        ));
    }
    for (option, p) in d.launch_options.iter().zip(children(p, 9)) {
        key(&option.key, "LaunchOption.key")?;
        if option.name.is_empty() {
            return Err(error(invalid_text, "LaunchOption.name"));
        }
        text(&option.name, "LaunchOption.name")?;
        if option.platform.to_i32() == 0 {
            return Err(error(missing_field, "LaunchOption.platform"));
        }
        if !option.command.is_set() {
            return Err(error(missing_field, "LaunchOption.command"));
        }
        command(
            &option.command,
            children(p, 4).next().expect("present command"),
            option.platform.to_i32(),
        )?;
        if option.emulator.is_set() {
            let e = &option.emulator;
            if !manifest::uuid(&e.game_id) {
                return Err(error(invalid_uuid, "EmulatorRef.game_id"));
            }
            key(&e.launch_key, "EmulatorRef.launch_key")?;
            for s in &e.suggestions {
                if s.is_empty() {
                    return Err(error(invalid_text, "EmulatorRef.suggestions"));
                }
                text(s, "EmulatorRef.suggestions")?;
            }
        }
        for (k, v) in map(p, 6) {
            if k.is_empty() || !k.bytes().all(|c| (0x21..=0x7e).contains(&c)) {
                return Err(error(invalid_hint_key, "LaunchOption.hints"));
            }
            text(v, "LaunchOption.hints")?;
        }
        legacy(&option.legacy_command, "LaunchOption.legacy_command")?;
        text(&option.legacy_launch_id, "LaunchOption.legacy_launch_id")?;
    }
    let mut keys = HashSet::new();
    for option in &d.launch_options {
        if !keys.insert((option.platform.to_i32(), &option.key)) {
            return Err(error(
                duplicate_launch_key,
                "ReleaseDescriptor.launch_options",
            ));
        }
    }
    for (step, p) in d.setup_steps.iter().zip(children(p, 10)) {
        key(&step.id, "SetupStep.id")?;
        if step.platform.to_i32() == 0 {
            return Err(error(missing_field, "SetupStep.platform"));
        }
        if !step.command.is_set() {
            return Err(error(missing_field, "SetupStep.command"));
        }
        command(
            &step.command,
            children(p, 3).next().expect("present command"),
            step.platform.to_i32(),
        )?;
        legacy(&step.legacy_command, "SetupStep.legacy_command")?;
        text(&step.legacy_setup_id, "SetupStep.legacy_setup_id")?;
    }
    let mut ids = HashSet::new();
    for step in &d.setup_steps {
        if !ids.insert(&step.id) {
            return Err(error(duplicate_setup_id, "ReleaseDescriptor.setup_steps"));
        }
    }
    if d.kind.to_i32() == 2 && !d.launch_options.is_empty() {
        return Err(error(
            setup_only_with_launch_options,
            "ReleaseDescriptor.launch_options",
        ));
    }
    validate_command_budget(d)
}
fn legacy(value: &str, field: &'static str) -> Result<(), Diagnostic> {
    if value.contains('\0') {
        Err(error(Reason::invalid_text, field))
    } else {
        Ok(())
    }
}
fn command(c: &Command, p: &Projection, platform: i32) -> Result<(), Diagnostic> {
    use Reason::*;
    path::validate_base(&c.executable, "Command.executable")?;
    for a in &c.args {
        if a.contains('\0') {
            return Err(error(invalid_argument_text, "Command.args"));
        }
    }
    if !c.working_dir.is_empty() {
        path::validate_base(&c.working_dir, "Command.working_dir")?;
    }
    for (k, v) in map(p, 4) {
        if k.is_empty() || !display(k) || k.contains('=') {
            return Err(error(invalid_env_name, "Command.env"));
        }
        if v.contains('\0') {
            return Err(error(invalid_env_value, "Command.env"));
        }
    }
    if platform == 2 {
        let mut keys = HashSet::new();
        for (k, _) in map(p, 4) {
            if !keys.insert(path::collision_key(k)) {
                return Err(error(duplicate_env_name, "Command.env"));
            }
        }
    }
    Ok(())
}

/// The decoded-model command-budget substage; other structural checks are separate.
pub fn validate_command_budget(d: &ReleaseDescriptor) -> Result<(), Diagnostic> {
    let mut remaining = MAX_COMMAND_BYTES;
    let commands = d
        .launch_options
        .iter()
        .map(|o| (&*o.command, &o.legacy_command))
        .chain(
            d.setup_steps
                .iter()
                .map(|s| (&*s.command, &s.legacy_command)),
        );
    for (c, legacy) in commands {
        let strings = [&c.executable, &c.working_dir, legacy]
            .into_iter()
            .chain(c.args.iter())
            .chain(c.env.iter().flat_map(|(k, v)| [k, v]));
        for s in strings {
            remaining = remaining
                .checked_sub(s.len())
                .ok_or(error(Reason::byte_limit_exceeded, ""))?;
        }
    }
    Ok(())
}

/// Previously verified state for this same version. No storage is accessed.
#[derive(Clone, Copy, Debug)]
pub struct KnownRevision<'a> {
    pub revision: u64,
    pub payload: &'a [u8],
}
pub struct Context<'a> {
    pub expected_version_id: &'a str,
    pub expected_game_id: Option<&'a str>,
    /// Independently verified manifest and its exact original payload.
    pub manifest: &'a Manifest,
    pub manifest_payload: &'a [u8],
    pub known_manifest_id: Option<&'a ids::ManifestId>,
    pub known_revision: Option<KnownRevision<'a>>,
    pub windows: WindowsVariant,
}

/// descriptor.md §6, after structural checks. Entry lookup uses the verified
/// manifest's sorted paths and never follows symlinks or consults a filesystem.
pub fn validate_context(d: &Descriptor, c: &Context<'_>) -> Result<Vec<Diagnostic>, Diagnostic> {
    use Reason::*;
    let m = &d.model;
    if m.version_id != c.expected_version_id {
        return Err(error(
            descriptor_version_mismatch,
            "ReleaseDescriptor.version_id",
        ));
    }
    if c.expected_game_id.is_some_and(|g| m.game_id != g) {
        return Err(error(descriptor_game_mismatch, "ReleaseDescriptor.game_id"));
    }
    if ids::manifest_id(c.manifest_payload).as_slice() != m.manifest_id {
        return Err(error(manifest_id_mismatch, "ReleaseDescriptor.manifest_id"));
    }
    if c.manifest_payload.len() as u64 != m.manifest_size {
        return Err(error(
            manifest_size_mismatch,
            "ReleaseDescriptor.manifest_size",
        ));
    }
    if c.manifest.game_id != m.game_id || c.manifest.version_id != m.version_id {
        return Err(error(manifest_binding_mismatch, ""));
    }
    if c.manifest.total_size != m.install_size {
        return Err(error(
            install_size_mismatch,
            "ReleaseDescriptor.install_size",
        ));
    }
    if c.known_manifest_id
        .is_some_and(|id| id.as_slice() != m.manifest_id)
    {
        return Err(error(
            manifest_changed_for_version,
            "ReleaseDescriptor.manifest_id",
        ));
    }
    if let Some(known) = c.known_revision {
        if m.revision < known.revision {
            return Err(error(stale_revision, "ReleaseDescriptor.revision"));
        }
        if m.revision == known.revision && d.payload != known.payload {
            return Err(error(revision_conflict, ""));
        }
    }
    let commands = || {
        m.launch_options
            .iter()
            .map(|o| (&*o.command, o.platform.to_i32()))
            .chain(
                m.setup_steps
                    .iter()
                    .map(|s| (&*s.command, s.platform.to_i32())),
            )
    };
    let entries = &c.manifest.entries;
    for (command, _) in commands() {
        let index = entries
            .binary_search_by(|e| e.path.as_str().cmp(&command.executable))
            .map_err(|_| error(executable_not_found, "Command.executable"))?;
        if entries[index].r#type.to_i32() != 1 {
            return Err(error(executable_not_regular, "Command.executable"));
        }
        let dir = &command.working_dir;
        if !dir.is_empty() {
            let explicit = entries
                .binary_search_by(|e| e.path.cmp(dir))
                .ok()
                .is_some_and(|i| entries[i].r#type.to_i32() == 3);
            let prefix = format!("{dir}/");
            let i = entries.partition_point(|e| e.path.as_str() < prefix.as_str());
            if !explicit && !entries.get(i).is_some_and(|e| e.path.starts_with(&prefix)) {
                return Err(error(working_dir_not_found, "Command.working_dir"));
            }
        }
    }
    let mut warnings = Vec::new();
    if commands().any(|(_, platform)| platform == 2) {
        warnings = path::validate_windows(entries, c.windows)?;
        for (command, platform) in commands() {
            if platform == 2 {
                for (value, field) in [
                    (&command.executable, "Command.executable"),
                    (&command.working_dir, "Command.working_dir"),
                ] {
                    if !value.is_empty() {
                        for component in value.split('/') {
                            path::validate_windows_component(component, field)?;
                        }
                    }
                }
            }
        }
    }
    Ok(warnings)
}
