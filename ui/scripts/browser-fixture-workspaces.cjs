const { execFileSync } = require("node:child_process");
const { join } = require("node:path");

function seedWorkspace(directory, name, sameTenantAs) {
  const python = process.env.PYTHON ?? (process.platform === "win32" ? "python" : "python3");
  return JSON.parse(execFileSync(python, [join(__dirname, "../../tools/native_fixture.py")], {
    input: JSON.stringify({ fixture: "workspace", name, same_tenant_as: sameTenantAs }),
    env: { ...process.env, OC_DATA_DIR: directory },
    encoding: "utf8",
  }));
}

module.exports = { seedWorkspace };
