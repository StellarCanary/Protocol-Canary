---
name: Bug report
about: Something in stellar-canary behaves incorrectly
title: ""
labels: bug
---

## Summary

What happened, and what did you expect instead?

## Reproduction

```
stellar-canary <command and flags>
```

Include `.stellar-canary.toml` contents if relevant, and whether you used
`--fixtures-dir` with a `ProtocolCanary-Fixtures` checkout or a custom
directory.

## Output

Paste the terminal output or `--format json` report. Redact any endpoint
URL you don't want public; nothing else in the output should contain
secrets (Protocol Canary never handles private keys).

## Environment

- `stellar-canary version` output:
- OS:
- Installed via: `cargo install --git ...` / local checkout / other

## Exit code

What exit code did the CLI return? (`stellar-canary check; echo $?`)
