/**
 * @file Batch View page component for filtering and inspecting parallel
 * fleet audit/refit waves.
 */

import { Component, computed, inject } from "@angular/core";
import { NonNullableFormBuilder, ReactiveFormsModule } from "@angular/forms";
import { toSignal } from "@angular/core/rxjs-interop";
import { map, startWith } from "rxjs/operators";

import { StatusPillComponent } from "../../ui/status-pill.component";
import { ApiService } from "../../services/api.service";
import { BatchRun } from "../../services/api.models";

/** Active filter criteria for narrowing displayed batch runs. */
interface BatchFilters {
  /** Substring search query matched against batch label or ID. */
  query: string;
  /** Selected status filter (`"all"` or a specific status). */
  status: string;
  /** Selected owning team filter (`"all"` or a specific team). */
  owner: string;
}

/**
 * Batch View page component displaying filterable batch execution waves.
 */
@Component({
  selector: "shipshape-batch-page",
  templateUrl: "./batch.page.html",
  styleUrl: "./batch.page.css",
  imports: [ReactiveFormsModule, StatusPillComponent],
})
export class BatchPage {
  /** Non-nullable reactive form builder. */
  private readonly fb = inject(NonNullableFormBuilder);
  /** Backend API client. */
  private readonly api = inject(ApiService);

  /** Selectable status filter options. */
  readonly statusOptions = [
    "all",
    "Queued",
    "Running",
    "Complete",
    "Failed",
  ] as const;
  /** Selectable owner filter options. */
  readonly ownerOptions = [
    "all",
    "Core Platform",
    "Data Ops",
    "Runtime",
  ] as const;

  /** Reactive form controlling batch search and dropdown filters. */
  readonly filterForm = this.fb.group({
    query: "",
    status: "all",
    owner: "all",
  });

  /** Signal of raw batch runs loaded from `GET /api/batch/runs`. */
  private readonly runs = toSignal(
    this.api.getBatchRuns().pipe(map((response) => response.runs)),
    { initialValue: [] },
  );

  /** Computed list of all batch runs. */
  readonly batches = computed<BatchRun[]>(() => this.runs());

  /** Signal tracking current filter values from `filterForm`. */
  private readonly filters = toSignal(
    this.filterForm.valueChanges.pipe(
      startWith(this.filterForm.getRawValue()),
      map((value) => ({ ...this.filterForm.getRawValue(), ...value })),
    ),
    { initialValue: this.filterForm.getRawValue() },
  );

  /** Computed list of batch runs matching the active {@link filters}. */
  readonly filteredBatches = computed(() => {
    const filters = this.filters();
    return this.batches().filter((batch) => this.matchesFilter(batch, filters));
  });

  /**
   * Check whether `batch` satisfies all criteria in `filters`.
   *
   * @param batch - Candidate batch run entry.
   * @param filters - Active filter values.
   * @returns `true` if `batch` matches query, status, and owner filters.
   */
  private matchesFilter(batch: BatchRun, filters: BatchFilters): boolean {
    const query = filters.query.trim().toLowerCase();
    const matchesQuery =
      query.length === 0 ||
      batch.label.toLowerCase().includes(query) ||
      batch.id.toLowerCase().includes(query);
    const matchesStatus =
      filters.status === "all" || batch.status === filters.status;
    const matchesOwner =
      filters.owner === "all" || batch.owner === filters.owner;

    return matchesQuery && matchesStatus && matchesOwner;
  }
}
