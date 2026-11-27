"""実スクリプトの失敗・回収境界を模擬adbで確認するホスト試験。"""
import os
from pathlib import Path
import signal
import sqlite3
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
        print('android.surfaceflinger.layers android.surfaceflinger.frametimeline android.log')
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
elif args[:2] == ['shell', 'pidof']:
    names = ['com.maleicacid.tvinput', 'com.android.tv', 'mediatuner', 'system_server',
             'media.swcodec', 'audioserver', 'surfaceflinger', 'android.hardware.tv.tuner-service.maleicacid2']
    print(123 + names.index(args[2]))
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
        processor = self.bin / 'trace_processor_shell'
        processor.write_text("#!/bin/sh\nprintf 'coverage_ok\\n1\\n'\n")
        processor.chmod(0o755)
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
        archive_path = next(self.root.glob('ok.archive.*/*.tar.gz'))
        with tarfile.open(archive_path) as archive:
            self.assertIn(b'backtrace complete', archive.extractfile('ok/unavailable/native-123.txt').read())
        again = subprocess.run(['bash', str(ROOT / 'diagnostics/capture_playback.sh'), str(out)],
                               env=self.env, capture_output=True, timeout=5)
        self.assertNotEqual(0, again.returncode)

    def test_archive_failure_keeps_capture_without_success_path(self):
        tar = self.bin / 'tar'
        tar.write_text('#!/bin/sh\nexit 2\n')
        tar.chmod(0o755)
        out = self.root / 'archive-failed'
        result = subprocess.run(['bash', str(ROOT / 'diagnostics/capture_playback.sh'), str(out)],
                                env=dict(self.env, CAPTURE_CASE='ok'), text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)
        self.assertNotEqual(0, result.returncode)
        self.assertIn('archive: 1', result.stdout)
        self.assertNotIn('渡すファイル:', result.stdout)
        self.assertTrue((out / 'logcat.txt').exists())
        self.assertEqual([], list(self.root.glob('archive-failed.archive.*/*.tar.gz')))

    def test_trace_processor_failure_is_not_success(self):
        (self.bin / 'trace_processor_shell').write_text('#!/bin/sh\nexit 42\n')
        result, out = self.capture('processor-failed')
        self.assertNotEqual(0, result.returncode)
        self.assertIn('evidence-coverage\t1', (out / 'status.tsv').read_text())

    def test_uncorrelated_native_coverage_is_not_capture_success(self):
        (self.bin / 'trace_processor_shell').write_text(
            "#!/bin/sh\nprintf 'coverage_ok,trace_coverage_ok,native_instance_coverage_ok,target_playback_correlated\\n0,1,1,0\\n'\n")
        result, out = self.capture('ok')
        self.assertNotEqual(0, result.returncode, result.stdout)
        self.assertIn('evidence-coverage\t1', (out / 'status.tsv').read_text())

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


