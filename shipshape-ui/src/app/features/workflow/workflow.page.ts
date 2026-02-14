import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  computed,
  effect,
  inject,
  signal,
} from "@angular/core";
import { NonNullableFormBuilder, ReactiveFormsModule } from "@angular/forms";
import { takeUntilDestroyed, toSignal } from "@angular/core/rxjs-interop";
import { map, startWith } from "rxjs/operators";

import { ApiService } from "../../services/api.service";
import { DiffFile } from "../../services/api.models";
import { CodeEditorComponent } from "../../ui/code-editor.component";
import { CodeMergeComponent } from "../../ui/code-merge.component";
import {
  StatusPillComponent,
  StatusTone,
} from "../../ui/status-pill.component";

type DockerStatus = "idle" | "running" | "success" | "failed";
type TabKey = "edit" | "diff";

interface ToolOption {
  id: string;
  label: string;
  description: string;
}

interface FileTreeItem {
  id: string;
  label: string;
  path: string;
  depth: number;
  kind: "dir" | "file";
  file?: DiffFile;
}

interface FileTreeNode {
  name: string;
  path: string;
  children: Map<string, FileTreeNode>;
  file?: DiffFile;
}

interface EditorModel {
  code: string;
  language: string;
}

@Component({
  selector: "shipshape-workflow-page",
  templateUrl: "./workflow.page.html",
  styleUrl: "./workflow.page.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    ReactiveFormsModule,
    CodeEditorComponent,
    CodeMergeComponent,
    StatusPillComponent,
  ],
})
export class WorkflowPage {
  private readonly fb = inject(NonNullableFormBuilder);
  private readonly api = inject(ApiService);
  private readonly destroyRef = inject(DestroyRef);
  private readonly diffDrafts = new Map<string, string>();
  private readonly lastPersisted = new Map<string, string>();

  readonly toolOptions: ToolOption[] = [
    {
      id: "audit",
      label: "Audit sweep",
      description: "Inventory coverage risk and test gaps.",
    },
    {
      id: "refit",
      label: "Refit patches",
      description: "Apply mechanical fixes and refactorings.",
    },
    {
      id: "drydock",
      label: "Drydock CI",
      description: "Generate Docker + pipeline verification.",
    },
    {
      id: "docs",
      label: "Docs tune-up",
      description: "Normalize README + usage notes.",
    },
  ];

  readonly workflowForm = this.fb.group({
    repoUrl: "https://github.com/shipshape/fleet-core",
    forkTargetType: "org",
    forkTarget: "shipshape",
    gitlabNamespace: "shipshape-private",
    gitlabProject: "fleet-core-mirror",
    clonePath: "/workspaces/shipshape/fleet-core",
  });

  private readonly formValues = toSignal(
    this.workflowForm.valueChanges.pipe(
      startWith(this.workflowForm.getRawValue()),
      map(() => this.workflowForm.getRawValue()),
    ),
    { initialValue: this.workflowForm.getRawValue() },
  );

  readonly selectedTools = signal<string[]>(["audit", "refit", "drydock"]);
  readonly cloneStatus = signal<"idle" | "running" | "complete">("idle");
  readonly toolsStatus = signal<"idle" | "running" | "complete">("idle");
  readonly dockerStatus = signal<DockerStatus>("idle");
  readonly allowPublishOverride = signal(false);
  readonly activeTab = signal<TabKey>("edit");

  readonly files = signal<DiffFile[]>([]);
  readonly selectedFile = signal<DiffFile | null>(null);
  readonly selectedModified = signal<string>("");
  readonly dockerLogs = signal<string[]>([]);
  readonly toolRunSummary = signal<string>("Tools are idle.");

  readonly projectReady = computed(
    () => this.formValues().repoUrl.trim().length > 0,
  );
  readonly forkReady = computed(
    () => this.formValues().forkTarget.trim().length > 0,
  );
  readonly mirrorReady = computed(() => {
    const values = this.formValues();
    return (
      values.gitlabNamespace.trim().length > 0 &&
      values.gitlabProject.trim().length > 0
    );
  });

