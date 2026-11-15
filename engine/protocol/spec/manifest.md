<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-FileCopyrightText: 2019 Nathan Fiedler
SPDX-License-Identifier: 0BSD AND MIT
-->

# Manifest format 3

A manifest describes the complete file tree of one game version. It is immutable: a version maps to exactly one
`manifest_id` for its whole lifetime. Content, launch data and physical storage stay separate: launch and setup data
live in the release descriptor ([descriptor.md](descriptor.md)), and where the bytes are stored on the server never
appears in a manifest. Chunks never span files. There is no platform field, no encryption, no compression, no delta
chain and no reference to an earlier version.

Payload type: `application/vnd.impluvium.manifest.v3+protobuf`, signed with the content key. Schema:
[`impluvium/manifest/v3/manifest.proto`](../proto/impluvium/manifest/v3/manifest.proto).

## 1. Messages

### 1.1 `Manifest`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `format` | `uint32` | required | MUST be 3 | Format number. |
| 2 | `game_id` | `string` | required | canonical UUID text, 36 bytes | Game this version belongs to. |
| 3 | `version_id` | `string` | required | canonical UUID text, 36 bytes | The version this manifest describes. |
| 4 | `created_at` | `Timestamp` | required | [encoding.md §5](encoding.md#5-timestamps) | Taken from stable version metadata (§8). |
| 5 | `header` | `Header` | required | – | Content hash and chunking parameters. |
| 6 | `required_features` | repeated `string` | optional | ≤ 64 elements; each 1–128 bytes, feature name syntax | Features a consumer must implement (§7). |
| 7 | `total_size` | `uint64` | optional (0 for a tree without non-empty files) | – | Sum of `size` over all regular-file entries. |
| 8 | `entries` | repeated `Entry` | optional (an empty tree has none) | ≤ 2 000 000 elements | The tree, strictly ascending by path bytes. |
| 9 | `chunks` | repeated `Chunk` | conditional: required if any regular file has `size > 0` | ≤ 4 000 000 elements | Distinct chunks, strictly ascending by id bytes. |

### 1.2 `Header`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `hash_algorithm` | `HashAlgorithm` | required | listed value | Hash for file hashes and chunk ids (§3). |
| 2 | `chunking` | `Chunking` | required | – | Producer chunking parameters (§9). |

### 1.3 `HashAlgorithm`

| Value | Name | Digest |
|---|---|---|
| 0 | `HASH_ALGORITHM_UNSPECIFIED` | Never valid. |
| 1 | `HASH_ALGORITHM_SHA256` | SHA-256 (FIPS 180-4), 32 bytes. Default for producers. |
| 2 | `HASH_ALGORITHM_BLAKE3_256` | BLAKE3 hash mode (no key, no key derivation), 32-byte output. |

### 1.4 `Chunking`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `algorithm` | `ChunkingAlgorithm` | required | non-zero; unknown values accepted | Algorithm the producer used. |
| 2 | `min_size` | `uint64` | optional | – | Producer minimum chunk size. |
| 3 | `avg_size` | `uint64` | optional | – | Producer target average chunk size. |
| 4 | `max_size` | `uint64` | optional | – | Producer maximum chunk size. |

Consumers never recompute chunk boundaries and never validate chunk sizes against these parameters; they are recorded
for producers and for diagnostics. Values from the producer default are in §9.

### 1.5 `ChunkingAlgorithm`

| Value | Name | Meaning |
|---|---|---|
| 0 | `CHUNKING_ALGORITHM_UNSPECIFIED` | Never valid. |
| 1 | `CHUNKING_ALGORITHM_FASTCDC_V2020` | FastCDC 2020, normalized chunking level 1 (§9). |

An unknown non-zero value is accepted; it only describes how the producer cut the files.

### 1.6 `Entry`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `path` | `string` | required | 1–4096 bytes; components 1–255 bytes; [paths.md](paths.md) | Relative path. |
| 2 | `type` | `EntryType` | required | listed value | Entry type. |
| 3 | `size` | `uint64` | conditional (§5) | – | File size of a regular file. |
| 4 | `hash` | `bytes` | conditional (§5) | exactly 32 bytes when present | Hash of the whole file content. |
| 5 | `mode` | `uint32` | optional | ≤ `0o777` (511) | Permission bits. |
| 6 | `symlink_target` | `string` | conditional (§5) | 1–4096 bytes; [paths.md §3](paths.md#3-symbolic-link-targets) | Target of a symbolic link. |
| 7 | `chunk_indexes` | repeated `uint32` | conditional (§5) | all entries together ≤ 16 000 000 elements | Indexes into `Manifest.chunks`, in file order. |

### 1.7 `EntryType`

| Value | Name | Meaning |
|---|---|---|
| 0 | `ENTRY_TYPE_UNSPECIFIED` | Never valid. |
| 1 | `ENTRY_TYPE_REGULAR` | Regular file. |
| 2 | `ENTRY_TYPE_SYMLINK` | Symbolic link. |
| 3 | `ENTRY_TYPE_DIRECTORY` | Empty directory. Directories that contain entries are implied by the paths and are not listed. |

An unknown value is rejected (`invalid_entry_type`): a consumer cannot install an entry whose type it does not know.

### 1.8 `Chunk`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `id` | `bytes` | required | exactly 32 bytes | Hash of the chunk bytes with the manifest's algorithm. |
| 2 | `size` | `uint64` | required | 1 to 16 777 216 | Chunk size in bytes. |

## 2. Identity

`manifest_id = SHA-256(payload)` over the exact payload bytes ([envelope.md §8.1](envelope.md#81-manifest_id)). The
`manifest_id` does not depend on the content hash algorithm and does not change when the envelope is re-signed.

## 3. Content hashes

1. One content hash algorithm per manifest, declared in `header.hash_algorithm`. It defines every `Entry.hash` and every
   `Chunk.id`.
2. Producers use `HASH_ALGORITHM_SHA256`. Consumers of this revision support both listed algorithms.
3. A manifest with `HASH_ALGORITHM_UNSPECIFIED` or without `header` is rejected; it is not read as a SHA-256 manifest.
   An unknown algorithm value is rejected as unsupported (`unsupported_hash_algorithm`).
4. `Entry.hash` of a regular file is the hash of the whole file content. `Chunk.id` is the hash of the chunk bytes.

## 4. Chunk table

1. `chunks` holds every distinct chunk exactly once.
2. `chunks` is strictly ascending by the 32 bytes of `id`, compared as unsigned bytes. Equal ids are rejected as
   unsorted (`unsorted_chunks`); since ids are unique, one id never names two sizes.
3. Every chunk is referenced by at least one regular-file entry (`unreferenced_chunk`).
4. `size` is at least 1 and at most 16 777 216 (16 MiB). A size of 0 is rejected (`chunk_size_zero`); a larger size is
   rejected (`chunk_size_too_large`).
5. A chunk is one contiguous byte range of one file. The same chunk id may occur in several files and several times in
   one file.

## 5. Entry rules by type

| Field | `REGULAR`, `size = 0` | `REGULAR`, `size > 0` | `SYMLINK` | `DIRECTORY` |
|---|---|---|---|---|
| `size` | 0 | > 0 | 0 | 0 |
| `hash` | 32 bytes: hash of the empty content | 32 bytes: hash of the content | empty | empty |
| `mode` | ≤ `0o777` | ≤ `0o777` | 0 | ≤ `0o777` |
| `symlink_target` | empty | empty | 1–4096 bytes, [paths.md §3](paths.md#3-symbolic-link-targets) | empty |
| `chunk_indexes` | empty | at least 1 element | empty | empty |

A field that the table requires to be empty or 0 but that is set is rejected (`invalid_entry_fields`). A `mode` above
`0o777` is rejected (`invalid_mode`); for a symbolic link a non-zero `mode` is rejected (`invalid_mode`). A `hash` of
another length than 32 bytes on a regular file is rejected (`invalid_hash_length`).

### 5.1 Regular files with content

1. Every element of `chunk_indexes` is less than the number of chunks (`chunk_index_out_of_range`).
2. The chunk sizes of `chunk_indexes`, summed in order with overflow checking, equal `size`
   (`chunk_size_mismatch`; an intermediate sum above 2^64 − 1 is `size_overflow`).
3. The offset of the i-th chunk is the sum of the sizes of the chunks before it. Chunks tile the file without gaps or
   overlaps.
4. If `chunk_indexes` has exactly one element, its chunk id equals `hash` (`single_chunk_hash_mismatch`). The whole-file
   hash and the identical chunk id are both kept; there is no special encoding that omits one of them.

### 5.2 Empty files

A regular file with `size = 0` has no chunk reference, and its `hash` is the hash of the empty byte string under the
manifest's algorithm. There is no zero-length chunk and no depot request for an empty file. A regular file with
`size = 0` and a non-empty `chunk_indexes` is rejected (`invalid_entry_fields`).

### 5.3 Tree structure

1. `entries` is strictly ascending by the bytes of `path`, compared as unsigned bytes. Equal paths are rejected as
   unsorted (`unsorted_entries`).
2. No entry path is a proper ancestor of another entry path: for an entry `P`, no other entry starts with `P + "/"`
   (`path_parent_conflict`). This covers a regular file or symbolic link used as a directory and a `DIRECTORY` entry that
   is not empty.
3. Path and symbolic-link rules of [paths.md](paths.md) apply to every entry.

### 5.4 Totals

`total_size` equals the overflow-checked sum of `size` over all regular-file entries (`total_size_mismatch`; overflow is
`size_overflow`).

## 6. No platform data

A manifest contains no platform, architecture or operating-system field. Platforms follow from the release descriptor.
Whether a tree is acceptable for Windows is decided by the Windows profile of [paths.md §4](paths.md#4-windows-profile),
which consumers apply when the context requires it.

## 7. Required features

1. `required_features` is strictly ascending by bytes and contains no duplicates (`unsorted_required_features`).
2. Every element is a feature name ([encoding.md §6.3](encoding.md#63-feature-and-capability-names)), otherwise
   `invalid_name`.
3. A consumer rejects a manifest that lists a feature it does not implement (`unknown_required_feature`). This revision
   defines no manifest feature, so every non-empty list is rejected by consumers of this revision.
4. New consumer behaviour that older consumers must not silently ignore (for example a per-chunk codec) is introduced
   only together with a new required feature or a new format number.

## 8. Producer rules and determinism

A producer that builds the same tree with the same version metadata and the same parameters MUST produce the same
manifest bytes and the same chunk ids, independent of traversal order and of the number of parallel workers.

1. `format = 3`; `game_id` and `version_id` from the version.
2. `created_at` comes from stable version metadata (the time the version was created). It MUST NOT be the time of the
   build run.
3. `header.hash_algorithm = HASH_ALGORITHM_SHA256`; `header.chunking` records the parameters actually used (§9).
4. `required_features` is empty unless a later revision defines a feature that the manifest needs.
5. Entries are sorted by path bytes. Empty directories get a `DIRECTORY` entry; non-empty directories get none.
6. Chunks of each file are computed per §9; `chunks` is sorted by id and deduplicated; the indexes in
   `chunk_indexes` are assigned after this sorting.
7. `mode` carries only the permission bits (`st_mode & 0o777`); symbolic links get 0.
8. The payload is serialised with the producer encoding of [encoding.md §2.7](encoding.md#27-producer-encoding):
   `chunk_indexes` is one packed field per entry and is omitted for entries without chunks; `Timestamp.nanos` is
   omitted when it is 0.
9. The manifest contains no source paths, layer information, storage locations, timestamps of files or ownership.

## 9. Chunking (producer policy)

Consumers read the explicit chunk boundaries and never recompute them. Chunking is producer policy, recorded in
`header.chunking`. A producer that writes `CHUNKING_ALGORITHM_FASTCDC_V2020` with the parameters of §9.1 MUST cut files
exactly as this section defines, so that equal file contents always yield equal chunk ids.

### 9.1 Parameters

| Name | Value | Recorded in |
|---|---:|---|
| `MIN` | 65 536 | `header.chunking.min_size` |
| `AVG` | 262 144 | `header.chunking.avg_size` |
| `MAX` | 1 048 576 | `header.chunking.max_size` |

All three values are even; the procedure below is defined for exactly these values.

### 9.2 Files

1. A regular file of size 0 has no chunk (§5.2).
2. A regular file with `0 < size ≤ MAX` is exactly one chunk covering the whole file. FastCDC is not applied to it.
3. A regular file with `size > MAX` is split by repeated application of `cut` (§9.4):

```
pos = 0
while pos < size:
    L = cut(file[pos .. size])      # the remaining bytes of the file
    emit the chunk file[pos .. pos + L]
    pos = pos + L
```

Each call of `cut` starts with a fingerprint of 0. Chunking never continues across files.

### 9.3 Constants

All values are unsigned 64-bit integers, written in hexadecimal.

- `GEAR[0..255]`: the 256 values of Table G (§9.6). Each value equals the first 8 bytes, read as a big-endian unsigned
  integer, of the MD5 digest of a string of 64 bytes that all have the value of the table index. The table is
  normative; the derivation rule allows it to be regenerated and checked.
- `GEAR_LS[i] = (GEAR[i] << 1) mod 2^64` for every index `i`.
- `MASK_S = 0x0000d91707537000` (19 bits set) and `MASK_L = 0x0000d90703537000` (17 bits set). They are the masks for
  normalized chunking level 1 at `AVG = 2^18`: the stricter mask, used before the target average position, has
  `18 + 1` bits set, and the looser mask, used after it, has `18 − 1` bits set.
- `MASK_S_LS = MASK_S << 1 = 0x0001b22e0ea6e000` and `MASK_L_LS = MASK_L << 1 = 0x0001b20e06a6e000`.

No seed is applied: the tables are used exactly as listed.

Table G and the two masks are third-party material under the MIT license; everything else in this file is 0BSD. The
source and the notice that the MIT license requires are in `engine/protocol/NOTICE`.

### 9.4 Cut procedure

`cut(w)` returns the length of the next chunk of the byte string `w`. `n` is a byte count, `i` and `a` are byte
positions in `w` (0-based), `/` is integer division rounding down, `h` is an unsigned 64-bit fingerprint, `<<` discards
bits shifted beyond bit 63, `+` is addition modulo 2^64 and `&` is bitwise AND.

```
cut(w):
    n = len(w)
    if n <= MIN:
        return n
    center = AVG
    if n > MAX:
        n = MAX
    else if n < AVG:
        center = n
    h = 0
    i = MIN / 2
    while i < center / 2:
        a = 2 * i
        h = (h << 2) + GEAR_LS[w[a]]
        if (h & MASK_S_LS) == 0:
            return a
        h = h + GEAR[w[a + 1]]
        if (h & MASK_S) == 0:
            return a + 1
        i = i + 1
    while i < n / 2:
        a = 2 * i
        h = (h << 2) + GEAR_LS[w[a]]
        if (h & MASK_L_LS) == 0:
            return a
        h = h + GEAR[w[a + 1]]
        if (h & MASK_L) == 0:
            return a + 1
        i = i + 1
    return n
```

Consequences, stated for clarity:

1. The bytes `w[0 .. MIN − 1]` are never read (cut-point skipping); the first candidate position is `a = MIN`.
2. The returned length excludes the byte whose addition to `h` satisfied the test: a return value `a` ends the chunk
   before `w[a]`, a return value `a + 1` ends it before `w[a + 1]`.
3. Inside the loops `a + 1 ≤ n − 1`, so no byte beyond the window is read. If `n` is odd, the last byte of the window is
   never a cut candidate; if no earlier candidate matched, the chunk ends at the end of the window.
4. Every chunk except the last of a file has a size of at least `MIN`; every chunk has a size of at least 1 and at most
   `MAX`. The last chunk of a file can be shorter than `MIN`.
5. The fingerprint `h` is not recorded anywhere.

### 9.5 Cut-point vectors

The cut-point vectors give the input bytes, the parameters of §9.1 and the complete sequence of chunk offsets and
lengths. They cover at least: an empty file, a 1-byte file, file sizes `MIN`, `AVG` and `MAX` each minus one, equal and
plus one, a remainder at the end of a file, repetitive content (for example a file of zero bytes) and an insertion in
the middle of a file. The vectors are computed independently from
this section; the shipped implementation is not their source.

### 9.6 Table G

Each line starts with the index of its first value; the four values on a line have consecutive indexes.

```
  0  0x3b5d3c7d207e37dc 0x784d68ba91123086 0xcd52880f882e7298 0xeacf8e4e19fdcca7
  4  0xc31f385dfbd1632b 0x1d5f27001e25abe6 0x83130bde3c9ad991 0xc4b225676e9b7649
  8  0xaa329b29e08eb499 0xb67fcbd21e577d58 0x0027baaada2acf6b 0xe3ef2d5ac73c2226
 12  0x0890f24d6ed312b7 0xa809e036851d7c7e 0xf0a6fe5e0013d81b 0x1d026304452cec14
 16  0x03864632648e248f 0xcdaacf3dcd92b9b4 0xf5e012e63c187856 0x8862f9d3821c00b6
 20  0xa82f7338750f6f8a 0x1e583dc6c1cb0b6f 0x7a3145b69743a7f1 0xabb20fee404807eb
 24  0xb14b3cfe07b83a5d 0xb9dc27898adb9a0f 0x3703f5e91baa62be 0xcf0bb866815f7d98
 28  0x3d9867c41ea9dcd3 0x1be1fa65442bf22c 0x14300da4c55631d9 0xe698e9cbc6545c99
 32  0x4763107ec64e92a5 0xc65821fc65696a24 0x76196c064822f0b7 0x485be841f3525e01
 36  0xf652bc9c85974ff5 0xcad8352face9e3e9 0x2a6ed1dceb35e98e 0xc6f483badc11680f
 40  0x3cfd8c17e9cf12f1 0x89b83c5e2ea56471 0xae665cfd24e392a9 0xec33c4e504cb8915
 44  0x3fb9b15fc9fe7451 0xd7fd1fd1945f2195 0x31ade0853443efd8 0x255efc9863e1e2d2
 48  0x10eab6008d5642cf 0x46f04863257ac804 0xa52dc42a789a27d3 0xdaaadf9ce77af565
 52  0x6b479cd53d87febb 0x6309e2d3f93db72f 0xc5738ffbaa1ff9d6 0x6bd57f3f25af7968
 56  0x67605486d90d0a4a 0xe14d0b9663bfbdae 0xb7bbd8d816eb0414 0xdef8a4f16b35a116
 60  0xe7932d85aaaffed6 0x08161cbae90cfd48 0x855507beb294f08b 0x91234ea6ffd399b2
 64  0xad70cf4b2435f302 0xd289a97565bc2d27 0x8e558437ffca99de 0x96d2704b7115c040
 68  0x0889bbcdfc660e41 0x5e0d4e67dc92128d 0x72a9f8917063ed97 0x438b69d409e016e3
 72  0xdf4fed8a5d8a4397 0x00f41dcf41d403f7 0x4814eb038e52603f 0x9dafbacc58e2d651
 76  0xfe2f458e4be170af 0x4457ec414df6a940 0x06e62f1451123314 0xbd1014d173ba92cc
 80  0xdef318e25ed57760 0x9fea0de9dfca8525 0x459de1e76c20624b 0xaeec189617e2d666
 84  0x126a2c06ab5a83cb 0xb1321532360f6132 0x65421503dbb40123 0x2d67c287ea089ab3
 88  0x6c93bff5a56bd6b6 0x4ffb2036cab6d98d 0xce7b785b1be7ad4f 0xedb42ef6189fd163
 92  0xdc905288703988f6 0x365f9c1d2c691884 0xc640583680d99bfe 0x3cd4624c07593ec6
 96  0x7f1ea8d85d7c5805 0x014842d480b57149 0x0b649bcb5a828688 0xbcd5708ed79b18f0
100  0xe987c862fbd2f2f0 0x982731671f0cd82c 0xbaf13e8b16d8c063 0x8ea3109cbd951bba
104  0xd141045bfb385cad 0x2acbc1a0af1f7d30 0xe6444d89df03bfdf 0xa18cc771b8188ff9
108  0x9834429db01c39bb 0x214add07fe086a1f 0x8f07c19b1f6b3ff9 0x56a297b1bf4ffe55
112  0x94d558e493c54fc7 0x40bfc24c764552cb 0x931a706f8a8520cb 0x32229d322935bd52
116  0x2560d0f5dc4fefaf 0x9dbcc48355969bb6 0x0fd81c3985c0b56a 0xe03817e1560f2bda
120  0xc1bb4f81d892b2d5 0xb0c4864f4e28d2d7 0x3ecc49f9d9d6c263 0x51307e99b52ba65e
124  0x8af2b688da84a752 0xf5d72523b91b20b6 0x6d95ff1ff4634806 0x562f21555458339a
128  0xc0ce47f889336346 0x487823e5089b40d8 0xe4727c7ebc6d9592 0x5a8f7277e94970ba
132  0xfca2f406b1c8bb50 0x5b1f8a95f1791070 0xd304af9fc9028605 0x5440ab7fc930e748
136  0x312d25fbca2ab5a1 0x10f4a4b234a4d575 0x90301d55047e7473 0x3b6372886c61591e
140  0x293402b77c444e06 0x451f34a4d3e97dd7 0x3158d814d81bc57b 0x034942425b9bda69
144  0xe2032ff9e532d9bb 0x62ae066b8b2179e5 0x9545e10c2f8d71d8 0x7ff7483eb2d23fc0
148  0x00945fcebdc98d86 0x8764bbbe99b26ca2 0x1b1ec62284c0bfc3 0x58e0fcc4f0aa362b
152  0x5f4abefa878d458d 0xfd74ac2f9607c519 0xa4e3fb37df8cbfa9 0xbf697e43cac574e5
156  0x86f14a3f68f4cd53 0x24a23d076f1ce522 0xe725cd8048868cc8 0xbf3c729eb2464362
160  0xd8f6cd57b3cc1ed8 0x6329e52425541577 0x62aa688ad5ae1ac0 0x0a242566269bf845
164  0x168b1a4753aca74b 0xf789afefff2e7e3c 0x6c3362093b6fccdb 0x4ce8f50bd28c09b2
168  0x006a2db95ae8aa93 0x975b0d623c3d1a8c 0x18605d3935338c5b 0x5bb6f6136cad3c71
172  0x0f53a20701f8d8a6 0xab8c5ad2e7e93c67 0x40b5ac5127acaa29 0x8c7bf63c2075895f
176  0x78bd9f7e014a805c 0xb2c9e9f4f9c8c032 0xefd6049827eb91f3 0x2be459f482c16fbd
180  0xd92ce0c5745aaa8c 0x0aaa8fb298d965b9 0x2b37f92c6c803b15 0x8c54a5e94e0f0e78
184  0x95f9b6e90c0a3032 0xe7939faa436c7874 0xd16bfe8f6a8a40c9 0x44982b86263fd2fa
188  0xe285fb39f984e583 0x779a8df72d7619d3 0xf2d79a8de8d5dd1e 0xd1037354d66684e2
192  0x004c82a4e668a8e5 0x31d40a7668b044e6 0xd70578538bd02c11 0xdb45431078c5f482
196  0x977121bb7f6a51ad 0x73d5ccbd34eff8dd 0xe437a07d356e17cd 0x47b2782043c95627
200  0x9fb251413e41d49a 0xccd70b60652513d3 0x1c95b31e8a1b49b2 0xcae73dfd1bcb4c1b
204  0x34d98331b1f5b70f 0x784e39f22338d92f 0x18613d4a064df420 0xf1d8dae25f0bcebe
208  0x33f77c15ae855efc 0x3c88b3b912eb109c 0x956a2ec96bafeea5 0x1aa005b5e0ad0e87
212  0x5500d70527c4bb8e 0xe36c57196421cc44 0x13c4d286cc36ee39 0x5654a23d818b2a81
216  0x77b1dc13d161abdc 0x734f44de5f8d5eb5 0x60717e174a6c89a2 0xd47d9649266a211e
220  0x5b13a4322bb69e90 0xf7669609f8b5fc3c 0x21e6ac55bedcdac9 0x9b56b62b61166dea
224  0xf48f66b939797e9c 0x35f332f9c0e6ae9a 0xcc733f6a9a878db0 0x3da161e41cc108c2
228  0xb7d74ae535914d51 0x4d493b0b11d36469 0xce264d1dfba9741a 0xa9d1f2dc7436dc06
232  0x70738016604c2a27 0x231d36e96e93f3d5 0x7666881197838d19 0x4a2a83090aaad40c
236  0xf1e761591668b35d 0x7363236497f730a7 0x301080e37379dd4d 0x502dea2971827042
240  0xc2c5eb858f32625f 0x786afb9edfafbdff 0xdaee0d868490b2a4 0x617366b3268609f6
244  0xae0e35a0fe46173e 0xd1a07de93e824f11 0x079b8b115ea4cca8 0x93a99274558faebb
248  0xfb1e6e22e08a03b3 0xea635fdba3698dd0 0xcf53659328503a5c 0xcde3b31e6fd5d780
252  0x8e3e4221d3614413 0xef14d0d86bf1a22c 0xe1d830d3f16c5ddb 0xaabd2b2a451504e1
```

## 10. Validation order

A consumer validates a manifest envelope with the procedure of [envelope.md §7](envelope.md#7-verification-procedure)
(expected payload type `…manifest.v3…`, trusted set = the pinned content key). Step 11 applies the binary profile and
the limits of [limits.md §3](limits.md#3-signed-documents) to the payload, in the scan order of [encoding.md §2.8](encoding.md#28-scan-order-and-precedence). Step 13 then runs the
following checks; the first failing check determines the reason.

| Order | Check | Reason |
|---:|---|---|
| 1 | `format` is not 0 | `missing_field` |
| 2 | `format` equals 3 | `unsupported_manifest_format` |
| 3 | `game_id`, then `version_id`: canonical UUID text | `invalid_uuid` |
| 4 | `created_at` present | `missing_field` |
| 5 | `header` present | `missing_header` |
| 6 | `header.hash_algorithm` is not 0 | `missing_field` |
| 7 | `header.hash_algorithm` is a listed value | `unsupported_hash_algorithm` |
| 8 | `header.chunking` present | `missing_chunking` |
| 9 | `header.chunking.algorithm` is not 0 | `missing_field` |
| 10 | `required_features`: each element in order is a feature name | `invalid_name` |
| 11 | `required_features` strictly ascending | `unsorted_required_features` |
| 12 | `required_features` contains only implemented features | `unknown_required_feature` |
| 13 | For each chunk in table order: `id` has 32 bytes (`invalid_chunk_id`), `size ≠ 0` (`chunk_size_zero`), `size ≤ 16 777 216` (`chunk_size_too_large`), `id` greater than the previous id (`unsorted_chunks`) | as listed |
| 14 | For each entry in order, the checks of §10.1 | as listed |
| 15 | `total_size` equals the sum | `total_size_mismatch` or `size_overflow` |
| 16 | Every chunk is referenced; the first unreferenced index in table order is reported | `unreferenced_chunk` |
| 17 | If the Windows profile is requested: [paths.md §4](paths.md#4-windows-profile) | Windows reasons |

### 10.1 Checks per entry

For entry `i`, in this order:

1. Path rules of [paths.md §2](paths.md#2-base-profile) in their order.
2. Path strictly greater than the path of entry `i − 1` (`unsorted_entries`).
3. No proper ancestor of the path is the path of an earlier entry (`path_parent_conflict`). Ancestors always sort before
   their descendants, so this check only looks at earlier entries.
4. `type` is a listed value (`invalid_entry_type`).
5. Fields allowed for the type (§5): `size`, `hash`, `symlink_target`, `chunk_indexes` in this order
   (`invalid_entry_fields`); for regular files a `hash` of 32 bytes (`invalid_hash_length`).
6. `mode` (`invalid_mode`).
7. Regular files with `size = 0`: `hash` equals the hash of the empty byte string under the manifest's algorithm
   (`empty_file_hash_mismatch`). Regular files with `size > 0`: each index in order (`chunk_index_out_of_range`); the
   running sum (`size_overflow`); the sum equals `size` (`chunk_size_mismatch`); the single-chunk rule
   (`single_chunk_hash_mismatch`).
8. Symbolic links: target rules of [paths.md §3](paths.md#3-symbolic-link-targets) in their order.

## 11. After validation (informative)

A consumer that installs a version downloads each distinct chunk once, checks its length and its hash before writing
it, and writes it to every (file, offset) that references it. It verifies each file against `Entry.hash` (or each chunk
against its id) before it reports the version as installed, and it stores the exact signed envelope bytes as the
baseline of the installation. Updates are computed from two manifests as a set difference; the server never computes
deltas.
