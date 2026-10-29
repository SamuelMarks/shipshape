/**
 * @file Control Room page component for configuring and queuing batch
 * audit, refit, and launch runs.
 */

import {
  Component,
  DestroyRef,
  computed,
  inject,
  linkedSignal,
  signal,
} from "@angular/core";
import { NonNullableFormBuilder, ReactiveFormsModule } from "@angular/forms";
import { toSignal, takeUntilDestroyed } from "@angular/core/rxjs-interop";

import { StatusPillComponent } from "../../ui/status-pill.component";
import { ApiService } from "../../services/api.service";
import {
  ActivityLog,
  ControlQueueRequest,
  MechanicOption,
} from "../../services/api.models";

/**
 * Control Room page component allowing operators to select mechanics,
 * configure repository sources, and queue fleet workflows.
 */
@Component({
  selector: "shipshape-control-room-page",
  templateUrl: "./control.page.html",
  styleUrl: "./control.page.css",
  imports: [ReactiveFormsModule, StatusPillComponent],
})
export class ControlRoomPage {
  /** Non-nullable reactive form builder. */
  private readonly fb = inject(NonNullableFormBuilder);
  /** Backend API client. */
  private readonly api = inject(ApiService);
  /** Component destroy reference for cleaning up queue subscriptions. */
  private readonly destroyRef = inject(DestroyRef);

  /** Signal of control room options and initial activity log from `GET /api/control/options`. */
  private readonly options = toSignal(this.api.getControlOptions(), {
    initialValue: { mechanics: [], activity: [] },
  });

  /** Available refit mechanics loaded from the backend. */
  readonly mechanics = computed<MechanicOption[]>(
    () => this.options().mechanics,
  );
  /** Currently selected mechanic IDs. */
  readonly selectedMechanics = signal<string[]>(["cpp-types", "ci-drydock"]);

  /** Reactive form for source type, source value, workflow mode, and dry-run toggle. */
  readonly controlForm = this.fb.group({
    sourceType: "url",
    sourceValue: "https://github.com/shipshape/fleet-core",
    mode: "audit",
    dryRun: true,
  });

  /** Activity log entries initialized from `options` and prepended on new runs. */
  readonly activityLog = linkedSignal<ActivityLog[]>(
    () => this.options().activity,
  );

  /**
   * Toggle selection of `option` in {@link selectedMechanics}.
   *
   * @param option - Mechanic option to toggle.
   */
  toggleMechanic(option: MechanicOption): void {
    this.selectedMechanics.update((current) => {
      if (current.includes(option.id)) {
        return current.filter((item) => item !== option.id);
      }
      return [...current, option.id];
    });
  }

  /**
   * Submit the current form and mechanic selection to `POST /api/control/queue`
   * and prepend the resulting activity log entry.
   */
  queueRun(): void {
    const formValue = this.controlForm.getRawValue();
    const payload: ControlQueueRequest = {
      sourceType: formValue.sourceType,
      sourceValue: formValue.sourceValue,
      mode: formValue.mode,
      dryRun: formValue.dryRun,
      mechanicIds: this.selectedMechanics(),
    };
    this.api
      .queueControlRun(payload)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe((response) => {
        this.activityLog.update((items) => [response.log, ...items]);
      });
  }
}
