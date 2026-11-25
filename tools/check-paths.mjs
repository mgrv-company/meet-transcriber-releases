// 불변식: 결과 파일 안에 만든 사람이나 만든 컴퓨터의 이름·경로가 한 건도 없다.
// 사용: node tools/check-paths.mjs <파일> [--term 값]...   (한 건이라도 있으면 종료 코드 1)
// 바이트를 ASCII와 UTF-16LE(Windows 문자열) 두 모양으로, 대소문자 구분 없이 찾는다.
// 보고에는 규칙 이름과 위치만 쓴다. 주변 글자는 공개 로그에 다시 남기지 않으려고 쓰지 않는다.
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const escape = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
// 글자 사이에 널 바이트를 끼운 모양(UTF-16LE)이면 wide로 만든다. latin1로 읽으므로 바이트 하나가 글자 하나다.
// 본문을 소문자로 바꿔 비교하므로 찾는 글도 소문자로 바꾼다.
const seq = (text, wide) => [...text.toLowerCase()].map((c) => escape(c) + (wide ? "\\x00" : "")).join("");

export const fixedTerms = ["/Users/"];

// Windows의 \Users\Public\ 는 사람 이름이 아니라 모든 컴퓨터에 있는 폴더라 뺀다.
const rules = (extraTerms) => [
  ...fixedTerms.map((term) => ({ name: term, source: (wide) => seq(term, wide) })),
  {
    name: "\\Users\\(Public 제외)",
    source: (wide) => `${seq("\\Users\\", wide)}(?!${seq("Public\\", wide)})`,
  },
  ...extraTerms.map((term) => ({ name: `--term ${term.length}자`, source: (wide) => seq(term, wide) })),
];

// 돌려주는 값: 발견 목록 [{ rule, encoding, offset }]. 빈 배열이어야 통과다.
export const findLeaks = (bytes, extraTerms = []) => {
  if (extraTerms.some((term) => term.length < 3))
    throw new Error("--term 값은 세 글자 이상이어야 한다(너무 짧으면 아무 데나 걸린다)");
  const text = Buffer.from(bytes).toString("latin1").toLowerCase();
  const found = [];
  for (const rule of rules(extraTerms.map((term) => term.toLowerCase())))
    for (const wide of [false, true])
      for (const match of text.matchAll(new RegExp(rule.source(wide), "g")))
        found.push({ rule: rule.name, encoding: wide ? "utf-16le" : "ascii", offset: match.index });
  return found;
};

const main = (argv) => {
  const [file, ...rest] = argv;
  const terms = [];
  for (let i = 0; i < rest.length; i += 2) {
    if (rest[i] !== "--term" || rest[i + 1] === undefined) {
      process.stderr.write("사용: node tools/check-paths.mjs <파일> [--term 값]...\n");
      return 2;
    }
    terms.push(rest[i + 1]);
  }
  if (file === undefined) {
    process.stderr.write("검사할 파일이 없다\n");
    return 2;
  }
  const bytes = readFileSync(file);
  if (bytes.length === 0) {
    process.stderr.write("파일이 비어 있다. 검사할 수 없는 파일은 통과시키지 않는다\n");
    return 1;
  }
  const found = findLeaks(bytes, terms);
  if (found.length === 0) {
    process.stdout.write(`경로 검사 통과: ${bytes.length}바이트, 0건\n`);
    return 0;
  }
  process.stderr.write(`경로 검사 실패: ${found.length}건\n`);
  for (const item of found.slice(0, 20))
    process.stderr.write(`- 규칙 ${item.rule}, ${item.encoding}, 위치 ${item.offset}\n`);
  return 1;
};

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) process.exitCode = main(process.argv.slice(2));
