# `stellar-canary cache`

Manage the local result cache that `check` writes to `.stellar-canary-cache`
in the project root.

## `cache clear`

```
Usage: stellar-canary cache clear [OPTIONS]

Options:
  -h, --help             Print help
  --cache-dir <PATH>     Directory for cached fixture execution results
                         (default: .stellar-canary-cache in the project root)
  --project-root <PATH>  Directory to treat as the project root
```

`cache clear` deletes every cached fixture execution result under the cache
directory and prints how much disk space was reclaimed, in a human-readable
form (bytes, KiB, MiB, …).

- The cache directory defaults to `.stellar-canary-cache` in the project root.
  Use `--cache-dir <PATH>` to clear a different location, for example a CI
  runner's persistent cache or a tmpfs mount.
- Relative `--cache-dir` paths resolve against `--project-root` when that flag
  is given.
- An empty or missing cache directory is not an error: `cache clear` reports
  `0 bytes reclaimed` and exits with code `0`. It never creates a missing
  directory.

The reclaimed count is measured before deletion, so the reported byte total
always matches what was removed.
