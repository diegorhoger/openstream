# Plugin and connector SDK

## V1 package

```text
plugin.openstream/
  manifest.toml
  component.wasm
  icon.svg
  schemas/config.schema.json
  schemas/state.schema.json
  migrations/
  SIGNATURE
```

The manifest defines reverse-DNS ID, semantic version, SDK range, publisher, action types, schemas, requested capabilities, migrations, content hashes, and signature.

## Runtime

Third-party runtime is Wasmtime Component Model with a narrow WIT world. Plugins export description, validation, invocation, cancellation, and optional migration. Host imports are capability-specific: structured logging, scoped state, monotonic time, randomness, visual feedback, domain-restricted HTTP, user-selected filesystem handles, and an opaque integration broker.

Plugins receive no raw environment, arbitrary filesystem, sockets, process execution, clipboard, credential-vault API, secret reference, or secret bytes. When an integration needs a credential, the user selects an Engine-owned connection and the plugin receives only an opaque handle. The broker performs a typed approved operation and returns a redacted result; it never resolves a secret into plugin memory.

## Default limits

- 64 MiB memory; 256 MiB hard marketplace maximum.
- 10 MiB component.
- Fuel plus epoch interruption.
- Two-second synchronous invocation.
- Per-plugin concurrency four.
- No network, files, environment, clipboard, process, or secret access by default.

HTTP grants specify HTTPS, exact domain/port, redirect policy, response limit, DNS-rebinding defense, and private/link-local denial unless a separately reviewed local-network capability is granted. Files use user-selected handles, not string paths.

Permission increases on update require new consent. Marketplace packages are signed. Unsigned sideloading requires explicit developer mode and a per-install warning. Native dynamic plugins are out of scope.

Privileged process execution is never a plugin import. Any later built-in adapter must satisfy the hardened process boundary in `SECURITY.md`.
