export interface FleetMetric {
  label: string;
  value: string;
  trend?: string | null;
  detail?: string | null;
  tone: "good" | "warn" | "bad" | "info";
}

export interface VesselStatus {
  id: string;
  name: string;
  healthScore: number;
  coverageRisk: string;
  lastRun: string;
  tone: "good" | "warn" | "bad" | "info";
  statusLabel: string;
}

export interface FleetAlert {
  id: string;
  title: string;
  description: string;
  tone: "good" | "warn" | "bad" | "info";
}

export interface DashboardResponse {
  metrics: FleetMetric[];
  vessels: VesselStatus[];
  alerts: FleetAlert[];
}

export interface BatchRun {
  id: string;
  label: string;
  owner: string;
  targetCount: number;
  status: "Queued" | "Running" | "Complete" | "Failed" | string;
  health: string;
  lastRun: string;
  tone: "good" | "warn" | "bad" | "info";
}

export interface BatchRunsResponse {
  runs: BatchRun[];
}

export interface DiffFile {
  path: string;
  summary: string;
  language: string;
  original: string;
  modified: string;
  tone: "good" | "warn" | "bad" | "info";
  statusLabel: string;
}

export interface DiffListingResponse {
  files: DiffFile[];
}

export interface DiffUpdateRequest {
  path: string;
  modified: string;
}

export interface DiffUpdateResponse {
  file: DiffFile;
}

export interface MechanicOption {
  id: string;
  label: string;
  description: string;
}

export interface ActivityLog {
  id: string;
  title: string;
  detail: string;
  time: string;
  tone: "good" | "warn" | "bad" | "info";
}

export interface ControlOptionsResponse {
  mechanics: MechanicOption[];
  activity: ActivityLog[];
}

export interface ControlQueueRequest {
  sourceType: string;
  sourceValue: string;
  mode: string;
  dryRun: boolean;
  mechanicIds: string[];
}

export interface ControlQueueResponse {
  runId: string;
  log: ActivityLog;
}
