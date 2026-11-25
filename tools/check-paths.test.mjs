// 불변식 시험: 「누출 문자열이 어떤 모양·인코딩·위치·잡음 속에 있어도 찾아내고, 없으면 한 건도 걸리지 않는다.」
// 사례 몇 개가 아니라 축의 곱집합으로 위반이 0건인지 본다. 실행: node --test tools/
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { findLeaks } from "./check-paths.mjs";

// 같은 입력이면 같은 잡음이 나오는 난수(재현할 수 있게).
const rng = (seed) => () => {
  seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
  return seed / 2 ** 32;
};

const encode = (text, wide) => (wide ? Buffer.from(text, "utf16le") : Buffer.from(text, "latin1"));
const noise = (next, length, alphabet) =>
  Buffer.from(Array.from({ length }, () => alphabet[Math.floor(next() * alphabet.length)]));

// 잡음 알파벳: 전체 바이트 / 글자·널·구분자처럼 누출 문자열과 닮은 바이트만(경계에서 잘못 이어 붙는지 보려고)
const alphabets = {
  anyByte: Array.from({ length: 256 }, (_, i) => i),
  lookalike: [...Buffer.from("usersUSERS/\\.-_ap\0")],
};

// 누출 문자열: [문자열, 추가 --term]. 모양 변형(대소문자, 구분자, 이중 역슬래시, 앞뒤 경로)을 모두 곱한다.
const leakShapes = (() => {
  const base = ["/Users/alice/.cargo", "\\Users\\bob\\.cargo", "C:\\Users\\runneradmin\\x"];
  const out = [];
  for (const text of base)
    for (const variant of [(t) => t, (t) => t.toUpperCase(), (t) => t.toLowerCase(), (t) => t.replaceAll("\\", "\\\\"), (t) => `src/${t}/lib.rs`])
      out.push([variant(text), []]);
  out.push(["D:\\agent\\build\\t", ["agent"]], ["hello runneradmin world", ["runneradmin"]], ["RUNNER-x", ["runner-x"]]);
  return out;
})();

// 누출이 아닌 것: \Users\Public\(어느 대소문자·인코딩이든)와 닮은 글자들.
const cleanShapes = ["C:\\Users\\Public\\Documents", "c:\\users\\public\\x", "/usr/lib/users", "Users", "user\\s", "\\Users", "/Users"];

test("누출 문자열은 모양·인코딩·위치·잡음 어디에 있어도 찾는다", () => {
  const next = rng(1);
  let count = 0;
  const failures = [];
  for (const [leak, terms] of leakShapes)
    for (const wide of [false, true])
      for (const alphabetName of Object.keys(alphabets))
        for (const place of ["start", "middle", "end", "only"])
          for (let round = 0; round < 12; round += 1) {
            const alphabet = alphabets[alphabetName];
            const left = place === "start" || place === "only" ? Buffer.alloc(0) : noise(next, 1 + Math.floor(next() * 300), alphabet);
            const right = place === "end" || place === "only" ? Buffer.alloc(0) : noise(next, 1 + Math.floor(next() * 300), alphabet);
            const bytes = Buffer.concat([left, encode(leak, wide), right]);
            count += 1;
            if (findLeaks(bytes, terms).length === 0) failures.push(`${JSON.stringify(leak)} ${wide ? "wide" : "ascii"} ${alphabetName} ${place}`);
          }
  assert.deepEqual(failures.slice(0, 5), [], `위반 ${failures.length}건 / ${count}건`);
  assert.ok(count > 3000, `시험 수 ${count}`);
});

test("누출이 없는 바이트는 한 건도 걸리지 않는다(Public, 닮은 글자, 잡음)", () => {
  const next = rng(2);
  let count = 0;
  const failures = [];
  for (const clean of cleanShapes)
    for (const wide of [false, true])
      for (let round = 0; round < 30; round += 1) {
        // 닮은 글자 잡음은 누출 문자열을 우연히 만들 수 없는 알파벳(u,s,e,r,대소문자,/,\,.,-,_,a,p,널)이라도 "users" 줄은 조립될 수 있어 길이를 짧게 둔다.
        const left = noise(next, Math.floor(next() * 3), [0x41, 0x42, 0x00, 0xff]);
        const right = noise(next, Math.floor(next() * 3), [0x41, 0x42, 0x00, 0xff]);
        const bytes = Buffer.concat([left, encode(clean, wide), right]);
        count += 1;
        const found = findLeaks(bytes);
        // "/Users" 처럼 뒤에 구분자가 없는 것과 "\Users"(뒤 구분자 없음)는 누출이 아니다.
        if (found.length > 0) failures.push(`${JSON.stringify(clean)} ${wide ? "wide" : "ascii"}`);
      }
  assert.deepEqual(failures.slice(0, 5), [], `오탐 ${failures.length}건 / ${count}건`);
  assert.ok(count > 300);
});

test("--term이 짧으면 검사를 시작하지 않는다(아무 데나 걸리는 것을 막는다)", () => {
  assert.throws(() => findLeaks(Buffer.from("x"), ["ab"]));
  assert.throws(() => findLeaks(Buffer.from("x"), [""]));
});

test("실행 파일 모양 끝점: 비었거나 없는 파일은 통과하지 않는다", () => {
  const dir = mkdtempSync(join(tmpdir(), "paths-"));
  const script = new URL("./check-paths.mjs", import.meta.url).pathname;
  const run = (...args) => spawnSync("node", [script, ...args], { encoding: "utf8" });
  const clean = join(dir, "clean");
  const leaky = join(dir, "leaky");
  const empty = join(dir, "empty");
  writeFileSync(clean, Buffer.from([1, 2, 3, 255]));
  writeFileSync(leaky, Buffer.concat([Buffer.from([1, 2]), encode("C:\\Users\\bob\\x", true)]));
  writeFileSync(empty, Buffer.alloc(0));
  assert.equal(run(clean).status, 0);
  assert.equal(run(leaky).status, 1);
  assert.equal(run(empty).status, 1);
  assert.notEqual(run(join(dir, "nope")).status, 0);
  assert.equal(run(clean, "--term", "bob").status, 0);
  assert.equal(run(leaky, "--term", "bob").status, 1);
  assert.equal(run().status, 2);
  assert.ok(!run(leaky).stderr.includes("bob"), "보고에 주변 글자를 쓰지 않는다");
  assert.ok(execFileSync("node", ["--version"]).length > 0);
});
