# meet-transcriber-releases

회의록 데스크톱 앱이 쓰는 로컬 전사 프로그램(`meet-transcriber`)의 소스, 빌드, 릴리스를 한 곳에 둔 저장소다. 안은 whisper.cpp를 감싼 작은 프로그램이다.

## 받기

- 앱이 설치할 때 이 저장소의 릴리스(`sidecar-v<버전>`)에서 파일을 받고, 해시가 맞는지 확인한 뒤 쓴다. 직접 받을 필요는 없다.
- 릴리스 파일: `meet-transcriber-darwin-arm64`(Mac, Apple Silicon, Metal), `meet-transcriber-windows-x64.exe`(Windows, x64, Vulkan). 파일마다 `.sha256`이 곁들여 있다.
- 해시 확인: Mac은 받은 폴더에서 `shasum -a 256 -c meet-transcriber-darwin-arm64.sha256`, Windows는 `certutil -hashfile meet-transcriber-windows-x64.exe SHA256`의 값을 `.sha256` 파일의 값과 비교한다.
- 전사 모델은 이 저장소에 두지 않는다(Hugging Face의 공개 모델을 앱이 받는다).

## 소스와 빌드

- 소스: `src/`(Rust). 표준 입출력의 줄 JSON으로 소리 조각을 받아 글을 돌려준다.
- 직접 빌드: Rust와 cmake가 있는 컴퓨터에서 Mac은 `cargo build --release --features metal`, Windows는 `cargo build --release --features vulkan`(Vulkan SDK, clang-cl, Ninja 필요).
- 릴리스 빌드는 Actions의 `build` 워크플로를 사람이 버전을 넣어 직접 돌릴 때만 한다. Mac과 Windows를 함께 빌드하고, 결과 파일 안에 만든 사람이나 컴퓨터의 이름·경로가 남지 않았는지 `tools/check-paths.mjs`로 검사한 뒤, 둘 다 통과해야 릴리스에 올린다.
- 경로 검사 시험: `node --test tools/*.test.mjs`
