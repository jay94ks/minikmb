# Fixed supervisor: only a newly allocated minikmb-format-* / disk.img regular file.
import json, os, pathlib, signal, stat, subprocess, sys, time

image = pathlib.Path(sys.argv[1])
size, kind, token = int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
root = image.parent
lease, done = root / 'lease', root / 'done'
process = None
closed = True
reason = 'invalid'
ok = False

def alive(last, changed):
    try:
        value = lease.read_text()
    except OSError:
        value = 'cancel'
    if value == 'cancel':
        return value, changed, False
    if value != last:
        changed = time.monotonic()
    return value, changed, time.monotonic() - changed < 3

def stop():
    global closed
    if process is None or process.poll() is not None:
        return
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(process.pid, sig)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=1)
            return
        except subprocess.TimeoutExpired:
            pass
    closed = process.poll() is not None

try:
    if (image.name != 'disk.img' or not root.name.startswith('minikmb-format-')
        or image.is_symlink() or not stat.S_ISREG(image.stat().st_mode)
        or image.stat().st_size != size or image.resolve() != root.resolve() / 'disk.img'
        or kind not in (1, 2, 3)):
        raise ValueError('invalid private file')
    commands = {
        1: (['/usr/sbin/mkfs.fat', '-F', '32', '-s', '1'], ['/usr/sbin/fsck.fat', '-n']),
        2: (['/usr/sbin/mkfs.exfat'], ['/usr/sbin/fsck.exfat', '-n']),
        3: (['/usr/sbin/mke2fs', '-q', '-F', '-t', 'ext4', '-m', '0', '-E', 'lazy_itable_init=0,lazy_journal_init=0,nodiscard'], ['/usr/sbin/e2fsck', '-fn']),
    }[kind]
    last, changed = '', time.monotonic()
    ok = True
    for reason, args in zip(('mkfs', 'fsck'), commands):
        if not pathlib.Path(args[0]).is_file():
            reason, ok = 'missing_tool', False
            break
        last, changed, live = alive(last, changed)
        if not live:
            reason, ok = 'cancel', False
            break
        process = subprocess.Popen(args + [str(image)], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, start_new_session=True)
        deadline = time.monotonic() + 120
        while process.poll() is None:
            last, changed, live = alive(last, changed)
            if not live or time.monotonic() > deadline:
                reason, ok = ('cancel' if not live else 'timeout'), False
                stop()
                break
            time.sleep(.05)
        if not ok or process.returncode != 0:
            ok = False
            break
except Exception:
    ok = False
finally:
    stop()
    temporary = root / 'done.partial'
    temporary.write_text(json.dumps(dict(token=token, closed=closed, ok=ok, reason=reason)))
    temporary.replace(done)
sys.exit(0 if ok and closed else 1)
