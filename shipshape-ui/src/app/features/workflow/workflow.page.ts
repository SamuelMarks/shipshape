/**
 * @file End-to-end Workflow Studio page component for cloning, running tools,
 * editing diffs, verifying Docker builds, and publishing PR/mirror branches.
 */

import {
  Component,
  DestroyRef,
  computed,
  inject,
  signal,
} from "@angular/core";
import { NonNullableFormBuilder, ReactiveFormsModule } from "@angular/forms";
import { takeUntilDestroyed, toSignal } from "@angular/core/rxjs-interop";
import { map, startWith } from "rxjs/operators";

import { ApiService } from "../../services/api.service";
import { DiffFile } from "../../services/api.models";
import { DiffDraftController } from "../../services/diff-draft.controller";
import { CodeEditorComponent } from "../../ui/code-editor.component";
import { CodeMergeComponent } from "../../ui/code-merge.component";
import {
  StatusPillComponent,
  StatusTone,
} from "../../ui/status-pill.component";

/** Docker verification status states. */
type DockerStatus = "idle" | "running" | "success" | "failed";
/** Active workspace editor tab identifier. */
type TabKey = "edit" | "diff";

/** Selectable workflow tool descriptor. */
interface ToolOption {
  /** Unique tool identifier. */
  id: string;
  /** Display label. */
  label: string;
  /** Brief explanation of the tool's purpose. */
  description: string;
}

/** Flattened file-tree entry rendered in the workspace sidebar. */
interface FileTreeItem {
  /** Unique path identifier. */
  id: string;
  /** Segment display label. */
  label: string;
  /** Full relative path. */
  path: string;
  /** Nesting depth (`0`-indexed). */
  depth: number;
  /** Entry kind (`"dir"` or `"file"`). */
  kind: "dir" | "file";
  /** Associated diff file when `kind === "file"`. */
  file?: DiffFile;
}

/** Intermediate trie node used when building the hierarchical file tree. */
interface FileTreeNode {
  /** Segment name. */
  name: string;
  /** Accumulated relative path. */
  path: string;
  /** Child nodes keyed by segment name. */
  children: Map<string, FileTreeNode>;
  /** Associated diff file if this node represents a file. */
  file?: DiffFile;
}

/**
 * Workflow Studio page component orchestrating repository intake, tool runs,
 * interactive diff editing, Docker verification, and GitHub/GitLab publishing.
 */
@Component({
  selector: "shipshape-workflow-page",
  templateUrl: "./workflow.page.html",
  styleUrl: "./workflow.page.css",
  imports: [
    ReactiveFormsModule,
    CodeEditorComponent,
    CodeMergeComponent,
    StatusPillComponent,
  ],
})
export class WorkflowPage {
  /** Non-nullable reactive form builder. */
  private readonly fb = inject(NonNullableFormBuilder);
  /** Backend API service client. */
  private readonly api = inject(ApiService);
  /** Component destroy reference used to clean up subscriptions. */
  private readonly destroyRef = inject(DestroyRef);

  /** Available workflow tool options. */
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

  /** Reactive form controlling repository, fork, and GitLab mirror settings. */
  readonly workflowForm = this.fb.group({
    repoUrl: "https://github.com/shipshape/fleet-core",
    forkTargetType: "org",
    forkTarget: "shipshape",
    gitlabNamespace: "shipshape-private",
    gitlabProject: "fleet-core-mirror",
    clonePath: "/workspaces/shipshape/fleet-core",
  });

  /** Signal tracking current raw values of `workflowForm`. */
  private readonly formValues = toSignal(
    this.workflowForm.valueChanges.pipe(
      startWith(this.workflowForm.getRawValue()),
      map(() => this.workflowForm.getRawValue()),
    ),
    { initialValue: this.workflowForm.getRawValue() },
  );

  /** Currently selected tool IDs. */
  readonly selectedTools = signal<string[]>(["audit", "refit", "drydock"]);
  /** Current repository clone status. */
  readonly cloneStatus = signal<"idle" | "running" | "complete">("idle");
  /** Current tool execution status. */
  readonly toolsStatus = signal<"idle" | "running" | "complete">("idle");
  /** Current Docker verification status. */
  readonly dockerStatus = signal<DockerStatus>("idle");
  /** Manual override allowing publishing even when Docker verification fails. */
  readonly allowPublishOverride = signal(false);
  /** Active editor tab (`"edit"` or `"diff"`). */
  readonly activeTab = signal<TabKey>("edit");

  /** Diff files produced by the most recent tool run. */
  readonly files = signal<DiffFile[]>([]);
  /** Shared diff draft and persistence controller. */
  private readonly drafts = new DiffDraftController(this.files);
  /** Currently selected diff file. */
  readonly selectedFile = this.drafts.selectedFile;
  /** Currently active modified code for the selected file. */
  readonly selectedModified = this.drafts.selectedModified;
  /** Read-only original code model for the selected file. */
  readonly originalModel = this.drafts.originalModel;
  /** Editable modified code model for the selected file. */
  readonly modifiedModel = this.drafts.modifiedModel;

