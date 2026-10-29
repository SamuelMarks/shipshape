/**
 * @file Reusable KPI metric card component for the Fleet Dashboard.
 */

import { Component, input } from "@angular/core";

/** Visual tone variants supported by {@link MetricCardComponent}. */
export type MetricTone = "good" | "warn" | "bad" | "info";

/**
 * Displays a single fleet metric with label, primary value, optional trend badge,
 * optional detail line, and semantic status tone styling.
 */
@Component({
  selector: "shipshape-metric-card",
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
  /** Metric title label. */
  label = input.required<string>();
  /** Primary metric value string. */
  value = input.required<string>();
  /** Optional trend delta string. */
  trend = input<string | null>(null);
  /** Optional secondary detail text. */
  detail = input<string | null>(null);
  /** Semantic tone (`"good"`, `"warn"`, `"bad"`, or `"info"`). */
  tone = input<MetricTone>("info");
}
