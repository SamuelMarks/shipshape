/**
 * @file Shared diff draft and persistence state management for the Diff Viewer
 * and Workflow Studio pages.
 */

import { DestroyRef, Signal, computed, inject, linkedSignal } from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";

import { DiffFile } from "./api.models";
import { ApiService } from "./api.service";

/**
 * Code and language model consumed by CodeMirror editor components.
 */
export interface EditorModel {
  /** Source code content. */
  code: string;
  /** Language identifier (e.g., `"rust"`, `"typescript"`, `"plaintext"`). */
  language: string;
}

/**
 * Reactive controller managing selected diff files, unsaved draft edits,
 * and deduplicated backend persistence via {@link ApiService}.
 */
export class DiffDraftController {
  /** API client used to persist diff updates. */
  private readonly api = inject(ApiService);
  /** Component destroy reference used to clean up in-flight subscriptions. */
  private readonly destroyRef = inject(DestroyRef);
  /** In-memory map of file paths to modified draft contents. */
  private readonly diffDrafts = new Map<string, string>();
  /** In-memory map of file paths to the last content persisted to the server. */
  private readonly lastPersisted = new Map<string, string>();

  /** Currently selected diff file, defaulting to the first file in `files`. */
  readonly selectedFile: ReturnType<typeof linkedSignal<DiffFile | null>>;
  /** Currently active modified code for the selected diff file. */
  readonly selectedModified: ReturnType<typeof linkedSignal<string>>;
  /** Read-only original code model for the selected file. */
  readonly originalModel: Signal<EditorModel>;
  /** Editable modified code model for the selected file. */
  readonly modifiedModel: Signal<EditorModel>;

  /**
   * Create a new diff draft controller bound to a reactive `files` signal.
   *
   * @param files - Signal emitting the current list of diff files.
   */
  constructor(private readonly files: Signal<DiffFile[]>) {
    this.selectedFile = linkedSignal<DiffFile | null>(
      () => this.files()[0] ?? null,
    );
    this.selectedModified = linkedSignal<string>(() => {
      const file = this.selectedFile();
      return file ? (this.diffDrafts.get(file.path) ?? file.modified) : "";
    });
    this.originalModel = computed<EditorModel>(() => {
      const selected = this.selectedFile();
      if (!selected) {
        return { code: "", language: "plaintext" };
      }
      return { code: selected.original, language: selected.language };
    });
    this.modifiedModel = computed<EditorModel>(() => {
      const selected = this.selectedFile();
      if (!selected) {
        return { code: "", language: "plaintext" };
      }
      return { code: this.selectedModified(), language: selected.language };
    });
  }

  /**
   * Stash the current file's draft and switch selection to `file`.
   *
   * @param file - Diff file to select.
   */
  selectFile(file: DiffFile): void {
    this.stashCurrentDraft();
    this.selectedFile.set(file);
  }

  /**
   * Update the modified content for the currently selected file and persist if changed.
   *
   * @param modified - Updated source text.
   */
  updateModified(modified: string): void {
    this.selectedModified.set(modified);
    const current = this.selectedFile();
    if (!current) {
      return;
    }
    this.diffDrafts.set(current.path, modified);
    this.persistDraft(current, modified);
  }

  /**
   * Clear all cached drafts and persisted baselines.
   */
  reset(): void {
    this.diffDrafts.clear();
    this.lastPersisted.clear();
  }

  /**
   * Save the active draft in memory and trigger persistence before switching files.
   */
  private stashCurrentDraft(): void {
    const current = this.selectedFile();
    if (!current) {
      return;
    }
    const modified = this.selectedModified();
    this.diffDrafts.set(current.path, modified);
    this.persistDraft(current, modified);
  }

  /**
   * Persist `modified` content for `file` to the backend if it differs from the baseline.
   *
   * @param file - Target diff file.
   * @param modified - Modified source content.
   */
  private persistDraft(file: DiffFile, modified: string): void {
    const baseline = this.lastPersisted.get(file.path) ?? file.modified;
    if (baseline === modified) {
      return;
    }
    this.api
      .updateDiff({ path: file.path, modified })
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe((response) => {
        this.lastPersisted.set(file.path, response.file.modified);
      });
  }
}
