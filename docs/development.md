# Development and publishing

Run the same Rust checks as CI (tests use temporary playlists and loopback servers, not external IPTV or mpv):

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
```

Build and preview the documentation with Python 3:

```sh
python3 -m venv .venv
. .venv/bin/activate
pip install -r docs/requirements.txt
mkdocs build --strict
mkdocs serve
```

Open the local address printed by MkDocs. The generated `site/` is disposable.

Pull requests and pushes to `main` run Rust checks, a strict documentation build, and Trivy filesystem scanning for HIGH/CRITICAL vulnerabilities (including unfixed) and secrets. Configure branch protection or a ruleset to require these checks before merge. On a push to `main`, the Pages deployment job runs only after the Rust, Trivy, documentation, and Docker Compose integration jobs succeed. In repository Settings → Pages, select **GitHub Actions** as the source; the repository's plan and visibility must support Pages. GitHub Pages deployment also uses the `github-pages` environment, whose protection rules can require approval. If Pages is unavailable for a private repository, a plan or visibility change is required before publishing. Do not assume a successful build means a successful deployment; inspect the Actions deployment and published URL.
