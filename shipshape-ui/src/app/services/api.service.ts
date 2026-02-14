import { Injectable, InjectionToken, inject } from '@angular/core';
import { HttpClient } from '@angular/common/http';

import {
  BatchRunsResponse,
  ControlOptionsResponse,
  ControlQueueRequest,
  ControlQueueResponse,
  DashboardResponse,
  DiffListingResponse
} from './api.models';

export const API_BASE_URL = new InjectionToken<string>('API_BASE_URL', {
  providedIn: 'root',
  factory: () => 'http://127.0.0.1:8080'
});

@Injectable({ providedIn: 'root' })
export class ApiService {
  private readonly http = inject(HttpClient);
  private readonly baseUrl = inject(API_BASE_URL);

  getDashboard() {
    return this.http.get<DashboardResponse>(this.url('/dashboard'));
  }

  getBatchRuns() {
    return this.http.get<BatchRunsResponse>(this.url('/batch/runs'));
  }

  getDiffListing() {
    return this.http.get<DiffListingResponse>(this.url('/diffs'));
  }

  getControlOptions() {
    return this.http.get<ControlOptionsResponse>(this.url('/control/options'));
  }

  queueControlRun(payload: ControlQueueRequest) {
    return this.http.post<ControlQueueResponse>(this.url('/control/queue'), payload);
  }

  private url(path: string): string {
    return `${this.baseUrl}${path}`;
  }
}
