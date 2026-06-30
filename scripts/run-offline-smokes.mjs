import { readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const scripts = readdirSync(new URL(".", import.meta.url))
  .filter((name) => name.startsWith("smoke-") && name.endsWith(".mjs") && !name.includes("live-"))
  .sort();

const failures = [];
for (const script of scripts) {
  const result = spawnSync(process.execPath, [fileURLToPath(new URL(script, import.meta.url))], {
    cwd: fileURLToPath(new URL("..", import.meta.url)),
    encoding: "utf8"
  });
  if (result.status === 0) {
    process.stdout.write(`PASS ${script}\n`);
  } else {
    failures.push(script);
    process.stderr.write(result.stdout);
    process.stderr.write(result.stderr);
    process.stderr.write(`FAIL ${script}\n`);
  }
}

if (failures.length > 0) {
  throw new Error(`${failures.length} offline smoke contract(s) failed: ${failures.join(", ")}`);
}

console.log(`All ${scripts.length} offline smoke contracts passed.`);
