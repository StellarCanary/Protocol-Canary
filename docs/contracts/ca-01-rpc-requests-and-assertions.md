# CA-01: RPC request parameters and assertions

Status: frozen for implementation planning, contract version 1.
D-06 is approved (the response-size cap in section 6 and request parameter bounds in section 3).
Authoritative for: which Stellar RPC methods a fixture may call, how request
parameters are declared, how responses are asserted on, how errors are
classified, and what stays compatible.

Everything about Stellar RPC below was read from the official method pages under
<https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/>
on 2026-10-09 (`getNetwork`, `getLatestLedger`, `getHealth`, `getVersionInfo`,
`getFeeStats`, `getLedgerEntries`, `getTransaction`, `getLedgers`,
`getTransactions`, `getEvents`, `simulateTransaction`) and from real responses
captured the same day (see CF-04 section 5). Where a page does not say, this
document says "not documented" and does not guess.

## 1. Existing behavior

Checked against `main` at `1ff7908`:

- A fixture selects a method with `method = "get-network"` or
  `method = "get-latest-ledger"`. These are the only two values
  (`RpcMethod::from_str` in `crates/canary-rpc/src/runner.rs`); anything else is
  rejected when the fixture is turned into an `RpcFixture`. The Fixtures validator
  has the same two (`RPC_METHODS` in `tools/validate/validate.py`).
- Both methods take no parameters, and a fixture cannot supply any: the client
  sends `{}` (`HttpRpcClient::get_network`, `get_latest_ledger`).
- The engine deserializes the response into a typed model (`NetworkInfo`:
  `friendbotUrl?`, `passphrase`, `protocolVersion`; `LatestLedger`: `id`,
  `protocolVersion`, `sequence`) and runs assertions against that model
  re-serialized to JSON. Fields the model does not name are dropped before any
  assertion sees them. The shipped `p28-rpc-latest-ledger` header explains the
  consequence: a `field-absent` assertion on a dropped field would pass for the
  wrong reason.
- Four assertion kinds exist: `field-exists`, `field-absent`, `field-equals`,
  `field-type` (types `string`, `number`, `bool`, `object`, `array`, `null`).
  The `field` is a single top-level key. `field-equals` compares JSON values
  exactly: `28` and `28.0` are different values, a string never equals a number.
- Transport failures, timeouts and HTTP 429 are retried up to 3 attempts with a
  linear 200 ms step; HTTP 5xx is a transport error; a JSON-RPC `error` object,
  invalid JSON and a response with neither `result` nor `error` are not retried.
  All of these end as a result with status `error` (exit code 3).
- A real `getLatestLedger` response on Mainnet carried a `metadataXdr` field of
  about 1.28 million characters on 2026-10-09. The engine downloads and discards it.

## 2. Methods

A fixture's `method` is the kebab-case form of the upstream method name. The
mapping is mechanical and listed so nothing is inferred:

