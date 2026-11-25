const password = "browser-fixture-password";
let passwordHash;

async function fixtureAccounts(workspaceId, keys) {
  passwordHash ??= import("../dist/console-accounts.js").then((accounts) =>
    accounts.hashConsolePassword(password));
  const hash = await passwordHash;
  return keys.map((key) => ({
    username: `fixture-${key}`,
    passwordHash: hash,
    grants: [{ workspaceId, name: workspaceId, authorization: `Bearer ${key}` }],
  }));
}

async function fixtureSignIn(page, key, interaction = "keyboard") {
  await page.getByLabel("账号", { exact: true }).fill(`fixture-${key}`);
  await page.getByLabel("密码", { exact: true }).fill(password);
  const login = page.getByRole("button", { name: "登录", exact: true });
  if (interaction === "touch") await login.tap();
  else if (interaction === "mouse") await login.click();
  else await login.press("Enter");
}

module.exports = { fixtureAccounts, fixtureSignIn, password };
