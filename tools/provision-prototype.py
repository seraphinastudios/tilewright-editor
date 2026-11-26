#!/usr/bin/env python3
"""Provision a fresh Linux-only, private prototype session; never adopt state.

This trusted deployment helper establishes World/recovery separation that the
released Rust SourceRoot API cannot independently prove. It copies data only;
project data never chooses executable commands. A retained descriptor is passed
to the app, avoiding a pathname reopen after verification.
"""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys

MAX_FILES = 4096
MAX_BYTES = 64 << 20
PERSISTENT_LOCAL = {'ext2', 'ext3', 'ext4', 'xfs', 'btrfs', 'f2fs', 'zfs', 'bcachefs'}
# A proposal file as the proposal store names it, and its size bound
# (tilewright_editor_proposal::limits::BYTES).
PROPOSAL_NAME = re.compile(r'proposal-[0-9a-f]{64}\.json')
MAX_PROPOSAL = 16 << 20
PERSON_PREFIX = 'human:'
MAX_ACTOR = 256
# A person's favourites file (tilewright_editor_ui::favourites): its size
# bound and format, the name the editor reads in the session's palettes/
# directory, and the names of the files it writes back there.
MAX_PALETTES = 64 << 10
PALETTES_FORMAT = 'tilewright.editor-favourites/1'
PALETTES_FILE = 'favourites.json'
SAVED_PALETTES = re.compile(r'favourites-[0-9]{4}\.json')


def directory(path):
    return os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)


def kernel_text(path, maximum=1 << 20):
    with open(path, 'rb') as stream:
        data = stream.read(maximum + 1)
    if len(data) > maximum:
        raise ValueError('kernel mount evidence exceeds bound')
    return data.decode('ascii')


def persistent_mount(fd):
    """Correlate this retained directory FD with current kernel mount evidence.

    World also checks ACL/filesystem compatibility. This additional deployment
    check excludes volatile filesystems whose fsync succeeds without surviving
    reboot. It does not attest physical hardware or hostile root processes.
    """
    value = os.fstat(fd)
    ids = [line.split()[1] for line in kernel_text(f'/proc/self/fdinfo/{fd}', 4096).splitlines()
           if line.startswith('mnt_id:')]
    if len(ids) != 1 or not ids[0].isdigit():
        raise ValueError('cannot identify retained directory mount')
    matches = [line for line in kernel_text('/proc/self/mountinfo').splitlines()
               if line.split(' ', 1)[0] == ids[0]]
    if len(matches) != 1 or matches[0].count(' - ') != 1:
        raise ValueError('retained directory mount is unknown')
    before, after = matches[0].split(' - ')
    fields, filesystem = before.split(), after.split()
    device = f'{os.major(value.st_dev)}:{os.minor(value.st_dev)}'
    if len(fields) < 6 or len(filesystem) < 3 or fields[2] != device:
        raise ValueError('retained mount identity does not match directory device')
    kind = filesystem[0]
    if kind not in PERSISTENT_LOCAL or 'rw' not in fields[5].split(','):
        raise ValueError(f'prototype durable editing refuses volatile, unknown or unsupported filesystem: {kind}')
    return {'mount_id': int(ids[0]), 'device': device, 'filesystem': kind}


def identity(fd):
    value = os.fstat(fd)
    if not stat.S_ISDIR(value.st_mode) or value.st_uid != os.getuid() or stat.S_IMODE(value.st_mode) != 0o700:
        raise ValueError('session directories must be owned by this user with mode 0700')
    return {'device': value.st_dev, 'inode': value.st_ino, 'uid': value.st_uid, 'mode': '0700',
            'mount': persistent_mount(fd)}


def person_actor(text):
    """The engine's actor rule (1 to 256 printable ASCII characters, no
    space at either end) with the human: prefix and a name after it."""
    if not text.startswith(PERSON_PREFIX) or len(text) == len(PERSON_PREFIX):
        raise ValueError('a person\'s actor is human:<name>')
    if len(text) > MAX_ACTOR or not all(' ' <= character <= '~' for character in text):
        raise ValueError('an actor is at most 256 printable ASCII characters')
    if text.endswith(' '):
        raise ValueError('an actor does not end with a space')
    return text


def read_regular(path, what, maximum, bound, dir_fd=None):
    """The bytes and status of the file at `path`: a regular file, not a
    link, at most `maximum` bytes (`bound` in words). It is opened once,
    without following a link and without blocking on a pipe, read whole and
    closed; it is never written."""
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=dir_fd)
    except OSError as error:
        raise ValueError(f'the {what} must be an existing readable regular file, not a link') from error
    try:
        value = os.fstat(fd)
        if not stat.S_ISREG(value.st_mode):
            raise ValueError(f'the {what} must be a regular file')
        if value.st_size > maximum:
            raise ValueError(f'the {what} is larger than {bound}')
        chunks = []
        total = 0
        while True:
            chunk = os.read(fd, 1 << 20)
            if not chunk:
                break
            total += len(chunk)
            if total > maximum:
                raise ValueError(f'the {what} is larger than {bound}')
            chunks.append(chunk)
        if total != value.st_size:
            raise ValueError(f'the {what} changed while it was read')
        return b''.join(chunks), value
    finally:
        os.close(fd)


