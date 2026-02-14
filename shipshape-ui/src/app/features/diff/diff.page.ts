import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal
} from '@angular/core';
import { MonacoEditorModule } from 'ngx-monaco-editor';
import { toSignal } from '@angular/core/rxjs-interop';
import { map } from 'rxjs/operators';

import { StatusPillComponent } from '../../ui/status-pill.component';
import { ApiService } from '../../services/api.service';
import { DiffFile } from '../../services/api.models';

interface MonacoModel {
  code: string;
  language: string;
}

@Component({
  selector: 'shipshape-diff-page',
  templateUrl: './diff.page.html',
  styleUrl: './diff.page.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MonacoEditorModule, StatusPillComponent]
})
export class DiffPage {
  private readonly api = inject(ApiService);

  readonly files = toSignal(
    this.api.getDiffListing().pipe(map((response) => response.files)),
    { initialValue: [] }
  );

  readonly selectedFile = signal<DiffFile | null>(null);

  readonly diffOptions = {
    renderSideBySide: true,
    readOnly: true,
    automaticLayout: true,
    minimap: { enabled: false }
  };

  readonly originalModel = computed<MonacoModel>(() => {
    const selected = this.selectedFile();
    if (!selected) {
      return { code: '', language: 'plaintext' };
    }
    return { code: selected.original, language: selected.language };
  });

  readonly modifiedModel = computed<MonacoModel>(() => {
    const selected = this.selectedFile();
    if (!selected) {
      return { code: '', language: 'plaintext' };
    }
    return { code: selected.modified, language: selected.language };
  });

  constructor() {
    effect(() => {
      const files = this.files();
      if (!this.selectedFile() && files.length > 0) {
        this.selectedFile.set(files[0]!);
      }
    });
  }

  selectFile(file: DiffFile): void {
    this.selectedFile.set(file);
  }
}
