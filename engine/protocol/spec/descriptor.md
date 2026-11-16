<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Release descriptor version 1

A release descriptor carries what is needed to set up and start one game version: launch options with stable keys,
structured commands, setup steps, the kind of release and its dependencies. It is signed separately from the manifest
and has its own revision, so that correcting a command changes neither the manifest nor the `manifest_id`.

Payload type: `application/vnd.impluvium.release.v1+protobuf`, signed with the content key. Schema:
[`impluvium/manifest/v3/descriptor.proto`](../proto/impluvium/manifest/v3/descriptor.proto).

## 1. Messages

### 1.1 `ReleaseDescriptor`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `game_id` | `string` | required | canonical UUID text | Game of the version. |
| 2 | `version_id` | `string` | required | canonical UUID text | The described version. |
| 3 | `revision` | `uint64` | required | ≥ 1 | Revision for `version_id`; increases with every edit. |
| 4 | `manifest_id` | `bytes` | required | exactly 32 bytes | `manifest_id` of the version's manifest. |
| 5 | `manifest_size` | `uint64` | required | 1 to 536 870 912 | Byte length of the manifest payload. |
| 6 | `install_size` | `uint64` | optional | – | Equal to `Manifest.total_size`. |
| 7 | `kind` | `ReleaseKind` | required | listed value | Kind of release. |
| 8 | `requires` | repeated `string` | optional | ≤ 8 elements, each canonical UUID text, no duplicates | Versions this version directly requires. |
| 9 | `launch_options` | repeated `LaunchOption` | optional | ≤ 128 elements | Ways to start the game. |
| 10 | `setup_steps` | repeated `SetupStep` | optional | ≤ 128 elements | Setup steps. |
| 11 | `required_features` | repeated `string` | optional | ≤ 64 elements; each 1–128 bytes, feature name syntax | Features a consumer must implement. |

### 1.2 `ReleaseKind`

| Value | Name | Meaning |
|---|---|---|
| 0 | `RELEASE_KIND_UNSPECIFIED` | Never valid. |
| 1 | `RELEASE_KIND_GAME` | Launchable content. May also have setup steps. |
| 2 | `RELEASE_KIND_SETUP_ONLY` | Content that only runs setup steps; it has no launch options. |

An unknown value is rejected as unsupported (`unsupported_release_kind`).

### 1.3 `Platform`

| Value | Name |
|---|---|
| 0 | `PLATFORM_UNSPECIFIED` (never valid) |
| 1 | `PLATFORM_LINUX` |
| 2 | `PLATFORM_WINDOWS` |
| 3 | `PLATFORM_MACOS` |

In a descriptor, a launch option or setup step with an unknown non-zero platform value is accepted and ignored by
clients that do not know the platform; it never activates the Windows profile.

### 1.4 `LaunchOption`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `key` | `string` | required | 1–128 bytes, display text | Stable key, unique per (game, platform), never changed (§3). |
| 2 | `name` | `string` | required | 1–128 bytes, display text | Display name. |
| 3 | `platform` | `Platform` | required | non-zero | Target platform. |
| 4 | `command` | `Command` | required | – | What to execute. |
| 5 | `emulator` | `EmulatorRef` | optional | – | Emulator launch option that runs this content. |
| 6 | `hints` | `map<string, string>` | optional | ≤ 64 entries; key 1–128 bytes of `0x21`–`0x7E`; value ≤ 4 096 bytes, display text | Namespaced hints (§4). |
| 7 | `legacy_command` | `string` | optional | ≤ 4 096 bytes, no U+0000 | Original command text from an import. Display only. |
| 8 | `legacy_launch_id` | `string` | optional | ≤ 128 bytes, display text | Original launch identifier from an import. |

