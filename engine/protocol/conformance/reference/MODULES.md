# Modules and imports

The runtime graph contains package modules and Node standard modules. The container entry statically imports @noble/hashes/blake3 and passes the hash function to container/blake3.ts. Package-local imports use explicit .ts or .mjs extensions.

Protocol modules are api, auth-rpc, documents, ed25519, events, fastcdc, flows, get-routes, json-profile, models, paths, primitives, profile, recipes, rpc, state-contracts and transport in src/. Validation modules are loader, gates, format-gates, provenance and publication. The locations module defines the package and test data roots.

The standard-library command entries are tooling/stage.mjs and tooling/check.mjs. Their shared option parser is tooling/options.mjs; tooling/imports.mjs inventories static runtime and type-only edges. Run the imports contract with an explicit --out directory to obtain the complete module and import list. Only src/loader.ts reads data or package files; its consumers receive bytes, text and inventories. Only tooling/output.mjs writes reports, below the resolved --out directory.

The counterexamples module evaluates finite operation replacements through the signed-case evaluator used by the ordinary gates. It imports no file-system or execution module and loads no generated source. Provenance validates value-bound confirmations before execution, including construction-value references.

The container entry is container/run.mjs and its dependency adapter is container/blake3.ts. Delivered-tree checks enumerate the package's source, tooling, container and documentation files and the test data. They exclude node_modules and directories outside that delivered structure. Dependency version, installation and licence validation belong to the repository lock-file and licence checks.

The type-tool graph is separate: TypeScript, Node declarations and dependency declarations are supplied as pinned validation tools. Type checking runs with --noEmit. The source inventory does not substitute for a resolved dependency scan.

## Explicit file read permissions

readPackageText accepts only README.md, FORMAT.md and MODULES.md. Package metadata and licence files are enumerated for size/type checks without opening their contents. Test data reads remain confined to the test data root.

readSelfCheckText is a separate, fixed source-text permission for the following files only. Each file is needed by the static runtime/type import inventory and the generic source-path checks; the container sources are read as text, never imported by the local stage graph. Any new source file requires an explicit list change. Other paths are rejected.

- `container/blake3.ts`
- `container/run.mjs`
- `src/api.ts`
- `src/auth-rpc.ts`
- `src/counterexamples.ts`
- `src/documents.ts`
- `src/ed25519.ts`
- `src/events.ts`
- `src/fastcdc.ts`
- `src/flows.ts`
- `src/format-gates.ts`
- `src/gates.ts`
- `src/get-routes.ts`
- `src/json-profile.ts`
- `src/loader.ts`
- `src/locations.ts`
- `src/models.ts`
- `src/paths.ts`
- `src/primitives.ts`
- `src/profile.ts`
- `src/provenance.ts`
- `src/publication.ts`
- `src/recipes.ts`
- `src/rpc.ts`
- `src/state-contracts.ts`
- `src/transport.ts`
- `tooling/check.mjs`
- `tooling/imports.mjs`
- `tooling/options.mjs`
- `tooling/output.mjs`
- `tooling/stage.mjs`
