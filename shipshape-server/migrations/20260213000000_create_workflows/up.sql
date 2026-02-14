CREATE TABLE workflows (
    id TEXT PRIMARY KEY NOT NULL,
    vessel_id TEXT NOT NULL,
    status TEXT NOT NULL,
    pr_url TEXT,
    pipeline_url TEXT,
    created_at TIMESTAMP NOT NULL
);

CREATE TABLE workflow_steps (
    id TEXT PRIMARY KEY NOT NULL,
    workflow_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    detail TEXT,
    created_at TIMESTAMP NOT NULL,
    FOREIGN KEY (workflow_id) REFERENCES workflows(id)
);