### 1.5 `Command`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `executable` | `string` | required | path rules of [paths.md §2](paths.md#2-base-profile) | Path of a regular file in the manifest, relative to the installation root. |
| 2 | `args` | repeated `string` | optional | ≤ 256 elements, each ≤ 4 096 bytes, no U+0000 | Arguments, in order. |
| 3 | `working_dir` | `string` | optional | empty, or path rules of [paths.md §2](paths.md#2-base-profile) | Working directory relative to the installation root; empty means the root. |
| 4 | `env` | `map<string, string>` | optional | ≤ 128 entries; key 1–128 bytes, display text without `=`; value ≤ 4 096 bytes, no U+0000; for `PLATFORM_WINDOWS` no two keys with equal Windows collision key (§5.3) | Environment variables to set. |

A command is not interpreted by a shell. The arguments are passed to the program exactly as given, without splitting,
quoting or placeholder expansion. Environment entries are set in the started process and replace inherited values of
the same name; the launcher defines the base environment.

### 1.6 `EmulatorRef`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `game_id` | `string` | required | canonical UUID text | Game that provides the emulator. |
| 2 | `launch_key` | `string` | required | 1–128 bytes, display text | Launch key of the emulator for the same platform. |
| 3 | `suggestions` | repeated `string` | optional | ≤ 32 elements, each 1–256 bytes, display text | Opaque suggestions kept from an import, shown to the user or offered to the launcher; never executed. |

### 1.7 `SetupStep`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `id` | `string` | required | 1–128 bytes, display text | Identifier, unique within the descriptor. |
| 2 | `platform` | `Platform` | required | non-zero | Target platform. |
| 3 | `command` | `Command` | required | – | What to execute. |
| 4 | `idempotent` | `bool` | optional | – | True if running the step again is harmless. |
| 5 | `legacy_command` | `string` | optional | ≤ 4 096 bytes, no U+0000 | Original command text from an import. Display only. |
| 6 | `legacy_setup_id` | `string` | optional | ≤ 128 bytes, display text | Original setup identifier from an import. |

## 2. Limits for command strings

The **command bytes** of a descriptor are the sum, over all launch options and setup steps, of `len(executable)`, the
lengths of all `args`, `len(working_dir)`, the lengths of all `env` keys and values, and `len(legacy_command)`. They are
at most 262 144 bytes (256 KiB). The lengths are decoded value lengths ([limits.md §1](limits.md#1-principles)
item 7); framing does not count. The sum is computed with overflow checking.

## 3. Stable launch keys

1. A launch key identifies a launch option of a game on one platform across all versions and revisions of that game. A
   producer never changes the key of an existing launch option and never reuses a key for a different launch option of
   the same game and platform.
2. Integrations that need a stable identifier (for example a shortcut in another launcher) derive it from
   (`game_id`, platform, `key`) and store the derived value; they never recompute it from display names.
3. Within one descriptor, (`platform`, `key`) is unique (`duplicate_launch_key`). The same key MAY appear on different
   platforms.

## 4. Hints

Hint keys are namespaced (`namespace.name`). Registered keys in this revision:

| Key | Meaning |
|---|---|
| `umu.game_id` | Game identifier for a compatibility runner on Linux. |
| `umu.store` | Store identifier for a compatibility runner on Linux. |

Unknown keys are ignored. Hints never change which program is executed.

## 5. Structural validation order

A consumer validates a descriptor envelope with [envelope.md §7](envelope.md#7-verification-procedure) (expected
payload type `…release.v1…`, trusted set = the pinned content key); step 11 applies the binary profile and the limits of
[limits.md §3](limits.md#3-signed-documents). Step 13 runs the following checks in order:

| Order | Check | Reason |
|---:|---|---|
| 1 | `game_id`, then `version_id` canonical UUID text | `invalid_uuid` |
| 2 | `revision ≥ 1` | `invalid_revision` |
| 3 | `manifest_id` has 32 bytes | `invalid_manifest_id` |
| 4 | `1 ≤ manifest_size ≤ 536 870 912` | `invalid_manifest_size` |
| 5 | `kind` is not 0 | `missing_field` |
| 6 | `kind` is a listed value | `unsupported_release_kind` |
| 7 | `requires` in order: canonical UUID text (`invalid_uuid`); not equal to an earlier element (`duplicate_requirement`); not equal to `version_id` (`self_requirement`) | as listed |
| 8 | `required_features`: names, strictly ascending, implemented (as [manifest.md §7](manifest.md#7-required-features)) | `invalid_name`, `unsorted_required_features`, `unknown_required_feature` |
| 9 | Each launch option in order, §5.1 | as listed |
| 10 | (`platform`, `key`) of each launch option differs from all earlier ones | `duplicate_launch_key` |
| 11 | Each setup step in order, §5.2 | as listed |
| 12 | `id` of each setup step differs from all earlier ones | `duplicate_setup_id` |
| 13 | `kind = RELEASE_KIND_SETUP_ONLY` implies no launch options | `setup_only_with_launch_options` |
| 14 | Command bytes ≤ 262 144 (§2) | `byte_limit_exceeded` |

Platform-specific checks use the option or step's declared platform, not the verifier's host. In particular, the
Windows environment-key collision check of §5.3 runs on every host, including for options the client will not execute.

For diagnostics, `duplicate_requirement` and `self_requirement` identify `ReleaseDescriptor.requires`;
`duplicate_launch_key` identifies `ReleaseDescriptor.launch_options`, `duplicate_setup_id` identifies
`ReleaseDescriptor.setup_steps`, and `setup_only_with_launch_options` identifies `ReleaseDescriptor.launch_options`.
The combined command-byte limit of §2 and §5 step 14 identifies `""`, not any one command field.

### 5.1 Checks per launch option

1. `key`: 1–128 bytes of display text (`invalid_key`).
2. `name`: 1–128 bytes of display text (`invalid_text`).
3. `platform` is not 0 (`missing_field`).
4. `command` present (`missing_field`), then §5.3.
5. `emulator`, if present: `game_id` canonical UUID text (`invalid_uuid`); `launch_key` 1–128 bytes of display text
   (`invalid_key`); each suggestion 1–256 bytes of display text (`invalid_text`).
6. `hints` in wire order: key 1–128 bytes, each byte in `0x21`–`0x7E` (`invalid_hint_key`); value display text
   (`invalid_text`).
7. `legacy_command` contains no U+0000 (`invalid_text`); `legacy_launch_id` is display text (`invalid_text`).

### 5.2 Checks per setup step

1. `id`: 1–128 bytes of display text (`invalid_key`).
2. `platform` is not 0 (`missing_field`).
3. `command` present (`missing_field`), then §5.3.
4. `legacy_command` contains no U+0000 (`invalid_text`); `legacy_setup_id` is display text (`invalid_text`).

### 5.3 Checks per command

1. `executable`: base path rules of [paths.md §2](paths.md#2-base-profile), diagnostic field `Command.executable`.
2. Each argument in order contains no U+0000 (`invalid_argument_text`).
3. `working_dir`: empty, or base path rules, diagnostic field `Command.working_dir`.
4. `env` in wire order: key 1–128 bytes of display text without `=` (`invalid_env_name`); value contains no U+0000
   (`invalid_env_value`).
5. If the launch option or setup step has `PLATFORM_WINDOWS`: no two `env` keys have the same Windows collision key
   ([paths.md §4.4](paths.md#44-collisions)), because Windows treats environment names without regard to case; the
   second key in wire order is reported (`duplicate_env_name`). For other platforms keys are compared as bytes, and
   the map rule of [encoding.md §2.4](encoding.md#24-occurrence-rules) already rejects equal keys.

Argument-element failures identify `Command.args`. Environment key, value and collision failures identify
`Command.env`, without a key suffix; hint-map checks in §5.1 identify `LaunchOption.hints`.

## 6. Context checks

Context checks need data beyond the descriptor. The caller supplies:

- the **expected `version_id`**: the version the caller asked for (the `version_id` of the `OpenDownload` request or
  of the descriptor GET path); and the **expected `game_id`** if the caller knows the game of that version (for
  example from `VersionSummary.game_id` or a stored installation);
- the verified manifest that the descriptor references (its payload bytes and decoded model);
- the highest descriptor revision it already knows for the same `version_id` together with that descriptor's payload
  bytes, and the `manifest_id` it already knows for the `version_id`, if any.

The checks run after the structural checks, in this order.

| Order | Check | Reason |
|---:|---|---|
| 0a | `version_id` equals the expected `version_id` | `descriptor_version_mismatch` |
| 0b | If an expected `game_id` is supplied: `game_id` equals it | `descriptor_game_mismatch` |
| 1 | `SHA-256(manifest payload) = manifest_id` | `manifest_id_mismatch` |
| 2 | `len(manifest payload) = manifest_size` | `manifest_size_mismatch` |
| 3 | `Manifest.game_id = game_id` and `Manifest.version_id = version_id` | `manifest_binding_mismatch` |
| 4 | `Manifest.total_size = install_size` | `install_size_mismatch` |
| 5 | If a `manifest_id` is already known for `version_id`, it equals `manifest_id`: a known version never silently changes its manifest | `manifest_changed_for_version` |
| 6 | If a revision is already known: `revision` is not lower | `stale_revision` |
| 7 | If the known revision equals `revision`: the payload bytes are identical | `revision_conflict` |
| 8 | For each launch option, then each setup step, in order: `executable` names an entry (`executable_not_found`) of type `REGULAR` (`executable_not_regular`); `working_dir` is empty, a `DIRECTORY` entry, or a proper ancestor of an entry (`working_dir_not_found`) | as listed |
| 9 | If any launch option or setup step has `PLATFORM_WINDOWS`: the manifest passes the Windows profile of [paths.md §4](paths.md#4-windows-profile) (publication variant before publication; installation variant for every receiving client, on every host), and the components of `executable` and `working_dir` of every Windows launch option and setup step pass [paths.md §4.1](paths.md#41-characters) to §4.3 | Windows reasons |

The diagnostic field of each context check is fixed independently of which input supplied the expected value:

| Check | Diagnostic field |
|---|---|
| 0a, 0b | `ReleaseDescriptor.version_id`, `ReleaseDescriptor.game_id`, respectively |
| 1, 5 | `ReleaseDescriptor.manifest_id` |
| 2 | `ReleaseDescriptor.manifest_size` |
| 3 (the joint game/version binding) | `""` |
| 4 | `ReleaseDescriptor.install_size` |
| 6 | `ReleaseDescriptor.revision` |
| 7 (whole-payload identity) | `""` |
| 8 | `Command.executable` or `Command.working_dir`, according to the failing predicate |
| 9 | The manifest-profile field from paths.md §4.6, or `Command.executable` / `Command.working_dir` for the command-component checks |

Clients keep the last verified descriptor of each installed version to start games offline, and they reject a revision
lower than the highest one they have seen for that `version_id`.

## 7. Publication checks

Before the server publishes a descriptor revision of a version `V`, it evaluates the dependency graph **as it would be
after the publication**: the new revision of `V` together with the current published descriptors of all other
versions. The **closure** of a version is the version itself and every version reachable from it through `requires`.
The **affected versions** are `V` and every published version whose closure contains `V`.

For `V` and then for every affected version (in ascending order of `version_id` bytes), the server checks:

| Order | Check | Reason |
|---:|---|---|
| 1 | Every version in the closure exists and has a published descriptor | `dependency_not_found` |
| 2 | The closure contains no cycle | `dependency_cycle` |
| 3 | The closure contains at most 8 distinct `manifest_id`s, the version's own included | `ticket_set_too_large` |

These graph checks identify diagnostic field `""`: they concern a closure of documents, not one schema field.

If any check fails for any affected version, the revision is not published, and the reason names the version that
failed. The evaluation and the publication happen atomically with respect to other publications, so two concurrent
publications cannot together produce a graph that neither checked. Thus a new revision of a required version can
never make an already published version exceed 8 manifests or form a cycle.

The limit of 8 direct `requires` does not replace the limit of 8 manifests for the whole closure. A version whose
closure exceeds 8 manifests cannot be published, so the server never has to shorten a ticket's manifest set.

Storing revisions and dependency relations is the server's responsibility. Running commands and setup steps is the
engine's responsibility; this specification runs nothing.
