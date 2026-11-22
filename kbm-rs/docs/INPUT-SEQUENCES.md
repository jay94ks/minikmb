# 한 요청으로 클릭과 키 배열 전송

기존 키/버튼 DOWN·UP, 마우스 이동, 휠, `hid_chord`, `hid_click`은 유지합니다.
펌웨어와 USB wire opcode를 변경하지 않습니다. GUI/REST/MCP가 공유하는 활성
채널 하나를 사용하며 UID나 여러 채널을 지정하지 않습니다.

## 클릭·더블클릭

기존 `hid_click` / `POST /api/v1/active/hid/click`은 그대로 `count: 1/2`를 받습니다.
추가한 `hid_double_click` / `POST /api/v1/active/hid/double-click`은 다음과 같습니다.

```json
{"x":640,"y":360,"button":1}
```

좌표는 현재 캡처 영상의 픽셀입니다. `frame_id`, `input_session`, `hold_ms`도
선택적으로 받습니다. 더블클릭은 한 요청에서 버튼 DOWN/UP 두 쌍을 처리합니다.

## 키 배열

MCP 도구 `hid_key_sequence`:

```json
{"keys":["a","b","c"]}
```

REST `POST /api/v1/active/hid/keys/sequence`는 같은 객체 또는 배열 본문을 받습니다.

```json
["a","b","c"]
```

전송 순서는 `a DOWN → a UP → b DOWN → b UP → c DOWN → c UP`입니다.
LLM 추론이나 클라이언트의 다음 호출을 기다리지 않습니다. 필요하면 객체 형식에
기존 `input_session`을 넣어 같은 소유권을 유지할 수 있습니다.

1~128개 항목을 지원합니다. 영문 대소문자·숫자·ASCII 기호는 US 물리 키로
매핑하며, 대문자와 이동된 기호는 Shift DOWN/UP도 포함합니다. 이름으로는
`Enter`, `Tab`, `Backspace`, `Space`, `ArrowLeft`, `F1`~`F24`, `Lang1` 등 기존
키 목록과 `Ctrl`, `Shift`, `Alt`, `Win`을 사용할 수 있습니다. 대상 OS의
키보드 배열·Caps Lock·IME 상태에 따른 출력까지 문자 삽입으로 보장하지는 않습니다.
임의 Unicode 문자열이나 `"Ctrl+a"` 형식은 받지 않습니다.

전체 배열을 전송 전에 검증합니다. 잘못된 마지막 항목이 있어도 앞부분을 보내지
않습니다. 키를 이미 누르고 있으면 먼저 해제해야 합니다. 내부적으로 최대8개의
기존 HID 명령을 미리 큐에 넣고 각 묶음의 실제 HID_DONE을 모두 확인합니다.
한 키와 그 Shift의 DOWN/UP은 같은 묶음에 들어갑니다. 묶음 경계에서 대기할
때는 해당 키가 해제된 상태입니다. 동일 키보드의 다른 요청은 배열 완료까지
기다리며 마우스의 독립 처리와 기존 명령의 동작은 유지합니다.

응답 예:

```json
{"usb_completed":true,"completed_keys":3,"packets":6,"uid":"0x010101010101010101010101","generation":1}
```

중간 실패 때는 이미 제출한 명령의 결과를 수습하고 남은 묶음을 중단합니다.
원래 연결에서 UP/해제만 시도하며 DOWN은 재시도하지 않습니다. 부분 실행 가능성이
있는 `HID_SEQUENCE_FAILED`/`uncertain: true` 응답 뒤 전체 배열을 자동 재시도하면
안 됩니다. 재연결된 MCU나 다른 활성 채널로 나머지를 옮겨 보내지 않습니다.

이 기능은 호출 사이 CoT/네트워크 왕복 간격을 없애고 호스트의 반복 처리를
줄입니다. 펌웨어 수정 없이 호스트 OS·USB의 실제 시간 간격을 보장하는 기능은
아닙니다. 실제 HID 완료도 대상 앱이 해당 문자를 처리했다는 증거로 확대하지 않습니다.
