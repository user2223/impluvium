<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Paths and symbolic links

Two profiles govern names in the content tree. The **base profile** applies everywhere, on every platform, to producers
and consumers. The **Windows profile** applies in addition to publication for Windows, descriptor reception for a
Windows target on any host, and installation on Windows (§4.7). Content that is valid only on Linux keeps its Linux
names: the base profile does not restrict names to what Windows can represent.

Names are compared as exact bytes. There is no Unicode normalisation and no case folding in the base profile.

## 1. Where the rules apply

| Value | Base profile | Windows profile |
|---|---|---|
| `Entry.path` | always | Windows publication, Windows-target descriptor reception on any host, and Windows installation (§4.7) |
| `Entry.symlink_target` | always (§3) | not applicable (§4.5) |
| `Command.executable`, `Command.working_dir` of a descriptor | always ([descriptor.md](descriptor.md)) | for launch options and setup steps with `PLATFORM_WINDOWS` |

## 2. Base profile

A **path** is a relative, `/`-separated UTF-8 string. Its **components** are the substrings between `/` separators. The
following checks run in this order; the first failing check determines the reason.

| Order | Rule | Reason |
|---:|---|---|
| 1 | The path is not empty. | `path_empty` |
| 2 | The path is at most 4 096 bytes. (The pre-decode limit already enforces this for schema fields.) | `byte_limit_exceeded` |
| 3 | The path contains no U+0000 to U+001F, no U+007F to U+009F and no `\` (U+005C). | `path_forbidden_character` |
| 4 | The path does not begin with `/`. | `path_absolute` |
| 5 | The first component does not begin with an ASCII letter (`A`–`Z`, `a`–`z`) followed by `:`. | `path_drive_prefix` |
| 6 | No component is empty (no leading, trailing or doubled `/`). | `path_empty_component` |
| 7 | No component is `.` or `..`. | `path_dot_component` |
| 8 | Every component is at most 255 bytes. | `path_component_too_long` |

The forbidden characters of rule 3 are forbidden on all platforms, in paths and in symbolic-link targets alike.

For manifest-entry path checks, the diagnostic field is `Entry.path`. For command paths it is `Command.executable`
or `Command.working_dir`, as selected by the invoking rule in descriptor.md. A path check with no containing schema
field identifies `""`; it does not invent a field named `path`.

## 3. Symbolic-link targets

A symbolic-link target is relative and is resolved lexically, from the directory that contains the link. The builder
and the engine never follow links when they read or write the tree.

| Order | Rule | Reason |
|---:|---|---|
| 1 | The target is not empty. | `path_empty` |
| 2 | The target is at most 4 096 bytes. | `byte_limit_exceeded` |
| 3 | No U+0000 to U+001F, no U+007F to U+009F and no `\`. | `path_forbidden_character` |
| 4 | The target does not begin with `/`. | `path_absolute` |
| 5 | The first component does not begin with an ASCII letter followed by `:`. | `path_drive_prefix` |
| 6 | No component is empty. | `path_empty_component` |
| 7 | Every component is at most 255 bytes. | `path_component_too_long` |
| 8 | Lexical resolution stays inside the tree (below). | `symlink_escape` |

Components `.` and `..` are allowed in targets. Lexical resolution: start with the list of components of the link's
own path without its last component (the link's parent directory; the empty list for a link at the top level). For each
target component in order: `.` changes nothing; `..` removes the last element of the list, and if the list is already
empty the target escapes the tree (`symlink_escape`); any other component is appended. A target that resolves to the
top of the tree itself does not escape. The target does not need to name an existing entry.

Reasons from this table identify diagnostic field `Entry.symlink_target`, so that they differ from the same reason
on `Entry.path`.

## 4. Windows profile

The Windows profile runs only after the base profile has accepted the whole manifest.

### 4.1 Characters

No component contains `<`, `>`, `:`, `"`, `|`, `?` or `*` (`windows_forbidden_character`). Control characters, `\` and
`/` inside components are already excluded by the base profile.

