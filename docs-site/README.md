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
# stage the browser report viewer (D-08b) — `create-missing` is false, so
# the sidebar's Report Viewer entry fails the build until this is done
rm -rf src/viewer && cp -a ../viewer src/viewer

mdbook build      # writes static HTML to docs-site/book/, viewer at book/viewer/
mdbook serve      # local preview at http://localhost:3000 with live reload
```

`mdbook build` itself does **not** fail on a broken internal link — it
only renders Markdown to HTML (verified directly: an intentionally broken
link built successfully with exit code `0`). Link checking is a separate
step; see [Checking links](#checking-links) below and
[`../.github/workflows/docs.yml`](../.github/workflows/docs.yml), which
runs it in CI before deploying.

## Checking links

```bash
# one-time: install lychee (a static binary, no other dependency)
cargo install lychee --locked
# or download a prebuilt binary from
# https://github.com/lycheeverse/lychee/releases

rm -rf src/viewer && cp -a ../viewer src/viewer
mdbook build
lychee --offline book/          # internal links only, no network calls
lychee book/                    # also checks external (https://...) links
```

## Structure

- `book.toml` — mdBook configuration (title, search, theme). The bundled
  static report viewer (D-08b, E-090) is staged into `src/viewer/` by
  `docs.yml` before the build, so `book/viewer/` ships beside the pages
  at `/viewer/` and the sidebar link reaches the link check.
- `src/SUMMARY.md` — the table of contents; every page must be listed here
  or mdBook will not include it in the build.
- `src/*.md`, `src/cli/*.md` — the pages themselves.
- `src/viewer/` — the staged viewer bundle (not committed; produced by
  the workflow's copy step from the repository's `viewer/`).

## Deployment

See [`../.github/workflows/docs.yml`](../.github/workflows/docs.yml). On
every push to `main` that touches `docs-site/**`, GitHub Actions builds
this book with mdBook and deploys the static output to GitHub Pages using
`actions/deploy-pages`. There is no other hosting step, no server, and no
database — the deployed site is exactly the static HTML `mdbook build`
produces.
