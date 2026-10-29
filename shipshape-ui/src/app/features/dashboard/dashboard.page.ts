/**
 * @file Fleet Dashboard page component displaying KPIs, vessel statuses,
 * and active drift alerts.
 */

import { Component, computed, inject } from "@angular/core";
import { toSignal } from "@angular/core/rxjs-interop";

import { MetricCardComponent } from "../../ui/metric-card.component";
import { StatusPillComponent } from "../../ui/status-pill.component";
import { ApiService } from "../../services/api.service";
import {
  DashboardResponse,
  FleetAlert,
  FleetMetric,
  VesselStatus,
} from "../../services/api.models";

/** Empty initial state before `/api/dashboard` resolves. */
const EMPTY_DASHBOARD: DashboardResponse = {
  metrics: [],
  vessels: [],
  alerts: [],
};

/**
 * Dashboard page component rendering fleet health telemetry and vessel status cards.
 */
@Component({
  selector: "shipshape-dashboard-page",
  templateUrl: "./dashboard.page.html",
  styleUrl: "./dashboard.page.css",
  imports: [MetricCardComponent, StatusPillComponent],
})
export class DashboardPage {
  /** Backend API client. */
  private readonly api = inject(ApiService);

  /** Signal holding the latest dashboard response payload. */
  private readonly dashboard = toSignal(this.api.getDashboard(), {
    initialValue: EMPTY_DASHBOARD,
  });

  /** Fleet-level KPI metrics. */
  readonly metrics = computed<FleetMetric[]>(() => this.dashboard().metrics);
  /** Monitored vessel status rows. */
  readonly vessels = computed<VesselStatus[]>(() => this.dashboard().vessels);
  /** Active fleet alerts. */
  readonly alerts = computed<FleetAlert[]>(() => this.dashboard().alerts);

  /** Count of vessels currently in the `"good"` health tone. */
  readonly healthyCount = computed(
    () => this.vessels().filter((vessel) => vessel.tone === "good").length,
  );
}
