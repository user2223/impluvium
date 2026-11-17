# Reference data format 2

Own code and format documentation are 0BSD. Foreign numerical inputs and notices retain their separate provenance.

## Corpus shards

`INDEX.json` pins each corpus shard and asset by SHA-256. A corpus shard has format `reference-corpus-v2`, its logical `group`, and local `rules`, `origins` and expected `values` tables. A case refers to these tables. Provenance defaults apply separately to every input and expected leaf; explicit path overrides take precedence. The origin field ID `$case$path` expands to the case ID followed by the JSON pointer. This preserves field-level origins without repeating hashes and rules. A singleton `$value` reference is metadata, never executable data. Small unpooled `reference-corpus-v1` shards carry the same case fields, rule strings, expected objects and explicit provenance pointers directly. Both formats undergo the same field validation after unpacking.

## Byte recipes

Byte recipes are stored in pinned `recipes-NNN.json` assets with format `byte-recipes-v1`. Each recipe has an ID matching `bytes-[0-9]{6}`, `op`, `byteLengthU64` and `sha256Hex`. Lengths and other unsigned integers are canonical decimal strings in the u64 range; literal bytes and digests use lowercase hex. Every referenced recipe carries its own length and digest. Extra recipe fields are rejected. The permitted operation-specific fields are:

| Operation | Fields and meaning |
|---|---|
| `literal` | `hex`: those exact octets. |
| `repeat` | `source`, `countU64`: concatenate that many exact source sequences. |
| `concat` | `parts`: concatenate the listed recipe IDs in list order; an empty list denotes empty bytes. |
| `insert` | `source`, `insert`, `atU64`: source prefix before `atU64`, inserted bytes, then the remaining source. Position may equal source length. |
| `slice` | `source`, `fromU64`, `takeU64`: the stated half-open range, wholly within the source. |
| `counter` | `startU64`, `stepU64`, `width`, `endian`, `outputBytesU64`: word j is `(start + j*step) mod 2^(8*width)`, encoded in `width` bytes in `little` or `big` byte order; concatenate words and truncate to exactly `outputBytesU64`. Width is 1, 2, 4 or 8; start and step must fit it. This is file content, never a FastCDC seed. |

Compiled bounds cannot be widened by data: 8192 stored nodes, 256 distinct reachable nodes and 4096 node visits per root, depth 32 (root depth 1), 128 concatenation children, 65536 literal bytes, and 1073741824 designated bytes per recipe. Cycles and missing references fail. Each node's structural length is checked before expansion. Before use, its complete byte stream and every reachable child's stream are checked against their length and SHA-256. Processing blocks have at most 65536 bytes. Materialization above 33554432 bytes is forbidden; larger probes consume only a verified stream.

A singleton `{"$bytes":"bytes-NNNNNN"}` denotes the complete hex value of those bytes when a case executes. A singleton `{"$utf8":"bytes-NNNNNN"}` denotes strict UTF-8 text. These markers cannot introduce code. Large JSON inputs still designate one exact complete UTF-8 JSON text; normative Base64 inside that text is unchanged. Materialized test bytes are discarded after their case. `size-precheck` cases use `streamRecipe` and are explicitly total-size substages, not valid full documents.

## Canonical span encoding

Full short cut sequences use decimal `offsetU64`/`lengthU64` pairs. Their canonical encoding is ASCII `offset:length\n` for each span in order, with no leading zeros. The first offset is zero, spans are contiguous, lengths positive, and sums fit u64. The empty sequence encodes as empty bytes. A future summary must carry count, SHA-256 of exactly this encoding, and indexed samples; it cannot replace a full semantic expectation where the protocol needs every element.

## Calculation provenance

Pinned provenance-NNN.json files use the field-calculations-v1 format. Local sources, rules and methods tables remove repeated metadata. Every field record retains its stable fieldId and references a rule, a one-sentence calculation method and source entries. Each source names its document, sections and source-file SHA-256. Numerical foreign inputs retain their original source digest and license notices.

Each record has secondCalculation with method and result. The initial method is empty and result is pending. A confirmed result requires a nonempty independent method and an actual comparison against the fixed expectation. Verification never changes these fields. The provenance test counts confirmed and pending records; a comparison pass does not accept an expectation whose independent calculation remains pending.

