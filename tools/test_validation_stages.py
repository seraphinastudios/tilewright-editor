"""Prove the validation stages CI runs in parallel add up to the full run."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
from collections import Counter
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
STAGES = ('checks', 'tests', 'package')
LOGGER = f'''#!{sys.executable}
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
# Each run has its own font pack path and package directories: log their roles.
arguments = ['<font pack>' if argument == os.environ['TILEWRIGHT_PREPARED_FONT_PACK'] else argument
             for argument in sys.argv[1:]]
if name == 'package-linux.sh':
    arguments[0] = '<output>'
with pathlib.Path(os.environ['STAGE_COMMAND_LOG']).open('a') as output:
    output.write(json.dumps([name] + arguments) + '\\n')
if name == 'package-linux.sh':
    out = pathlib.Path(sys.argv[1])
    out.mkdir()
    for name in ('linux-amd64', 'source'):
        (out / f'tilewright-editor-{{sys.argv[2]}}-{{name}}.tar.gz').write_bytes(b'same')
'''


class ValidationStageTests(unittest.TestCase):
    def fixture(self, root):
        (root / 'tools').mkdir()
        shutil.copyfile(ROOT / 'tools/validate.sh', root / 'tools/validate.sh')
        subprocess.run(['git', 'init', '--quiet', str(root)], check=True)
        binary = root / 'bin'
        binary.mkdir()
        for path in [binary / command for command in ('python3', 'cargo', 'cargo-deny', 'cargo-auditable', 'jq', 'syft')] + [
                root / 'tools/check-foundations.sh', root / 'tools/package-linux.sh']:
            path.write_text(LOGGER)
            path.chmod(0o755)
        rustc = binary / 'rustc'
        rustc.write_text('#!/bin/sh\nprintf "rustc 1.97.1 (test)\\n"\n')
        rustc.chmod(0o755)
        environment = os.environ.copy()
        environment.update(PATH=str(binary) + os.pathsep + environment['PATH'],
                           STAGE_COMMAND_LOG=str(root / 'commands.jsonl'),
                           TILEWRIGHT_PREPARED_FONT_PACK=str(root / 'font pack'))
        return environment

    def run_validation(self, *stages):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            environment = self.fixture(root)
            result = subprocess.run(['bash', 'tools/validate.sh', *stages], cwd=root, env=environment,
                                    capture_output=True)
            log = root / 'commands.jsonl'
            commands = [tuple(json.loads(line)) for line in log.read_text().splitlines()] if log.exists() else []
            return result, commands

    def test_stages_together_run_exactly_the_full_validation(self):
        result, full = self.run_validation()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        parts = {}
        for stage in STAGES:
            result, parts[stage] = self.run_validation(stage)
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            self.assertTrue(parts[stage], stage)
        self.assertEqual(Counter(full), sum((Counter(commands) for commands in parts.values()), Counter()))
        for stage, commands in parts.items():
            others = {command for other, rest in parts.items() if other != stage for command in rest}
            self.assertFalse(others & set(commands), stage)
        self.assertIn(('cargo', 'test', '--locked', '--workspace', '--all-targets'), parts['tests'])
        self.assertIn(('cargo', 'clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings'), parts['checks'])
        self.assertEqual([command[0] for command in parts['package']], ['package-linux.sh', 'package-linux.sh'])

    def test_named_stages_run_in_the_full_order(self):
        _, full = self.run_validation()
        result, named = self.run_validation('package', 'checks', 'tests')
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(named, full)

    def test_unknown_stage_is_refused_before_any_command(self):
        result, commands = self.run_validation('checks', 'everything')
        self.assertEqual(result.returncode, 2)
        self.assertIn(b'unknown validation stage', result.stderr)
        self.assertEqual(commands, [])


if __name__ == '__main__':
    unittest.main()