| `method` value | JSON-RPC method | Request params | Documented result fields | Engine today |
|---|---|---|---|---|
| `get-network` | `getNetwork` | none | `passphrase` string, `protocolVersion` number, `friendbotUrl` string (optional) | supported |
| `get-latest-ledger` | `getLatestLedger` | none | `id` string (64 hex), `protocolVersion` number, `sequence` number, `closeTime` string, `headerXdr` string, `metadataXdr` string | supported |
| `get-health` | `getHealth` | none | `status` string, `latestLedger` number, `latestLedgerCloseTime` string, `oldestLedger` number, `oldestLedgerCloseTime` string, `ledgerRetentionWindow` number | specified here |
| `get-version-info` | `getVersionInfo` | none | `version` string, `commitHash` string, `buildTimestamp` string, `captiveCoreVersion` string, `protocolVersion` integer | specified here |
| `get-fee-stats` | `getFeeStats` | none | `sorobanInclusionFee` object, `inclusionFee` object, `latestLedger` number; each fee object: `max`, `min`, `mode`, `p10` to `p99` (`p10 p20 p30 p40 p50 p60 p70 p80 p90 p95 p99`), `transactionCount` as strings, `ledgerCount` number | specified here |
| `get-ledger-entries` | `getLedgerEntries` | `keys` (required, array of base64 `LedgerKey` strings, at most 200), `xdrFormat` | `latestLedger` number, `entries` array of `key`, `xdr`, `lastModifiedLedgerSeq`, `liveUntilLedgerSeq` | specified, no fixture until a request is verified |
| `get-transaction` | `getTransaction` | `hash` (required, 64 lowercase hex), `xdrFormat` | `status` (`SUCCESS`, `NOT_FOUND`, `FAILED`), `txHash`, `latestLedger`, `latestLedgerCloseTime`, `oldestLedger`, `oldestLedgerCloseTime`, plus optional fields present only for `SUCCESS` or `FAILED` | specified, same condition |
| `get-ledgers` | `getLedgers` | `startLedger` (number), `pagination` (`cursor`, `limit` 1 to 200), `xdrFormat` | `ledgers` array, `latestLedger`, `latestLedgerCloseTime`, `oldestLedger`, `oldestLedgerCloseTime`, `cursor` | specified, same condition |
| `get-transactions` | `getTransactions` | `startLedger`, `pagination`, `xdrFormat` | `transactions` array, `latestLedger`, `latestLedgerCloseTimestamp`, `oldestLedger`, `oldestLedgerCloseTimestamp`, `cursor` | specified, same condition |
| `get-events` | `getEvents` | `startLedger`, `endLedger`, `filters` (at most 5), `pagination` (`limit` 1 to 10000), `xdrFormat` | `latestLedger`, `oldestLedger`, `latestLedgerCloseTime`, `oldestLedgerCloseTime`, `events`, `cursor` | specified, same condition |

Corrections to earlier planning documents: the method that reports the node's
version is **`getVersionInfo`**, fixture value `get-version-info`. There is no
`getVersion`. Wherever an earlier note says `getVersion`, read `getVersionInfo`.

Not allowed, ever: `sendTransaction`, and any method not in the table. The set is
closed in code; a fixture cannot name a method by free text. `simulateTransaction`
belongs to the Soroban surface (unsigned transaction, simulation only) and is
specified by the existing Soroban fixture contract, not here.

Notes taken from the method pages that constrain fixtures:

- Params are sent as a named object, never positionally.
- `getTransaction`, `getLedgers`, `getTransactions` and `getEvents` serve only a
  bounded recent history (the stock default is 120960 ledgers, roughly 7 days;
  operators can change it, and `getHealth` reports the live range). A fixture that
  names a ledger sequence, a transaction hash or a cursor will therefore stop
  working when that data ages out, so such a fixture is not durable and is not
  accepted without a design that avoids a hard-coded historical value.
- `getLedgerEntries`: the page does not say what happens to a key with no
  entry. It says `entries` holds "all found ledger entries", which suggests
  missing keys are omitted. That is an inference and must be checked against a
  real response before a fixture relies on it.
- `getVersionInfo`: the page does not say whether the method is gated by a node
  version. An endpoint that does not have it answers with a JSON-RPC error
  (section 5).
- `xdrFormat`: the engine only requests the default (`base64`). A fixture may
  not request `json`, because that changes the response shape the assertions are
  written against.

## 3. Request parameters in a fixture

Zero-parameter methods need nothing. Parameterized methods take a `[request]`
table whose keys are the upstream parameter names, exactly as in the table:

<!-- contract-example: rpc-request-v1 -->
```toml
# Illustrative only. The key and values are placeholders, not real ledger data.
id = "example-rpc-entries"
protocol = 28
surface = "rpc"
category = "ledger"
description = "illustrative parameterized request"
source_reference = "https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/getLedgerEntries"

method = "get-ledger-entries"

[request]
keys = ["AAAAAA=="]

[[assert]]
kind = "field-type"
field = "entries"
expected_type = "array"
```

Rules, all checked when the fixture is loaded so a bad request is an invalid
fixture (exit code 4) and never a runtime surprise:

1. `[request]` is allowed only for methods whose table row has parameters, and is
   required if the row lists a required parameter.
2. Keys must be among the documented parameter names for that method. An unknown
   key is rejected rather than forwarded.
3. Types follow the page: arrays of strings for `keys`; a 64-character lowercase
   hex string for `hash`; non-negative integers for ledger numbers; `limit` within
   its documented range. `xdrFormat`, if present, must be `"base64"`.