## Package layout and execution

Data locations derive exclusively from the package module location: ../../testdata/ relative to the package directory. They do not depend on the process working directory. The loader rejects symbolic links in every path component below each root and exposes data only. Delivered-tree enumeration excludes node_modules and directories outside the package's declared structure.

Every JavaScript command requires --out <directory>. Reports are written exclusively below that directory, which must be outside the package and test data roots after real-path resolution. No output directory is created when the argument is absent or invalid. Only tooling/output.mjs writes reports; an existing file symlink is replaced without writing to its target. Type checking uses the separately installed compiler with --noEmit and writes no files.

All example commands are relative to the package directory: node tooling/stage.mjs --stage 3 --out ../../results/3, node tooling/stage.mjs --stage 4 --out ../../results/4, and node tooling/stage.mjs --stage 5 --out ../../results/5. Stages 4 and 5 select signed-format and protocol/model checks. Stage 6 evaluates behavioral counterexamples only when their prerequisites are satisfied. The eight package scripts define typecheck, imports, corpus, coverage, primitives, vectors, limits and counterexamples; runtime contracts accept --out, and vectors additionally accepts --stage 4. Without that selection, vectors executes every group.

## Publication requirements

The delivered test data and package files total at most 8388608 bytes, with at most 1048576 bytes per file. Runtime comparison contracts check these bounds before comparing cases. Generic publication checks validate module import containment and fixed filesystem locations. The only external data directory is the designated testdata/ directory. Reports use only the explicit output argument. Standard Node modules and the pinned container-only dependency are allowed imports.

Generic publication checks validate module and metadata path containment. Missing normative precedence is represented by a blocked case with a condition that names the unspecified rule and its source sections. Such a case contains no expected outcome.

## Injected state models

Ticket-cache cases classified cache-state-substage operate below full ticket verification. Ticket octets are opaque cache identities. The separate claims, valid-key metadata and freshVerificationValid/freshReason values are injected inputs to that state layer; they do not assert that those claims were decoded from those octets or that a full signature check has run. A successful state-layer result is not a full ticket known answer.

Auth state outputs describe the final modeled stored snapshot, rather than transitions performed only by the current invocation. On reaching the atomic recheck, true atomicConsumed, atomicCompleted, atomicRevoked, atomicDeleted, atomicDenied or atomicApprovalRemoved flags inject the corresponding stored-state changes before checks run. Unnamed components retain their supplied state until an operation changes them. These partial snapshots do not reconstruct the complete execution history of another transaction. A losing operation does not undo an injected transition.

For a successful token-checked RevokeDevice exchange, context.authenticatingDeviceId identifies the device bound to the authenticated token. The revokesCallingDevice result compares that identity with the request target. RevokeSelf always targets the caller. A RevokeDevice fixture without the token device identity is incomplete and cannot produce that result.

## Report file replacement

Report output uses a new, exclusively created file in the resolved output directory, followed by rename to the requested report name. Existing report files are never opened for writing, so replacement does not overwrite another name for the same inode. File symlinks are replaced as directory entries; directories and symlinks to directories are rejected. Temporary files are removed on failure. Cryptographic randomness is used only for temporary file names, never as corpus input or an expected value.

Package documentation reads are limited to README.md, FORMAT.md and MODULES.md. The separate source-text read permission has the exact fixed list and purposes documented in MODULES.md.

## Bound successful responses

For a chunk response, context.successLimit is the concrete successful response-body read bound fixed before receiving a response. It is the known manifest chunk size for a full request, or the normalized requested range length for a ranged request, rather than the generic maximum chunk size. Response headers do not choose or widen it. Response-protection fixtures examine supplied response bytes and their request binding; they do not assert an outbound authentication exchange.

## Local diagnostics and response heads

The field of a local diagnostic or warning is determined by errors.md section 5 and its explicit chapter assignments. Corpus labels are exact protocol expectations; no independent naming convention applies. Nonempty warning lists may carry parallel warningFields strings in the same order. Empty warning lists omit that projection.

