import { ChangeDetectionStrategy, Component, input } from "@angular/core";

export type StatusTone = "good" | "warn" | "bad" | "info";

@Component({
  selector: "shipshape-status-pill",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span
      class="status-pill"
      [class.is-good]="tone() === 'good'"
      [class.is-warn]="tone() === 'warn'"
      [class.is-bad]="tone() === 'bad'"
      [class.is-info]="tone() === 'info'"
    >
      {{ label() }}
    </span>
  `,
  host: {
    class: "status-pill-host",
  },
})
export class StatusPillComponent {
  label = input.required<string>();
  tone = input<StatusTone>("info");
}