  readonly canClone = computed(
    () => this.projectReady() && this.forkReady() && this.mirrorReady(),
  );
  readonly hasChanges = computed(() => this.files().length > 0);
  readonly showNothingToDo = computed(
    () => this.toolsStatus() === "complete" && !this.hasChanges(),
  );

  readonly canPublish = computed(
    () => this.dockerStatus() === "success" || this.allowPublishOverride(),
  );

  readonly forkActionLabel = computed(() => {
    const values = this.formValues();
    const target = values.forkTarget.trim();
    if (values.forkTargetType === "personal") {
      return target
        ? `Fork to GitHub personal account: ${target}`
        : "Fork to GitHub personal account";
    }
    return target ? `Fork to GitHub org: ${target}` : "Fork to GitHub org";
  });

  readonly mirrorActionLabel = computed(() => {
    const values = this.formValues();
    const namespace = values.gitlabNamespace.trim();
    const project = values.gitlabProject.trim();
    const target =
      namespace && project
        ? `${namespace}/${project}`
        : "private GitLab mirror";
    return `Mirror to private GitLab: ${target}`;
  });

  readonly cloneStatusLabel = computed(() => {
    switch (this.cloneStatus()) {
      case "running":
        return "Cloning";
      case "complete":
        return "Cloned";
      default:
        return "Awaiting clone";
    }
  });

  readonly cloneStatusTone = computed<StatusTone>(() => {
    switch (this.cloneStatus()) {
      case "complete":
        return "good";
      case "running":
        return "info";
      default:
        return "warn";
    }
  });

  readonly toolsStatusLabel = computed(() => {
    switch (this.toolsStatus()) {
      case "running":
        return "Running";
      case "complete":
        return "Complete";
      default:
        return "Idle";
    }
  });

  readonly toolsStatusTone = computed<StatusTone>(() => {
    switch (this.toolsStatus()) {
      case "complete":
        return "good";
      case "running":
        return "info";
      default:
        return "warn";
    }
  });

  readonly dockerStatusLabel = computed(() => {
    switch (this.dockerStatus()) {
      case "running":
        return "Running";
      case "success":
        return "Passed";
      case "failed":
        return "Failed";
      default:
        return "Idle";
    }
  });

  readonly dockerStatusTone = computed<StatusTone>(() => {
    switch (this.dockerStatus()) {
      case "success":
        return "good";
      case "failed":
        return "bad";
      case "running":
        return "info";
      default:
        return "warn";
    }
  });

  readonly fileTree = computed<FileTreeItem[]>(() =>
    this.buildFileTree(this.files()),
  );