  /** Console output lines from the Docker verification step. */
  readonly dockerLogs = signal<string[]>([]);
  /** Human-readable summary of the latest tool run. */
  readonly toolRunSummary = signal<string>("Tools are idle.");

  /** Whether a non-empty repository URL has been entered. */
  readonly projectReady = computed(
    () => this.formValues().repoUrl.trim().length > 0,
  );
  /** Whether a non-empty GitHub fork target has been entered. */
  readonly forkReady = computed(
    () => this.formValues().forkTarget.trim().length > 0,
  );
  /** Whether both GitLab namespace and project have been entered. */
  readonly mirrorReady = computed(() => {
    const values = this.formValues();
    return (
      values.gitlabNamespace.trim().length > 0 &&
      values.gitlabProject.trim().length > 0
    );
  });

  /** Whether all prerequisites for cloning are satisfied. */
  readonly canClone = computed(
    () => this.projectReady() && this.forkReady() && this.mirrorReady(),
  );
  /** Whether at least one changed file is staged in the workspace. */
  readonly hasChanges = computed(() => this.files().length > 0);
  /** Whether tools finished with zero changed files. */
  readonly showNothingToDo = computed(
    () => this.toolsStatus() === "complete" && !this.hasChanges(),
  );

  /** Whether the publish actions are unlocked. */
  readonly canPublish = computed(
    () => this.dockerStatus() === "success" || this.allowPublishOverride(),
  );

  /** Button label for the GitHub fork action. */
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

  /** Button label for the GitLab mirror action. */
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

  /** Human-readable status pill label for the clone step. */
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

  /** Visual tone for the clone status pill. */
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

  /** Human-readable status pill label for the tools step. */
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

  /** Visual tone for the tools status pill. */
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

  /** Human-readable status pill label for the Docker test step. */
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

  /** Visual tone for the Docker test status pill. */
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

  /** Flattened file tree computed from `files`. */
  readonly fileTree = computed<FileTreeItem[]>(() =>
    this.buildFileTree(this.files()),
  );

  /** Status message describing the clone workspace path and state. */
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

  /**
   * Toggle selection of `option` in `selectedTools`.
   *
   * @param option - Tool option to toggle.
   */
  toggleTool(option: ToolOption): void {
    this.selectedTools.update((current) => {
      if (current.includes(option.id)) {
        return current.filter((item) => item !== option.id);
      }
      return [...current, option.id];
    });
  }

  /**
   * Mark the repository workspace as cloned when all clone prerequisites are met.
   */
  cloneRepo(): void {
    if (!this.canClone()) {
      return;
    }
    this.cloneStatus.set("complete");
  }

  /**
   * Execute the selected workflow tools and load resulting diff files.
   */
  runTools(): void {
    this.toolsStatus.set("running");
    this.drafts.reset();
    this.files.set([]);
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

  /**
   * Select `file` in the editor after stashing any pending draft.
   *
   * @param file - Diff file to select.
   */
  selectFile(file: DiffFile): void {
    this.drafts.selectFile(file);
  }

  /**
   * Update the modified content for the active diff file and persist changes.
   *
   * @param value - Updated source text.
   */
  updateModified(value: string): void {
    this.drafts.updateModified(value);
  }

  /**
   * Run the simulated Docker build and test suite against the staged changes.
   */
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

  /**
   * Update the manual publish override signal from a checkbox change `event`.
   *
   * @param event - DOM checkbox change event.
   */
  updateOverride(event: Event): void {
    const target = event.target as HTMLInputElement | null;
    if (!target) {
      return;
    }
    this.allowPublishOverride.set(target.checked);
  }

  /**
   * Build a flattened, depth-annotated file tree from `files`.
   *
   * @param files - Diff files to organize into a directory/file tree.
   * @returns Flattened list of directory and file tree items.
   */
  private buildFileTree(files: DiffFile[]): FileTreeItem[] {
    const root: FileTreeNode = { name: "", path: "", children: new Map() };

    for (const file of files) {
      const segments = file.path.split("/").filter(Boolean);
      let current = root;
      let currentPath = "";
      segments.forEach((segment, index) => {
        currentPath = currentPath ? `${currentPath}/${segment}` : segment;
        let next = current.children.get(segment);
        if (!next) {
          next = {
            name: segment,
            path: currentPath,
            children: new Map(),
          };
          current.children.set(segment, next);
        }
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

  /**
   * Sort directories before files, then alphabetically by name.
   *
   * @param a - First tree node.
   * @param b - Second tree node.
   * @returns Negative if `a < b`, positive if `a > b`, or `0`.
   */
  private compareTreeNodes(a: FileTreeNode, b: FileTreeNode): number {
    const aIsFile = Boolean(a.file) && a.children.size === 0;
    const bIsFile = Boolean(b.file) && b.children.size === 0;
    if (aIsFile !== bIsFile) {
      return aIsFile ? 1 : -1;
    }
    return a.name.localeCompare(b.name);
  }
}
