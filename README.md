# 🚢 ShipShape

<!-- coverage-badges:start -->

![Test Coverage](https://img.shields.io/badge/test%20coverage-100%25-brightgreen) ![Doc Coverage](https://img.shields.io/badge/doc%20coverage-100%25-brightgreen)

<!-- coverage-badges:end -->

### Code Integrity, Batch Refactoring, and CI Verification at Scale.

**ShipShape** is a unified platform for maintaining engineering standards across massive fleets of repositories. It combines a high-performance **Rust SDK/CLI** with a **Web Dashboard** to audit code quality, automatically fix engineering anti-patterns, and verify changes via ephemeral CI environments.

It doesn't just lint code; it **refits** it. From converting unstructured Notebooks into libraries to enforcing strict C++ types and percolating error handling in Go/C, ShipShape ensures your fleet is seaworthy.

## ✨ Current Features

### 🧠 Core Platform

- Mechanic registry and orchestration in `shipshape-core`
- Health scoring and coverage heuristics for repo quality
- Language census via `tokei`
- JSON and Markdown report outputs
- PR template interpolation for consistent review summaries

### 🛠 Mechanics (Auto-Refactoring)

- **Notebooks:** Converts Jupyter Notebooks into installable libraries (via `lib2notebook2lib`)
- **C++:** Audits and fixes type safety issues (via `type-correct`)
- **C & Go:** Detects swallowed errors and threads proper error handling (via `cdd-c` and `go-auto-err-handling`)
- Mechanic command specs with consistent inputs/outputs for batch runs

### ⚓ Drydock (CI Verification)

- Stack detection for Rust, Python, Node, C++, Go, CMake, and notebook-only repos
- Dynamic `Dockerfile` and CI pipeline generation
- Ephemeral GitLab mirror builds to verify changes before opening a GitHub PR

### 💻 CLI Workflows

- **Auth:** GitHub device flow shared with the server session
- **Audit:** Single URL, local path, or batch file inputs
- **Refit:** Apply fixes locally with `--dry-run` support
- **Launch:** Fix, drydock verification, and GitHub PR creation
- Output formats: human, JSON, Markdown

### 🧭 Fleet Intelligence & Scale

- Parallel batch operations across 100+ repositories
- Fleet-level audit summaries and health scoring
- Strict CI quality gates (100% test and doc coverage enforced in this repo)

### 🛰 Server & Integrations

- `actix-web` backend with `diesel` + PostgreSQL
- GitHub PR creation and GitLab mirror orchestration
- Encrypted token storage with rotation-ready keys
- Centralized workflow execution for CLI + UI

### 🖥 Web Dashboard (Angular 21)

- Fleet health overview and batch run monitoring
- Diff viewer for refit changes (CodeMirror editor)
- Workflow studio for cloning, tool runs, edit/diff review, and Docker-gated publish actions
- Control room for launch workflows and approvals

## 🗺 Roadmap (Proposed)

Near-term (next 1–2 releases)

- Expand mechanic coverage for Python packaging, Java/Kotlin, and Terraform
- Policy-as-code checks for org-wide engineering standards
- Better drydock caching to accelerate CI verification

Mid-term

- GitHub Actions verification mode in addition to GitLab mirrors
- RBAC + SSO (OIDC/SAML) for enterprise access control
- Cross-repo dependency impact analysis

Longer-term

- Mechanic marketplace with signed distributions
- Fleet-wide trend analytics with time-series exports
- Automated remediation campaigns with approval workflows

## 🏗 Architecture

ShipShape follows a **Hexagonal Architecture** (Ports & Adapters) to ensure distinct separation between core logic, file system operations, and network I/O.

```mermaid
%%{init: {"theme": "base", "themeVariables": {"primaryColor": "#4285f4", "primaryTextColor": "#20344b", "primaryBorderColor": "#20344b", "lineColor": "#20344b", "secondaryColor": "#34a853", "tertiaryColor": "#f9ab00", "background": "#ffffff", "fontFamily": "Google Sans, Roboto Mono, sans-serif", "fontSize": "14px"}}}%%
flowchart LR
  UI["Web UI (Angular)"]:::ui
  CLI["CLI (shipshape-cli)"]:::cli
  Server["API Server (shipshape-server)"]:::server
  Core["Core Domain (shipshape-core)"]:::core
  Mechanics["Mechanics (external tools)"]:::mech
  GitHub["GitHub"]:::ext
  GitLab["GitLab Mirror"]:::ext
  Postgres["PostgreSQL"]:::ext
  CI["Ephemeral CI (Drydock)"]:::ci

  UI --> Server
  CLI --> Core
  CLI --> Server
  Server --> Core
  Core --> Mechanics
  Server --> GitHub
  Server --> GitLab
  Server --> Postgres
  Core --> CI
  CI --> GitLab

  classDef ui fill:#57caff,stroke:#20344b,color:#20344b;
  classDef cli fill:#ffd427,stroke:#20344b,color:#20344b;
  classDef server fill:#4285f4,stroke:#20344b,color:#ffffff;
  classDef core fill:#34a853,stroke:#20344b,color:#ffffff;
  classDef mech fill:#ff7daf,stroke:#20344b,color:#20344b;
  classDef ci fill:#f9ab00,stroke:#20344b,color:#20344b;
  classDef ext fill:#ffffff,stroke:#20344b,color:#20344b;
```

| Crate                  | Description                                                                        |
| :--------------------- | :--------------------------------------------------------------------------------- |
| **`shipshape_core`**   | The brain. Contains the `Mechanic` traits, `Drydock` heuristics, and domain logic. |
| **`shipshape_cli`**    | The terminal interface using `clap` and `tokio`.                                   |
| **`shipshape_server`** | `actix-web` backend with `diesel` (Postgres) and GitHub/GitLab API integrations.   |
| **`shipshape_ui`**     | Angular 21 frontend with CodeMirror-based editors for editable diff reviews.       |

---

## 🚀 Installation

### CLI

```bash
cargo install shipshape-cli
```

### Server & Web UI

_Requires PostgreSQL and a GitLab instance (SaaS or Self-Hosted)._

```bash
# Token encryption key (base64 32-byte key, comma-separated for rotation)
export SHIPSHAPE_TOKEN_KEYS=$(openssl rand -base64 32)

# Start the backend
cargo run --bin shipshape-server

# Serve the frontend (Angular)
cd shipshape-ui && ng serve
```

### Server Container (Alpine)

Build the server image (bundles Python, Go, Clang, and the mechanic CLIs).

```bash
docker build -t shipshape-server .
docker run --rm -p 8080:8080 \
  -e DATABASE_URL=postgres://user:pass@host:5432/db \
  -e SHIPSHAPE_TOKEN_KEYS="$(openssl rand -base64 32)" \
  shipshape-server
```

Override the pip-installed mechanics if needed.

```bash
docker build \
  --build-arg SHIPSHAPE_PIP_LIBS="cdd-c type-correct lib2notebook2lib go-auto-err-handling" \
  -t shipshape-server .
```

### Docker Compose (Alpine dependencies)

Runs Postgres and the ShipShape server together.

```bash
export SHIPSHAPE_TOKEN_KEYS=$(openssl rand -base64 32)
docker compose up --build
```

### Alternative install (native for Windows, Linux, etc.)

Install Rust, `git`, and ensure you have your PostgreSQL services setup.

#### PostgreSQL

One way to install PostgreSQL is with my cross-platform https://github.com/SamuelMarks/libscript:

```sh
$ [ -d /tmp/libscript ] || git clone --depth=1 --single-branch https://github.com/SamuelMarks/libscript /tmp/libscript
$ env -i HOME="$HOME" \
         PATH="$PATH" \
         POSTGRES_USER='rest_user' \
         POSTGRES_SERVICE_PASSWORD='addGoodPasswordhere' \
         POSTGRES_PASSWORD='rest_pass' \
         POSTGRES_HOST='localhost' \
         POSTGRES_DB='shipshape_db' \
         '/tmp/libscript/_lib/_storage/postgres/setup.sh'
```

(on Windows use `set` and `libscript\_lib\_storage\postgres\setup.cmd`)

Then use `export DATABASE_URL='postgres://rest_user:rest_pass@localhost/shipshape_db'`

See `USAGE.md` for a fuller walkthrough.

---

## 💻 CLI Usage

### 0. Authenticate the CLI

Sign in with the GitHub device flow to share the server session.

```bash
shipshape login --server-url http://127.0.0.1:8080
```

### 1. Audit a Repository

Get a detailed health report, including language breakdown and detected anti-patterns.

```bash
shipshape audit https://github.com/username/repo --format json
```

### 2. Batch Refit (The "Fix" Loop)

Apply fixes to a list of repositories, generating patches locally.

```bash
shipshape refit --batch ./repos.txt \
    --mechanics "cpp-types,notebook-lib,go-err" \
    --dry-run
```

### 3. The "Launch" Workflow

The full lifecycle: Fix code, generate a Docker image, verify in GitLab, and open a GitHub PR.

```bash
shipshape launch ./my-project \
    --fix-all \
    --mirror-to-gitlab "git@gitlab.com:me/private-mirror.git" \
    --pr-template-override ./templates/strict.md
```

---

## 🤝 Contributing

We enforce strict standards. PRs will be rejected if they decrease coverage metrics.

1.  **Tests:** Must remain at **100%** line and branch coverage.
2.  **Docs:** Must remain at **100%**. No missing links or docstrings allowed.
3.  **Format:** Run `cargo fmt` and `cargo clippy -- -D warnings`.

```bash
# Verify standards before pushing
cargo test && cargo doc --no-deps --document-private-items
```

---

<div align="center">
  <sub>Built with 🦀 Rust and ❤️ by SamuelMarks</sub>
</div>
