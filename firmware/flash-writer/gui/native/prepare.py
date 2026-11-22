"""Prepare an isolated pinned flashrom source copy with bounded Windows I/O."""
import hashlib
import difflib
from pathlib import Path
import shutil

root = Path(__file__).resolve().parents[1]
original = root.parent / 'build/tools/flashrom-1.8.0'
source = root / 'native/build/flashrom-src'
for line in (root / 'native/UPSTREAM-SHA256SUMS').read_text(encoding='utf-8').splitlines():
    expected, name = line.split('  ', 1)
    if hashlib.sha256((original / name).read_bytes()).hexdigest() != expected:
        raise RuntimeError('Pinned upstream source changed: ' + name)
shutil.copytree(original, source, ignore=shutil.ignore_patterns('.git'), dirs_exist_ok=True)
serial = (original / 'serial.c').read_text(encoding='utf-8')
serial = serial.replace('dcb.StopBits = ONESTOPBIT;', '''dcb.StopBits = ONESTOPBIT;
	/* frwr embedded frontend: finite USB serial read/write waits. */
	COMMTIMEOUTS bounded = {MAXDWORD, 0, 5000, 0, 5000};
	if (!SetCommTimeouts(fd, &bounded)) return 1;''', 1)
serial = serial.replace('''if (!tmp)
			msg_pdbg2("Empty read\\n");''', '''if (!tmp) {
			msg_perr("Serial read timed out.\\n");
			return 1;
		}''', 1)
serial = serial.replace('''return 1;
		}
#else
		tmp = write(sp_fd, buf, writecnt);''', '''return 1;
		}
		if (!tmp) {
			msg_perr("Serial write timed out.\\n");
			return 1;
		}
#else
		tmp = write(sp_fd, buf, writecnt);''', 1)
assert 'frwr embedded frontend' in serial and 'Serial read timed out.' in serial and 'Serial write timed out.' in serial
(source / 'serial.c').write_text(serial, encoding='utf-8', newline='\n')
before = (original / 'serial.c').read_text(encoding='utf-8')
(root / 'native/serial-timeout.patch').write_text(''.join(difflib.unified_diff(
    before.splitlines(keepends=True), serial.splitlines(keepends=True), fromfile='a/serial.c', tofile='b/serial.c')), encoding='utf-8')
print('Prepared isolated serprog library source:', source)
