//! Database models for ShipShape server.

use chrono::NaiveDateTime;
use diesel::prelude::*;

use crate::schema::{
    auth_sessions, diagnostics, launches, refits, users, vessels, voyages, workflow_steps,
    workflows,
};

#[derive(Debug, Clone, Queryable, Insertable, Identifiable, Selectable)]
#[diesel(table_name = voyages)]
/// Voyage database record.
pub struct Voyage {
    /// Voyage identifier.
    pub id: String,
    /// Current status string.
    pub status: String,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Insertable, Identifiable, Selectable)]
#[diesel(table_name = users)]
/// User account linked to GitHub authentication.
pub struct User {
    /// User identifier.
    pub id: String,
    /// GitHub user id.
    pub github_id: String,
    /// GitHub login handle.
    pub github_login: String,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = users)]
/// Insertable user account.
pub struct NewUser {
    /// User identifier.
    pub id: String,
    /// GitHub user id.
    pub github_id: String,
    /// GitHub login handle.
    pub github_login: String,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Insertable, Identifiable, Associations, Selectable)]
#[diesel(table_name = auth_sessions)]
#[diesel(belongs_to(User, foreign_key = user_id))]
/// Authentication session for a user.
pub struct AuthSession {
    /// Session identifier.
    pub id: String,
    /// Associated user id.
    pub user_id: String,
    /// ShipShape access token.
    pub shipshape_token: String,
    /// GitHub OAuth token.
    pub github_token: String,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
    /// Last usage timestamp.
    pub last_used_at: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = auth_sessions)]
/// Insertable auth session.
pub struct NewAuthSession {
    /// Session identifier.
    pub id: String,
    /// Associated user id.
    pub user_id: String,
    /// ShipShape access token.
    pub shipshape_token: String,
    /// GitHub OAuth token.
    pub github_token: String,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
    /// Last usage timestamp.
    pub last_used_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Insertable, Identifiable, Associations, Selectable)]
#[diesel(table_name = vessels)]
#[diesel(belongs_to(Voyage, foreign_key = voyage_id))]
/// Vessel database record.
pub struct Vessel {
    /// Vessel identifier.
    pub id: String,
    /// Parent voyage identifier.
    pub voyage_id: String,
    /// Repository URL if applicable.
    pub repo_url: Option<String>,
    /// Local path if applicable.
    pub local_path: Option<String>,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Insertable, Identifiable, Associations, Selectable)]
#[diesel(table_name = diagnostics)]
#[diesel(belongs_to(Vessel, foreign_key = vessel_id))]
/// Diagnostics record associated with a vessel.
pub struct Diagnostic {
    /// Diagnostic identifier.
    pub id: String,
    /// Associated vessel identifier.
    pub vessel_id: String,
    /// JSON report payload.
    pub report_json: String,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Insertable, Identifiable, Associations, Selectable)]
#[diesel(table_name = refits)]
#[diesel(belongs_to(Vessel, foreign_key = vessel_id))]
/// Refit record associated with a vessel.
pub struct Refit {
    /// Refit identifier.
    pub id: String,
    /// Associated vessel identifier.
    pub vessel_id: String,
    /// Refit status.
    pub status: String,
    /// Whether changes were applied.
    pub applied: bool,
    /// Output log or summary.
    pub output: Option<String>,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Insertable, Identifiable, Associations, Selectable)]
#[diesel(table_name = launches)]
#[diesel(belongs_to(Voyage, foreign_key = voyage_id))]
/// Launch record associated with a voyage.
pub struct Launch {
    /// Launch identifier.
    pub id: String,
    /// Associated voyage identifier.
    pub voyage_id: String,
    /// Launch status.
    pub status: String,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Insertable, Identifiable, Associations, Selectable)]
#[diesel(table_name = workflows)]
#[diesel(belongs_to(Vessel, foreign_key = vessel_id))]
/// Workflow database record.
pub struct WorkflowRecord {
    /// Workflow identifier.
    pub id: String,
    /// Associated vessel identifier.
    pub vessel_id: String,
    /// Workflow status.
    pub status: String,
    /// GitHub pull request URL.
    pub pr_url: Option<String>,
    /// GitLab pipeline URL.
    pub pipeline_url: Option<String>,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = workflows)]
/// Insertable workflow record.
pub struct NewWorkflow {
    /// Workflow identifier.
    pub id: String,
    /// Associated vessel identifier.
    pub vessel_id: String,
    /// Workflow status.
    pub status: String,
    /// GitHub pull request URL.
    pub pr_url: Option<String>,
    /// GitLab pipeline URL.
    pub pipeline_url: Option<String>,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Queryable, Insertable, Identifiable, Associations, Selectable)]
#[diesel(table_name = workflow_steps)]
#[diesel(belongs_to(WorkflowRecord, foreign_key = workflow_id))]
/// Workflow step database record.
pub struct WorkflowStepRecord {
    /// Workflow step identifier.
    pub id: String,
    /// Parent workflow identifier.
    pub workflow_id: String,
    /// Step kind.
    pub kind: String,
    /// Step status.
    pub status: String,
    /// Optional detail message.
    pub detail: Option<String>,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = workflow_steps)]
/// Insertable workflow step record.
pub struct NewWorkflowStep {
    /// Workflow step identifier.
    pub id: String,
    /// Parent workflow identifier.
    pub workflow_id: String,
    /// Step kind.
    pub kind: String,
    /// Step status.
    pub status: String,
    /// Optional detail message.
    pub detail: Option<String>,
    /// Creation timestamp.
    pub created_at: NaiveDateTime,
}
