import { TestBed } from '@angular/core/testing';
import { of } from 'rxjs';
import { NGX_MONACO_EDITOR_CONFIG, NgxMonacoEditorConfig } from 'ngx-monaco-editor';

import { DiffPage } from './diff.page';
import { ApiService } from '../../services/api.service';
import { DiffListingResponse } from '../../services/api.models';

const monacoTestConfig: NgxMonacoEditorConfig = {
  baseUrl: 'assets/monaco/vs'
};
const mockDiffs: DiffListingResponse = {
  files: [
    {
      path: 'src/main.rs',
      summary: 'Update core',
      language: 'rust',
      original: 'fn main() {}',
      modified: 'fn main() { println!("hi"); }',
      tone: 'good',
      statusLabel: 'Modified'
    },
    {
      path: 'README.md',
      summary: 'Docs',
      language: 'markdown',
      original: 'Hello',
      modified: 'Hello world',
      tone: 'info',
      statusLabel: 'Modified'
    }
  ]
};

describe('DiffPage', () => {
  it('selects files and exposes Monaco models', () => {
    TestBed.configureTestingModule({
      imports: [DiffPage],
      providers: [
        {
          provide: ApiService,
          useValue: {
            getDiffListing: () => of(mockDiffs)
          }
        },
        {
          provide: NGX_MONACO_EDITOR_CONFIG,
          useValue: monacoTestConfig
        }
      ]
    });

    const fixture = TestBed.createComponent(DiffPage);
    fixture.detectChanges();

    const page = fixture.componentInstance;
    expect(page.selectedFile()?.path).toBe('src/main.rs');

    page.selectFile(mockDiffs.files[1]);
    expect(page.modifiedModel().code).toContain('Hello world');
  });

  it('returns empty models when no file is selected', () => {
    TestBed.configureTestingModule({
      imports: [DiffPage],
      providers: [
        {
          provide: ApiService,
          useValue: {
            getDiffListing: () => of({ files: [] })
          }
        },
        {
          provide: NGX_MONACO_EDITOR_CONFIG,
          useValue: monacoTestConfig
        }
      ]
    });

    const fixture = TestBed.createComponent(DiffPage);
    fixture.detectChanges();

    const page = fixture.componentInstance;
    expect(page.originalModel().code).toBe('');
    expect(page.modifiedModel().language).toBe('plaintext');
  });
});