def read_proposal(path):
    """The bytes of the proposal file at `path`, checked and read before
    anything is created: a regular file, not a link, at most 16 MiB, named
    as the proposal store names it."""
    if PROPOSAL_NAME.fullmatch(path.name) is None:
        raise ValueError('a proposal file is named proposal-<64 lowercase hex digits>.json')
    return read_regular(path, 'proposal', MAX_PROPOSAL, '16 MiB')[0]


def read_palettes(path):
    """The bytes and status of the person's favourites file at `path`,
    checked and read before anything is created: a regular file, not a
    link, at most 64 KiB. Its contents are the editor's to check: a
    malformed file is ignored there, as a whole."""
    return read_regular(path, 'palettes file', MAX_PALETTES, '64 KiB')


def return_palettes(directory_fd, target, original, session):
    """After the editor ends: copy the newest favourites file the editor
    wrote into the session's palettes/ directory back to the person's
    `target`, or say where it is when that is not safe.

    It is copied back only when it is the editor's (a regular private file
    of this user, at most 64 KiB, a JSON object of the favourites format)
    and `target` is still the file the session copied in: the same file,
    with the same bytes (`original`, its SHA-256), so nothing the person
    changed meanwhile is lost. The bytes go to a new file beside `target`
    (exclusive create, with `target`'s mode), synced, then renamed over it
    in one step and the directory synced. Returns what it did, for the
    helper's last line; never the person's path or contents."""
    saved = sorted(name for name in os.listdir(directory_fd) if SAVED_PALETTES.fullmatch(name))
    if not saved:
        return {'returned': False, 'reason': 'the editor saved no change'}
    newest = saved[-1]
    kept = {'returned': False, 'kept': str(session / 'palettes' / newest)}
    try:
        data, value = read_regular(newest, 'saved favourites', MAX_PALETTES, '64 KiB', dir_fd=directory_fd)
        if value.st_uid != os.getuid() or stat.S_IMODE(value.st_mode) != 0o600:
            raise ValueError('the saved favourites are not a private file of this user')
        document = json.loads(data.decode('utf-8'))
        if not isinstance(document, dict) or document.get('format') != PALETTES_FORMAT:
            raise ValueError('the saved favourites are not a favourites file')
        digest = hashlib.sha256(data).hexdigest()
        if digest == original['sha256']:
            return {'returned': False, 'reason': 'the saved favourites equal the file given', 'sha256': digest}
        current, status = read_palettes(target)
        if (hashlib.sha256(current).hexdigest() != original['sha256']
                or (status.st_dev, status.st_ino) != original['identity']):
            raise ValueError('the file given changed during the session, so it was not replaced')
        parent = os.open(target.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
        try:
            staged = f'.{target.name}.tilewright-{os.getpid()}'
            fd = os.open(staged, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                         0o600, dir_fd=parent)
            try:
                try:
                    os.fchmod(fd, stat.S_IMODE(status.st_mode))
                    view = memoryview(data)
                    while view:
                        view = view[os.write(fd, view):]
                    os.fsync(fd)
                finally:
                    os.close(fd)
                now = os.stat(target.name, dir_fd=parent, follow_symlinks=False)
                if not stat.S_ISREG(now.st_mode) or (now.st_dev, now.st_ino) != original['identity']:
                    raise ValueError('the file given changed during the session, so it was not replaced')
                os.rename(staged, target.name, src_dir_fd=parent, dst_dir_fd=parent)
            except BaseException:
                # Only the stage this call made, never the person's file.
                try:
                    os.unlink(staged, dir_fd=parent)
                except FileNotFoundError:
                    pass
                raise
            os.fsync(parent)
        finally:
            os.close(parent)
        return {'returned': True, 'from': newest, 'sha256': digest}
    except (OSError, ValueError, UnicodeDecodeError) as error:
        return {**kept, 'reason': str(error)}


def write_proposal(directory_fd, name, data):
    """Write bytes as a new private file in one of the session's
    directories (proposal/, palettes/) and sync it and the directory."""
    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=directory_fd)
    try:
        view = memoryview(data)
        while view:
            view = view[os.write(fd, view):]
        os.fsync(fd)
    finally:
        os.close(fd)
    os.fsync(directory_fd)


