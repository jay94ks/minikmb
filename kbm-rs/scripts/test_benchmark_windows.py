"""Measurement accounting tests; generated files and owned Python processes only."""
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import time
import unittest
from benchmark_windows import Tree, Windows, cpu_percent, png_size


class MeasurementTests(unittest.TestCase):
    def test_cpu_counts_all_threads_and_distinguishes_one_core_from_machine(self):
        self.assertEqual(cpu_percent(10, 20_000_010, 4, 8),
                         {"one_core_percent": 50, "machine_percent": 6.25})
        for args in [(1, 0, 1, 1), (0, 1, 0, 1), (0, 1, 1, 0)]:
            with self.assertRaises(ValueError):
                cpu_percent(*args)

    def test_only_valid_png_geometry_is_accepted(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "frame.png"
            path.write_bytes(b"\x89PNG\r\n\x1a\n" + struct.pack(">I", 13) + b"IHDR" + struct.pack(">II", 1600, 1025))
            self.assertEqual(png_size(path), [1600, 1025])
            path.write_text(json.dumps({"fake": True}))
            with self.assertRaises(ValueError):
                png_size(path)

    @unittest.skipUnless(sys.platform == "win32", "Win32 process counters")
    def test_process_tree_keeps_child_and_exited_cpu_without_observing_other_processes(self):
        with tempfile.TemporaryDirectory() as root:
            pidfile = str(Path(root) / "pid.txt")
            # Actual child process tree, no application/device access.
            code = "import subprocess,sys,time; p=subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)']); open(sys.argv[1],'w').write(str(p.pid)); time.sleep(30)"
            process = subprocess.Popen([sys.executable, "-c", code, pidfile], creationflags=subprocess.CREATE_NO_WINDOW)
            tree = Tree(Windows(), process.pid)
            try:
                deadline = time.monotonic() + 5
                while not Path(pidfile).exists():
                    if time.monotonic() > deadline:
                        self.fail("child did not start")
                    time.sleep(.05)
                child = int(Path(pidfile).read_text())
                sample = tree.sample()
                # Windows' Python launcher can insert another owned Python process.
                self.assertTrue({process.pid, child}.issubset(tree.owned))
                self.assertGreaterEqual(sample["live_processes"], 2)
                self.assertLessEqual(sample["live_processes"], 4)
                self.assertGreater(sample["private_commit_bytes"], 0)
                self.assertGreater(sample["working_set_bytes"], 0)
                tree.terminate_owned()
                process.wait(timeout=5)
                deadline = time.monotonic() + 5
                while tree.sample()["live_processes"] and time.monotonic() < deadline:
                    time.sleep(.05)
                after = tree.sample()
                self.assertEqual(after["live_processes"], 0)
                self.assertEqual(after["private_commit_bytes"], 0)
                self.assertGreaterEqual(after["cpu_100ns"], sample["cpu_100ns"])
            finally:
                tree.terminate_owned()
                tree.close()
                process.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
