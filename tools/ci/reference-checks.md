# Reference integration counterexamples

The test files below use built-in Node modules. Import fixtures are parsed by
Node's built-in TypeScript stripper without executing them. They call the same
edge admission function as the compiler-based scanner. This tests resolution
and admission; it does not replace the full AST extraction, type checking or
inspection of installed dependency declarations.

## Import admission

`test/reference-import-admission.test.mjs` contains these cases. The positive
fixture and harmless comment control must pass before rejected mutations count.

| Test name | Rule | Required diagnostic |
|---|---|---|
| `reference admission rejects direct import at the resolved product path` | Local imports cannot reach product/generated code. | `forbidden resolved path` containing the real product file. |
| `reference admission rejects package imports alias at the resolved product path` | Package aliases do not bypass real-path admission. | Same real product path. |
| `reference admission rejects pure type import at the resolved product path` | Erased type imports remain in scope. | Same real product path. |
| `reference admission rejects aliased type import at the resolved product path` | Type aliases remain in scope. | Same real product path. |
| `reference admission rejects literal dynamic import at the resolved product path` | Resolve known targets before rejecting runtime loading. | Same real product path. |
| `reference admission rejects CommonJS import at the resolved product path` | CommonJS cannot bypass the boundary. | Same real product path. |
| `reference admission follows reexports before rejecting the product path` | Follow the bridge into the forbidden source. | Same real product path. |
| `reference admission follows type reexports before rejecting the product path` | Follow erased reexports too. | Same real product path. |
| `reference admission rejects a computed unknown target without pretending it resolved` | Unknown loading expressions fail closed. | `unanalyzable target` and the importing file. |
| `reference admission rejects an unanalyzable package export after resolving its installed target` | Unsupported package export fallbacks fail closed. | `unanalyzable package`. |
| `reference admission keeps an allowed import and harmless comment control, but rejects runtime loading of it` | Even an allowed target cannot grant dynamic import or require. | `forbidden` containing the resolved local target. |

Every rejected fixture first passes syntax parsing. An unknown expression has
no resolved target; inventing one would misrepresent the evidence.

`test/reference-resolution.test.mjs` also covers installed aliases, Node
import/require/type export conditions, missing declarations, `.ts` declaration
substitution, symlink escapes and per-module filesystem/dependency grants.
`reference-import-cases.mjs` supplies the full compiler-AST counterexamples:
direct/aliased/reexported types, import-type expressions, dynamic paths, escaped
require, CommonJS bindings, WASM, transitive dependency exports/declarations,
filesystem capabilities, flag shadowing, handle escapes and reader/writer
separation. Those AST fixtures are analyzed rather than executed and require
the pinned compiler. Syntax failures are not successful mutation detections.

The existing AST fixture IDs and required diagnostics are:

| IDs | Required diagnostic |
|---|---|
| `direct-type`, `alias-type`, `reexport-type`, `import-type-expression`, `alias-reexport-type`, `literal-dynamic-product`, `symlink-product`, `transitive-package-export`, `poisoned-type-export` | `forbidden resolved path` |
| `dynamic-path`, `literal-dynamic-own-module` | `forbidden dynamic import` |
| `aliased-require`, `literal-require-own-module` | `forbidden require` |
| `commonjs-module` | `forbidden reflective capability` |
| `file-read`, `unbounded-async-file-read` | `forbidden builtin` |
| `process-loader` | `forbidden process capability` |
| `type-tool-in-runtime` | `forbidden resolved package` |
| `native-code` | `executable extension` |
| `local-container-dependency` | `forbidden container dependency` |
| `contract-container-transitive` | `forbidden container module` |
| `runtime-module-resolution` | `forbidden loader capability` |
| `reader-cannot-write`, `writer-cannot-read`, `filesystem-namespace` | `unapproved filesystem capability` |
| `filesystem-capability-reexport` | `filesystem capability escapes` |
| `writer-open-not-exclusive`, `reader-open-flags` | `unapproved filesystem open flags` |
| `writer-handle-read`, `writer-handle-export`, `reader-handle-write`, `reader-handle-export` | `filesystem handle escapes or uses an unapproved operation` |
| `writer-handle-assignment` | `unapproved filesystem handle assignment` |
| `writer-reset-shadowed` | `filesystem reset binding is shadowed` |
| `reader-no-randomness-grant` | `unapproved crypto capability` |
| `reader-flags-shadowed` | `filesystem flag binding is shadowed` |
| `reader-open-unowned` | `filesystem open requires an owned constant handle` |
| `missing-declaration-import` | `unresolved file import` |

