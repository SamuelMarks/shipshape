import { expect, test, Page } from "@playwright/test";

type DiffPayload = {
  files: Array<{
    path: string;
    summary: string;
    language: string;
    original: string;
    modified: string;
    tone: "good" | "warn" | "bad" | "info";
    statusLabel: string;
  }>;
};

const authUser = {
  id: "u-42",
  login: "pilot",
  githubId: "42",
};

const diffsWithChanges: DiffPayload = {
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

const diffsEmpty: DiffPayload = { files: [] };

const stubAuth = async (page: Page) => {
  await page.addInitScript(() => {
    window.localStorage.setItem("shipshape.token", "e2e-token");
  });

  await page.route("**/auth/me", (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(authUser),
    }),
  );
};

const stubDiffs = async (page: Page, payload: DiffPayload) => {
  await page.route("**/diffs", async (route) => {
    const request = route.request();
    if (request.method() === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(payload),
      });
      return;
    }

    if (request.method() === "POST") {
      const data = request.postDataJSON() as { path: string; modified: string };
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          file: {
            path: data.path,
            summary: "Updated",
            language: "rust",
            original: "fn main() {}",
            modified: data.modified,
            tone: "info",
            statusLabel: "Modified",
          },
        }),
      });
      return;
    }

    await route.continue();
  });
};

test("runs the workflow with edits, diff review, and Docker-gated publish actions", async ({
  page,
}) => {
  await stubAuth(page);
  await stubDiffs(page, diffsWithChanges);

  await page.goto("/workflow");

  await page
    .getByLabel("New project URL")
    .fill("https://github.com/acme/rocket");
  await page.getByLabel("Fork destination").selectOption("org");
  await page.getByLabel("Fork location").fill("acme");
  await page.getByLabel("GitLab namespace").fill("acme-private");
  await page.getByLabel("GitLab mirror project").fill("rocket-mirror");

  await page.getByRole("button", { name: "Clone" }).click();
  const cloneHeader = page
    .getByRole("heading", { name: "Step 3: Clone" })
    .locator("..");
  await expect(
    cloneHeader.locator("shipshape-status-pill", { hasText: "Cloned" }),
  ).toBeVisible();

  await page.getByRole("button", { name: "Run tools" }).click();
  await expect(
    page.getByText("Tools complete. 2 files changed."),
  ).toBeVisible();

  await expect(page.getByRole("heading", { name: "File tree" })).toBeVisible();
  await expect(page.getByRole("button", { name: /main\.rs/ })).toBeVisible();

  await expect(page.getByRole("tab", { name: "Edit" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Diff" })).toBeVisible();

  await page.getByRole("tab", { name: "Diff" }).click();
  await expect(page.locator("shipshape-code-merge")).toBeVisible();

  await page.getByRole("tab", { name: "Edit" }).click();
  await expect(page.locator("shipshape-code-editor")).toBeVisible();

  const editorInput = page.locator("shipshape-code-editor .cm-content").first();
  await expect(editorInput).toBeVisible();
  await editorInput.click();
  const updateRequest = page.waitForRequest((request) => {
    if (!request.url().includes("/diffs") || request.method() !== "POST") {
      return false;
    }
    const payload = request.postDataJSON() as { modified?: string } | null;
    return payload?.modified?.includes("edited") ?? false;
  });
  await page.keyboard.press("ControlOrMeta+A");
  await page.keyboard.type('fn main() { println!("edited"); }');

  const request = await updateRequest;
  const payload = request.postDataJSON() as { modified: string };
  expect(payload.modified).toContain("edited");

  await page.getByRole("button", { name: "Test with Docker" }).click();
  await expect(
    page.getByText("Docker test succeeded: all suites green."),
  ).toBeVisible();

  await expect(
    page.getByRole("button", { name: "Fork to GitHub org: acme" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", {
      name: "Mirror to private GitLab: acme-private/rocket-mirror",
    }),
  ).toBeVisible();
});

test("shows nothing to do when tools report no changes", async ({ page }) => {
  await stubAuth(page);
  await stubDiffs(page, diffsEmpty);

  await page.goto("/workflow");

  await page.getByRole("button", { name: "Run tools" }).click();
  await expect(page.getByText("Tools complete. Nothing to do.")).toBeVisible();
  await expect(
    page.locator(".change-summary", { hasText: "Nothing to do." }),
  ).toBeVisible();
  await expect(page.getByText("No changes to review.")).toBeVisible();
});

test("allows publish override even if Docker tests fail", async ({ page }) => {
  await stubAuth(page);
  await stubDiffs(page, diffsEmpty);

  await page.goto("/workflow");

  await page.getByRole("button", { name: "Run tools" }).click();
  await page.getByRole("button", { name: "Test with Docker" }).click();
  await expect(
    page.getByText("Docker test failed: no changes staged for verification."),
  ).toBeVisible();

  await expect(
    page.getByRole("button", { name: /Fork to GitHub/ }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: /Mirror to private GitLab/ }),
  ).toHaveCount(0);

  await page
    .getByLabel("Step 10: Allow fork + mirror even if tests fail")
    .check();

  await expect(
    page.getByRole("button", { name: /Fork to GitHub/ }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /Mirror to private GitLab/ }),
  ).toBeVisible();
});
