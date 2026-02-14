import { TestBed, fakeAsync, flush } from "@angular/core/testing";
import { of } from "rxjs";

import { WorkflowPage } from "./workflow.page";
import { ApiService } from "../../services/api.service";
import { DiffFile, DiffListingResponse } from "../../services/api.models";

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

const emptyDiffs: DiffListingResponse = { files: [] };

describe("WorkflowPage", () => {
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
      imports: [WorkflowPage],
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

    const fixture = TestBed.createComponent(WorkflowPage);
    fixture.detectChanges();

    return { fixture, page: fixture.componentInstance, updateDiff };
  };

  it("runs tools and loads diff files", fakeAsync(() => {
    const { fixture, page } = setup();

    page.runTools();
    flush();
    fixture.detectChanges();

    expect(page.toolsStatus()).toBe("complete");
    expect(page.files().length).toBe(2);
    expect(page.selectedFile()?.path).toBe("src/main.rs");
    expect(page.toolRunSummary()).toContain("2 files changed");
    expect(page.showNothingToDo()).toBeFalse();
  }));

  it("handles empty tool selections and no-change responses", fakeAsync(() => {
    const { page } = setup(emptyDiffs);

    page.selectedTools.set([]);
    page.runTools();

    expect(page.toolsStatus()).toBe("complete");
    expect(page.toolRunSummary()).toContain("No tools selected");
    expect(page.showNothingToDo()).toBeTrue();

    page.selectedTools.set(["audit"]);
    page.runTools();
    flush();

    expect(page.toolRunSummary()).toContain("Nothing to do");
    expect(page.showNothingToDo()).toBeTrue();
  }));

  it("toggles tools and respects clone readiness", fakeAsync(() => {
    const { page } = setup();

    const firstTool = page.toolOptions[0]!;
    page.toggleTool(firstTool);
    expect(page.selectedTools()).not.toContain(firstTool.id);

    page.toggleTool(firstTool);
    expect(page.selectedTools()).toContain(firstTool.id);

    page.workflowForm.patchValue({
      repoUrl: "",
      forkTarget: "",
      gitlabNamespace: "",
      gitlabProject: "",
    });
    flush();

    expect(page.projectReady()).toBeFalse();
    expect(page.forkReady()).toBeFalse();
    expect(page.mirrorReady()).toBeFalse();
    expect(page.canClone()).toBeFalse();

    page.cloneRepo();
    expect(page.cloneStatus()).toBe("idle");

    page.workflowForm.patchValue({
      repoUrl: "https://github.com/acme/rocket",
      forkTarget: "acme",
      gitlabNamespace: "acme-private",
      gitlabProject: "rocket",
    });
    flush();

    expect(page.canClone()).toBeTrue();
    page.cloneRepo();
    expect(page.cloneStatus()).toBe("complete");
  }));

  it("computes labels, tones, action text, and editor models", () => {
    const { page } = setup();

    page.cloneStatus.set("idle");
    expect(page.cloneStatusLabel()).toBe("Awaiting clone");
    expect(page.cloneStatusTone()).toBe("warn");
    expect(page.cloneSummary()).toContain("Ready to clone");

    page.workflowForm.patchValue({ clonePath: "" });
    expect(page.cloneSummary()).toContain("workspace");

    page.cloneStatus.set("running");
    expect(page.cloneStatusLabel()).toBe("Cloning");
    expect(page.cloneStatusTone()).toBe("info");
    expect(page.cloneSummary()).toContain("Cloning into");

    page.cloneStatus.set("complete");
    expect(page.cloneStatusLabel()).toBe("Cloned");
    expect(page.cloneStatusTone()).toBe("good");
    expect(page.cloneSummary()).toContain("Cloned into");

    page.toolsStatus.set("idle");
    expect(page.toolsStatusLabel()).toBe("Idle");
    expect(page.toolsStatusTone()).toBe("warn");

    page.toolsStatus.set("running");
    expect(page.toolsStatusLabel()).toBe("Running");
    expect(page.toolsStatusTone()).toBe("info");

    page.toolsStatus.set("complete");
    expect(page.toolsStatusLabel()).toBe("Complete");
    expect(page.toolsStatusTone()).toBe("good");

    page.dockerStatus.set("idle");
    expect(page.dockerStatusLabel()).toBe("Idle");
    expect(page.dockerStatusTone()).toBe("warn");

    page.dockerStatus.set("running");
    expect(page.dockerStatusLabel()).toBe("Running");
    expect(page.dockerStatusTone()).toBe("info");

    page.dockerStatus.set("failed");
    expect(page.dockerStatusLabel()).toBe("Failed");
    expect(page.dockerStatusTone()).toBe("bad");

    page.dockerStatus.set("success");
    expect(page.dockerStatusLabel()).toBe("Passed");
    expect(page.dockerStatusTone()).toBe("good");

    page.workflowForm.patchValue({
      forkTargetType: "org",
      forkTarget: "shipshape",
    });
    expect(page.forkActionLabel()).toBe("Fork to GitHub org: shipshape");

    page.workflowForm.patchValue({ forkTargetType: "org", forkTarget: "" });
    expect(page.forkActionLabel()).toBe("Fork to GitHub org");

    page.workflowForm.patchValue({
      forkTargetType: "personal",
      forkTarget: "pilot",
    });
    expect(page.forkActionLabel()).toBe(
      "Fork to GitHub personal account: pilot",
    );

    page.workflowForm.patchValue({
      forkTargetType: "personal",
      forkTarget: "",
    });
    expect(page.forkActionLabel()).toBe("Fork to GitHub personal account");

    page.workflowForm.patchValue({ gitlabNamespace: "", gitlabProject: "" });
    expect(page.mirrorActionLabel()).toBe(
      "Mirror to private GitLab: private GitLab mirror",
    );

    page.workflowForm.patchValue({
      gitlabNamespace: "acme",
      gitlabProject: "rocket",
    });
    expect(page.mirrorActionLabel()).toBe(
      "Mirror to private GitLab: acme/rocket",
    );

    page.selectedFile.set(null);
    expect(page.originalModel().language).toBe("plaintext");
    expect(page.modifiedModel().code).toBe("");

    page.selectedFile.set(mockDiffs.files[0]!);
    page.selectedModified.set("tuned");
    expect(page.originalModel().code).toContain("fn main");
    expect(page.modifiedModel().code).toBe("tuned");
  });

  it("handles docker test gating and overrides", () => {
    const { page } = setup();

    page.runDockerTest();
    expect(page.dockerStatus()).toBe("failed");
    expect(page.dockerLogs()[page.dockerLogs().length - 1]).toContain(
      "run tools",
    );

    page.toolsStatus.set("complete");
    page.files.set([]);
    page.runDockerTest();
    expect(page.dockerStatus()).toBe("failed");
    expect(page.dockerLogs()[page.dockerLogs().length - 1]).toContain(
      "no changes",
    );

    page.workflowForm.patchValue({ gitlabProject: "" });
    page.files.set([mockDiffs.files[0]!]);
    page.runDockerTest();
    expect(page.dockerStatus()).toBe("success");
    expect(page.dockerLogs()[0]).toContain("shipshape-preview");

    page.allowPublishOverride.set(false);
    page.dockerStatus.set("failed");
    expect(page.canPublish()).toBeFalse();

    page.updateOverride({ target: { checked: true } } as unknown as Event);
    expect(page.allowPublishOverride()).toBeTrue();
    expect(page.canPublish()).toBeTrue();
  });

  it("persists edits and avoids redundant saves", fakeAsync(() => {
    const { page, updateDiff } = setup();

    page.selectedFile.set(null);
    page.updateModified("noop");
    expect(updateDiff).not.toHaveBeenCalled();

    page.runTools();
    flush();
    const target = page.files()[0]!;
    page.selectFile(target);

    updateDiff.calls.reset();
    page.updateModified(target.modified);
    expect(updateDiff).not.toHaveBeenCalled();

    page.updateModified("updated content");
    expect(updateDiff).toHaveBeenCalledWith({
      path: target.path,
      modified: "updated content",
    });

    page.selectFile(mockDiffs.files[1]);
  }));

  it("builds a file tree with mixed nodes and sorting", () => {
    const { page } = setup();

    const customFiles: DiffFile[] = [
      {
        path: "src",
        summary: "Folder marker",
        language: "text",
        original: "src",
        modified: "src",
        tone: "info",
        statusLabel: "Modified",
      },
      {
        path: "src/main.rs",
        summary: "Update core",
        language: "rust",
        original: "fn main() {}",
        modified: "fn main() {}",
        tone: "good",
        statusLabel: "Modified",
      },
      {
        path: "beta/file.txt",
        summary: "Beta",
        language: "text",
        original: "beta",
        modified: "beta",
        tone: "info",
        statusLabel: "Modified",
      },
      {
        path: "alpha/file.txt",
        summary: "Alpha",
        language: "text",
        original: "alpha",
        modified: "alpha",
        tone: "info",
        statusLabel: "Modified",
      },
    ];

    const tree = (page as any).buildFileTree(customFiles) as Array<{
      label: string;
      kind: string;
    }>;

    expect(
      tree.some((node) => node.kind === "dir" && node.label === "alpha"),
    ).toBeTrue();
    expect(
      tree.some((node) => node.kind === "file" && node.label === "main.rs"),
    ).toBeTrue();

    const compareTreeNodes = (page as any).compareTreeNodes as (
      a: any,
      b: any,
    ) => number;
    const fileNode = {
      name: "file.txt",
      path: "file.txt",
      children: new Map(),
      file: customFiles[0],
    };
    const dirNode = {
      name: "dir",
      path: "dir",
      children: new Map([["file.txt", fileNode]]),
    };
    const dirAlpha = { name: "alpha", path: "alpha", children: new Map() };
    const dirBeta = { name: "beta", path: "beta", children: new Map() };

    expect(compareTreeNodes(fileNode, dirNode)).toBe(1);
    expect(compareTreeNodes(dirNode, fileNode)).toBe(-1);
    expect(compareTreeNodes(dirAlpha, dirBeta)).toBeLessThan(0);
  });

  it("ignores override updates when the event target is missing", () => {
    const { page } = setup();

    page.allowPublishOverride.set(false);
    page.updateOverride({ target: null } as unknown as Event);

    expect(page.allowPublishOverride()).toBeFalse();
  });
});
