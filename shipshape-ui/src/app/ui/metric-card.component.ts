import { ChangeDetectionStrategy, Component, input } from "@angular/core";

export type MetricTone = "good" | "warn" | "bad" | "info";

@Component({
  selector: "shipshape-metric-card",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div
      class="metric-card"
      [class.is-good]="tone() === 'good'"
      [class.is-warn]="tone() === 'warn'"
      [class.is-bad]="tone() === 'bad'"
      [class.is-info]="tone() === 'info'"
    >
      <div class="metric-card__header">
        <span class="metric-card__label">{{ label() }}</span>
        @if (trend()) {
          <span class="metric-card__trend">{{ trend() }}</span>
        }
      </div>
      <div class="metric-card__value">{{ value() }}</div>
      @if (detail()) {
        <div class="metric-card__detail">{{ detail() }}</div>
      }
    </div>
  `,
  host: {
    class: "metric-card-host",
  },
})
export class MetricCardComponent {
  label = input.required<string>();
  value = input.required<string>();
  trend = input<string | null>(null);
  detail = input<string | null>(null);
  tone = input<MetricTone>("info");
}