### 4.2 Trailing dot or space

No component ends with `.` (U+002E) or a space (U+0020) (`windows_trailing_dot_or_space`).

### 4.3 Reserved names

Let the **stem** of a component be its part before the first `.` (the whole component if there is none), with trailing
spaces removed. A component is reserved (`windows_reserved_name`) if its stem equals, comparing ASCII letters without
regard to case, one of:

- `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`;
- `COM` or `LPT` followed by one of `0`–`9`, U+00B9 (superscript one), U+00B2 (superscript two) or U+00B3 (superscript
  three).

Examples: `nul`, `Nul.txt`, `aux.tar.gz`, `com1`, `LPT¹.log` are reserved; `aux1`, `nul_`, `console`, `con-x` are not.

### 4.4 Collisions

Two different paths collide on Windows if their **collision keys** are equal. The collision key of a path is the path
with every code point `c` replaced by `fold(upper(c))`, where:

- `upper(c)` is the simple uppercase mapping of `c` from `UnicodeData.txt` (field 12) of Unicode 16.0.0, or `c` itself
  if the field is empty;
- `fold(c)` is the simple case folding of `c` from `CaseFolding.txt` of Unicode 16.0.0, using the entries with status
  `C` and `S`, or `c` itself if there is none.

The rule deliberately treats more names as colliding than a particular Windows file system might (for example, a name
with U+212A KELVIN SIGN collides with the same name with `k`), so that a tree accepted by the profile cannot collide on
any Windows version. Implementations MUST use the Unicode 16.0.0 data, not the Unicode version of their platform or
library.

The paths compared are the **closure** of the manifest: every entry path and every proper ancestor directory of an
entry path. For example `A/x` and `a/y` collide, because their ancestors `A` and `a` collide.

### 4.5 Symbolic links

In the **publication** variant of the profile, every `SYMLINK` entry yields the warning `windows_symlink`, which is
reported to the publisher and does not reject the manifest. In the **installation** variant, a manifest that
contains a `SYMLINK` entry is rejected (`unsupported_symlink`), including during descriptor reception on any host
(§4.7). Targets are not checked against §4.1 to §4.4.

### 4.6 Check order

For each entry in manifest order: for each component of its path in order, §4.1, §4.2, §4.3; then §4.4 for the entry's
ancestors (shortest first) and then the entry path, each compared with the keys of all closure paths seen so far; then
§4.5. The first rejection determines the reason; warnings are collected and do not stop the checks.

Manifest path-component and collision checks identify `Entry.path`, including collisions of ancestor directories;
the symbolic-link rejection or warning of §4.5 identifies `Entry.type`. Windows component checks invoked for command
paths retain `Command.executable` or `Command.working_dir`. No index, ancestor suffix or map key is appended.

### 4.7 When the Windows profile applies

1. **Publication.** If the release descriptor of a version has at least one launch option or setup step with
   `PLATFORM_WINDOWS`, the server validates the version's manifest with the publication variant before it publishes the
   descriptor. A rejection prevents publication.
2. **Descriptor reception.** Under [descriptor.md §6](descriptor.md#6-context-checks) step 9, every receiving client
   validates the manifest with the installation variant when any launch option or setup step has `PLATFORM_WINDOWS`,
   even on Linux or macOS. The same manifest and descriptor yield the same result on every receiving host; use the
   fixed Unicode 16.0.0 mappings of §4.4, never the host's file-system comparison or Unicode version.
3. **Installation.** An engine that installs on Windows validates the manifest with the installation variant before it
   writes anything.
4. Neither variant is applied to content that is only published and installed for other platforms.

### 4.8 Out of scope

The profile does not restrict total path length, does not consider 8.3 short names and does not apply Unicode
normalisation. The engine's path-safe writer handles these at installation time.