HTTP/1.1 response protection counts each informational and final head separately, from the first status-line octet through the terminating empty line, under transport.md section 6.1. The response-head-size operation is a decoded HTTP/2 header-list precheck: protocol h2, ordered heads and ordered nameHex/valueHex pairs; it reports accepted and sizesU64, or headers_too_large with empty field, zero-based headIndex and measuredU64. It does not decode HPACK or assert a full HTTP/2 exchange. Encoding diagnostic probes are profile substages, not complete signed documents or authenticated API exchanges.

## Chunk response observations

A chunk response model supplies two observations at different boundaries. The parsed HTTP response body is the received transfer. `context.completeChunkHex` is the complete candidate buffer exposed by the synthetic assembly or cache boundary before any target write. It is an independently supplied input, not a replacement HTTP body and not content inferred from the expected chunk identifier. Different observations can be injected to probe consistency; the model does not assert that a successful transfer and assembly produced that combination.

HTTP framing and response-body read limits use the response body. Complete-candidate length and manifest-algorithm hashing use `context.completeChunkHex` and the manifest's chunk size and identifier. For a full transfer, compare the response body with the complete candidate. For a ranged transfer, normalize the request's single Range under tickets.md section 7.3 and compare the response body with the corresponding candidate slice, including both endpoints. When a request is supplied, its path identifier and Range bind these comparisons; otherwise the supplied context bindings apply.

The integrity conditions are conjunctive: candidate length, complete-candidate hash and agreement with the received full body or range all belong to the pre-write check. Neither observation silently overwrites the other. The requested projection evaluates these observations under tickets.md sections 7.3–7.4 and the ordinary response handling of transport.md section 6. This is a response-and-candidate consistency model, not solely a classification of the wire body in isolation.

## Value-bound independent confirmations

A confirmed secondCalculation has method independent, result confirmed, checks 1 or 3, and valueSha256 as 64 lowercase hex digits. Check bit 1 means independently recomputed; bit 2 additionally means recomputed without expected results. Pending records have an empty method and neither checks nor valueSha256. Reports count both categories without attributing a calculation to an individual.

The binding is SHA-256 over UTF-8 of reference-confirmation-v1, LF, the JSON-encoded fieldId, LF, and the canonical JSON value. Canonical values have no whitespace; object keys are sorted by UTF-16 code units, array order is retained, strings use JSON escaping, and numbers are exact safe integers. Unsigned 64-bit values remain their prescribed decimal strings. A changed field identity or value invalidates its existing confirmation; changing an INDEX pin does not renew it.

construction-values-NNN.json assets have format construction-confirmation-values-v1 and fields with fieldId plus exactly one of result or valueRef. A valueRef selects caseId and a JSON Pointer path beginning with input or expected. An optional closed projection is envelope-payload or envelope-pae, selecting the binary Envelope payload or its exact pre-authentication encoding. References reuse existing corpus data; they do not execute data as code. Every resulting value is checked against its field-bound confirmation. Every calculated field must have a current value binding.

## Behavioral counterexamples

Stage 6 and counterexamples execute a fixed list of small operation replacements through the ordinary signed-case evaluator. Each recipe names its primary rule, witnesses and target operation. A successful baseline precedes the variants. Detection requires a reached target and a fixed-result comparison failure, a field-confirmation mismatch, or a checked-expectation snapshot mismatch. An unexpected exception, absent witness or empty execution is an invalid run, never successful detection. No generated source, dynamic loading or process isolation is used.

Identical replacements, comment-only replacements, informational metadata edits and permitted unknown Envelope fields are separate equivalence controls. Their surviving results are reported as ineffective. A rule-violating survivor or invalid execution fails the suite. Reports retain the executed operation path and observed test failure for every witness. Reports use compact JSON; whitespace is not significant.

expectationsAccepted reports confirmation of the fixed data independently of execution dependencies. A pending dependency remains pending even when its expectation is independently confirmed. Coverage checks the case inventory, specification conditions and confirmed value bindings; it does not install a missing execution dependency.

Normative literals retain their direct rule provenance. A separate in-memory snapshot of indexed expectations detects later changes to these literals as well as other expectation edits. That snapshot does not add an independent confirmation or change the independently recomputed field counts. Informational rule annotations are outside the value and expectation-snapshot bindings.
