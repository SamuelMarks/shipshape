/**
 * @file Unit tests for {@link DiffDraftController}.
 */

import { signal } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { of } from "rxjs";

import { DiffFile } from "./api.models";
import { ApiService } from "./api.service";
import { DiffDraftController } from "./diff-draft.controller";

const sampleFiles: DiffFile[] = [
  {
    path: "src/lib.rs",
    summary: "Core library",
    language: "rust",
    original: "pub fn a() {}",
    modified: "pub fn a() { /* mod */ }",
    tone: "good",
    statusLabel: "Modified",
  },
  {
    path: "README.md",
    summary: "Docs",
    language: "markdown",
    original: "# Title",
    modified: "# Updated Title",
    tone: "info",
    statusLabel: "Modified",
  },
];

describe("DiffDraftController", () => {
  it("manages selection, drafts, persistence, and reset", () => {
    const updateDiff = jasmine.createSpy("updateDiff").and.callFake((payload) =>
      of({
        file: {
          ...sampleFiles[0],
          path: payload.path,
          modified: payload.modified,
        },
      }),
    );

    TestBed.configureTestingModule({
      providers: [
        {
          provide: ApiService,
          useValue: { updateDiff },
        },
      ],
    });

    const filesSignal = signal<DiffFile[]>(sampleFiles);
    const controller = TestBed.runInInjectionContext(
      () => new DiffDraftController(filesSignal),
    );

    expect(controller.selectedFile()?.path).toBe("src/lib.rs");
    expect(controller.originalModel().code).toBe("pub fn a() {}");
    expect(controller.modifiedModel().code).toBe("pub fn a() { /* mod */ }");

    controller.updateModified("pub fn a() { /* edited */ }");
    expect(updateDiff).toHaveBeenCalledTimes(1);

    // Repeating same modification does not re-post
    controller.updateModified("pub fn a() { /* edited */ }");
    expect(updateDiff).toHaveBeenCalledTimes(1);

    controller.selectFile(sampleFiles[1]);
    expect(controller.selectedFile()?.path).toBe("README.md");

    controller.selectFile(sampleFiles[0]);
    expect(controller.selectedModified()).toBe("pub fn a() { /* edited */ }");

    controller.selectFile(sampleFiles[1]);
    controller.reset();
    controller.selectFile(sampleFiles[0]);
    expect(controller.selectedModified()).toBe("pub fn a() { /* mod */ }");

    // Null selection handling
    controller.selectedFile.set(null);
    expect(controller.originalModel().language).toBe("plaintext");
    expect(controller.modifiedModel().code).toBe("");
    controller.updateModified("ignored");
    controller.selectFile(sampleFiles[0]);
    expect(controller.selectedFile()?.path).toBe("src/lib.rs");
  });
});
