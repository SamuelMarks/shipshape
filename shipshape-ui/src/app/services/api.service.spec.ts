import { TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import {
  HttpTestingController,
  provideHttpClientTesting
} from '@angular/common/http/testing';

import { ApiService, API_BASE_URL } from './api.service';

describe('ApiService', () => {
  it('calls the expected endpoints', () => {
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: API_BASE_URL, useValue: 'http://api' }
      ]
    });

    const api = TestBed.inject(ApiService);
    const controller = TestBed.inject(HttpTestingController);

    api.getDashboard().subscribe();
    api.getBatchRuns().subscribe();
    api.getDiffListing().subscribe();
    api.getControlOptions().subscribe();
    api.queueControlRun({
      sourceType: 'url',
      sourceValue: 'https://example.com',
      mode: 'audit',
      dryRun: true,
      mechanicIds: []
    }).subscribe();

    const dashboard = controller.expectOne('http://api/dashboard');
    const batchRuns = controller.expectOne('http://api/batch/runs');
    const diffs = controller.expectOne('http://api/diffs');
    const controlOptions = controller.expectOne('http://api/control/options');
    const controlQueue = controller.expectOne('http://api/control/queue');

    expect(controlQueue.request.method).toBe('POST');

    dashboard.flush({ metrics: [], vessels: [], alerts: [] });
    batchRuns.flush({ runs: [] });
    diffs.flush({ files: [] });
    controlOptions.flush({ mechanics: [], activity: [] });
    controlQueue.flush({ runId: 'run-1', log: { id: '1', title: 'Queued', detail: '', time: '', tone: 'info' } });
  });

  it('uses the default API base URL', () => {
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()]
    });

    const baseUrl = TestBed.inject(API_BASE_URL);
    expect(baseUrl).toBe('http://127.0.0.1:8080');
  });
});
