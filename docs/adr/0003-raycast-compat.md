# 3. Raycast Script Command Compatibility

* Status: Decided
* Date: 2026-08-31

## Context
Many users have existing script command setups written for Raycast. We want them to easily reuse these scripts in aeroFi. However, supporting full Raycast extensions (which are React/TypeScript applications running on Node.js/V8) would require bundling a JavaScript runtime (like QuickJS, Deno, or Node), which would push memory usage well beyond our < 30 MB idle / < 40 MB active RSS budget.

## Decision
1. We support Raycast **Script Commands** by parsing their metadata tags (such as `# @raycast.title`, `# @raycast.mode`, `# @raycast.icon`, `# @raycast.argument*`) in the indexing scanner.
2. We explicitly do **not** support the Raycast Extension API, TypeScript/React extensions, or integration with the Raycast Store.

## Consequences
- **Positive**:
  - Existing Raycast-compatible scripts work unmodified when added to the scripts folder.
  - Memory footprint remains very low since no JavaScript engine is compiled or loaded into the daemon.
  - We do not have to maintain compatibility with a third-party extension framework API.
- **Negative / Trade-offs**:
  - Rich UI extensions or store integrations cannot be used in aeroFi; script functionality is restricted to basic command-line execution and metadata directives.
