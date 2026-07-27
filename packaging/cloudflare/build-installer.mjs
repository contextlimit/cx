import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const moduleDirectory = path.dirname(fileURLToPath(import.meta.url));
const sourcePath = path.resolve(moduleDirectory, "../../install.sh");
const outputPath = process.argv[2]
  ? path.resolve(process.argv[2])
  : path.join(moduleDirectory, "generated/installer.mjs");

const source = await fs.readFile(sourcePath, "utf8");
const encoded = Buffer.from(source, "utf8").toString("base64");
await fs.mkdir(path.dirname(outputPath), { recursive: true });
await fs.writeFile(outputPath, `export default ${JSON.stringify(encoded)};\n`);
console.log(`generated ${outputPath}`);
