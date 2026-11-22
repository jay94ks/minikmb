# 쓰기 가능한 이미지와 SD 카드 식별 정보

## 키와 패키지 전달 흐름

1. 관리 PC에서 EXT4·exFAT·FAT32 중 하나로 빈 512바이트 섹터 이미지를 준비한다. 관리 USB의 생성 작업이 SD 카드에 별도의 이미지 파일을 만든다.
2. 관리 PC에서 이미지를 USB 디스크로 마운트해 파일을 쓰거나, 명령 프로토콜로 이미지의 섹터를 읽고 수정한다. 후자의 경우 관리 PC 운영체제에 드라이브를 마운트할 필요가 없다. 호스트 도구가 이미지 파일의 내부 파일시스템을 처리한다.
3. 기록을 동기화하고 관리 쪽 접근을 끝낸 뒤 같은 이미지를 타깃에 연결한다. 복사만 할 때는 읽기 전용으로 연결할 수 있고, 타깃에서 수정해야 할 때는 쓰기 가능 옵션을 사용한다.
4. 타깃에서 키나 `.deb`·`.rpm` 파일을 복사한 뒤 매체를 분리한다. 두 운영체제가 동일한 파일시스템을 독립적으로 캐시하는 동안에는 해당 이미지의 소유권을 이전하지 않는다. 펌웨어 로그에는 이미지 내용과 키를 기록하지 않는다.

읽기 전용 2,048바이트 ISO 광학 매체와 512바이트 가상 디스크는 별도의 매체다. 타깃 USB에는 기존 광학 LUN과 별도 가상 디스크 LUN을 제공한다. 관리 USB는 명령 CDC를 유지하면서 가상 디스크 마운트와 직접 섹터 접근을 지원하도록 설계한다. 같은 이미지의 접근 경로는 대상 MSC, 관리 MSC, 관리 직접 접근 중 하나를 점유한다. 파일시스템별 데이터 구조를 처리하는 코드는 관리 PC에 둔다.

## 관리 PC에서 마운트 없이 패키지 넣기

```bash
python3 tools/prepare_writable_disk.py handoff.img --format exfat --size-mib 64 --partitioned --include package.deb --include package.rpm
```

