CREATE TABLE users (
    id TEXT PRIMARY KEY NOT NULL,
    github_id TEXT NOT NULL,
    github_login TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL
);

CREATE TABLE auth_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL,
    shipshape_token TEXT NOT NULL,
    github_token TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    last_used_at TIMESTAMP NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id)
);

CREATE INDEX idx_auth_sessions_token ON auth_sessions(shipshape_token);