def copy_tree(source, destination):
    """Copy bounded regular data without links or special files, then sync it."""
    files = 0
    total = 0
    entries = 0
    # Source is explicitly supplied data, not executable input. No ignored files
    # or ambient contents are bundled into releases by this provisioning helper.
    def visit(src_fd, dst_fd, depth=0):
        nonlocal files, total, entries
        if depth > 32:
            raise ValueError('sample exceeds directory depth bound')
        for name in sorted(os.listdir(src_fd)):
            entries += 1
            if entries > MAX_FILES:
                raise ValueError('sample exceeds entry bound')
            if name in ('.', '..') or '/' in name:
                raise ValueError('invalid source entry')
            value = os.stat(name, dir_fd=src_fd, follow_symlinks=False)
            if stat.S_ISDIR(value.st_mode):
                os.mkdir(name, mode=0o700, dir_fd=dst_fd)
                child_src = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=src_fd)
                child_dst = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=dst_fd)
                try:
                    visit(child_src, child_dst, depth + 1)
                finally:
                    os.close(child_src)
                    os.close(child_dst)
            elif stat.S_ISREG(value.st_mode):
                files += 1
                total += value.st_size
                if files > MAX_FILES or total > MAX_BYTES:
                    raise ValueError('sample exceeds provisioning bounds')
                src = os.open(name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=src_fd)
                dst = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=dst_fd)
                try:
                    opened = os.fstat(src)
                    if (opened.st_dev, opened.st_ino, opened.st_size) != (value.st_dev, value.st_ino, value.st_size):
                        raise ValueError('source changed while provisioning')
                    remaining = value.st_size
                    while remaining:
                        chunk = os.read(src, min(remaining, 1 << 20))
                        if not chunk:
                            raise ValueError('source changed while copying')
                        remaining -= len(chunk)
                        view = memoryview(chunk)
                        while view:
                            view = view[os.write(dst, view):]
                    if os.read(src, 1):
                        raise ValueError('source grew while copying')
                    os.fsync(dst)
                finally:
                    os.close(src)
                    os.close(dst)
            else:
                raise ValueError('sample contains a link or special file')
        os.fsync(dst_fd)
    src = directory(source)
    try:
        visit(src, destination)
    finally:
        os.close(src)
    return {'files': files, 'bytes': total}


# What the receipt and the editor's frame records call the shared field.
SHARED_STRING_FIELD = 'tilewright-ui 0.1.2'


