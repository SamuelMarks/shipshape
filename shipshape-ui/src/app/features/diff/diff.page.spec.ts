import { TestBed } from "@angular/core/testing";
import { of } from "rxjs";

import { DiffPage } from "./diff.page";
import { ApiService } from "../../services/api.service";
import { DiffListingResponse } from "../../services/api.models";

const mockDiffs: DiffListingResponse = {
  files: [
    {
      path: "src/main.rs",
      summary: "Update core",
      language: "rust",
      original: "fn main() {}",
      modified: 'fn main() { println!("hi"); }',
      tone: "good",
      statusLabel: "Modified",
    },
    {
      path: "README.md",
      summary: "Docs",
      language: "markdown",
      original: "Hello",
      modified: "Hello world",
      tone: "info",
      statusLabel: "Modified",
    },
  ],
};

describe("DiffPage", () => {
  const setup = (diffResponse: DiffListingResponse = mockDiffs) => {
    const updateDiff = jasmine.createSpy("updateDiff").and.callFake((payload) =>
      of({
        file: {
          path: payload.path,
          summary: "",
          language: "rust",
          original: "",
          modified: payload.modified,
          tone: "info",
          statusLabel: "Modified",
        },
      }),
    );

    TestBed.configureTestingModule({
      imports: [DiffPage],
      providers: [
        {
          provide: ApiService,
          useValue: {
            getDiffListing: () => of(diffResponse),
            updateDiff,
          },
        },
      ],
    });

    const fixture = TestBed.createComponent(DiffPage);
    fixture.detectChanges();

    return { fixture, page: fixture.componentInstance, updateDiff };
  };

  it("selects files and exposes diff models", () => {
    const { page } = setup();

    expect(page.selectedFile()?.path).toBe("src/main.rs");
    expect(page.originalModel().language).toBe("rust");

    page.selectFile(mockDiffs.files[1]);
    expect(page.modifiedModel().code).toContain("Hello world");
  });

  it("persists edits and keeps drafts across file switches", () => {
    const edited = 'fn main() { println!("edited"); }';
    const { page, updateDiff } = setup();

    page.updateModified(edited);
    expect(updateDiff).toHaveBeenCalledWith({
      path: "src/main.rs",
      modified: edited,
    });

    page.selectFile(mockDiffs.files[1]);
    page.selectFile(mockDiffs.files[0]);

    expect(page.modifiedModel().code).toBe(edited);
  });

  it("skips updates when no file is selected or content is unchanged", () => {
    const { page, updateDiff } = setup();

    page.selectedFile.set(null);
    page.updateModified("no-file");
    expect(updateDiff).not.toHaveBeenCalled();

    page.selectFile(mockDiffs.files[0]);
    updateDiff.calls.reset();
    page.updateModified(mockDiffs.files[0].modified);
    expect(updateDiff).not.toHaveBeenCalled();
  });

  it("returns empty models when no file is selected", () => {
    const updateDiff = jasmine
      .createSpy("updateDiff")
      .and.returnValue(of({ file: mockDiffs.files[0] }));
    TestBed.configureTestingModule({
      imports: [DiffPage],
      providers: [
        {
          provide: ApiService,
          useValue: {
            getDiffListing: () => of({ files: [] }),
            updateDiff,
          },
        },
      ],
    });

    const fixture = TestBed.createComponent(DiffPage);
    fixture.detectChanges();

    const page = fixture.componentInstance;
    expect(page.originalModel().code).toBe("");
    expect(page.modifiedModel().language).toBe("plaintext");
  });
});
