import { hashConsolePassword } from "../dist/console-accounts.js";

try {
  const chunks = [];
  let bytes = 0;
  for await (const chunk of process.stdin) {
    bytes += chunk.length;
    if (bytes > 1026) throw new Error("Password input is too long.");
    chunks.push(chunk);
  }
  const password = Buffer.concat(chunks).toString("utf8").replace(/\r?\n$/, "");
  console.log(await hashConsolePassword(password));
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
