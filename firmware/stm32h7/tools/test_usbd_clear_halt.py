"""Compile the real core and compare actual-cancel vs pending-completion ownership."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--project', type=Path, default=Path(__file__).resolve().parent.parent)
args = parser.parse_args()
tinyusb = args.project / 'Middlewares/Third_Party/TinyUSB/src'
with tempfile.TemporaryDirectory() as directory:
    scratch = Path(directory)
    (scratch / 'test_tusb_config.h').write_text(
        '#define CFG_TUSB_MCU OPT_MCU_NONE\n#define CFG_TUSB_OS OPT_OS_NONE\n'
        '#define TUP_DCD_ENDPOINT_MAX 9\n#define CFG_TUD_ENABLED 1\n'
        '#define CFG_TUD_CDC 1\n#define CFG_TUD_HID 2\n#define CFG_TUD_MSC 1\n'
        '#define CFG_TUD_MSC_EP_BUFSIZE 4096\n#define CFG_TUD_NCM 1\n'
        '#define pdFALSE 0\n#define xQueueSendToBack(q,e,t) 1\n')
    for target in (False, True):
        binary = scratch / ('target' if target else 'management')
        flags = ['-DMINIKMB_TUSB_TARGET_INSTANCE=1'] if target else []
        subprocess.run([os.getenv('CC', 'gcc'), '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror',
                        '-ffunction-sections', '-fdata-sections', *flags,
                        '-DCFG_TUSB_CONFIG_FILE="test_tusb_config.h"', '-I', str(scratch),
                        '-isystem', str(tinyusb), str(Path(__file__).with_suffix('.c')),
                        '-Wl,--gc-sections', '-o', str(binary)], check=True)
        print(binary.name, flush=True)
        subprocess.run([str(binary)], check=True)
