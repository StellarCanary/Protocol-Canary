# Soroban ScVal Fixture Specification & Authoring Guide

This document defines the Soroban `ScVal` (Contract Value) encoding rules, schema, and authoring guidelines for `Protocol-Canary` test fixtures.

It covers both fixture execution contexts:
1. **Soroban Contract Invocations (`surface = "soroban"`)**: High-level declarative arguments in fixture files (`[[args]]`), which the runner translates into XDR `ScVal` structures to build unsigned `InvokeHostFunction` transaction envelopes for `simulateTransaction`.
2. **Raw XDR Fixtures (`surface = "xdr"`)**: Low-level base64-encoded `ScVal` payloads (`type = "ScVal"`) evaluated directly against the official `stellar-xdr` crate for decode, roundtrip, and equality checks.

---

## 1. Soroban Contract Invocations (`surface = "soroban"`)

### 1.1 The `[[args]]` Table Schema

In a Soroban fixture, contract arguments are supplied in order as an array of tables named `args`. Each entry represents exactly one `ScVal`:

```toml
[[args]]
kind = "<type-name>"
value = <typed-value>
```

#### Strict Validation Rules
- **No Unknown Keys**: Each `[[args]]` table accepts only `kind` and `value` (and `value` is omitted for `void`). Any unknown key causes exit code 4 (`FixtureError::Parse`).
- **Nesting Limit**: Nested composite collections (`vec` and `map`) may not exceed a recursion depth of **8 levels** (`MAX_ARG_DEPTH = 8`). Excessive nesting is rejected with `BuilderError::RecursionDepthExceeded`.
- **Unique Map Keys**: Soroban maps are keyed collections. A map argument with duplicate keys is rejected with `BuilderError::DuplicateMapKey`.
- **Canonical Map Sorting**: The runner automatically sorts map entries in ascending key order when building the XDR envelope. Authors do not need to pre-sort entries, and entry order in the TOML file does not affect the deterministic output.
- **Secret Seed Protection**: A secret seed strkey (`S...`) supplied as an address is rejected immediately and is never logged or echoed in error messages.

---

## 2. Supported ScVal Variants & Encoding Rules

`Protocol-Canary` supports all scalar, identifier, wide-integer, temporal, raw-data, and composite variants needed to model Soroban contract calls.

| `kind` | Description | Accepted TOML Format | Bounds / Constraints |
|---|---|---|---|
| `void` | Carries no payload | Omitted or ignored | Maps to `ScVal::Void` |
| `bool` | Boolean value | TOML boolean (`true` or `false`) | Maps to `ScVal::Bool` |
| `u32` | 32-bit unsigned integer | Integer (`0` to `4294967295`) | Non-negative integer |
| `i32` | 32-bit signed integer | Integer (`-2147483648` to `2147483647`) | Standard signed integer |
| `u64` | 64-bit unsigned integer | Integer (`0` to `18446744073709551615`) | Non-negative integer |
| `i64` | 64-bit signed integer | Integer (`-9223372036854775808` to `9223372036854775807`) | Standard signed integer |
| `u128` | 128-bit unsigned integer | Decimal string or `0x`-prefixed hex string | `0` to `2^128 - 1` |
| `i128` | 128-bit signed integer | Decimal string or `0x`-prefixed hex string | `-2^127` to `2^127 - 1` |
| `u256` | 256-bit unsigned integer | Decimal string or `0x`-prefixed hex string | `0` to `2^256 - 1` |
| `i256` | 256-bit signed integer | Decimal string or `0x`-prefixed hex string | `-2^255` to `2^255 - 1` |
| `duration` | Elapsed time in seconds | Non-negative integer | 64-bit unsigned count of seconds |
| `timepoint` | Unix epoch timestamp | Non-negative integer | 64-bit unsigned Unix seconds |
| `symbol` | Soroban symbol | ASCII string | Maximum 32 characters (`[a-zA-Z0-9_]`) |
| `string` | UTF-8 text | UTF-8 string | Arbitrary UTF-8 string |
| `bytes` | Binary data | `0x`-prefixed hex or standard base64 | Hex: even digits. Base64: canonical padded (`=`) |
| `address` | Stellar Account or Contract | Valid strkey string | `G...` (Ed25519 public key) or `C...` (Contract ID) |
| `vec` | Ordered list of values | Array of `{ kind, value }` tables | Up to 8 levels of nesting |
| `map` | Key-value associative mapping | Array of `{ key, value }` tables | Unique keys, auto-sorted, depth <= 8 |

---

## 3. Type-Specific Details

### 3.1 128-Bit and 256-Bit Wide Integers (`u128`, `i128`, `u256`, `i256`)
Standard TOML parsers represent integers as 64-bit signed values. Values exceeding 64 bits must be written as strings to preserve full precision without loss:

- **Decimal format**: A base-10 digit string (with optional leading `-` for signed kinds).
- **Hexadecimal format**: Prefixed with `0x` or `0X` (e.g. `"0xffffffffffffffffffffffffffffffff"`).
- **Negative Hex**: Prefixed with `-0x` or `-0X` (e.g. `"-0x80000000000000000000000000000000"`).

Values exceeding the bit width fail with `BuilderError::IntegerOverflowError`. Non-numeric or floating-point strings fail with `BuilderError::InvalidArgument`.

### 3.2 Binary Data (`bytes`)
To avoid ambiguity where a string like `"abcd"` is valid in both hex and base64 alphabets, `Protocol-Canary` applies a deterministic rule:
- **Hexadecimal**: If the string starts with `0x` or `0X`, it is decoded as hex digits. The length must be even.
- **Base64**: If the string does not start with `0x`, it is decoded as standard padded base64. The length must be a multiple of 4 with canonical `=` padding and zero trailing bits.

### 3.3 Addresses (`address`)
Addresses must be encoded as valid Stellar strkey strings with correct CRC16 checksums:
- **Accounts**: Public Ed25519 addresses starting with `G` (e.g. `GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF`).
- **Contracts**: Contract hash addresses starting with `C` (e.g. `CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC`).
- **Prohibited**: Secret seeds (`S...`) are rejected with an explicit error and are never included in output.

### 3.4 Composite Structures (`vec` and `map`)
- **`vec`**: An array of nested tables, where each element is an argument table `{ kind = "...", value = ... }`.
- **`map`**: An array of nested tables, where each element has `{ key = { kind = "...", value = ... }, value = { kind = "...", value = ... } }`.
- **Recursion**: Vectors and maps can contain further vectors and maps up to 8 levels deep. Exceeding depth 8 fails fixture validation.

---

## 4. Complete TOML Fixture Examples

The following complete fixture files demonstrate each ScVal variant in valid and invalid scenarios.

### 4.1 Scalar & Primitive Arguments

<!-- contract-example: scval-valid -->
```toml
id = "p28-soroban-scval-scalars"
protocol = 28
surface = "soroban"
category = "scval"
description = "Verify simulation of scalar ScVal arguments (void, bool, integers, symbols, strings)"
source_reference = "CAP-0085"
required_capabilities = ["soroban-contract"]

source_account = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"
contract_id = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
function = "test_scalars"
sequence_number = 1

[[args]]
kind = "void"

[[args]]
kind = "bool"
value = true

[[args]]
kind = "u32"
value = 42

[[args]]
kind = "i32"
value = -42

[[args]]
kind = "u64"
value = 1000000000

[[args]]
kind = "i64"
value = -1000000000

[[args]]
kind = "symbol"
value = "transfer"

[[args]]
kind = "string"
value = "Hello, Stellar Protocol 28!"

[expect]
kind = "simulation-success"
```

### 4.2 Wide Integers and Temporal Types

<!-- contract-example: scval-valid -->
```toml
id = "p28-soroban-scval-wide-and-time"
protocol = 28
surface = "soroban"
category = "scval"
description = "Verify simulation of 128-bit, 256-bit integers and time types"
source_reference = "CAP-0085"
required_capabilities = ["soroban-contract"]

source_account = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"
contract_id = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
function = "test_wide"
sequence_number = 1

[[args]]
kind = "u128"
value = "340282366920938463463374607431768211455"

[[args]]
kind = "i128"
value = "-170141183460469231731687303715884105728"

[[args]]
kind = "u256"
value = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"

[[args]]
kind = "i256"
value = "-57896044618658097711785492504343953926634992332820282019728792003956564819968"

[[args]]
kind = "duration"
value = 86400

[[args]]
kind = "timepoint"
value = 1729166400

[expect]
kind = "simulation-success"
```

### 4.3 Binary Data and Strkey Addresses

<!-- contract-example: scval-valid -->
```toml
id = "p28-soroban-scval-bytes-and-addresses"
protocol = 28
surface = "soroban"
category = "scval"
description = "Verify hex bytes, base64 bytes, account addresses, and contract addresses"
source_reference = "CAP-0085"
required_capabilities = ["soroban-contract"]

source_account = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"
contract_id = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
function = "test_bytes_address"
sequence_number = 1

[[args]]
kind = "bytes"
value = "0xdeadbeefcafe"

[[args]]
kind = "bytes"
value = "3q2+7w=="

[[args]]
kind = "address"
value = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"

[[args]]
kind = "address"
value = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"

[expect]
kind = "simulation-success"
```

### 4.4 Composite Collections: Vectors and Maps

