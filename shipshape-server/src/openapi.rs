//! OpenAPI specification for ShipShape server.

use utoipa::OpenApi;

use shipshape_core::{CoverageReport, FleetReport, Violation};

use crate::routes::{
    ActivityLog, AuthConfigResponse, AuthGithubRequest, AuthGithubResponse, AuthGithubTokenRequest,
    AuthUser, BatchRun, BatchRunsResponse, ControlOptionsResponse, ControlQueueRequest,
    ControlQueueResponse, DashboardResponse, DiagnosticsReport, DiagnosticsResponse, DiffFile,
    DiffListingResponse, DiffUpdateRequest, DiffUpdateResponse, ErrorResponse, FleetAlert,
    FleetMetric, LaunchRequest, LaunchResponse, MechanicOption, RefitRequest, RefitResponse,
    VesselStatus, VoyageBoardRequest, VoyageBoardResponse,
};
use crate::workflows::{
    GitLabSpec, PatchSpec, PullRequestSpec, RepoSpec, WorkflowRequest, WorkflowResult,
    WorkflowStatus, WorkflowStep, WorkflowStepKind,
};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::routes::auth_config,
        crate::routes::auth_github,
        crate::routes::auth_github_token,
        crate::routes::auth_me,
        crate::routes::dashboard,
        crate::routes::batch_runs,
        crate::routes::diffs,
        crate::routes::diff_update,
        crate::routes::control_options,
        crate::routes::control_queue,
        crate::routes::voyage_board,
        crate::routes::vessel_diagnostics,
        crate::routes::vessel_refit,
        crate::routes::vessel_workflow,
        crate::routes::voyage_launch,
        crate::routes::openapi_json
    ),
    components(
        schemas(
            AuthConfigResponse,
            AuthGithubRequest,
            AuthGithubResponse,
            AuthGithubTokenRequest,
            AuthUser,
            FleetMetric,
            FleetAlert,
            VesselStatus,
            DashboardResponse,
            BatchRun,
            BatchRunsResponse,
            DiffFile,
            DiffListingResponse,
            DiffUpdateRequest,
            DiffUpdateResponse,
            MechanicOption,
            ActivityLog,
            ControlOptionsResponse,
            ControlQueueRequest,
            ControlQueueResponse,
            VoyageBoardRequest,
            VoyageBoardResponse,
            DiagnosticsReport,
            DiagnosticsResponse,
            RefitRequest,
            RefitResponse,
            ErrorResponse,
            RepoSpec,
            PatchSpec,
            PullRequestSpec,
            GitLabSpec,
            CoverageReport,
            FleetReport,
            Violation,
            WorkflowRequest,
            WorkflowResult,
            WorkflowStep,
            WorkflowStepKind,
            WorkflowStatus,
            LaunchRequest,
            LaunchResponse
        )
    ),
    tags(
        (name = "auth", description = "Authentication"),
        (name = "dashboard", description = "Dashboard data"),
        (name = "batch", description = "Batch runs"),
        (name = "diffs", description = "Diff viewer"),
        (name = "control", description = "Control room"),
        (name = "voyage", description = "Voyage management"),
        (name = "vessel", description = "Vessel operations"),
        (name = "system", description = "System endpoints")
    )
)]
/// OpenAPI specification for the ShipShape server.
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::ApiDoc;
    use utoipa::OpenApi;

    #[test]
    fn openapi_includes_expected_paths() {
        let doc = ApiDoc::openapi();
        let paths = doc.paths.paths;

        assert!(paths.contains_key("/voyage/board"));
        assert!(paths.contains_key("/vessel/{id}/diagnostics"));
        assert!(paths.contains_key("/vessel/{id}/refit"));
        assert!(paths.contains_key("/vessel/{id}/workflow"));
        assert!(paths.contains_key("/voyage/launch"));
        assert!(paths.contains_key("/auth/config"));
        assert!(paths.contains_key("/auth/github"));
        assert!(paths.contains_key("/auth/github/token"));
        assert!(paths.contains_key("/auth/me"));
        assert!(paths.contains_key("/dashboard"));
        assert!(paths.contains_key("/batch/runs"));
        assert!(paths.contains_key("/diffs"));
        assert!(paths.contains_key("/control/options"));
        assert!(paths.contains_key("/control/queue"));
        assert!(paths.contains_key("/openapi.json"));
    }
}