4. Documented maxima are upper bounds. The engine may enforce lower ones. Initial
   engine bounds (approved under D-06): at most 50 `keys`, `limit` at most 50, at most 2
   `filters`.
5. The request is the only thing a fixture can influence on the wire. It cannot
   change the endpoint, add headers, or choose a method outside the table.
6. Values are data. They are not evaluated, interpolated or read from the
   environment.

## 4. Assertions

### 4.1 What they run against

Assertions evaluate against the JSON `result` object exactly as the endpoint
returned it, not against a typed projection. For every fixture published so far
the outcome is identical, because each field they name is present or absent in
the raw object exactly as in the projection (for example `friendbotUrl` is
absent from a real `getLatestLedger` result either way). The one possible
difference is a `field-absent` assertion on a field the old model dropped: it used
to pass for the wrong reason and will now evaluate truthfully. That is the
intended correction, and the regression test in section 9 guards the rest.

A response whose `result` is not an object is an `error` ("unexpected response
shape"), not an assertion failure.

### 4.2 Kinds

Existing, semantics unchanged:

| Kind | Passes when |
|---|---|
| `field-exists` | the key is present, including when its value is `null` |
| `field-absent` | the key is not present (a `null` value is *present*) |
| `field-equals` | the key is present and its JSON value equals `value` exactly |
| `field-type` | the key is present and its value has `expected_type` |

Extensions, frozen here and implemented as separate issues. Each takes `field` or
`path` (4.3), and each **fails** (does not error) when the field is missing or has
the wrong type; no value is coerced from a string to a number or the reverse:

| Kind | Extra keys | Passes when |
|---|---|---|
| `field-one-of` | `values` (non-empty array) | the value equals one of `values` exactly |
| `field-number-range` | `min` and/or `max` (integers, inclusive) | the value is a JSON integer within the bounds |
| `field-length` | `min_items` and/or `max_items` (non-negative integers) | the value is an array whose length is within the bounds |

`expected_type` also accepts `integer` (a JSON number with no fractional part) in
addition to the existing six. Upstream returns several large quantities as strings
(`getFeeStats` fee values, `closeTime`); asserting on them as numbers fails by
design, and asserting on their range is out of scope for version 1.

An RPC fixture must have at least one `[[assert]]` (engine 0.2.0 and the
Fixtures validator agree; before 0.2.0 the engine accepted none and the fixture
passed while checking nothing). Unknown keys in a fixture body or in an assertion
entry are errors for the same reason.

### 4.3 Paths

`field = "name"` stays a top-level key. A nested location uses `path`, an array
whose elements are object keys (strings) or array indexes (non-negative integers):

```toml
path = ["sorobanInclusionFee", "p50"]
path = ["entries", 0, "liveUntilLedgerSeq"]
```

Exactly one of `field` and `path` must be given. There are no wildcards, no
filters and no expression language; each element is a literal, so no escaping
rules exist and a key containing a dot is unambiguous. Walking a path stops at the
first element that is missing, an index past the end, or a type that cannot be
indexed by that element, and that is "missing" for `field-exists`, `field-absent`
and the failure message of the others. `null` in the middle of a path is "missing"
for the remainder. Depth is limited to 8 elements.

## 5. Errors and result classification

| Situation | Result status | Exit code | Retried |
|---|---|---|---|
| An assertion does not hold | `fail` | 1 | no |
| Transport error, HTTP 5xx, TLS failure | `error` | 3 | yes, 3 attempts |
| Timeout | `error` | 3 | yes |
| HTTP 429 | `error` | 3 | yes |
| JSON-RPC `error` object, any code | `error`, message includes the code | 3 | no |
| JSON-RPC code `-32601` (method not found, defined by the JSON-RPC 2.0 specification) | `error`, message says the endpoint does not provide the method | 3 | no |
| Body is not JSON, or has neither `result` nor `error`, or `result` is not an object | `error` | 3 | no |
| Response larger than the size cap (section 6) | `error` | 3 | no |
| Invalid `[request]` or unknown `method` | invalid fixture | 4 | n/a |
| Endpoint passphrase differs from `--network` | configuration error | 2 | n/a |
| Observed protocol differs from the target | warning on stderr; the run continues | n/a | n/a |

A fixture never converts an `error` into a pass. A fixture cannot assert that a
JSON-RPC error is expected in version 1.

## 6. Limits and cost

- The response size cap is approved under D-06 at 16 MiB, which is
  far above the Mainnet `getLatestLedger` observation (about 1.3 MB) and well
  below anything that would exhaust memory. A larger body is an `error`.
- `get-latest-ledger` returns a large `metadataXdr` field. Each additional
  fixture on that method downloads it again. Prefer asserting several fields in one
  fixture, or use `get-health` / `get-network` for liveness-style checks.
- Concurrency and retry settings are unchanged (`--max-concurrency`,
  `--rpc-timeout`).

## 7. Compatibility boundaries

- This is additive to fixture format v1: new `method` values, an optional
  `[request]` table, optional `path` and new assertion kinds. A fixture that uses
  none of them is unchanged.
- An engine release that predates a method rejects a fixture that names it
  (`unsupported rpc method`). A pack that uses a new method therefore requires a
  minimum engine version; CF-06's `compatibleEngines` is where that is recorded.
- The Fixtures validator and JSON Schema (`RPC_METHODS`, `RPC_ASSERT_KINDS`,
  `RPC_ASSERT_TYPES`) must change in the same release train as the engine; the
  existing schema-sync test is the check.
- Moving assertions from the typed projection to the raw `result` is the one
  behavior change; section 4.1 explains why it does not change the outcome of any
  published fixture.
- Method names, parameter names and result field names are the upstream ones. If
  upstream renames one, the engine follows by a new `method` value, not by
  reinterpreting the old one.

## 8. Security and trust boundaries

Fixtures are untrusted data. The method set is closed and read-only; the engine
sends one JSON-RPC 2.0 POST per call to the configured endpoint with no
credentials and no custom headers. Request values are validated and bounded
before use. Responses are untrusted: they are parsed with a size cap, never
evaluated, and only compared. No secret, private key or signature is involved,
and nothing is submitted to a network.

## 9. Validation and test requirements

- Per method: a mock-server test that the exact JSON-RPC request body is sent
  (method name, named `params`), a pass, a fail, and each row of section 5.
- Per assertion kind: present, missing, `null`, wrong type, and boundary values,
  with no string/number coercion.
- Path walking: missing key, index past the end, `null` mid-path, depth 8 and 9.
- Request validation: unknown key, wrong type, out-of-range limit, `xdrFormat =
  "json"`, `[request]` on a zero-parameter method.
- A regression test that the shipped `p28-*` RPC fixtures yield identical
  statuses before and after the move to raw responses (use recorded responses).
- Python validator and schema accept and reject the same inputs as the engine;
  cross-language vectors live with CF-02's conformance vectors.
- Real-endpoint verification of any new method is a CF-04 record, not a unit
  test.

## 10. Cross-repository dependencies

CF-04 (a fixture for a new method needs a verification record), CF-06 (minimum
engine version), CF-02 (fixture bytes include `[request]`). The Action is
unaffected: it does not interpret fixtures.

## 11. Independent implementation units

| Unit | Repo | Depends on |
|---|---|---|
| CA-01-A `get-health` method and typed handling | Protocol-Canary | none |
| CA-01-B `get-version-info` method | Protocol-Canary | none |
| CA-01-C `get-fee-stats` method | Protocol-Canary | none |
| CA-01-D Assertions on the raw `result` object, with the regression test of section 9 | Protocol-Canary | none |
| CA-01-E `path` addressing | Protocol-Canary | CA-01-D |
| CA-01-F `field-one-of`, `field-number-range`, `field-length`, `integer` type | Protocol-Canary | CA-01-D |
| CA-01-G Response size cap and `-32601` classification | Protocol-Canary | D-06 |
| CA-01-H `[request]` parsing and validation | Protocol-Canary | none |
| CA-01-I `get-ledger-entries` | Protocol-Canary | CA-01-H |
| CA-01-J `get-transaction`, `get-ledgers`, `get-transactions`, `get-events` | Protocol-Canary | CA-01-H; each also needs a durable-fixture design (section 2) before a shipped fixture exists |
| CA-01-K Validator, schema and docs for each new method value | ProtocolCanary-Fixtures | the engine unit it mirrors |
| CA-01-L Validator and schema for `[request]`, `path` and the new kinds | ProtocolCanary-Fixtures | CA-01-E, CA-01-F, CA-01-H |
