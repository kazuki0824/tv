"""実スクリプトの失敗・回収境界を模擬adbで確認するホスト試験。"""
import os
from pathlib import Path
import signal
import subprocess
import tarfile
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
ADB = r'''#!/usr/bin/env python3
import os, sys, time
from pathlib import Path
args = sys.argv[1:]
mode = os.environ.get('CAPTURE_CASE', 'ok')
if args[:2] == ['shell', 'id']: print(0)
elif args[:2] == ['shell', 'getprop']:
    print('test-fingerprint' if 'ro.build.fingerprint' in args else 'INFO')
elif args[:2] == ['shell', 'perfetto']:
    if '--query' in args:
        print('android.surfaceflinger.layers android.surfaceflinger.frametimeline')
    elif any(a.startswith('--attach=') for a in args): print('stopped')
elif args[:1] == ['pull']: Path(args[-1]).write_bytes(b'test trace')
elif args[:1] == ['logcat']:
    print('capture-log', flush=True)
    while True: time.sleep(.1)
elif args[:3] == ['shell', 'uiautomator', 'events']:
    if mode != 'interrupt':
        print('TYPE_NOTIFICATION_STATE_CHANGED PackageName: com.android.tv; ClassName: android.widget.Toast; Text: [The video is unexpectedly unavailable]', flush=True)
    while True: time.sleep(.1)
elif args[:3] == ['shell', 'pm', 'path']: print('package:/system_ext/priv-app/Test/Test.apk')
elif args[:2] == ['shell', 'sha256sum'] or (len(args) == 2 and args[1].startswith('for path')):
    print('a' * 64 + '  /system/test')
elif args[:2] == ['shell', 'pidof']: print('123')
elif args[:2] == ['shell', 'debuggerd']:
    time.sleep(.1)
    print('backtrace complete')
elif args[:2] == ['shell', 'dumpsys']:
    if '-l' in args: print('tv_input\nmedia.resource_manager\nmedia.audio_flinger\nSurfaceFlinger\nandroid.hardware.tv.tuner.ITuner/default')
    elif 'media.resource_manager' in args and mode != 'ok':
        print({'missing': "Can't find service: media.resource_manager", 'denied': 'Permission Denial', 'empty': '', 'error': 'Error: dump failed'}.get(mode, 'state'))
    else: print('state complete')
elif args[:2] == ['shell', 'cat']: print('100.0 50.0')
'''


class CaptureTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        adb = self.bin / 'adb'
        adb.write_text(ADB)
        adb.chmod(0o755)
        self.env = dict(os.environ, PATH=f'{self.bin}:{os.environ["PATH"]}')

    def tearDown(self):
        self.temp.cleanup()

    def capture(self, mode):
        out = self.root / mode
        result = subprocess.run(['bash', str(ROOT / 'diagnostics/capture_playback.sh'), str(out)],
                                env=dict(self.env, CAPTURE_CASE=mode), text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)
        archives = list(self.root.glob(f'{mode}.archive.*/*.tar.gz'))
        self.assertEqual(1, len(archives), result.stdout)
        with tarfile.open(archives[0]) as archive:
            archived = archive.extractfile(f'{mode}/status.tsv').read()
        self.assertEqual((out / 'status.tsv').read_bytes(), archived)
        self.assertIn(str(archives[0]), result.stdout)
        return result, out

    def test_success_waits_and_archives(self):
        result, out = self.capture('ok')
        self.assertEqual(0, result.returncode, result.stdout)
        self.assertIn('capture\t0', (out / 'status.tsv').read_text())
        self.assertIn('backtrace complete', (out / 'unavailable/native-123.txt').read_text())
        again = subprocess.run(['bash', str(ROOT / 'diagnostics/capture_playback.sh'), str(out)],
                               env=self.env, capture_output=True, timeout=5)
        self.assertNotEqual(0, again.returncode)

    def test_zero_exit_error_output_is_not_success(self):
        for mode, expected in [('missing', 'not-found'), ('denied', 'permission-denied'),
                               ('empty', 'empty-output'), ('error', 'error-output')]:
            with self.subTest(mode=mode):
                result, out = self.capture(mode)
                self.assertNotEqual(0, result.returncode, result.stdout)
                self.assertIn(expected, (out / 'unavailable/media.resource_manager.status').read_text())

    def test_interrupt_still_archives(self):
        out = self.root / 'interrupted'
        proc = subprocess.Popen(['bash', str(ROOT / 'diagnostics/capture_playback.sh'), str(out)],
                                env=dict(self.env, CAPTURE_CASE='interrupt'), text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 10
            while not (out / 'logcat.txt').exists() and time.monotonic() < deadline:
                time.sleep(.05)
            proc.send_signal(signal.SIGTERM)
            output, _ = proc.communicate(timeout=20)
            self.assertNotEqual(0, proc.returncode, output)
            self.assertEqual(1, len(list(self.root.glob('interrupted.archive.*/*.tar.gz'))), output)
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait()


if __name__ == '__main__':
    unittest.main()
