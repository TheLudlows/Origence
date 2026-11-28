const { readFileSync } = require("node:fs");
const { chromium } = require("playwright");

async function main() {
  const { ui, token, run } = JSON.parse(readFileSync(0, "utf8"));
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM });
  try {
    const page = await browser.newPage();
    page.setDefaultTimeout(10_000);
    await page.goto(ui + "/#runs/" + run);
    const key = page.getByLabel("访问 Key", { exact: true });
    const keyLogin = page.getByRole("button", { name: "用 Key 登录", exact: true });
    await key.or(keyLogin).waitFor();
    if (await keyLogin.isVisible()) await keyLogin.click();
    await key.fill(token);
    await page.getByRole("button", { name: "使用 Key 登录", exact: true }).click();
    await page.getByText("Claude Code prompt (hook metadata only)", { exact: true }).waitFor();
    await page.getByRole("cell", { name: "claude_code_hook", exact: true }).waitFor();
    console.log("PASS Chromium login to the installed console and Hook Run detail");
  } finally {
    await browser.close();
  }
}

main().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
