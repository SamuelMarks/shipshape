//! Diesel schema definitions for ShipShape server.

diesel::table! {
    voyages (id) {
        id -> Text,
        status -> Text,
        created_at -> Timestamp,
    }
}

diesel::table! {
    users (id) {
        id -> Text,
        github_id -> Text,
        github_login -> Text,
        created_at -> Timestamp,
    }
}

diesel::table! {
    auth_sessions (id) {
        id -> Text,
        user_id -> Text,
        shipshape_token -> Text,
        github_token -> Text,
        created_at -> Timestamp,
        last_used_at -> Timestamp,
    }
}

diesel::table! {
    vessels (id) {
        id -> Text,
        voyage_id -> Text,
        repo_url -> Nullable<Text>,
        local_path -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    diagnostics (id) {
        id -> Text,
        vessel_id -> Text,
        report_json -> Text,
        created_at -> Timestamp,
    }
}

diesel::table! {
    refits (id) {
        id -> Text,
        vessel_id -> Text,
        status -> Text,
        applied -> Bool,
        output -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    launches (id) {
        id -> Text,
        voyage_id -> Text,
        status -> Text,
        created_at -> Timestamp,
    }
}

diesel::table! {
    workflows (id) {
        id -> Text,
        vessel_id -> Text,
        status -> Text,
        pr_url -> Nullable<Text>,
        pipeline_url -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    workflow_steps (id) {
        id -> Text,
        workflow_id -> Text,
        kind -> Text,
        status -> Text,
        detail -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::joinable!(vessels -> voyages (voyage_id));
diesel::joinable!(diagnostics -> vessels (vessel_id));
diesel::joinable!(refits -> vessels (vessel_id));
diesel::joinable!(launches -> voyages (voyage_id));
diesel::joinable!(workflows -> vessels (vessel_id));
diesel::joinable!(workflow_steps -> workflows (workflow_id));
diesel::joinable!(auth_sessions -> users (user_id));

diesel::allow_tables_to_appear_in_same_query!(
    voyages,
    users,
    auth_sessions,
    vessels,
    diagnostics,
    refits,
    launches,
    workflows,
    workflow_steps,
);
