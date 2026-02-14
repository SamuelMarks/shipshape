import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
} from "@angular/core";
import { NonNullableFormBuilder, ReactiveFormsModule } from "@angular/forms";
import { toSignal } from "@angular/core/rxjs-interop";
import { map, startWith } from "rxjs/operators";

import { StatusPillComponent } from "../../ui/status-pill.component";
import { ApiService } from "../../services/api.service";
import { BatchRun } from "../../services/api.models";

interface BatchFilters {
  query: string;
  status: string;
  owner: string;
}

@Component({
  selector: "shipshape-batch-page",
  templateUrl: "./batch.page.html",
  styleUrl: "./batch.page.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ReactiveFormsModule, StatusPillComponent],
})
export class BatchPage {
  private readonly fb = inject(NonNullableFormBuilder);
  private readonly api = inject(ApiService);

  readonly statusOptions = [
    "all",
    "Queued",
    "Running",
    "Complete",
    "Failed",
  ] as const;
  readonly ownerOptions = [
    "all",
    "Core Platform",
    "Data Ops",
    "Runtime",
  ] as const;

  readonly filterForm = this.fb.group({
    query: "",
    status: "all",
    owner: "all",
  });

  private readonly runs = toSignal(
    this.api.getBatchRuns().pipe(map((response) => response.runs)),
    { initialValue: [] },
  );

  readonly batches = computed<BatchRun[]>(() => this.runs());

  private readonly filters = toSignal(
    this.filterForm.valueChanges.pipe(
      startWith(this.filterForm.getRawValue()),
      map((value) => ({ ...this.filterForm.getRawValue(), ...value })),
    ),
    { initialValue: this.filterForm.getRawValue() },
  );

  readonly filteredBatches = computed(() => {
    const filters = this.filters();
    return this.batches().filter((batch) => this.matchesFilter(batch, filters));
  });

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
