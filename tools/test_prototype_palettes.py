"""Check the helper's --palettes: checked before anything is created, copied
into a private palettes/ child, recorded by size only, and the editor's
newest saved file copied back, however the editor ends, only when that is
safe, with nothing from the person's path or file on the helper's output."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

SCRIPT = Path(__file__).with_name('provision-prototype.py')
spec = importlib.util.spec_from_file_location('prototype_provision_palettes', SCRIPT)
provision = importlib.util.module_from_spec(spec)
spec.loader.exec_module(provision)

# Personal-looking content that must never reach the receipt.
PERSONAL = b'{"format": "tilewright.editor-favourites/1", "objects": ["personalprototype"]}\n'
SAVED = b'{\n  "format": "tilewright.editor-favourites/1",\n  "terrain": [],\n  "scatter": [],\n  "objects": []\n}\n'


def persistent_root(case, temporary):
    root = Path(temporary)
    fd = provision.directory(root)
    try:
        try:
            provision.persistent_mount(fd)
        except ValueError as error:
            case.skipTest(f'persistent test root unavailable: {error}')
    finally:
        os.close(fd)
    return root


def run_helper(root, *extra):
    source = root / 'source'
    if not source.exists():
        source.mkdir()
        (source / 'world.jsonc').write_bytes(b'{}\n')
    command = [sys.executable, str(SCRIPT), '--world', str(source), '--destination', str(root / 'session'), *extra]
    return subprocess.run(command, capture_output=True)


@unittest.skipUnless(sys.platform == 'linux', 'Linux descriptor provisioning')
class PalettesOptionTests(unittest.TestCase):
    def test_the_file_is_copied_into_a_private_child_and_recorded_by_size_only(self):
        with tempfile.TemporaryDirectory(dir=os.environ.get('TILEWRIGHT_PERSISTENT_TEST_ROOT')) as temporary:
            root = persistent_root(self, temporary)
            source = root / 'config' / 'my-favourites.json'
            source.parent.mkdir()
            source.write_bytes(PERSONAL)
            os.chmod(source, 0o640)
            before = source.stat()
            result = run_helper(root, '--palettes', str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            session = root / 'session'
            copied = session / 'palettes' / 'favourites.json'
            self.assertEqual(copied.read_bytes(), PERSONAL)
            self.assertEqual([path.name for path in (session / 'palettes').iterdir()], ['favourites.json'])
            self.assertEqual(stat.S_IMODE((session / 'palettes').stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(copied.stat().st_mode), 0o600)
            self.assertNotEqual(copied.stat().st_ino, source.stat().st_ino)
            after = source.stat()
            self.assertEqual((before.st_mode, before.st_mtime_ns, before.st_ino),
                             (after.st_mode, after.st_mtime_ns, after.st_ino))
            text = (session / 'provision.json').read_text()
            receipt = json.loads(text)
            self.assertEqual(receipt['palettes'], {'given': True, 'bytes': len(PERSONAL)})
            # No digest: a small file's digest can be inverted.
            self.assertNotIn(hashlib.sha256(PERSONAL).hexdigest(), text)
            self.assertEqual(set(receipt['roots']), {'session', 'world', 'recovery', 'evidence', 'palettes'})
            # Neither the person's path, the file's name nor its contents.
            for personal in ('personalprototype', 'my-favourites', str(source.parent)):
                self.assertNotIn(personal, text)
                self.assertNotIn(personal, result.stdout.decode())
            self.assertEqual(json.loads(result.stdout)['provision'], receipt)

    def test_bad_files_and_the_scripted_demonstration_are_refused_before_anything_is_created(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = root / 'config'
            config.mkdir()
            good = config / 'good.json'
            good.write_bytes(PERSONAL)
            link = config / 'link.json'
            link.symlink_to(good)
            directory = config / 'directory.json'
            directory.mkdir()
            oversize = config / 'oversize.json'
            with open(oversize, 'wb') as stream:
                stream.truncate((64 << 10) + 1)
            fifo = config / 'fifo.json'
            os.mkfifo(fifo)
            linked = root / 'linked'
            linked.symlink_to(config)
            cases = [
                (['--palettes', str(link)], b'not a link'),
                (['--palettes', str(directory)], b'regular file'),
                (['--palettes', str(fifo)], b'regular file'),
                (['--palettes', str(oversize)], b'64 KiB'),
                (['--palettes', str(config / 'missing.json')], b'existing readable'),
                (['--palettes', str(good), '--demo-script'], b'never reach evidence'),
                (['--palettes', str(good), '--reference', '--demo-script'], b'never reach evidence'),
                (['--palettes', str(good), '--reference'], b'never reach evidence'),
                (['--palettes', str(linked / 'good.json')], b'not a link'),
                (['--palettes', str(config) + '/'], b'regular file'),
            ]
            for extra, message in cases:
                with self.subTest(extra=extra):
                    result = run_helper(root, *extra)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)
                    self.assertFalse((root / 'session').exists())
            self.assertEqual(good.read_bytes(), PERSONAL)

    def test_the_editor_is_given_the_directory_exactly_when_the_file_is(self):
        children = {'world': 3, 'recovery': 4, 'evidence': 5, 'palettes': 6}
        args = argparse.Namespace(font_pack=Path('/fonts'), demo_script=False, reference=False,
                                  shared_string_field=False)
        without = provision.editor_command(Path('/editor'), children, args, None, None)
        self.assertNotIn('--palettes-fd', without)
        given = provision.editor_command(Path('/editor'), children, args, None, None, True)
        self.assertEqual(given, without + ['--palettes-fd', '6'])


def fd_open(path):
    return os.open(path, os.O_RDONLY | os.O_DIRECTORY)


@unittest.skipUnless(sys.platform == 'linux', 'Linux descriptor provisioning')
class ReturnTests(unittest.TestCase):
    """return_palettes: the newest saved file back to the person's file, only
    when that is safe; otherwise where it is, with a fixed reason word."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.session = self.root / 'session'
        self.palettes = self.session / 'palettes'
        self.palettes.mkdir(parents=True, mode=0o700)
        (self.palettes / 'favourites.json').write_bytes(PERSONAL)
        # Names a person might not want on a shared screen or log.
        self.folder = self.root / 'secretfolder'
        self.folder.mkdir()
        self.target = self.folder / 'secret-favourites.json'
        self.target.write_bytes(PERSONAL)
        os.chmod(self.target, 0o640)
        self.original = provision.open_palettes(self.target)

    def tearDown(self):
        os.close(self.original['directory'])
        for path in (self.folder, self.root):
            if path.exists() and not path.is_symlink():
                os.chmod(path, 0o700)
        self.temporary.cleanup()

    def save(self, name, data=SAVED, mode=0o600):
        path = self.palettes / name
        path.write_bytes(data)
        os.chmod(path, mode)
        return path

    def run_return(self):
        fd = fd_open(self.palettes)
        try:
            outcome = provision.return_palettes(fd, self.original, self.session)
        finally:
            os.close(fd)
        # Nothing from the person's path or file, nor any digest.
        self.assertNotIn('secret', json.dumps(outcome))
        self.assertNotIn('sha256', outcome)
        return outcome

    def entries(self):
        return sorted(path.name for path in self.folder.iterdir())

    def assert_kept(self, outcome, reason, data=PERSONAL):
        self.assertEqual(outcome, {'returned': False, 'kept': str(self.palettes / 'favourites-0001.json'),
                                   'reason': reason})
        if data is not None:
            self.assertEqual(self.target.read_bytes(), data)
        # No stage is left beside it.
        self.assertFalse([name for name in self.entries() if '.tilewright-' in name])

    def test_nothing_saved_leaves_the_file_alone(self):
        self.assertEqual(self.run_return(), {'returned': False, 'reason': 'no_change'})
        self.assertEqual(self.target.read_bytes(), PERSONAL)

    def test_the_newest_saved_file_replaces_the_unchanged_file_keeping_its_mode(self):
        self.save('favourites-0001.json', SAVED.replace(b'"objects": []', b'"objects": ["a"]'))
        self.save('favourites-0010.json')
        self.save('favourites-0002.json', SAVED.replace(b'"objects": []', b'"objects": ["b"]'))
        # Not the editor's names: ignored.
        self.save('favourites-99999.json', b'other')
        self.save('favourites.json.bak', b'other')
        self.assertEqual(self.run_return(), {'returned': True, 'from': 'favourites-0010.json'})
        self.assertEqual(self.target.read_bytes(), SAVED)
        self.assertEqual(stat.S_IMODE(self.target.stat().st_mode), 0o640)
        self.assertNotEqual(self.target.stat().st_ino, self.original['identity'][1])
        self.assertEqual(self.entries(), ['secret-favourites.json'])
        self.assertEqual((self.palettes / 'favourites-0010.json').read_bytes(), SAVED)

    def test_saved_bytes_equal_to_the_file_given_change_nothing(self):
        self.save('favourites-0001.json', PERSONAL)
        before = self.target.stat()
        self.assertEqual(self.run_return(), {'returned': False, 'reason': 'same_bytes'})
        self.assertEqual(self.target.stat().st_ino, before.st_ino)

    def test_a_file_edited_in_place_is_kept(self):
        self.save('favourites-0001.json')
        self.target.write_bytes(PERSONAL + b' ')
        self.assert_kept(self.run_return(), 'target_changed', PERSONAL + b' ')

    def test_a_file_replaced_by_the_same_bytes_is_found_before_any_stage_is_made(self):
        self.save('favourites-0001.json')
        other = self.folder / 'other'
        other.write_bytes(PERSONAL)
        os.replace(other, self.target)
        opened = []
        real = os.open

        def recording(path, flags, *args, **kwargs):
            if flags & os.O_EXCL:
                opened.append(path)
            return real(path, flags, *args, **kwargs)
        with mock.patch.object(provision.os, 'open', side_effect=recording):
            outcome = self.run_return()
        self.assert_kept(outcome, 'target_changed')
        self.assertEqual(opened, [])

    def test_a_file_replaced_while_the_stage_is_written_is_kept_and_the_stage_removed(self):
        self.save('favourites-0001.json')
        newer = PERSONAL.replace(b'personalprototype', b'newer')
        real = os.fsync
        calls = []

        def swapping(fd):
            calls.append(fd)
            if len(calls) == 1:
                # After the content check, before the last identity check.
                other = self.folder / 'other'
                other.write_bytes(newer)
                os.replace(other, self.target)
            return real(fd)
        with mock.patch.object(provision.os, 'fsync', side_effect=swapping):
            outcome = self.run_return()
        self.assert_kept(outcome, 'target_changed', newer)
        self.assertEqual(self.entries(), ['secret-favourites.json'])

    def test_a_link_at_the_name_is_kept_and_never_followed(self):
        self.save('favourites-0001.json')
        copy = self.root / 'copy.json'
        copy.write_bytes(PERSONAL)
        self.target.unlink()
        self.target.symlink_to(copy)
        self.assert_kept(self.run_return(), 'target_changed', None)
        self.assertTrue(self.target.is_symlink())
        self.assertEqual(copy.read_bytes(), PERSONAL)

    def test_a_file_the_person_cannot_write_is_kept(self):
        self.save('favourites-0001.json')
        os.chmod(self.target, 0o444)
        self.assert_kept(self.run_return(), 'not_writable')
        self.assertEqual(stat.S_IMODE(self.target.stat().st_mode), 0o444)

    def test_a_saved_file_that_is_not_the_editors_is_kept_where_it_is(self):
        cases = [{'mode': 0o644}, {'data': b'[]'}, {'data': b'{"format": "other/1"}'},
                 {'data': b'\xff'}, {'data': b'{' + b' ' * (64 << 10)}]
        for options in cases:
            with self.subTest(options=options):
                path = self.save('favourites-0001.json', **options)
                self.assert_kept(self.run_return(), 'saved_invalid')
                path.unlink()
        link = self.palettes / 'favourites-0001.json'
        link.symlink_to(self.palettes / 'favourites.json')
        self.assert_kept(self.run_return(), 'saved_invalid')

    def test_an_entry_at_the_stage_name_is_neither_replaced_nor_removed(self):
        self.save('favourites-0001.json')
        stage = self.folder / f'.{self.target.name}.tilewright-{os.getpid()}'
        stage.write_bytes(b'someone else')
        outcome = self.run_return()
        self.assertEqual(outcome['reason'], 'stage_exists')
        self.assertEqual(stage.read_bytes(), b'someone else')
        self.assertEqual(self.target.read_bytes(), PERSONAL)

    @unittest.skipIf(os.geteuid() == 0, 'root writes through directory modes')
    def test_a_directory_the_person_cannot_write_gives_a_fixed_word(self):
        self.save('favourites-0001.json')
        os.chmod(self.folder, 0o500)
        self.assert_kept(self.run_return(), 'permission')

    def test_the_copy_back_goes_through_the_directory_opened_at_provisioning(self):
        self.save('favourites-0001.json')
        # The directory is moved away and a link to another directory,
        # holding a file of the same name and bytes, put in its place.
        moved = self.root / 'moved'
        os.rename(self.folder, moved)
        elsewhere = self.root / 'elsewhere'
        elsewhere.mkdir()
        (elsewhere / self.target.name).write_bytes(PERSONAL)
        self.folder.symlink_to(elsewhere)
        self.assertEqual(self.run_return(), {'returned': True, 'from': 'favourites-0001.json'})
        self.assertEqual((moved / self.target.name).read_bytes(), SAVED)
        self.assertEqual((elsewhere / self.target.name).read_bytes(), PERSONAL)
        self.folder.unlink()
        os.rename(moved, self.folder)


