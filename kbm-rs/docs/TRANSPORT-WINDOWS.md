# Windows 관리 USB 통신

2026-11-21: 호스트 네트워크 공유를 켠 뒤 관리 MCU 연결이 끊긴다는 실기 보고를
받았습니다. 기존 로그에서 모드 변경 성공 뒤 약 3~6초 내 연결 종료·복귀를 확인했으나,
그 로그만으로 물리 USB 분리와 앱의 세션 종료를 구분할 수는 없었습니다.

기존 `serialport::set_timeout(2ms)`는 설치된 serialport 4.10.1의 Windows 구현에서
읽기와 쓰기 제한을 함께 설정합니다. 따라서 큰 NCM 프레임의 쓰기 지연이 관리 연결
종료로 이어질 수 있었습니다. 이 경로는 제거했으며, 실제 보고된 끊김의 직접 원인인지와
수정 효과는 실기 확인 전까지 확정하지 않습니다. 임시로 검토한 250ms 쓰기 대기는
최종 구현에 사용하지 않습니다.

## 완료 이벤트 기반 워커

- COM 포트를 `FILE_FLAG_OVERLAPPED`로 한 번 열고, 같은 핸들에서 HELLO/INFO와 UID를
  검증합니다. 운영 경로에서 포트를 다시 열거나 임의 COM 번호를 선택하지 않습니다.
- 읽기·쓰기마다 별도 OVERLAPPED, 이벤트, 고정 크기 버퍼를 유지합니다. 대기 중인
  커널 I/O의 버퍼를 변경·재할당하지 않습니다. 프레임 하나의 쓰기가 끝나기 전 다음
  프레임을 쓰지 않으며, 부분 완료 시 남은 suffix만 제출합니다.
- `WaitForMultipleObjects`가 RX 완료, TX 완료, 새 명령/종료 알림을 함께 기다립니다.
  데이터가 없거나 I/O가 아직 pending인 것은 오류가 아닙니다. 짧은 COM I/O 제한은 없습니다.
- COM 수신 큐에 데이터가 없을 때는 1바이트 읽기를 대기시키고, 큐에 데이터가 있으면
  최대 8KiB씩 배치로 읽습니다. 한 회차의 RX 처리량을 제한해 송신과 명령 처리가
  계속 진행되도록 합니다.
- 관리 프로토콜 ACK의 기존 1초 감시는 전체 프레임의 실제 쓰기 완료 뒤 시작합니다.
  I/O 완료 알림을 MCU ACK나 HID 완료로 취급하지 않습니다. ACK 뒤 HID_DONE 감시,
  채널별 순서, 입력 우선순위와 실패한 DOWN의 자동 재전송 금지는 유지합니다.
- 사용자의 추가 지시에 따라 MCU I/O 워커는 각 제한된 처리 묶음 뒤
  `std::thread::yield_now()`를 실행합니다. NCM 이벤트 루프도 잠금을 해제한 뒤
  `tokio::task::yield_now().await`로 양보합니다. 완료 이벤트가 계속 신호 상태이거나
  수신 스트림이 계속 준비되어 있어도 한 실행 경로가 무기한 CPU를 점유하지 않도록
  하며, 고정 시간 sleep은 추가하지 않습니다.
- 초기 HELLO/INFO 및 명시적 `--inspect`만 완료 이벤트를 기다리는 동기 어댑터를 씁니다.
  초기 검증 대기는 유한하며 운영 I/O 워커의 readiness 판단에는 사용하지 않습니다.
- 종료 시 `CancelIoEx` 후 취소 완료까지 확인한 다음 OVERLAPPED·버퍼·핸들을 해제합니다.
- 모드 요청은 `NETWORK_HOST_REQUESTED` 등으로, 워커 종료 원인은 `transport_failed`로
  기록합니다. UID·세션·오류 코드와 요청 메타데이터만 기록하며 키·파일·패킷 본문은
  기록하지 않습니다. 프로토콜 오류에는 해당 요청, 그 외에는 마지막 제출 요청이 표시됩니다.

## 검증

Windows named pipe로 실장치 없이 30ms 지연 송신, 반대 방향 동시 수신, 버퍼 보존,
명령 이벤트 깨우기, pending RX 종료 취소 및 작은 읽기 사이의 잔여 바이트 보존을
검증합니다. 프로토콜 테스트는 pending 쓰기 중 ACK 감시가 시작되지 않고, 쓰기 완료
뒤에만 시작되는지와 완료 관측보다 ACK가 먼저 처리돼도 중복 완료가 없는지 확인합니다.

Windows 공식 동작:
[GetOverlappedResult](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-getoverlappedresult),
[WaitForMultipleObjects](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitformultipleobjects).
