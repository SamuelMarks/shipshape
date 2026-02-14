# ShipShape Usage

## Quickstart (Docker Compose)

```bash
export SHIPSHAPE_TOKEN_KEYS=$(openssl rand -base64 32)
docker compose up --build
```

The API listens on `http://localhost:8080`.

Compose variables can be provided via `.env` (see `.env.example`).

## UI In Docker (Alpine)

Run the Angular UI in a Docker container and point it at the server.

```bash
docker run --rm -it -p 4200:4200 \
  -v "$PWD/shipshape-ui:/app" \
  -w /app \
  node:20-alpine \
  sh -c "npm install && npx ng serve --host 0.0.0.0 --poll 2000"
```

The UI will be available at `http://localhost:4200`.

## Server Container (Alpine)

```bash
docker build -t shipshape-server .
docker run --rm -p 8080:8080 \
  -e DATABASE_URL=postgres://user:pass@host:5432/db \
  -e SHIPSHAPE_TOKEN_KEYS="$(openssl rand -base64 32)" \
  shipshape-server
```

Override mechanic installs when building the image:

```bash
docker build \
  --build-arg SHIPSHAPE_PIP_LIBS="cdd-c type-correct lib2notebook2lib go-auto-err-handling" \
  -t shipshape-server .
```

## Local Development

```bash
export SHIPSHAPE_TOKEN_KEYS=$(openssl rand -base64 32)
export DATABASE_URL=postgres://user:pass@host:5432/db
cargo run --bin shipshape-server
```

```bash
cd shipshape-ui && ng serve
```

Open the Diff viewer to tweak refit patches in the right-hand pane; edits are saved to the server.

Open the Workflow Studio (`/workflow`) to launch a new project: enter the repo URL, select GitHub fork and private GitLab mirror targets, clone, run tools, edit changes, and run Docker verification before publishing.

## End-to-End Tests (UI)

```bash
cd shipshape-ui
npm run e2e
```

## CLI Workflows

Authenticate the CLI:

```bash
shipshape login --server-url http://127.0.0.1:8080
```

Audit a repository:

```bash
shipshape audit https://github.com/username/repo --format json
```

Batch refit:

```bash
shipshape refit --batch ./repos.txt \
  --mechanics "cpp-types,notebook-lib,go-err" \
  --dry-run
```

Launch workflow:

```bash
shipshape launch ./my-project \
  --fix-all \
  --mirror-to-gitlab "git@gitlab.com:me/private-mirror.git" \
  --pr-template-override ./templates/strict.md
```

## Environment Variables

Required:

- `DATABASE_URL`: PostgreSQL connection string for the server.
- `SHIPSHAPE_TOKEN_KEYS`: comma-separated base64 32-byte keys.

Optional:

- `SHIPSHAPE_UI_ORIGINS`: allowed UI origins for CORS.
- `SHIPSHAPE_UI_URL`: base URL used in server links.
- `SHIPSHAPE_WORKSPACE_ROOT`: root path for workspace checkouts.
- `SHIPSHAPE_KEEP_WORKSPACE`: set to `1` to keep temp workspaces.
- `SHIPSHAPE_GIT_AUTHOR_NAME`: override Git author name.
- `SHIPSHAPE_GIT_AUTHOR_EMAIL`: override Git author email.
- `SHIPSHAPE_WORKFLOW_MODE`: set to `mock` for test workflows.
