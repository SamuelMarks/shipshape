/**
 * @file Interactive side-by-side Diff Viewer page component.
 */

import { Component, inject } from "@angular/core";
import { toSignal } from "@angular/core/rxjs-interop";
import { map } from "rxjs/operators";

import { StatusPillComponent } from "../../ui/status-pill.component";
import { CodeMergeComponent } from "../../ui/code-merge.component";
import { ApiService } from "../../services/api.service";
import { DiffFile } from "../../services/api.models";
import { DiffDraftController } from "../../services/diff-draft.controller";

/**
 * Page component for reviewing and editing mechanical refit diffs side-by-side.
 */
@Component({
  selector: "shipshape-diff-page",
  templateUrl: "./diff.page.html",
  styleUrl: "./diff.page.css",
  imports: [CodeMergeComponent, StatusPillComponent],
})
export class DiffPage {
  /** Backend API service client. */
  private readonly api = inject(ApiService);

  /** Reactive list of diff files loaded from `/api/diffs`. */
  readonly files = toSignal(
    this.api.getDiffListing().pipe(map((response) => response.files)),
    { initialValue: [] },
  );

  /** Shared diff draft and persistence controller. */
  private readonly drafts = new DiffDraftController(this.files);

  /** Currently selected diff file. */
  readonly selectedFile = this.drafts.selectedFile;
  /** Currently active modified code for the selected diff file. */
  readonly selectedModified = this.drafts.selectedModified;
  /** Read-only original code model for the selected file. */
  readonly originalModel = this.drafts.originalModel;
  /** Editable modified code model for the selected file. */
  readonly modifiedModel = this.drafts.modifiedModel;

  /**
   * Select `file` in the diff viewer after stashing any pending draft.
   *
   * @param file - Diff file to select.
   */
  selectFile(file: DiffFile): void {
    this.drafts.selectFile(file);
  }

  /**
   * Update the modified content for the active diff file and persist changes.
   *
   * @param modified - Updated source text.
   */
  updateModified(modified: string): void {
    this.drafts.updateModified(modified);
  }
}
