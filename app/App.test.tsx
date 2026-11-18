// SPDX-License-Identifier: AGPL-3.0-or-later
// Headless tests of the workspace (`nsh test`): the app runs on the real engine without a window.
import { expect, open, test } from "nsh/test";

test("the workspace shows its views, sources and link bar", async () => {
  const app = await open("App.tsx", { width: 1280, height: 800 });
  const texts = app.texts();
  for (const t of ["All Downloads", "Downloading", "Completed", "Failed", "X", "Threads", "Instagram", "Files", "New Download"]) {
    expect(texts).toContain(t);
  }
});

test("switching views changes the title", async () => {
  const app = await open("App.tsx", { width: 1280, height: 800 });
  await app.press("Failed");
  expect(app.texts()).toContain("Failed");
  await app.press("Completed");
  expect(app.texts()).toContain("Completed");
});

test("settings open in a dialog and close with Done", async () => {
  const app = await open("App.tsx", { width: 1280, height: 800 });
  await app.press("role:button[Settings]");
  await app.waitFor("Download Folder");
  await app.press("Done");
  await app.waitUntil(() => !app.texts().includes("Download Folder"));
});