def editor_command(executable, children, args, proposal_name, actor, palettes=False):
    """The editor's arguments: the retained descriptors and the options given."""
    command = [str(executable), 'prototype',
               '--world-fd', str(children['world']),
               '--recovery-fd', str(children['recovery']),
               '--evidence-fd', str(children['evidence']),
               '--font-pack', str(args.font_pack)]
    if args.demo_script:
        command.append('--demo-script')
    if args.reference:
        command.append('--reference')
    if proposal_name:
        command += ['--proposal-fd', str(children['proposal']), '--proposal-name', proposal_name]
    if actor is not None:
        command += ['--actor', actor]
    if args.shared_string_field:
        command.append('--shared-string-field')
    if palettes:
        command += ['--palettes-fd', str(children['palettes'])]
    return command


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--destination', type=Path, required=True, help='new absent session directory')
    parser.add_argument('--world', type=Path, required=True, help='existing sample/import to copy as data')
    parser.add_argument('--editor', type=Path, help='trusted built editor executable; omit to provision only')
    parser.add_argument('--font-pack', type=Path, help='explicit absolute immutable font pack; required for editor launch')
    parser.add_argument('--demo-script', action='store_true')
    parser.add_argument('--reference', action='store_true', help='headless reference proof, no native acceptance')
    parser.add_argument('--proposal', type=Path,
                        help='a proposal file (proposal-<digest>.json) to open; copied into the session')
    parser.add_argument('--actor', help='human:<name>, the actor of the changes this session accepts')
    parser.add_argument('--shared-string-field', action='store_true',
                        help="opt-in: the inspector's string field and the zone name drafts use the shared tilewright-ui field")
    parser.add_argument('--palettes', type=Path,
                        help="a person's favourites file; copied into the session and copied back when the editor ends")
    args = parser.parse_args()
    # Checked, and the proposal read, before anything is created.
    actor = None if args.actor is None else person_actor(args.actor)
    proposal = None
    if args.proposal is not None:
        proposal = (args.proposal.name, read_proposal(args.proposal))
    palettes = None
    if args.palettes is not None:
        if args.demo_script or args.reference:
            raise ValueError('favourites are personal and never reach evidence: '
                             'the scripted demonstration runs without --palettes')
        # Absolute, so the copy back reaches the same file whatever the
        # editor does to the working directory.
        target = args.palettes.absolute()
        data, value = read_palettes(target)
        palettes = {'path': target, 'data': data, 'sha256': hashlib.sha256(data).hexdigest(),
                    'identity': (value.st_dev, value.st_ino)}
    font_record = None
    if args.editor and args.font_pack is None:
        raise ValueError('editor launch requires explicit --font-pack')
    if args.font_pack is not None:
        # Loaded from beside this helper by path, so `python3 -I` works and
        # nothing on the module path can stand in for it.
        spec = importlib.util.spec_from_file_location(
            'tilewright_font_pack', Path(__file__).resolve().with_name('font_pack.py'))
        font_pack = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(font_pack)
        if not args.font_pack.is_absolute():
            raise ValueError('font pack must be an explicit absolute directory')
        manifest = font_pack.read_regular(args.font_pack / 'manifest.json', font_pack.MAX_MANIFEST)
        font_record = {'scope': 'candidate/local data byte verification only; no registry proof',
                       'registry_proof': False, 'path': str(args.font_pack),
                       **font_pack.verify(args.font_pack, manifest)}
    if sys.platform != 'linux':
        raise ValueError('checked prototype editing is supported only on Linux')
    # Resolve only the existing trusted parent. The new final component is
    # created exclusively, and is never silently adopted or chmodded.
    parent = args.destination.parent.resolve(strict=True)
    name = args.destination.name
    if not name or name in ('.', '..'):
        raise ValueError('invalid session name')
    parent_fd = directory(parent)
    held = []
    try:
        persistent_mount(parent_fd)
        os.mkdir(name, mode=0o700, dir_fd=parent_fd)
        task_fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent_fd)
        held.append(task_fd)
        ids = {'session': identity(task_fd)}
        children = {}
        for child in (('world', 'recovery', 'evidence') + (('proposal',) if proposal else ())
                      + (('palettes',) if palettes else ())):
            os.mkdir(child, mode=0o700, dir_fd=task_fd)
            fd = os.open(child, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=task_fd)
            held.append(fd)
            children[child] = fd
            ids[child] = identity(fd)
        if len({(v['device'], v['inode']) for v in ids.values()}) != len(ids):
            raise ValueError('provisioned directory identity collision')
        copied = copy_tree(args.world, children['world'])
        if proposal:
            write_proposal(children['proposal'], *proposal)
        if palettes:
            write_proposal(children['palettes'], PALETTES_FILE, palettes['data'])
        os.fsync(task_fd)
        os.fsync(parent_fd)
        receipt = {'format': 'tilewright.prototype-provision/1', 'platform': sys.platform,
                   'roots': ids, 'copy': copied, 'font_pack': font_record, 'scope': 'fresh distinct retained roots; no arbitrary-root guarantee'}
        # Only with the options, so a receipt without them is unchanged.
        if proposal:
            receipt['proposal'] = {'name': proposal[0], 'bytes': len(proposal[1]),
                                   'sha256': hashlib.sha256(proposal[1]).hexdigest()}
        # The person's favourites by size and digest only: never their
        # path, name or contents.
        if palettes:
            receipt['palettes'] = {'bytes': len(palettes['data']), 'sha256': palettes['sha256']}
        if actor is not None:
            receipt['actor'] = actor
        if args.shared_string_field:
            receipt['string_field'] = SHARED_STRING_FIELD
        receipt_fd = os.open('provision.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                             0o600, dir_fd=task_fd)
        with os.fdopen(receipt_fd, 'w') as stream:
            json.dump(receipt, stream, indent=2)
            stream.write('\n')
            stream.flush()
            os.fsync(stream.fileno())
        os.fsync(task_fd)
        print(json.dumps({'session': str(parent / name), 'provision': receipt}), flush=True)
        # Revalidate the same retained FDs at use, after copying and syncing.
        for child, fd in children.items():
            if identity(fd) != ids[child]:
                raise ValueError('provisioned directory identity or mount changed before launch')
        if identity(task_fd) != ids['session']:
            raise ValueError('session identity or mount changed before launch')
        if args.editor:
            command = editor_command(args.editor.resolve(strict=True), children, args,
                                     proposal and proposal[0], actor, palettes is not None)
            # subprocess closes every other descriptor (close_fds), so only
            # the standard streams and these directories reach the editor.
            result = subprocess.run(command, pass_fds=tuple(children.values()), check=False)
            if palettes:
                outcome = return_palettes(children['palettes'], palettes['path'], palettes, parent / name)
                print(json.dumps({'palettes': outcome}), flush=True)
            return result.returncode
        return 0
    finally:
        for fd in reversed(held):
            os.close(fd)
        os.close(parent_fd)


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as error:
        raise SystemExit(f'prototype provisioning refused; preserve any created state for inspection: {error}') from error
