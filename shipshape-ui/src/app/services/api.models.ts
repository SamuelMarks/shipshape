/**
 * @file Data transfer object (DTO) interfaces for ShipShape REST API endpoints.
 */

/** Summary KPI metric card payload on the Fleet Dashboard. */
export interface FleetMetric {
  /** Metric title label. */
  label: string;
  /** Primary formatted metric value. */
  value: string;
  /** Optional delta/trend indicator. */
  trend?: string | null;
  /** Optional secondary detail line. */
  detail?: string | null;
  /** Visual severity/health tone. */
  tone: "good" | "warn" | "bad" | "info";
}

/** Per-repository vessel health entry on the Fleet Dashboard. */
export interface VesselStatus {
  /** Unique vessel identifier. */
  id: string;
  /** Display name or repository URL. */
  name: string;
  /** Heuristic health score (`0..100`). */
  healthScore: number;
  /** Qualitative coverage risk level (`"Low"`, `"Medium"`, `"High"`). */
  coverageRisk: string;
  /** Human-readable relative time of last inspection. */
  lastRun: string;
  /** Visual status tone. */
  tone: "good" | "warn" | "bad" | "info";
  /** Short status pill text. */
  statusLabel: string;
}

/** Fleet-wide operational alert item. */
export interface FleetAlert {
  /** Unique alert identifier. */
  id: string;
  /** Alert headline. */
  title: string;
  /** Detailed description of the alert condition. */
  description: string;
  /** Visual severity tone. */
  tone: "good" | "warn" | "bad" | "info";
}

/** Response payload from `GET /api/dashboard`. */
export interface DashboardResponse {
  /** Fleet-level KPI summary metrics. */
  metrics: FleetMetric[];
  /** Monitored vessel status rows. */
  vessels: VesselStatus[];
  /** Active fleet alerts. */
  alerts: FleetAlert[];
}

/** Batch execution wave summary entry. */
export interface BatchRun {
  /** Unique batch run identifier. */
  id: string;
  /** Descriptive batch wave label. */
  label: string;
  /** Owning engineering team name. */
  owner: string;
  /** Number of target repositories in the batch. */
  targetCount: number;
  /** Current execution status. */
  status: "Queued" | "Running" | "Complete" | "Failed" | string;
  /** Aggregate health summary string. */
  health: string;
  /** Relative timestamp of the latest execution. */
  lastRun: string;
  /** Visual status tone. */
  tone: "good" | "warn" | "bad" | "info";
}

/** Response payload from `GET /api/batch/runs`. */
export interface BatchRunsResponse {
  /** List of batch execution waves. */
  runs: BatchRun[];
}

/** Single file diff entry with original and modified source text. */
export interface DiffFile {
  /** Repository-relative file path. */
  path: string;
  /** Summary of the mechanical change applied to the file. */
  summary: string;
  /** Programming language identifier for syntax highlighting. */
  language: string;
  /** Original file content prior to refit. */
  original: string;
  /** Modified file content after refit or manual editing. */
  modified: string;
  /** Visual status tone. */
  tone: "good" | "warn" | "bad" | "info";
  /** Status label (`"Modified"`, `"Added"`, etc.). */
  statusLabel: string;
}

/** Response payload from `GET /api/diffs`. */
export interface DiffListingResponse {
  /** List of diff files available for review. */
  files: DiffFile[];
}

/** Request payload for `POST /api/diffs`. */
export interface DiffUpdateRequest {
  /** Repository-relative file path to update. */
  path: string;
  /** Updated modified source content. */
  modified: string;
}

/** Response payload from `POST /api/diffs`. */
export interface DiffUpdateResponse {
  /** Updated diff file record. */
  file: DiffFile;
}

/** Selectable refit mechanic descriptor. */
export interface MechanicOption {
  /** Canonical mechanic identifier. */
  id: string;
  /** Human-readable mechanic title. */
  label: string;
  /** Summary of what the mechanic audits or repairs. */
  description: string;
}

/** Control room activity log entry. */
export interface ActivityLog {
  /** Unique log entry identifier. */
  id: string;
  /** Activity headline. */
  title: string;
  /** Detailed description of the queued or completed action. */
  detail: string;
  /** Relative timestamp string. */
  time: string;
  /** Visual status tone. */
  tone: "good" | "warn" | "bad" | "info";
}

/** Response payload from `GET /api/control/options`. */
export interface ControlOptionsResponse {
  /** Available refit mechanics. */
  mechanics: MechanicOption[];
  /** Recent control room activity logs. */
  activity: ActivityLog[];
}

/** Request payload for `POST /api/control/queue`. */
export interface ControlQueueRequest {
  /** Repository source kind (`"url"`, `"file"`, `"dir"`, `"path"`). */
  sourceType: string;
  /** Repository URL or path string. */
  sourceValue: string;
  /** Workflow execution mode (`"audit"`, `"refit"`, `"launch"`). */
  mode: string;
  /** Whether to run in dry-run mode without applying changes. */
  dryRun: boolean;
  /** List of selected mechanic IDs to execute. */
  mechanicIds: string[];
}

/** Response payload from `POST /api/control/queue`. */
export interface ControlQueueResponse {
  /** Generated run/voyage UUID. */
  runId: string;
  /** Created activity log entry. */
  log: ActivityLog;
}
