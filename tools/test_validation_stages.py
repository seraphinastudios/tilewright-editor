"""Prove the validation stages CI runs in parallel add up to the full run."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
STAGES = ('checks', 'tests', 'package')
FIXTURES = 'crates/tilewright-editor-testkit/fixtures'
EDITOR = ('cargo', 'run', '--locked', '--quiet', '--package', 'tilewright-editor', '--')
STATUS = ('git', 'status', '--porcelain', '--untracked-files=all')
PACKAGE = ('package-linux.sh', '<{}>', '0.1.0-test.1', '<font pack>')
CMP = ('cmp', '<first>/tilewright-editor-0.1.0-test.1-{0}.tar.gz', '<second>/tilewright-editor-0.1.0-test.1-{0}.tar.gz')
# Every command tools/validate.sh runs with no argument, in order, with the
# stage that runs it ('every' for all of them). Commands are logged as run,
# with SDL_VIDEO_DRIVER when it is set and run-specific paths named by role.
EXPECTED = [
    ('every', ('git', 'rev-parse', '--show-toplevel')),
    ('every', STATUS),
    ('checks', ('python3', '-m', 'unittest', 'discover', '-s', 'tools', '-p', 'test_*.py')),
    ('every', ('rustc', '--version')),
    ('checks', ('cargo', 'fmt', '--all', '--check')),
    ('checks', ('cargo', 'clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings')),
    ('tests', ('cargo', 'test', '--locked', '--workspace', '--all-targets')),
    ('tests', ('cargo', 'test', '--locked', '--workspace', '--doc')),
    ('checks', ('cargo', 'deny', '--locked', 'check')),
    ('checks', ('check-foundations.sh',)),
    ('tests', EDITOR + ('version',)),
    ('tests', EDITOR + ('open', f'{FIXTURES}/meadow')),
    ('tests', EDITOR + ('headless', f'{FIXTURES}/meadow')),
    ('tests', EDITOR + ('overview', f'{FIXTURES}/prototype-neutral')),
    ('tests', EDITOR + ('lint', '--json', f'{FIXTURES}/prototype-neutral')),
    ('tests', ('SDL_VIDEO_DRIVER=dummy',) + EDITOR + ('window',)),
    ('package', tuple(part.format('first') for part in PACKAGE)),
    ('package', tuple(part.format('second') for part in PACKAGE)),
    ('package', tuple(part.format('linux-amd64') for part in CMP)),
    ('package', tuple(part.format('source') for part in CMP)),
    ('every', ('git', 'diff', '--check')),
    ('every', STATUS),
]
TOOLS = {'checks': {'cargo', 'cargo-deny', 'jq'}, 'tests': {'cargo'}, 'package': {'cargo', 'cargo-auditable', 'jq', 'syft'}}
LOGGER = f'''#!{sys.executable}
import json, os, pathlib, re, subprocess, sys
name = pathlib.Path(sys.argv[0]).name
arguments = []
for argument in sys.argv[1:]:
    if argument == os.environ['TILEWRIGHT_PREPARED_FONT_PACK']:
        argument = '<font pack>'
    arguments.append(re.sub(r'^.*/tilewright-editor-release-(first|second)\\.[^/]+', r'<\\1>', argument))
prefix = [f'SDL_VIDEO_DRIVER={{os.environ["SDL_VIDEO_DRIVER"]}}'] if 'SDL_VIDEO_DRIVER' in os.environ else []
with open(os.environ['STAGE_COMMAND_LOG'], 'a') as output:
    output.write(json.dumps(prefix + [name] + arguments) + '\\n')
if name in ('git', 'cmp'):
    sys.exit(subprocess.run([os.environ['REAL_' + name.upper()], *sys.argv[1:]]).returncode)
if name == 'rustc':
    print('rustc 1.97.1 (test)')
if name == 'cargo' and sys.argv[1:3] == ['test', '--locked'] and 'STRAY_FILE' in os.environ:
    pathlib.Path(os.environ['STRAY_FILE']).write_text('left by a test')
if name == 'package-linux.sh':
    out = pathlib.Path(sys.argv[1])
    out.mkdir()
    for kind in ('linux-amd64', 'source'):
        (out / f'tilewright-editor-{{sys.argv[2]}}-{{kind}}.tar.gz').write_bytes(b'same')
'''


def expected(stages):
    return [command for stage, command in EXPECTED if stage == 'every' or stage in stages]


class ValidationStageTests(unittest.TestCase):
    def run_validation(self, *stages, missing=None, stray=None):
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as logs:
            root = Path(directory)
            (root / 'tools').mkdir()
            shutil.copyfile(ROOT / 'tools/validate.sh', root / 'tools/validate.sh')
            subprocess.run(['git', 'init', '--quiet', str(root)], check=True)
            binary = root / 'bin'
            binary.mkdir()
            # Only the stubs and the utilities the script needs are on PATH.
            for utility in ('awk', 'mktemp', 'rmdir', 'rm', 'diff'):
                (binary / utility).symlink_to(shutil.which(utility))
            stubs = [binary / command for command in
                     ('python3', 'cargo', 'cargo-deny', 'cargo-auditable', 'jq', 'syft', 'rustc', 'git', 'cmp')
                     if command != missing]
            for path in stubs + [root / 'tools/check-foundations.sh', root / 'tools/package-linux.sh']:
                path.write_text(LOGGER)
                path.chmod(0o755)
            log = Path(logs) / 'commands.jsonl'
            environment = {key: value for key, value in os.environ.items() if key != 'SDL_VIDEO_DRIVER'}
            environment.update(PATH=str(binary), STAGE_COMMAND_LOG=str(log),
                               REAL_GIT=shutil.which('git'), REAL_CMP=shutil.which('cmp'),
                               TILEWRIGHT_PREPARED_FONT_PACK=str(Path(logs) / 'font pack'))
            if stray:
                environment['STRAY_FILE'] = str(root / stray)
            result = subprocess.run([shutil.which('bash'), 'tools/validate.sh', *stages], cwd=root, env=environment,
                                    capture_output=True)
            commands = [tuple(json.loads(line)) for line in log.read_text().splitlines()] if log.exists() else []
            return result, commands

    def test_full_run_is_the_expected_command_list(self):
        result, commands = self.run_validation()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(commands, expected(STAGES))

    def test_each_stage_runs_its_commands_and_together_they_run_all(self):
        for stage in STAGES:
            result, commands = self.run_validation(stage)
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            self.assertEqual(commands, expected({stage}), stage)
        self.assertTrue(all(stage in ('every',) + STAGES for stage, _ in EXPECTED))

    def test_named_stages_run_in_the_full_order(self):
        result, commands = self.run_validation('package', 'checks', 'tests')
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(commands, expected(STAGES))

    def test_each_stage_requires_the_tools_it_uses(self):
        for stage in STAGES:
            for tool in sorted(set().union(*TOOLS.values())):
                result, _ = self.run_validation(stage, missing=tool)
                if tool in TOOLS[stage]:
                    self.assertNotEqual(result.returncode, 0, (stage, tool))
                    self.assertIn(f'missing required tool: {tool}'.encode(), result.stderr)
                else:
                    self.assertEqual(result.returncode, 0, (stage, tool, result.stderr.decode()))

    def test_a_file_left_in_the_checkout_fails_the_stage(self):
        result, commands = self.run_validation('tests', stray='left-by-test.txt')
        self.assertEqual(result.returncode, 1)
        self.assertIn(b'validation changed the checkout', result.stderr)
        self.assertIn(b'left-by-test.txt', result.stderr)
        self.assertEqual(commands, expected({'tests'}))

    def test_unknown_stage_is_refused_before_any_command(self):
        result, commands = self.run_validation('checks', 'everything')
        self.assertEqual(result.returncode, 2)
        self.assertIn(b'unknown validation stage', result.stderr)
        self.assertEqual(commands, [('git', 'rev-parse', '--show-toplevel')])

    def test_workflow_runs_each_stage_once_and_the_gate_needs_every_job(self):
        workflow = (ROOT / '.github/workflows/validate.yml').read_text()
        jobs = re.split(r'^  ([a-z][a-z0-9-]*):\n', workflow.split('\njobs:\n', 1)[1], flags=re.MULTILINE)[1:]
        jobs = dict(zip(jobs[::2], jobs[1::2]))
        runs = [(job, stage) for job, text in jobs.items()
                for line in re.findall(r'run: tools/validate\.sh(.*)$', text, flags=re.MULTILINE)
                for stage in line.split()]
        self.assertEqual(sorted(stage for _, stage in runs), sorted(STAGES))
        self.assertEqual(len({job for job, _ in runs}), len(STAGES))
        gate = jobs['aggregate']
        self.assertIn('    name: Editor validation\n', gate)
        self.assertIn('    if: always()\n', gate)
        needs = re.search(r'^    needs: \[(.*)\]$', gate, flags=re.MULTILINE)[1].split(', ')
        self.assertEqual(sorted(needs), sorted(set(jobs) - {'aggregate'}))
        self.assertIn(f"length == {len(needs)} and all(.[]; .result == \"success\")", gate)


if __name__ == '__main__':
    unittest.main()
