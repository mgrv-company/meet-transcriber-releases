// 사용: node tools/write-sha256.mjs <파일>  → <파일>.sha256 에 "해시  파일이름" 한 줄을 쓴다(shasum -a 256 -c 로 확인되는 모양).
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { basename } from "node:path";

const file = process.argv[2];
if (file === undefined) {
  process.stderr.write("사용: node tools/write-sha256.mjs <파일>\n");
  process.exit(2);
}
const hash = createHash("sha256").update(readFileSync(file)).digest("hex");
writeFileSync(`${file}.sha256`, `${hash}  ${basename(file)}\n`);
process.stdout.write(`${hash}  ${basename(file)}\n`);
