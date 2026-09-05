# Authoring a Fixture

This is the contributor workflow for adding one new fixture to
[`ProtocolCanary-Fixtures`](https://github.com/StellarCanary/ProtocolCanary-Fixtures).
It exists to raise the bar on what counts as a real compatibility
assertion — fixtures are never added merely to increase a coverage
number, and a reviewer must be able to answer all of the following from
the fixture file and its header comment alone, without running anything:

1. What Stellar behavior does this test?
2. Why is that behavior important?
3. What protocol/CAP introduced it?
4. What is the expected result, and where does that expectation come from?
5. Is the test deterministic?

## Steps

1. **Identify a concrete compatibility assertion.** Read the CAP text, the
   upstream XDR definition, the upstream implementation, or the official
   release/API docs — in that order of preference. Never cite a source
   you have not actually checked describes the specific behavior you are
   asserting.
2. **Determine the applicable protocol version** and pick a stable ID
   following `p<protocol>-<surface>-<slug>` (e.g.
   `p28-xdr-cap85-external-ref-roundtrip`). IDs are lowercase and unique
   across the entire repository — the loader validates this across every
   `*.toml` file under a given `--fixtures-dir`, not just one file. Never
   rename an ID just because an implementation detail changed; if the
   semantic assertion itself changes, add a new ID instead of
   repurposing an old one.
3. **Create a fixture matching the real schema** — see [Fixtures](./fixtures-guide.md#schema)
   for the exact common fields and per-surface body. Set
   `source_reference` to an authoritative URL or CAP identifier, and add a
   header comment (a `#` block above the TOML body) explaining, in prose,
   how the expected value was derived or observed — e.g. "built with the
   official `stellar-xdr` 28.0.0 crate against the CAP-0083 `StellarValue`
   type", not "looks right". Use deterministic input: no fixture may
   depend on ledger state that changes between runs (a current ledger
   sequence, "the latest anything") unless the assertion is explicitly
   scoped and documented as a live-network check.
4. **Validate it:**
   ```bash
   python3 tools/validate/validate.py
   ```
5. **Run it through the real CLI:**
   ```bash
   stellar-canary fixtures --fixtures-dir <path> --protocol <N>
   stellar-canary check --fixtures-dir <path> --protocol <N>
   ```
6. **Add or update documentation** — the relevant `docs/protocol-NN.md`
   table in `ProtocolCanary-Fixtures`, and this site's
   [Protocol 28](./protocol-28.md) page if you touched that pack.
7. **Submit a pull request** against `ProtocolCanary-Fixtures`, following
   its own [`CONTRIBUTING.md`](https://github.com/StellarCanary/ProtocolCanary-Fixtures/blob/main/CONTRIBUTING.md).

## What does not belong in a fixture

- A shell command, script, or any other executable field — none exists in
  the schema, and a pull request introducing one is rejected regardless
  of stated purpose.
- A private key, seed phrase, or funded-account secret. A `source_account`
  is a public address only, used to build an *unsigned* transaction.
- A fixture whose expected value was guessed, "probably correct", or not
  independently checked against an authoritative source — see the
  CAP-0086 gap on the [Protocol 28](./protocol-28.md#what-this-pack-does-not-check)
  page for what happens instead when a real fixture cannot yet be built
  responsibly: it is documented as a gap, not filled with an
  unverifiable guess.
