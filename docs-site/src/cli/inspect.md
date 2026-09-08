# `inspect`

Prints offline project diagnostics. No fixtures run, and no network call
is made.

```text
Print offline project diagnostics

Usage: stellar-canary inspect [OPTIONS]

Options:
      --config <CONFIG>  Path to a configuration file (default: .stellar-canary.toml in the project root)
  -h, --help             Print help
```

## What it inspects

The same project-detection logic `check` uses internally, surfaced on its
own: the project root, its detected type, which Stellar-related
dependencies/artifacts were detected, the configured target protocol, and
which surfaces (`xdr`/`rpc`/`soroban`) are currently enabled.

## Example

Run inside a checkout of `Protocol-Canary` itself:

```bash
stellar-canary inspect
```

```text
Project root: /home/hollujay/Protocol-Canary
Project type: unknown

Detected Stellar SDK/XDR dependency: false
Detected Soroban contract usage: false
Detected RPC client dependency: false
Detected WASM artifact: false

Configured protocol: 28
Available compatibility surfaces:
  xdr:     enabled
  rpc:     enabled
  soroban: enabled
```

`Project type: unknown` here is correct — this repository is Canary's own
source code, not a Stellar application, so none of the detection
heuristics (Soroban contract, RPC client, stellar-sdk dependency, WASM
artifact) match. Run `inspect` inside your own project to see its actual
detected type and which fixtures' `required_capabilities` it will satisfy.

`inspect` is useful before `check` when you want to confirm project
detection or configuration without waiting on a live network call.
