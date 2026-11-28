const { randomUUID } = require("node:crypto");

const password = "browser-fixture-password";

function fixtureAccounts(workspaceId, keys) {
  return keys.map((key) => ({ username: `fixture-${key}`, workspaceId, key }));
}

// Business UI fixtures supply synthetic roles independently of production login.
// Real workspace and administrator authentication are covered by the account suites.
function createFixtureUiServer(createUiServer, { apiBaseUrl, accounts }) {
  const server = createUiServer({ apiBaseUrl });
  const [route] = server.listeners("request");
  const sessions = new Map();
  const sessionPath = "/api/console/session";
  function reply(res, status, body) {
    res.writeHead(status, { "Content-Type": "application/json", "Cache-Control": "no-store" });
    res.end(JSON.stringify(body));
  }
  server.removeListener("request", route);
  server.on("request", async (req, res) => {
    const id = req.headers.cookie?.match(/(?:^|; )fixture-session=([^;]+)/)?.[1];
    const current = sessions.get(id);
    if (req.url !== sessionPath) {
      if (current && req.url?.startsWith("/api/") &&
          req.headers["x-origence-session"] === current.profile.session.binding &&
          req.headers["x-origence-workspace"] === current.account.workspaceId)
        req.headers.authorization = `Bearer ${current.account.key}`;
      return route(req, res);
    }
    const signedOut = { mode: "account", session: null };
    if (req.method === "GET") return reply(res, 200, current?.profile ?? signedOut);
    if (req.method === "DELETE") {
      sessions.delete(id);
      return reply(res, 200, signedOut);
    }
    if (req.method !== "POST") return reply(res, 405, {});
    try {
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const credentials = JSON.parse(Buffer.concat(chunks));
      const account = accounts.find(({ username }) => username === credentials.username);
      if (!account || credentials.password !== password) return reply(res, 401, {});
      const upstream = await fetch(new URL("/v1/whoami", apiBaseUrl), {
        headers: { Authorization: `Bearer ${account.key}` },
      });
      if (!upstream.ok) return reply(res, upstream.status, {});
      const user = await upstream.json();
      const profile = { mode: "workspace", session: {
        account: account.username,
        binding: randomUUID(),
        workspaces: [{ ...user, name: account.workspaceId }],
      } };
      const next = randomUUID();
      sessions.delete(id);
      sessions.set(next, { profile, account });
      res.setHeader("Set-Cookie", `fixture-session=${next}; HttpOnly; SameSite=Strict; Path=/api`);
      reply(res, 200, profile);
    } catch {
      reply(res, 502, {});
    }
  });
  return server;
}

async function fixtureSignIn(page, key, interaction = "keyboard") {
  await page.getByLabel("账号", { exact: true }).fill(`fixture-${key}`);
  await page.getByLabel("密码", { exact: true }).fill(password);
  const login = page.getByRole("button", { name: "登录", exact: true });
  if (interaction === "touch") await login.tap();
  else if (interaction === "mouse") await login.click();
  else await login.press("Enter");
}

module.exports = { createFixtureUiServer, fixtureAccounts, fixtureSignIn, password };