class CoverageTest(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.executescript("""
            CREATE TABLE android_logs(ts INT, tag TEXT, msg TEXT, utid INT);
            CREATE TABLE thread(utid INT, upid INT);
            INSERT INTO thread VALUES (1, 10), (2, 10), (3, 20);
            CREATE TABLE trace_bounds(start_ts INT, end_ts INT);
            CREATE TABLE stats(name TEXT, severity TEXT, value INT);
            INSERT INTO trace_bounds VALUES (0, 100);
            INSERT INTO android_logs VALUES (1, 'MaleicacidCapture', 'capture-begin:test', 1);
            INSERT INTO android_logs VALUES (99, 'MaleicacidCapture', 'capture-trigger:test', 1);
        """)
        for msg in ['PLAYBACK_SYNC_CREATE native=0x123', 'PLAYBACK_SYNC_INPUT native=0x123 buffer=1',
                    'PLAYBACK_SYNC_SCHEDULE native=0x123 buffer=1',
                    'PLAYBACK_SYNC_OUTPUT native=0x123 buffer=1']:
            self.db.execute('INSERT INTO android_logs VALUES (50, ?, ?, 1)', ('MediaSync', msg))
        self.sql = (ROOT / 'diagnostics/coverage.sql').read_text().replace('CAPTURE_KEY', 'test')

    def tearDown(self):
        self.db.close()

    def result(self, column='trace_coverage_ok'):
        cursor = self.db.execute(self.sql)
        return dict(zip([c[0] for c in cursor.description], cursor.fetchone()))[column]

    def test_complete_interval(self):
        self.assertEqual(1, self.result())

    def test_same_instance_does_not_prove_target_generation(self):
        self.assertEqual(1, self.result('native_instance_coverage_ok'))
        self.assertEqual(0, self.result('target_playback_correlated'))
        self.assertEqual(0, self.result('coverage_ok'))

    def test_different_instances_cannot_combine_stages(self):
        self.db.execute("UPDATE android_logs SET msg = REPLACE(msg, '0x123', '0x456') WHERE msg LIKE 'PLAYBACK_SYNC_OUTPUT %'")
        self.assertEqual(0, self.result('native_instance_coverage_ok'))
        self.assertEqual(0, self.result('coverage_ok'))

    def test_four_stages_distributed_across_four_instances(self):
        for stage, identity in [('CREATE', '0x1'), ('INPUT', '0x2'),
                                ('SCHEDULE', '0x3'), ('OUTPUT', '0x4')]:
            self.db.execute("UPDATE android_logs SET msg = REPLACE(msg, '0x123', ?) WHERE msg LIKE ?",
                            (identity, f'PLAYBACK_SYNC_{stage} %'))
        self.assertEqual(0, self.result('native_instance_coverage_ok'))
        self.assertEqual(0, self.result('coverage_ok'))

    def test_same_pointer_in_different_processes_cannot_combine_stages(self):
        self.db.execute("UPDATE android_logs SET utid = 3 WHERE msg LIKE 'PLAYBACK_SYNC_OUTPUT %'")
        self.assertEqual(0, self.result('native_instance_coverage_ok'))

    def test_threads_in_same_process_can_supply_stages(self):
        self.db.execute("UPDATE android_logs SET utid = 2 WHERE msg LIKE 'PLAYBACK_SYNC_OUTPUT %'")
        self.assertEqual(1, self.result('native_instance_coverage_ok'))
        self.assertEqual(0, self.result('coverage_ok'))

    def test_missing_process_or_native_identity_is_not_coverage(self):
        self.db.execute('DELETE FROM thread')
        self.assertEqual(0, self.result('native_instance_coverage_ok'))
        self.db.execute('INSERT INTO thread VALUES (1, 10)')
        self.db.execute("UPDATE android_logs SET msg = REPLACE(msg, ' native=0x123', '')")
        self.assertEqual(0, self.result('native_instance_coverage_ok'))

    def test_reused_pointer_is_not_one_lifetime(self):
        self.db.execute("INSERT INTO android_logs VALUES (60, 'MediaSync', 'PLAYBACK_SYNC_CREATE native=0x123', 1)")
        self.assertEqual(0, self.result('native_instance_coverage_ok'))

    def test_stages_outside_creation_lifetime_are_not_combined(self):
        self.db.execute("UPDATE android_logs SET ts = 40 WHERE msg LIKE 'PLAYBACK_SYNC_INPUT %'")
        self.assertEqual(0, self.result('native_instance_coverage_ok'))
        self.db.execute("UPDATE android_logs SET ts = 50 WHERE msg LIKE 'PLAYBACK_SYNC_INPUT %'")
        self.db.execute("INSERT INTO android_logs VALUES (55, 'MediaSync', 'PLAYBACK_SYNC_DESTROY native=0x123', 1)")
        self.assertEqual(1, self.result('native_instance_coverage_ok'))
        self.db.execute("UPDATE android_logs SET ts = 60 WHERE msg LIKE 'PLAYBACK_SYNC_OUTPUT %'")
        self.assertEqual(0, self.result('native_instance_coverage_ok'))

    def test_missing_marker(self):
        self.db.execute("DELETE FROM android_logs WHERE msg = 'capture-begin:test'")
        self.assertEqual(0, self.result())

    def test_pre_capture_native_logs_do_not_satisfy_evidence(self):
        self.db.execute("UPDATE android_logs SET ts = 0 WHERE tag = 'MediaSync'")
        self.assertEqual(0, self.result('native_instance_coverage_ok'))
        self.assertEqual(0, self.result('coverage_ok'))

    def test_data_loss_and_short_interval(self):
        for name, severity in [('ftrace_cpu_overrun_end', 'info'), ('long_trace_mode_bytes_overwritten', 'data_loss')]:
            self.db.execute('DELETE FROM stats')
            self.db.execute('INSERT INTO stats VALUES (?, ?, 1)', (name, severity))
            self.assertEqual(0, self.result())
        self.db.execute('DELETE FROM stats')
        self.db.execute('UPDATE trace_bounds SET start_ts = 10')
        self.assertEqual(0, self.result())


if __name__ == '__main__':
    unittest.main()