Positive controls are `baseline`, `comment-control`, `literal-report-key`,
`container-adapter-baseline`, `separate-reader-writer-baseline` and
`declaration-extension-baseline`. They must survive; none counts as a detected
harmful mutation.

## Required workflow execution

`test/protocol-workflow.test.mjs` applies the existing test pattern
`required checks/<step> rejects <mutation>` to each of:

- `Reference types and imports`
- `Reference conformance`
- `Reference counterexamples`

| Mutation | Required diagnostic |
|---|---|
| `removed` | `checks needs exactly one required step: <step>` |
| `skipped` | `checks required execution block differs: <step>` |
| `nonblocking` | Same execution-block diagnostic. |
| `suppressed failure` | Same execution-block diagnostic. |
| `conditional wrapper` | Same execution-block diagnostic. |
| `colon instead of invocation` | Same execution-block diagnostic. |
| `different shell` | Same execution-block diagnostic. |
| `different directory` | Same execution-block diagnostic. |
| `different job` | `checks needs exactly one required step: <step>` |
| `reordered` | `checks required step out of order:` |
| `job shell override` | Execution-block diagnostic for the affected step. |

This is 33 mutations for the reference steps within the full 27-block contract.
Existing tests also require both directory filters, separate manifest existence
checks, `--fail-if-no-match`, output arguments, the container invocation and the
pinned unconditional evidence upload, including after failure. The workflow
trigger, environment and job-dependency mutations remain in force.

## Product exclusion

`test/reference-products.test.mjs` exercises the production source-admission
check invoked by the required pins block as `node tools/ci/reference-products.mjs --root .`.
Removing this invocation is a workflow counterexample. Its real-source and synthetic positive
baselines must pass. Tests named `runtime reference boundary rejects <case>`
have these required errors:

| Case | Required diagnostic |
|---|---|
| `removed context exclusion` | `build-context exclusions` |
| `overridden context exclusion` | `build-context exclusions` |
| `workspace runtime dependency` | `forbidden reference dependency` |
| `aliased build dependency` | `forbidden reference dependency` |
| `file dependency` | `reference dependency` |
| `lock-only dependency` | `pnpm-lock.yaml:server` |
| `source import` | `reference source or dependency` |
| `source symlink` | `reference source linked` |
| `client resource escape` | `client resource escapes` |
| `client frontend escape` | `client frontend source escapes` |

The final Docker context exclusions protect the reference package and corpus.
The Node manifest/lock checks include build dependencies and aliases. Client
resource and frontend roots cannot expand to the repository root. This source
check complements the existing Docker context, frozen installation, Rust
product graph, artifact and tool-classification tests. Built server and client
artifacts still require validation in their actual build environment; static
source inspection is not proof against arbitrary computed application code.

## Reports and file access

Behavioral mutation admission requires fifteen baseline witnesses and seventeen
harmful changes: three PAE, five purpose, three claims, four expectation and two
priority mutations, plus four harmless controls. The additional purpose changes
are `purpose.omit-type-binding` and `purpose.omit-pae-domain`; each must produce
`ERR_ASSERTION:actual-versus-fixed-expectation` on its executed witness.

Container admission requires all 46 BLAKE3 results and complete version-3
coverage. The coverage package command invokes the existing container entry,
so its execution-bound coverage includes these hash results. No workflow step
is skipped or made nonblocking.

`test/reference-report.test.mjs` rejects false successes: failed/missing baseline
witnesses, unexecuted mutation targets, missing traces, syntax or wrong failures,
unchanged results counted as detections, surviving harmful changes, failing
harmless controls, missing/duplicate mutations, inconsistent category/summary
counts and pending confirmations. The required error is
`reference counterexample execution evidence is incomplete or inconsistent`,
or `reference acceptance lacks complete confirmation evidence`.

`test/reference-boundary.test.mjs` distinguishes confirmed expectations from
pending hash execution. Confirmation does not claim that a pending hash check
ran. Container execution must have no pending results and must include its
executed BLAKE3 evidence.

`test/reference-access.test.mjs` exercises the actual project loader and writer
in isolated copies: outside hardlinks, symbolic targets, directory changes at
the rename boundary, path escapes, documentation/source read lists, installed
directory exclusion and explicit output arguments. A boundary violation is an
assertion failure; filesystem, syntax or setup failures are not counted as
successful detections. The exact source exception remains covered separately
by `test/reference-source.test.mjs`.

`test/reference-notice.test.mjs` retains each approved foreign-source attribution,
the benchmark provenance reference, exact redistributed licence digests and the
restriction to individual numerical NIST pairs. The foreign source inventory
contains metadata about original inputs; it does not redistribute the original
source files listed in that metadata.