`--include`는 선택한 파일을 이미지 루트에 바이트 그대로 복사한다. EXT4는 mke2fs에 준비된 파일 트리를 넘긴다. exFAT와 FAT32에는 펌웨어와 같은 FatFs를 사용하는 호스트용 파일 이미지 도구를 쓴다. 호스트에서 생성한 DEB와 공개 Rocky Linux RPM을 세 포맷에 넣고 다시 추출했으며 SHA-256이 모두 일치했다. [RPM 시험 원본](https://dl.rockylinux.org/vault/rocky/9.0/BaseOS/x86_64/os/Packages/a/)

같은 명령에서 `--format`을 `ext4`, `exfat`, `fat32`로 선택한다. 패키지가 여러 개면 파일마다 `--include`를 반복한다. 패키지 이름과 바이트는 그대로 복사한다. 타깃에서 복사한 뒤 해당 배포판의 패키지 관리 도구로 설치할 수 있다.

준비한 `handoff.img`를 아래 `raw_disk.py create` 명령으로 SD에 전송한 뒤 `mount target`으로 내보낸다. `create`의 `--format`은 이미지 생성 때 선택한 값과 일치시킨다. 복사만 할 때는 기본 읽기 전용 연결을 사용한다.

Windows에서 USB 드라이브로 열 이미지에는 `--partitioned`를 사용한다. 1 MiB 위치에 MBR 파티션을 넣는다. Windows 시험에서는 파티션 테이블 없는 기존 exFAT 이미지의 용량은 인식됐고 파일시스템은 열리지 않았다. MBR exFAT 이미지에서는 파일까지 읽고 썼다. [Microsoft exFAT 부트 영역 규격](https://learn.microsoft.com/en-us/windows/win32/fileio/exfat-specification#3-volume-structure)

## 카드 프로필 도구

```powershell
python tools/card_profile.py --uid 000000000000000000000001 info
python tools/card_profile.py --uid 000000000000000000000001 init 520791dc-f818-4c16-bfd0-57901240f758
python tools/card_profile.py --uid 000000000000000000000001 write-user .\identifier.bin
python tools/card_profile.py --uid 000000000000000000000001 read-user .\identifier-copy.bin
```

초기화 예시의 UUID는 사용하는 보드의 값으로 대체한다. `write-user`와 `read-user`는 사용자 데이터를 파일로 전달하며 터미널에 내용을 출력하지 않는다. 세부 규격은 [관리 USB 프로토콜](USB-FS-PROTOCOL.md#카드-보드-id와-사용자-정의-식별값)에 있다.

## SD에 쓰기 디스크를 만들고 관리 명령으로 접근

```powershell
python tools/raw_disk.py --uid 000000000000000000000001 --disk-id 0x1001 create handoff.img --format exfat
python tools/raw_disk.py --uid 000000000000000000000001 --disk-id 0x1001 info
python tools/raw_disk.py --uid 000000000000000000000001 --disk-id 0x1001 read 0 .\sector0.bin
python tools/raw_disk.py --uid 000000000000000000000001 --disk-id 0x1001 write 0 .\sector0.bin
python tools/raw_disk.py --uid 000000000000000000000001 --disk-id 0x1001 apply handoff.img handoff-edited.img
```

새 이미지에 고유한 disk ID를 선택한다. 같은 SD에 광학 이미지와 여러 쓰기 디스크를 보관할 수 있다. 쓰기 디스크는 `MKB-DISK/Dxxxxxxx.IMG`에 저장한다. CRC32로 확인하는 `.OK1` 기록이 있는 디스크만 연다. `apply`는 로컬 편집본의 변경 섹터를 추려 현재 디스크를 확인하고, 일치하는 부분만 쓴 뒤 동기화와 재읽기를 한다. 직접 접근은 지정한 디스크 파일 안의 512바이트 섹터에만 적용된다. 새 이미지의 제조와 커밋은 광학 매체를 분리한 상태에서 실행한다. [관리 USB 디스크 명령](USB-FS-PROTOCOL.md#관리-직접-쓰기-디스크)

## 관리에서 기록하고 타깃에서 복사

```powershell
python tools/raw_disk.py --uid 000000000000000000000001 --disk-id 0x1001 mount management
python tools/raw_disk.py --uid 000000000000000000000001 status
python tools/raw_disk.py --uid 000000000000000000000001 --disk-id 0x1001 unmount
python tools/raw_disk.py --uid 000000000000000000000001 --disk-id 0x1001 mount target
```

관리 USB는 기본 쓰기 가능, 타깃 USB는 기본 읽기 전용이다. 필요하면 mount의 `--writable` 또는 `--no-writable`로 지정한다. OS에서 키와 DEB·RPM 등의 파일을 다 쓴 뒤 안전하게 분리한다. OS가 분리 명령을 보낸 경우 펌웨어가 자동으로 닫는다. 파일시스템을 사용 종료했는데도 상태가 열려 있으면 관리 `unmount`로 닫고 다른 쪽에 연결한다. MSC로 내보내는 동안에도 관리 CDC 명령을 사용할 수 있다. 이미지 편집은 OS 드라이브가 분리된 상태에서 앞의 직접 접근 명령으로 실행할 수 있다.

관리 USB의 MSC에는 쓰기 디스크 한 개가 표시된다. 타깃의 LUN 0은 읽기 전용 광학 매체이고, LUN 1은 별도의 512바이트 쓰기 디스크다. EXT4 볼륨을 파일 단위로 읽으려면 관리·타깃에 EXT4를 지원하는 OS가 필요하다. 파일 이름과 내용은 이미지 내부에 그대로 남는다. OS가 매체 변경을 자동으로 인식하지 않으면 볼륨 목록을 새로 고치거나 대상 USB를 재연결한다.

## 쓰기 가능한 디스크 제어 설계

카드 명령은 STORAGE 채널에 연결했다. 직접 디스크 I/O도 같은 채널을 사용하며 긴 작업은 기존 JOB_STATUS로 완료를 판단한다.

| 작업 | 내용 |
|---|---|
| 카드 조회·초기화 | 현재 STM32 UID, 카드에 저장된 보드 UUID와 등록 시의 STM32 UID를 읽는다. 초기화는 기존 FAT32·exFAT 카드에 MiniKMB 메타데이터를 만든다. |
| 사용자 식별값 읽기·쓰기 | 최대 4,096바이트의 임의 데이터를 오프셋 단위로 읽는다. 쓰기는 전체 길이를 지정하고 데이터 전송 뒤 커밋한다. |
| 디스크 생성·조회 | 선택한 포맷과 총 섹터 수로 SD 파일을 생성한다. 이름과 최종 완료를 반환한다. 완료 전 매체는 노출하지 않는다. |
| 디스크 마운트·분리 | 이미지 ID, 대상 경로, 읽기 전용·쓰기 가능을 지정한다. 기록 동기화와 요청 중인 I/O 정리가 완료된 뒤 소유권을 해제한다. |
| 디스크 직접 읽기·쓰기·동기화 | 관리 직접 접근에 연결된 이미지의 범위 안에서만 동작한다. 요청 크기로는 512바이트 섹터를 쓰고, 읽기 응답은 v1의 256바이트 결과 상한에 맞춰 나눌 수 있다. |

호스트 포맷 도구가 만든 새 이미지에서 0이 아닌 섹터를 추려 전송할 수 있다. 펌웨어는 파일에 남은 섹터를 실제로 0으로 초기화한 뒤 노출한다. 기존 이미지 편집의 경우 값이 0으로 바뀐 섹터도 변경분으로 전송한다. 호스트 포맷과 파일 읽기·변경은 이미지 파일을 대상으로 실행한다.

카드 초기화는 `MINIKBM/BOARD.ID`에 보드 UUID와 등록 당시의 STM32 UID를 기록한다. 두 MCU는 서로 다른 UID를 읽으며 같은 PCB라면 두 카드에 같은 보드 UUID를 지정할 수 있다. 빈 포맷 카드의 초기화에서 SD 파일시스템을 다시 포맷할 필요가 없다. 사용자 정의 데이터는 `USER0.BIN`·`USER1.BIN`을 번갈아 기록한다. CRC32가 일치하는 최신 값을 선택하고, 새 기록이 끊기면 이전 유효 값을 사용한다.

## 현재 구현과 검증 범위

- `tools/prepare_writable_disk.py`가 관리 PC에서 512바이트 섹터 이미지와 변경분을 준비한다. WSL에서 각각 64 MiB를 만들고 파일시스템 검사까지 통과했다. 처음 전송할 0이 아닌 섹터는 시험 조건에서 EXT4 31개, exFAT 37개, FAT32 6개였다.
- EXT4 이미지에 민감하지 않은 시험 파일을 운영체제에 마운트하지 않고 추가했다. 변경 섹터만 재적용한 이미지가 편집본과 바이트 단위로 일치했고 파일도 다시 읽혔다.
- 카드 ID와 사용자 정의 값 파일의 펌웨어 저장 모듈을 추가했다. STM32 컴파일과 FAT32·exFAT 모의 카드 시험에서 빈 카드 초기화, 4,096바이트 저장·재읽기, 잘린 최신 값을 건너뛰고 이전 값으로 복구하기가 통과했다. 호스트 도구와 관리 USB 명령을 연결해 전체 wire 시험을 통과했다. J7의 별도 시험 폴더에서 초기화, 4,096 B의 저장·재기동·슬롯 복구를 확인했다. 카드 식별 실기 (development reference omitted) 디스크의 생성·커밋·섹터 읽기·쓰기와 세션 만료 후 회수는 FAT32·exFAT 및 관리 wire 시험을 통과했다. 관리와 타깃의 두 MSC 경로를 연결하고 실기 검증을 진행한다.

240 MHz FS 빌드는 오류·경고 0건으로 통과했다. 채널 0에서 J7의 카드 존재와 현재 STM32 UID를 읽었다. 보드 UUID와 사용자 값은 미설정으로 읽혔다. 실제 카드 식별값은 변경하지 않았다. 재기동 뒤 기존 iPXE 이미지가 복구됐으며 재마운트 후 9,437,184바이트가 원본과 일치했다.

## J7 직접 I/O 실기

채널 0에서 DEB와 RPM을 담은 8 MiB exFAT 시험 이미지(파일시스템 바깥의 512바이트 시험 섹터 포함)를 ID `0x68ca417`로 생성했다. 8,389,120바이트를 64.750초에 초기화·기록·CRC32 검증했다. 전체 CRC32는 `24fdacc1`이다. 패키지 데이터를 포함한 0이 아닌 527개 섹터는 모두 관리 USB로 다시 읽어 원본과 비교했다. 별도 시험 섹터의 `apply`, 동기화, 닫기·재열기, 원상 복구도 통과했다. 동일 이미지를 관리 및 타깃 MSC 시험에 재사용한다.

512바이트 클러스터의 exFAT 시험에서 여러 이미지로 디렉터리가 늘어날 때 FatFs R0.12c가 생성한 항목을 다시 찾지 못했다. 확장 상태 비트를 제거한 뒤 fragment를 완결하고, 디스크에서 다시 연 디렉터리의 진행 중 fragment 필드를 초기화했다. FAT32·exFAT에 각각 여러 이미지를 저장하고 열며 광학 이미지 읽기를 병행하는 시험을 통과했다. [FatFs 패치](fatfs-exfat-directory.patch)

최종 FatFs 수정본은 CubeIDE 기본·240 MHz FS 빌드를 통과했다. 채널 0에 올린 뒤 ID `0x68ca417`을 다시 열어 exFAT 헤더와 마지막 섹터를 확인했다. 기존 iPXE 광학 이미지도 재마운트 후 9,437,184바이트가 일치했다. AXI 파일 작업 공간은 63,328바이트로 64 KiB 예약 범위 안이다.

## USB 디스크 실기 검증

채널 0에 검증 플래싱한 240 MHz FS 펌웨어에서 Windows에 관리 CDC COM13(CAFE:4002), 타깃 CDC COM14(CAFE:4015)가 잡혔다. 관리 USB에는 이동식 디스크 LUN 0, 타깃 USB에는 광학 LUN 0과 이동식 디스크 LUN 1이 잡혔다.

J7에 별도 MBR exFAT 시험 디스크 0x68ca418을 만들어 사용했다. 디스크는 18,433섹터이며 두 패키지를 담은 8 MiB 파티션은 LBA 2,048에 있다. J9에서 두 파일의 SHA-256을 확인하고 17,408바이트 시험 파일을 쓴 뒤 다시 읽었다. Windows에서 볼륨을 잠그고 분리하고 나서 관리 명령으로 디스크를 닫았다. J3에서 같은 디스크를 읽기 전용으로 열며 Windows의 읽기 전용 플래그도 확인했다. 타깃에서 아래 세 파일을 복사했고 각 원본의 SHA-256과 일치했다.

| 파일 | SHA-256 |
|---|---|
| minikmb-media-test_1.0_all.deb | 68bcea43bc2e9ac393aab0f72eacce5e64257a382bfad5c9872ebbfbef9774c1 |
| acl-2.3.1-3.el9.x86_64.rpm | 2e2219e4bcc328f3063aa51dbb7e69e74d9174cfe210227cf6801a82f6569954 |
| MiniKMB-transfer-test.bin | 29ddae051c64ecdddfd2865c60c8858f1ab1b33697c6b7f727f3680bd74226df |

같은 연결에서 iPXE 광학 이미지를 열어 9,437,184바이트 전체를 SCSI로 읽었다. 원본과 일치했고 타깃 CDC는 양방향 각 65,536바이트가 일치했다. J7의 기존 광학 이미지와 첫 번째 패키지 시험 이미지를 유지했다.

저장소 호스트 테스트는 두 MSC의 소유권 이관, USB 쓰기 버퍼 복사, 동기화 성공 후 응답, SCSI 범위 오류, 읽기 전용, USB 리셋 뒤 이전 읽기 폐기를 통과했다. 생성 도구가 만든 EXT4·exFAT·FAT32의 MBR 이미지는 Linux에서 세 파티션을 검사하고 DEB·RPM을 추출해 SHA-256을 비교했다. 실기에서 Windows가 USB 볼륨으로 열고 두 패키지를 추출한 형식은 exFAT와 FAT32다.

## FAT32와 EXT4 패키지 실기

FAT32 MBR 디스크 `0x68ca419`는 68,157,440바이트를 J7에 기록해 CRC32 `74c74214`가 일치했다. J3에 읽기 전용으로 연결한 Windows FAT32 볼륨에서 DEB와 RPM을 읽었고 두 파일의 SHA-256이 위 원본과 일치했다.

EXT4 MBR 디스크 `0x68ca41a`에서는 관리 호스트가 원본 이미지에서 DEB와 RPM을 추출해 SHA-256을 확인했다. 9,437,184바이트를 J7에 기록해 CRC32 `cc16a7d3`가 일치했고 J3가 대응하는 읽기 전용 USB 디스크를 열거했다.

관리 USB J9를 통해 EXT4의 0이 아닌 528개 섹터와 경계 샘플 3개를 읽었고 모두 원본과 일치했다. Windows에서 J3의 물리 디스크 경로는 관리자 권한을 요구했다. 같은 타깃 LUN에서 기존 exFAT 볼륨의 SCSI 핸들을 유지해 EXT4 전체를 읽은 결과는 아래에 있다. 시험 뒤 기존 exFAT 읽기 전용 디스크 `0x68ca418`과 iPXE 광학 매체가 연결된 상태를 확인했다.

### J3에서 EXT4 전체 섹터 읽기

Windows에서 채널 0 J3의 읽기 전용 LUN 1을 USB 식별자와 일련번호로 확인했다. 기존 exFAT 볼륨의 SCSI 핸들을 유지한 상태로 같은 LUN에 J7의 EXT4 `0x68ca41a`를 읽기 전용으로 연결했다. READ CAPACITY는 512바이트 섹터 18,432개를 반환했다. READ(10)으로 LBA 0부터 전체 9,437,184바이트를 읽었다. 원본과 바이트 단위로 일치했고 CRC32는 `cc16a7d3`, SHA-256은 `c742cf3764679a2993050ec04ef5cb9a24f6d78198149617fb36e45dc70eb7d4`였다. [Windows SCSI pass-through](https://learn.microsoft.com/en-us/windows-hardware/drivers/storage/handling-scsi-pass-through-requests)

J3에서 읽은 데이터의 MBR Linux 파티션을 WSL에서 `e2fsck -f -n`으로 검사했다. 같은 파티션에서 `debugfs`로 꺼낸 DEB 180,758바이트와 RPM 72,395바이트는 각각 위 원본 SHA-256과 일치했다. Windows는 USB 전송을 담당했고 WSL이 수신 이미지의 파일시스템을 읽었다. 시험 뒤 EXFAT `0x68ca418`을 다시 J3 읽기 전용으로 연결했다.
