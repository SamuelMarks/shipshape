import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  computed,
  effect,
  inject,
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

@Component({
  selector: "shipshape-control-room-page",
  templateUrl: "./control.page.html",
  styleUrl: "./control.page.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ReactiveFormsModule, StatusPillComponent],
})
export class ControlRoomPage {
  private readonly fb = inject(NonNullableFormBuilder);
  private readonly api = inject(ApiService);
  private readonly destroyRef = inject(DestroyRef);

  private readonly options = toSignal(this.api.getControlOptions(), {
    initialValue: { mechanics: [], activity: [] },
  });

  readonly mechanics = computed<MechanicOption[]>(
    () => this.options().mechanics,
  );
  readonly selectedMechanics = signal<string[]>(["cpp-types", "ci-drydock"]);

  readonly controlForm = this.fb.group({
    sourceType: "url",
    sourceValue: "https://github.com/shipshape/fleet-core",
    mode: "audit",
    dryRun: true,
  });

  readonly activityLog = signal<ActivityLog[]>([]);

  constructor() {
    effect(() => {
      if (
        this.activityLog().length === 0 &&
        this.options().activity.length > 0
      ) {
        this.activityLog.set(this.options().activity);
      }
    });
  }

  toggleMechanic(option: MechanicOption): void {
    this.selectedMechanics.update((current) => {
      if (current.includes(option.id)) {
        return current.filter((item) => item !== option.id);
      }
      return [...current, option.id];
    });
  }

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
