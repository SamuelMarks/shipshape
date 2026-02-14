import { TestBed } from "@angular/core/testing";
import { of } from "rxjs";

import { DashboardPage } from "./dashboard.page";
import { ApiService } from "../../services/api.service";
import { DashboardResponse } from "../../services/api.models";

const mockResponse: DashboardResponse = {
  metrics: [
    {
      label: "Active vessels",
      value: "2",
      trend: "+1",
      tone: "good",
    },
  ],
  vessels: [
    {
      id: "VX-1",
      name: "Aster",
      healthScore: 90,
      coverageRisk: "Low",
      lastRun: "1 hour ago",
      tone: "good",
      statusLabel: "Healthy",
    },
    {
      id: "VX-2",
      name: "Nova",
      healthScore: 65,
      coverageRisk: "High",
      lastRun: "2 hours ago",
      tone: "bad",
      statusLabel: "Critical",
    },
  ],
  alerts: [
    {
      id: "AL-1",
      title: "Alert",
      description: "Check",
      tone: "warn",
    },
  ],
};

describe("DashboardPage", () => {
  it("computes healthy vessel count", () => {
    TestBed.configureTestingModule({
      imports: [DashboardPage],
      providers: [
        {
          provide: ApiService,
          useValue: {
            getDashboard: () => of(mockResponse),
          },
        },
      ],
    });

    const fixture = TestBed.createComponent(DashboardPage);
    fixture.detectChanges();

    expect(fixture.componentInstance.healthyCount()).toBe(1);
  });

  it("exposes metrics and alerts", () => {
    TestBed.configureTestingModule({
      imports: [DashboardPage],
      providers: [
        {
          provide: ApiService,
          useValue: {
            getDashboard: () => of(mockResponse),
          },
        },
      ],
    });

    const fixture = TestBed.createComponent(DashboardPage);
    fixture.detectChanges();

    expect(fixture.componentInstance.metrics().length).toBe(1);
    expect(fixture.componentInstance.alerts().length).toBe(1);
  });
});
