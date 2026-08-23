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

Third-party runtime is Wasmtime Component Model with a narrow WIT world. Plugins export description, validation, invocation, cancellation, and optional migration. Host imports are capability-specific: structured logging, scoped state, monotonic time, randomness, visual feedback, named secret resolution, domain-restricted HTTP, user-selected filesystem handles, and integration broker.

Plugins receive no raw environment, filesystem, sockets, process execution, clipboard, or secrets.

## Default limits

- 64 MiB memory; 256 MiB hard marketplace maximum.
- 10 MiB component.
- Fuel plus epoch interruption.
- Two-second synchronous invocation.
- Per-plugin concurrency four.
- No network, files, environment, clipboard, process, or secrets by default.

HTTP grants specify HTTPS, exact domain/port, redirect policy, response limit, DNS-rebinding defense, and private/link-local denial unless a separately reviewed local-network capability is granted. Files use user-selected handles, not arbitrary string paths.

Permission increases on update require new consent. Marketplace packages are signed. Unsigned sideloading requires explicit developer mode and per-install warning. Native dynamic plugins are out of scope.

Privileged process execution remains an audited built-in adapter with an executable allowlist and argument preview.
