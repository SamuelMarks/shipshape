/**
 * @file Reusable status pill badge component.
 */

import { Component, input } from "@angular/core";

/** Visual tone variants supported by {@link StatusPillComponent}. */
export type StatusTone = "good" | "warn" | "bad" | "info";

/**
 * Renders a compact status badge styled according to {@link tone}.
 */
@Component({
  selector: "shipshape-status-pill",
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
  /** Text displayed inside the status pill. */
  label = input.required<string>();
  /** Semantic tone (`"good"`, `"warn"`, `"bad"`, or `"info"`). */
  tone = input<StatusTone>("info");
}
