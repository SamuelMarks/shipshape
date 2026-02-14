import { TestBed, fakeAsync, tick } from "@angular/core/testing";
import { of } from "rxjs";

import { BatchPage } from "./batch.page";
import { ApiService } from "../../services/api.service";
import { BatchRunsResponse } from "../../services/api.models";

const mockRuns: BatchRunsResponse = {
  runs: [
    {
      id: "B-1",
      label: "Northstar sweep",
      owner: "Core Platform",
      targetCount: 10,
      status: "Running",
      health: "90% healthy",
      lastRun: "Today",
      tone: "good",
    },
    {
      id: "B-2",
      label: "Notebook wave",
      owner: "Data Ops",
      targetCount: 4,
      status: "Queued",
      health: "Pending",
      lastRun: "Scheduled",
      tone: "info",
    },
  ],
};

describe("BatchPage", () => {
  it("filters batches by query and status", fakeAsync(() => {
    TestBed.configureTestingModule({
      imports: [BatchPage],
      providers: [
        {
          provide: ApiService,
          useValue: {
            getBatchRuns: () => of(mockRuns),
          },
        },
      ],
    });

    const fixture = TestBed.createComponent(BatchPage);
    fixture.detectChanges();

    const page = fixture.componentInstance;
    page.filterForm.patchValue({ query: "North", status: "Running" });
    tick();

    const results = page.filteredBatches();
    expect(results.length).toBe(1);
    expect(results[0].id).toBe("B-1");
  }));

  it("filters batches by owner", fakeAsync(() => {
    TestBed.configureTestingModule({
      imports: [BatchPage],
      providers: [
        {
          provide: ApiService,
          useValue: {
            getBatchRuns: () => of(mockRuns),
          },
        },
      ],
    });

    const fixture = TestBed.createComponent(BatchPage);
    fixture.detectChanges();

    const page = fixture.componentInstance;
    page.filterForm.patchValue({ owner: "Data Ops" });
    tick();

    const results = page.filteredBatches();
    expect(results.length).toBe(1);
    expect(results[0].id).toBe("B-2");
  }));
});
