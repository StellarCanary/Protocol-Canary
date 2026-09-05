# Protocol Canary documentation site

This directory is the source for the published documentation site at
<https://stellarcanary.github.io/Protocol-Canary/>. It is built with
[mdBook](https://rust-lang.github.io/mdBook/), the same tool used for *The
Rust Programming Language* book — a single static binary, Markdown
authoring, no Node.js toolchain, and built-in client-side search.

This site is the starting point for the developer journey across all
three `StellarCanary` repositories. It does not replace repository-level
documentation:

- Implementation detail (schemas, error taxonomy, network topology) is
  authoritative in [`Protocol-Canary`'s `docs/`](../docs/) and linked from
  here rather than duplicated.
- Repository-specific contributor process is authoritative in each
  repository's own `CONTRIBUTING.md`.

## Building locally

```bash
# one-time: install mdBook (a static binary, no other dependency)
cargo install mdbook --locked
# or download a prebuilt binary from
# https://github.com/rust-lang/mdBook/releases

cd docs-site
mdbook build      # writes static HTML to docs-site/book/
mdbook serve      # local preview at http://localhost:3000 with live reload
```

`mdbook build` fails the build on a broken internal link (mdBook validates
relative links between chapters at build time), so a successful build is
itself a partial correctness check.

## Structure

- `book.toml` — mdBook configuration (title, search, theme).
- `src/SUMMARY.md` — the table of contents; every page must be listed here
  or mdBook will not include it in the build.
- `src/*.md`, `src/cli/*.md` — the pages themselves.

## Deployment

See [`../.github/workflows/docs.yml`](../.github/workflows/docs.yml). On
every push to `main` that touches `docs-site/**`, GitHub Actions builds
this book with mdBook and deploys the static output to GitHub Pages using
`actions/deploy-pages`. There is no other hosting step, no server, and no
database — the deployed site is exactly the static HTML `mdbook build`
produces.
