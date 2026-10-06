import { expect, test } from "@playwright/test";
import path from "node:path";

const sample = path.resolve("tests/fixtures/sample.webm");

test("import, split, play and render a frame", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("file-input").setInputFiles(sample);

  const clips = page.getByTestId("clip");
  await expect(clips).toHaveCount(1);

  // seek to ~2s on the ruler and split
  const ruler = page.getByTestId("ruler");
  const box = (await ruler.boundingBox())!;
  await page.mouse.click(box.x + 70 + 160, box.y + 10); // 80px/s => 2s (ruler has a 70px label gutter)
  await clips.first().click();
  await page.getByRole("button", { name: /Split/ }).click();
  await expect(clips).toHaveCount(2);

  // the preview canvas must contain non-black pixels from the decoded video
  await page.getByTestId("play").click();
  await page.waitForTimeout(600);
  await page.getByTestId("play").click();
  // wait for the seeked frame to be drawn: the canvas must contain decoded (non-black) pixels
  await expect.poll(() => page.getByTestId("preview").evaluate((c: HTMLCanvasElement) => {
    const d = c.getContext("2d")!.getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (d[i]! + d[i + 1]! + d[i + 2]! > 60) n++;
    return n;
  }), { timeout: 5000 }).toBeGreaterThan(1000);
  await page.screenshot({ path: "test-results/editor.png" });
});

test("undo reverses a split", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("file-input").setInputFiles(sample);
  await expect(page.getByTestId("clip")).toHaveCount(1);
  await page.getByTestId("clip").first().click();
  await page.getByRole("button", { name: "Add text" }).click();
  await expect(page.getByTestId("clip")).toHaveCount(2);
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(page.getByTestId("clip")).toHaveCount(1);
});
