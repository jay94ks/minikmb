# 번들 글꼴·아이콘

2026-11-21 · 실행 파일에 포함하는 UI 자산 기록입니다.
전체 Rust 의존 크레이트/설치 패키지의 배포 검토를 대신하는 문서는 아닙니다.

| 자산 | 고정 원본 | 크기 | 라이선스 |
|---|---|---|---|
| Noto Sans KR Regular | Noto CJK Sans2.004, commit `523d033d6cb47f4a80c58a35753646f5c3608a78`, KR SubsetOTF | 4,644,748bytes | SIL OFL1.1 |
| Lucide 아이콘 글꼴 | 공식 `lucide-static` npm0.577.0의 `font/lucide.ttf` | 817,964bytes | ISC; Feather 유래 부분 MIT |

Noto의 [고정 원본](https://github.com/notofonts/noto-cjk/tree/Sans2.004/Sans/SubsetOTF/KR),
[라이선스](../assets/fonts/Noto-OFL.txt)와 폰트 자체의 Adobe 저작권 고지를 보존합니다.
Lucide는 [공식 글꼴 안내](https://lucide.dev/guide/static/font)를 사용하는 고정 버전이며
[ISC/MIT 전문](../assets/fonts/Lucide-LICENSE.txt)을 함께 포함합니다. 원본 글꼴을
수정하거나 이름을 바꾸지 않았습니다. font face·자산 이름은 MiniKMB 브랜드가 아닙니다.

두 글꼴·라이선스·고지는 `assets/fonts`에 보관하고 `assets.rs`가 실행 파일에
포함합니다. 설치된 맑은 고딕을 읽던 경로를 제거했습니다. 실행 시 폰트 설치·다운로드·
사용자 시스템 글꼴 복사가 필요하지 않습니다. 설정의 **글꼴·아이콘 라이선스**에서
출처·저작권·라이선스 전문을 오프라인으로 읽을 수 있습니다.

## 적용과 검증

Noto를 본문/버튼의 기본 글꼴로 사용합니다. 코드는 egui의 기본 모노스페이스를
유지하고 한글에는 Noto를 대체 글꼴로 사용합니다. Lucide의 private-use 영역은
별도 글꼴 family에 격리하므로 사용자 이름·코드·로그의 문자를 아이콘으로 해석하지
않습니다. 탐색 메뉴8개에16pt 아이콘/14pt 이름/8pt 간격을 사용합니다. 메뉴의
접근성 이름은 읽을 수 있는 레이블이며 private-use 글자를 포함시키지 않습니다.
실제 스크린 리더 수용 시험은 별도로 남아 있습니다.

`assets/manifest.json`에 고정 URL·commit/version·길이·SHA256·npm tarball의
SHA512 integrity·라이선스/고지 SHA256과 실제 사용하는 아이콘 codepoint를 기록합니다.
Lucide 전체 글꼴을 그대로 포함하며 사용 아이콘은 아래8개입니다. 전체 글꼴 유지로
추가 SVG 파서/래스터라이저나 런타임 네트워크 의존성을 넣지 않습니다.

| 화면 | Lucide 원래 이름 |
|---|---|
| 채널 | panels-top-left |
| 원격 제어 | monitor |
| 서버·API 키 | key-round |
| REST API 문서 | book-open |
| MCP 연동 안내 | plug |
| 설정 | settings |
| 진단 | activity |
| 동작 로그 | logs |

```powershell
python scripts/ui-assets.py
```

위 검사는 오프라인으로 파일/길이/라이선스/고지의 고정 해시를 확인합니다. 유지보수용
`--fetch`는 고정 공식 URL만 받아 해시를 확인하고 프로젝트의 자산 파일만 복원합니다.
압축 파일은 필요한 멤버만 메모리로 읽고 파일 시스템에 풀지 않습니다. 일반 Cargo
빌드·앱 실행·설치 시 이 스크립트를 실행하지 않습니다.

Rust 시험도 실행 파일에 넣는 같은 bytes/라이선스/고지 해시와8개 codepoint를
검사합니다. 실제 egui 글꼴 파서로 현대 한글 완성형11,172자 전체·코드의 한글
대체·아이콘8개의 존재/너비를100/150/200% 배율에서 확인합니다. 이는 글리프
검사이며 모든 대화상자의 OS DPI 전환·IME/키보드 탐색·접근성/장시간 동작이
검증됐다는 의미는 아닙니다.

크레이트 전체 라이선스·배포 구성/지원 Windows 버전·패키징·시작/메모리/CPU 및
영상/전송 성능 비교는 원래 M3 수용 범위로 유지합니다.
