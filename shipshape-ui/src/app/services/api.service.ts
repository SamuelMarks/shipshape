/**
 * @file HTTP client service for communicating with the ShipShape backend API.
 */

import { Injectable, InjectionToken, inject } from "@angular/core";
import { HttpClient } from "@angular/common/http";

import {
  BatchRunsResponse,
  ControlOptionsResponse,
  ControlQueueRequest,
  ControlQueueResponse,
  DashboardResponse,
  DiffListingResponse,
  DiffUpdateRequest,
  DiffUpdateResponse,
} from "./api.models";

/**
 * Injection token providing the base URL of the ShipShape server API.
 */
export const API_BASE_URL = new InjectionToken<string>("API_BASE_URL", {
  providedIn: "root",
  factory: () => "http://127.0.0.1:8080",
});

/**
 * Service encapsulating HTTP calls to ShipShape dashboard, batch, diff,
 * and control-room endpoints.
 */
@Injectable({ providedIn: "root" })
export class ApiService {
  /** Angular HTTP client. */
  private readonly http = inject(HttpClient);
  /** Configured API base URL. */
  private readonly baseUrl = inject(API_BASE_URL);

  /**
   * Fetch fleet dashboard metrics, vessels, and alerts (`GET /api/dashboard`).
   *
   * @returns Observable emitting the dashboard response payload.
   */
  getDashboard() {
    return this.http.get<DashboardResponse>(this.url("/api/dashboard"));
  }

  /**
   * Fetch batch execution waves (`GET /api/batch/runs`).
   *
   * @returns Observable emitting the batch runs response payload.
   */
  getBatchRuns() {
    return this.http.get<BatchRunsResponse>(this.url("/api/batch/runs"));
  }

  /**
   * Fetch the current diff file listing (`GET /api/diffs`).
   *
   * @returns Observable emitting the diff listing response payload.
   */
  getDiffListing() {
    return this.http.get<DiffListingResponse>(this.url("/api/diffs"));
  }

  /**
   * Persist modified content for a diff file (`POST /api/diffs`).
   *
   * @param payload - Diff file path and modified source content.
   * @returns Observable emitting the updated diff file response.
   */
  updateDiff(payload: DiffUpdateRequest) {
    return this.http.post<DiffUpdateResponse>(this.url("/api/diffs"), payload);
  }

  /**
   * Fetch available mechanics and recent activity logs (`GET /api/control/options`).
   *
   * @returns Observable emitting control room options and activity logs.
   */
  getControlOptions() {
    return this.http.get<ControlOptionsResponse>(
      this.url("/api/control/options"),
    );
  }

  /**
   * Queue a new control room run (`POST /api/control/queue`).
   *
   * @param payload - Control queue request parameters.
   * @returns Observable emitting the queued run ID and activity log entry.
   */
  queueControlRun(payload: ControlQueueRequest) {
    return this.http.post<ControlQueueResponse>(
      this.url("/api/control/queue"),
      payload,
    );
  }

  /**
   * Resolve a relative API `path` against {@link baseUrl}.
   *
   * @param path - API path starting with `'/'`.
   * @returns Full endpoint URL string.
   */
  private url(path: string): string {
    return `${this.baseUrl}${path}`;
  }
}
