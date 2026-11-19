"""Headless management checks. Temporary stores; no launchd or clipboard changes."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('manager', Path(__file__).parents[1] / 'local-federation.py')
manager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(manager)


class ManagerTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.root_patch = patch.object(manager, 'ROOT', self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)
        for home in ('a', 'b'):
            directory = self.root / home
            directory.mkdir(mode=0o700)
            manager.private_json(directory / 'config.json', {
                'privateKey': 'PRIVATE-SIGNING-KEY', 'communities': {'group': 'PRIVATE-CAPABILITY'},
            })
            manager.private_json(directory / 'join-0.json', {
                'community': 'group', 'accessToken': 'PRIVATE-CAPABILITY', 'serverAddr': {'id': home},
            })
            with contextlib.closing(sqlite3.connect(directory / 'events.sqlite')) as db:
                db.execute('CREATE TABLE events(id TEXT)')
                db.execute('INSERT INTO events VALUES(?)', ('saved-event',))
                db.commit()
            (directory / 'events.sqlite').chmod(0o600)

    def capture(self, function, *args, **kwargs):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            result = function(*args, **kwargs)
        return output.getvalue(), result

    @patch.object(manager, 'running', return_value=True)
    def test_status_is_readonly_and_excludes_credentials(self, _):
        before = (self.root / 'a/events.sqlite').read_bytes()
        output, result = self.capture(manager.status, True)
        self.assertEqual(json.loads(output)['homes'][0]['events'], 1)
        self.assertEqual(before, (self.root / 'a/events.sqlite').read_bytes())
        self.assertNotIn('PRIVATE', output)
        self.assertEqual(result[0]['service'], 'running')

    @patch.object(manager, 'running', return_value=False)
    def test_status_does_not_create_missing_database(self, _):
        (self.root / 'a/events.sqlite').unlink()
        result = manager.home_status('a')
        self.assertIsNone(result['events'])
        self.assertFalse((self.root / 'a/events.sqlite').exists())

    def test_invitation_hidden_unless_explicit(self):
        output, _ = self.capture(manager.invite, 'a')
        self.assertNotIn('PRIVATE-CAPABILITY', output)
        output, _ = self.capture(manager.invite, 'a', show=True)
        self.assertEqual(json.loads(output)['accessToken'], 'PRIVATE-CAPABILITY')

    @patch.object(manager.subprocess, 'run')
    def test_copy_uses_private_stdin_without_printing_secret(self, process):
        output, _ = self.capture(manager.invite, 'b', copy=True)
        self.assertEqual(process.call_args.args[0], ['pbcopy'])
        self.assertEqual(json.loads(process.call_args.kwargs['input'])['accessToken'], 'PRIVATE-CAPABILITY')
        self.assertNotIn('PRIVATE-CAPABILITY', output)

    def test_stale_invitation_is_rejected(self):
        manager.private_json(self.root / 'a/join-0.json', {'community': 'group', 'accessToken': 'old'})
        with self.assertRaisesRegex(RuntimeError, 'does not match'):
            manager.invite('a')

    def test_log_redaction_and_fail_closed_on_corrupt_config(self):
        raw = 'PRIVATE-SIGNING-KEY PRIVATE-CAPABILITY useful error'
        (self.root / 'a/stderr.log').write_text(raw)
        output, _ = self.capture(manager.logs, 'a')
        self.assertIn('useful error', output)
        self.assertNotIn('PRIVATE-', output)
        (self.root / 'b/config.json').write_text('{invalid')
        output, _ = self.capture(manager.logs, 'a')
        self.assertNotIn('PRIVATE-', output)
        self.assertIn('Logs withheld', output)

    def test_manager_lock_excludes_concurrent_commands_and_releases(self):
        with manager.management_lock():
            with self.assertRaisesRegex(RuntimeError, 'Another management command'):
                with manager.management_lock():
                    self.fail('Second management command acquired lock')
        with manager.management_lock():
            pass

    @patch.object(manager, 'running', return_value=True)
    @patch.object(manager.subprocess, 'run')
    def test_doctor_reads_real_protocol_without_printing_feed(self, process, _):
        process.return_value = subprocess.CompletedProcess([], 0, json.dumps({'posts': [{'body': 'PRIVATE POST BODY'}]}), '')
        output, healthy = self.capture(manager.doctor)
        self.assertTrue(healthy)
        self.assertEqual(process.call_count, 2)
        self.assertNotIn('PRIVATE', output)
        for call in process.call_args_list:
            self.assertEqual(call.args[0][1], 'feed')
            self.assertEqual(call.kwargs['timeout'], 30)

    @patch.object(manager, 'running', return_value=True)
    @patch.object(manager.subprocess, 'run')
    def test_doctor_fails_on_unreachable_home(self, process, _):
        process.return_value = subprocess.CompletedProcess([], 1, '', 'PRIVATE-CAPABILITY')
        output, healthy = self.capture(manager.doctor)
        self.assertFalse(healthy)
        self.assertNotIn('PRIVATE', output)
        self.assertIn('FAIL home A', output)

    def test_private_atomic_replace_keeps_permissions(self):
        path = self.root / 'credentials.json'
        manager.private_json(path, {'sequence': 1})
        manager.private_json(path, {'sequence': 2})
        self.assertEqual(json.loads(path.read_text())['sequence'], 2)
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertFalse(path.with_suffix('.tmp').exists())


if __name__ == '__main__':
    unittest.main()
