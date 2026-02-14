import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';

import { MetricCardComponent } from '../../ui/metric-card.component';
import { StatusPillComponent } from '../../ui/status-pill.component';
import { ApiService } from '../../services/api.service';
import {
  DashboardResponse,
  FleetAlert,
  FleetMetric,
  VesselStatus
} from '../../services/api.models';

const EMPTY_DASHBOARD: DashboardResponse = {
  metrics: [],
  vessels: [],
  alerts: []
};

@Component({
  selector: 'shipshape-dashboard-page',
  templateUrl: './dashboard.page.html',
  styleUrl: './dashboard.page.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MetricCardComponent, StatusPillComponent]
})
export class DashboardPage {
  private readonly api = inject(ApiService);

  private readonly dashboard = toSignal(this.api.getDashboard(), {
    initialValue: EMPTY_DASHBOARD
  });

  readonly metrics = computed<FleetMetric[]>(() => this.dashboard().metrics);
  readonly vessels = computed<VesselStatus[]>(() => this.dashboard().vessels);
  readonly alerts = computed<FleetAlert[]>(() => this.dashboard().alerts);

  readonly healthyCount = computed(() =>
    this.vessels().filter((vessel) => vessel.tone === 'good').length
  );
}