FAKE_EDITOR = r"""#!/usr/bin/env python3
# Saves one change like the editor, then quits on a signal or with an exit
# status, as asked.
import os, signal, sys, time
arguments = sys.argv[1:]
fd = int(arguments[arguments.index('--palettes-fd') + 1])
data = DATA
out = os.open('favourites-0001.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600, dir_fd=fd)
os.write(out, data)
os.close(out)
for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
    signal.signal(number, lambda *_: sys.exit(0))
code = os.environ.get('FAKE_EXIT')
if code:
    sys.exit(int(code))
open(os.environ['FAKE_READY'], 'w').close()
for _ in range(600):
    time.sleep(0.1)
sys.exit(9)
"""


@unittest.skipUnless(sys.platform == 'linux', 'Linux descriptor provisioning')
@unittest.skipUnless(os.environ.get('TILEWRIGHT_PREPARED_FONT_PACK'), 'needs the prepared font pack')
class LaunchTests(unittest.TestCase):
    """Through the helper with a stand-in editor: the copy back runs however
    the editor ends, and the helper's last line says so."""

    def launch(self, temporary, environment=None):
        root = persistent_root(self, temporary)
        editor = root / 'fake-editor'
        editor.write_text(FAKE_EDITOR.replace('DATA', repr(SAVED)))
        os.chmod(editor, 0o700)
        target = root / 'secret.json'
        target.write_bytes(PERSONAL)
        source = root / 'source'
        source.mkdir()
        (source / 'world.jsonc').write_bytes(b'{}\n')
        ready = root / 'ready'
        command = [sys.executable, str(SCRIPT), '--world', str(source), '--destination', str(root / 'session'),
                   '--editor', str(editor), '--font-pack', os.environ['TILEWRIGHT_PREPARED_FONT_PACK'],
                   '--palettes', str(target)]
        env = {**os.environ, 'FAKE_READY': str(ready), **(environment or {})}
        helper = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
                                  start_new_session=True)
        return helper, ready, target

    def finish(self, helper, target):
        stdout, stderr = helper.communicate(timeout=60)
        self.assertNotIn(b'Traceback', stderr)
        last = json.loads(stdout.decode().strip().splitlines()[-1])
        self.assertEqual(last, {'palettes': {'returned': True, 'from': 'favourites-0001.json'}})
        self.assertNotIn(b'secret', stdout + stderr)
        self.assertEqual(target.read_bytes(), SAVED)
        return helper.returncode

    def wait_ready(self, helper, ready):
        deadline = time.monotonic() + 60
        while not ready.exists():
            self.assertIsNone(helper.poll(), helper.stderr.read() if helper.poll() is not None else '')
            self.assertLess(time.monotonic(), deadline)
            time.sleep(0.05)

    def test_ctrl_c_in_the_terminal_still_copies_back(self):
        with tempfile.TemporaryDirectory(dir=os.environ.get('TILEWRIGHT_PERSISTENT_TEST_ROOT')) as temporary:
            helper, ready, target = self.launch(temporary)
            self.wait_ready(helper, ready)
            # A terminal's Ctrl-C reaches the whole foreground group.
            os.killpg(helper.pid, signal.SIGINT)
            self.assertEqual(self.finish(helper, target), 0)

    def test_sigterm_and_sighup_to_the_helper_are_passed_on_and_still_copy_back(self):
        for number in (signal.SIGTERM, signal.SIGHUP):
            with self.subTest(signal=number), \
                    tempfile.TemporaryDirectory(dir=os.environ.get('TILEWRIGHT_PERSISTENT_TEST_ROOT')) as temporary:
                helper, ready, target = self.launch(temporary)
                self.wait_ready(helper, ready)
                os.kill(helper.pid, number)
                self.assertEqual(self.finish(helper, target), 0)

    def test_an_editor_that_fails_still_has_its_change_copied_back(self):
        with tempfile.TemporaryDirectory(dir=os.environ.get('TILEWRIGHT_PERSISTENT_TEST_ROOT')) as temporary:
            helper, _ready, target = self.launch(temporary, {'FAKE_EXIT': '3'})
            self.assertEqual(self.finish(helper, target), 3)


if __name__ == '__main__':
    unittest.main()