  readonly cloneSummary = computed(() => {
    const path = this.formValues().clonePath.trim() || "workspace";
    switch (this.cloneStatus()) {
      case "running":
        return `Cloning into ${path}...`;
      case "complete":
        return `Cloned into ${path}.`;
      default:
        return `Ready to clone into ${path}.`;
    }
  });

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
      if (!this.selectedFile() && this.files().length > 0) {
        this.selectFile(this.files()[0]!);
      }
    });
  }

  toggleTool(option: ToolOption): void {
    this.selectedTools.update((current) => {
      if (current.includes(option.id)) {
        return current.filter((item) => item !== option.id);
      }
      return [...current, option.id];
    });
  }

  cloneRepo(): void {
    if (!this.canClone()) {
      return;
    }
    this.cloneStatus.set("running");
    this.cloneStatus.set("complete");
  }

  runTools(): void {
    this.toolsStatus.set("running");
    this.files.set([]);
    this.selectedFile.set(null);
    this.selectedModified.set("");
    this.diffDrafts.clear();
    this.lastPersisted.clear();
    const toolCount = this.selectedTools().length;
    if (toolCount === 0) {
      this.toolsStatus.set("complete");
      this.toolRunSummary.set("No tools selected. Nothing to run.");
      return;
    }

    this.toolRunSummary.set(`Running ${toolCount} tools...`);
    this.api
      .getDiffListing()
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe((response) => {
        this.files.set(response.files);
        this.toolsStatus.set("complete");
        this.toolRunSummary.set(
          response.files.length > 0
            ? `Tools complete. ${response.files.length} files changed.`
            : "Tools complete. Nothing to do.",
        );
      });
  }

  selectFile(file: DiffFile): void {
    this.stashCurrentDraft();
    this.seedPersisted(file);
    this.selectedFile.set(file);
    const draft = this.diffDrafts.get(file.path) ?? file.modified;
    this.selectedModified.set(draft);
  }

  updateModified(value: string): void {
    this.selectedModified.set(value);
    const current = this.selectedFile();
    if (!current) {
      return;
    }
    this.diffDrafts.set(current.path, value);
    this.persistDraft(current.path, value);
  }

  runDockerTest(): void {
    const values = this.formValues();
    const target = values.gitlabProject.trim() || "shipshape-preview";
    this.dockerStatus.set("running");
    const logs = [
      `$ docker build -t shipshape/${target} .`,
      "Step 1/6 : FROM node:20-alpine",
      "Step 2/6 : WORKDIR /workspace",
      "Step 3/6 : COPY . .",
      "Step 4/6 : RUN npm ci",
    ];

    if (this.toolsStatus() !== "complete") {
      this.dockerLogs.set([
        ...logs,
        "Docker test failed: run tools before testing.",
      ]);
      this.dockerStatus.set("failed");
      return;
    }

    if (!this.hasChanges()) {
      this.dockerLogs.set([
        ...logs,
        "Docker test failed: no changes staged for verification.",
      ]);
      this.dockerStatus.set("failed");
      return;
    }

    this.dockerLogs.set([
      ...logs,
      "Step 5/6 : RUN npm test",
      "Step 6/6 : exporting to image",
      "Docker test succeeded: all suites green.",
    ]);
    this.dockerStatus.set("success");
  }

  updateOverride(event: Event): void {
    const target = event.target as HTMLInputElement | null;
    if (!target) {
      return;
    }
    this.allowPublishOverride.set(target.checked);
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

  private seedPersisted(file: DiffFile): void {
    if (!this.lastPersisted.has(file.path)) {
      this.lastPersisted.set(file.path, file.modified);
    }
  }

  private persistDraft(path: string, modified: string): void {
    if (this.lastPersisted.get(path) === modified) {
      return;
    }
    this.api
      .updateDiff({ path, modified })
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe((response) => {
        this.lastPersisted.set(path, response.file.modified);
      });
  }

  private buildFileTree(files: DiffFile[]): FileTreeItem[] {
    const root: FileTreeNode = { name: "", path: "", children: new Map() };

    for (const file of files) {
      const segments = file.path.split("/").filter(Boolean);
      let current = root;
      let currentPath = "";
      segments.forEach((segment, index) => {
        currentPath = currentPath ? `${currentPath}/${segment}` : segment;
        if (!current.children.has(segment)) {
          current.children.set(segment, {
            name: segment,
            path: currentPath,
            children: new Map(),
          });
        }
        const next = current.children.get(segment)!;
        if (index === segments.length - 1) {
          next.file = file;
        }
        current = next;
      });
    }

    const output: FileTreeItem[] = [];

    const walk = (node: FileTreeNode, depth: number) => {
      const sorted = Array.from(node.children.values()).sort((a, b) =>
        this.compareTreeNodes(a, b),
      );

      for (const child of sorted) {
        const isFile = Boolean(child.file) && child.children.size === 0;
        output.push({
          id: child.path,
          label: child.name,
          path: child.path,
          depth,
          kind: isFile ? "file" : "dir",
          file: child.file,
        });
        if (!isFile) {
          walk(child, depth + 1);
        }
      }
    };

    walk(root, 0);
    return output;
  }

  private compareTreeNodes(a: FileTreeNode, b: FileTreeNode): number {
    const aIsFile = Boolean(a.file) && a.children.size === 0;
    const bIsFile = Boolean(b.file) && b.children.size === 0;
    if (aIsFile !== bIsFile) {
      return aIsFile ? 1 : -1;
    }
    return a.name.localeCompare(b.name);
  }
}
