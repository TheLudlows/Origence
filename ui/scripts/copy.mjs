import { cp, mkdir, readdir, rm } from "node:fs/promises";
await rm("dist/public/client", { recursive: true, force: true });
await mkdir("dist/public", { recursive: true });
await cp("public", "dist/public", { recursive: true });
await mkdir("dist/public/client", { recursive: true });
for (const source of await readdir("src/client")) {
  if (!source.endsWith(".ts") || source.endsWith(".d.ts")) continue;
  const output = source.replace(/\.ts$/, ".js");
  await cp(`dist/client/${output}`, `dist/public/client/${output}`);
}