<!-- contract-example: scval-valid -->
```toml
id = "p28-soroban-scval-composite-vec-map"
protocol = 28
surface = "soroban"
category = "scval"
description = "Verify nested vectors and associative maps with heterogeneous ScVal keys and values"
source_reference = "CAP-0085"
required_capabilities = ["soroban-contract"]

source_account = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"
contract_id = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
function = "test_composites"
sequence_number = 1

[[args]]
kind = "vec"
value = [
  { kind = "symbol", value = "token_a" },
  { kind = "symbol", value = "token_b" },
  { kind = "u32", value = 100 },
]

[[args]]
kind = "map"
value = [
  { key = { kind = "symbol", value = "admin" }, value = { kind = "address", value = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF" } },
  { key = { kind = "symbol", value = "threshold" }, value = { kind = "u32", value = 2 } },
  { key = { kind = "symbol", value = "sub_vec" }, value = { kind = "vec", value = [{ kind = "bool", value = true }, { kind = "bool", value = false }] } },
]

[expect]
kind = "simulation-success"
```

---

## 5. Raw XDR ScVal Fixtures (`surface = "xdr"`)

For testing low-level XDR serialization, roundtrip fidelity, and non-canonical rejection directly against `stellar-xdr`, use `surface = "xdr"` with `type = "ScVal"`.

### 5.1 ScVal XDR Decode Success / Roundtrip

<!-- contract-example: scval-valid -->
```toml
id = "p28-xdr-scval-roundtrip"
protocol = 28
surface = "xdr"
category = "scval"
description = "Verify round-trip encoding of ScVal::U32(42)"
source_reference = "RFC-4506"

type = "ScVal"
kind = "roundtrip"
value_base64 = "AAAAAwAAACo="
```

### 5.2 ScVal XDR Decode Failure

<!-- contract-example: scval-valid -->
```toml
id = "p28-xdr-scval-decode-failure"
protocol = 28
surface = "xdr"
category = "scval"
description = "Assert rejection of invalid XDR discriminant for ScVal"
source_reference = "RFC-4506"

type = "ScVal"
kind = "decode-failure"
value_base64 = "////AAAAAAA="
```

---

## 6. Negative Fixture Examples (Invalid ScVal Authorship)

The following examples demonstrate authorship errors that `Protocol-Canary` explicitly rejects during fixture validation.

### 6.1 Duplicate Map Keys

<!-- contract-example: scval-invalid -->
```toml
id = "p28-soroban-invalid-duplicate-map-key"
protocol = 28
surface = "soroban"
category = "scval"
description = "Map argument with duplicate keys must be rejected"

source_account = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"
contract_id = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
function = "test_dup"
sequence_number = 1

[[args]]
kind = "map"
value = [
  { key = { kind = "symbol", value = "key1" }, value = { kind = "u32", value = 1 } },
  { key = { kind = "symbol", value = "key1" }, value = { kind = "u32", value = 2 } },
]

[expect]
kind = "simulation-success"
```

### 6.2 Excessive Recursion Depth (> 8 levels)

<!-- contract-example: scval-invalid -->
```toml
id = "p28-soroban-invalid-excessive-depth"
protocol = 28
surface = "soroban"
category = "scval"
description = "Vec argument nested beyond MAX_ARG_DEPTH (8) must be rejected"

source_account = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"
contract_id = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
function = "test_depth"
sequence_number = 1

[[args]]
kind = "vec"
value = [
  { kind = "vec", value = [
    { kind = "vec", value = [
      { kind = "vec", value = [
        { kind = "vec", value = [
          { kind = "vec", value = [
            { kind = "vec", value = [
              { kind = "vec", value = [
                { kind = "vec", value = [
                  { kind = "u32", value = 1 }
                ]}
              ]}
            ]}
          ]}
        ]}
      ]}
    ]}
  ]}
]

[expect]
kind = "simulation-success"
```

### 6.3 Secret Seed Provided as Address

<!-- contract-example: scval-invalid -->
```toml
id = "p28-soroban-invalid-secret-seed-address"
protocol = 28
surface = "soroban"
category = "scval"
description = "Secret seeds are not addresses and must be strictly rejected without logging"

source_account = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"
contract_id = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
function = "test_seed"
sequence_number = 1

[[args]]
kind = "address"
value = "SBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"

[expect]
kind = "simulation-success"
```

---

## 7. Verification and Testing

Authors can verify newly created fixtures locally with `stellar-canary`:

```bash
# Check syntax and planned execution without contacting any RPC node:
stellar-canary inspect --fixtures-dir <fixtures-directory> --protocol 28

# Run simulation checks against a configured RPC endpoint:
stellar-canary check --fixtures-dir <fixtures-directory> --protocol 28

# Execute the test suite asserting documentation example validity:
cargo test -p canary-cli --test doc_scval_examples
```
