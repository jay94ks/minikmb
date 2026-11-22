"""Run the target USB adapter with a simulated host and TinyUSB endpoint."""

from pathlib import Path
import os
import subprocess
import tempfile


def main():
    project = Path(__file__).resolve().parent.parent
    with tempfile.TemporaryDirectory() as temp:
        scratch = Path(temp)
        (scratch / "FreeRTOS.h").touch()
        (scratch / "task.h").write_text(
            "void test_lock(void);\nvoid test_unlock(void);\n"
            "#define taskENTER_CRITICAL test_lock\n#define taskEXIT_CRITICAL test_unlock\n"
        )
        (scratch / "stm32h7xx.h").write_text(
            "#include <stdint.h>\nextern const uint8_t test_uid[12];\n#define UID_BASE test_uid\n"
        )
        (scratch / "test_tusb_config.h").write_text(
            "#define CFG_TUSB_MCU OPT_MCU_NONE\n#define CFG_TUSB_OS OPT_OS_NONE\n"
            "#define TUP_DCD_ENDPOINT_MAX 9\n#define CFG_TUD_ENABLED 1\n"
            "#define CFG_TUD_CDC 1\n#define CFG_TUD_HID 2\n#define CFG_TUD_HID_EP_BUFSIZE 8\n"
            "#define CFG_TUD_MSC 1\n#define CFG_TUD_MSC_EP_BUFSIZE 4096\n#define CFG_TUD_NCM 1\n"
        )
        tinyusb = project / "Middlewares/Third_Party/TinyUSB/src"
        usb = project / "Core/Src/usb"
        for speed in (0, 1):
            for fixture in ("test_target_hid_usb_fixture.c", "test_target_hid_independent_fixture.c"):
                binary = scratch / f"test-hid-{speed}-{fixture}"
                subprocess.run([
                    os.getenv("CC", "gcc"), "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-DMINIKMB_TARGET_HID=1",
                    "-DMINIKMB_TARGET_MSC=1", "-DMINIKMB_TARGET_NCM=1", f"-DMINIKMB_TARGET_HIGH_SPEED={speed}",
                    '-DCFG_TUSB_CONFIG_FILE="test_tusb_config.h"',
                    "-I", str(project / "Core/Inc"), "-I", str(scratch), "-isystem", str(tinyusb),
                    str(usb / "target_hid_usb.c"), str(usb / "target_hid_report.c"),
                    str(usb / "target_hid_queue.c"), str(usb / "management_hid.c"),
                    str(usb / "target_descriptors.c"),
                    str(project / "tools" / fixture), "-o", str(binary),
                ], check=True)
                subprocess.run([binary], check=True)


if __name__ == "__main__":
    main()
