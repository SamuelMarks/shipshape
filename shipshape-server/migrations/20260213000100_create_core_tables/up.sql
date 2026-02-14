CREATE TABLE voyages (
    id TEXT PRIMARY KEY NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL
);

CREATE TABLE vessels (
    id TEXT PRIMARY KEY NOT NULL,
    voyage_id TEXT NOT NULL,
    repo_url TEXT,
    local_path TEXT,
    created_at TIMESTAMP NOT NULL,
    FOREIGN KEY (voyage_id) REFERENCES voyages(id)
);

CREATE TABLE diagnostics (
    id TEXT PRIMARY KEY NOT NULL,
    vessel_id TEXT NOT NULL,
    report_json TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    FOREIGN KEY (vessel_id) REFERENCES vessels(id)
);

CREATE TABLE refits (
    id TEXT PRIMARY KEY NOT NULL,
    vessel_id TEXT NOT NULL,
    status TEXT NOT NULL,
    applied BOOLEAN NOT NULL,
    output TEXT,
    created_at TIMESTAMP NOT NULL,
    FOREIGN KEY (vessel_id) REFERENCES vessels(id)
);

CREATE TABLE launches (
    id TEXT PRIMARY KEY NOT NULL,
    voyage_id TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    FOREIGN KEY (voyage_id) REFERENCES voyages(id)
);
