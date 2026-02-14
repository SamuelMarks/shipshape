import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
} from "@angular/core";
import { toSignal } from "@angular/core/rxjs-interop";
import { map } from "rxjs/operators";

import { StatusPillComponent } from "../../ui/status-pill.component";
import { CodeMergeComponent } from "../../ui/code-merge.component";
import { ApiService } from "../../services/api.service";
import { DiffFile } from "../../services/api.models";

interface EditorModel {
  code: string;
  language: string;
}

@Component({
  selector: "shipshape-diff-page",
  templateUrl: "./diff.page.html",
  styleUrl: "./diff.page.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [CodeMergeComponent, StatusPillComponent],
})
export class DiffPage {
  private readonly api = inject(ApiService);
  private readonly diffDrafts = new Map<string, string>();
  private readonly lastPersisted = new Map<string, string>();

  readonly files = toSignal(
    this.api.getDiffListing().pipe(map((response) => response.files)),
    { initialValue: [] },
  );

  readonly selectedFile = signal<DiffFile | null>(null);
  readonly selectedModified = signal<string>("");

  readonly originalModel = computed<EditorModel>(() => {
    const selected = this.selectedFile();
    if (!selected) {
      return { code: "", language: "plaintext" };
    }
    return { code: selected.original, language: selected.language };
  });

  readonly modifiedModel = computed<EditorModel>(() => {
    const selected = this.selectedFile();
    if (!selected) {
      return { code: "", language: "plaintext" };
    }
    return { code: this.selectedModified(), language: selected.language };
  });

  constructor() {
    effect(() => {
      const files = this.files();
      if (!this.selectedFile() && files.length > 0) {
        this.selectFile(files[0]!);
      }
    });
  }

  selectFile(file: DiffFile): void {
    this.stashCurrentDraft();
    this.seedPersisted(file);
    this.selectedFile.set(file);
    const draft = this.diffDrafts.get(file.path) ?? file.modified;
    this.selectedModified.set(draft);
  }

  updateModified(modified: string): void {
    this.selectedModified.set(modified);
    const current = this.selectedFile();
    if (!current) {
      return;
    }
    this.diffDrafts.set(current.path, modified);
    this.persistDraft(current.path, modified);
  }

  private seedPersisted(file: DiffFile): void {
    if (!this.lastPersisted.has(file.path)) {
      this.lastPersisted.set(file.path, file.modified);
    }
  }

  private stashCurrentDraft(): void {
    const current = this.selectedFile();
    if (!current) {
      return;
    }
    const modified = this.selectedModified();
    this.diffDrafts.set(current.path, modified);
    this.persistDraft(current.path, modified);
  }

  private persistDraft(path: string, modified: string): void {
    if (this.lastPersisted.get(path) === modified) {
      return;
    }
    this.api.updateDiff({ path, modified }).subscribe((response) => {
      this.lastPersisted.set(path, response.file.modified);
    });
  }
}
