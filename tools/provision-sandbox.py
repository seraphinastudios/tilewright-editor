#!/usr/bin/env python3
"""Provision a fresh Linux-only agent sandbox and start the headless runner in it.

Every start makes a new uniquely named child of --destination-parent with
private world/, recovery/ and proposals/ directories, copies the world into
it with the prototype provisioner's rules and limits, writes provision.json,
and replaces itself with `tilewright-editor sandbox` (or, with --mcp, the
MCP editing server `tilewright-editor sandbox-mcp`), passing the three
directories as open descriptors. Nothing is adopted, reused or deleted.

Standard output belongs to the runner's protocol: this helper prints
nothing there. It writes one escaped line to standard error naming the
sandbox and its receipt. See docs/SANDBOX.md and docs/MCP-EDITING.md.
"""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import sys
import time

# The prototype provisioner's directory, mount, identity and copy checks,
# loaded from beside this file, never from the working directory or path.
_spec = importlib.util.spec_from_file_location(
    'tilewright_provision_prototype', Path(__file__).resolve().with_name('provision-prototype.py'))
provision = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(provision)

ACTOR_PREFIX = 'agent:'
MAX_ACTOR = 256
MAX_CLIENT_LABEL = 256
MAX_NOTICE = 8192
CHILDREN = ('world', 'recovery', 'proposals')
ATTEMPTS = 8


def sandbox_actor(text):
    """The engine's actor rule (1 to 256 printable ASCII characters, no
    space at either end) with the agent: prefix and a name after it."""
    if not text.startswith(ACTOR_PREFIX) or len(text) == len(ACTOR_PREFIX):
        raise ValueError('a sandbox actor is agent:<name>')
    if len(text) > MAX_ACTOR or not all(' ' <= character <= '~' for character in text):
        raise ValueError('an actor is at most 256 printable ASCII characters')
    if text.endswith(' '):
        raise ValueError('an actor does not end with a space')
    return text


def client_label(text):
    """An untrusted label: 1 to 256 bytes of UTF-8, recorded as given."""
    if text is None:
        return None
    try:
        size = len(text.encode('utf-8'))
    except UnicodeEncodeError as error:
        raise ValueError('a client label is UTF-8 text') from error
    if not 0 < size <= MAX_CLIENT_LABEL:
        raise ValueError('a client label is 1 to 256 bytes; omit it for none')
    return text


def notice(sandbox, receipt, digest):
    """One bounded line for standard error. JSON with ASCII escapes, so no
    path byte can reach the terminal unescaped; paths too long to fit are
    left out and the name of the sandbox is kept."""
    fields = {'sandbox': str(sandbox), 'receipt': str(receipt), 'receipt_sha256': digest}
    line = 'tilewright sandbox: ' + json.dumps(fields, ensure_ascii=True)
    if len(line) > MAX_NOTICE:
        fields = {'sandbox': None, 'name': sandbox.name, 'receipt_sha256': digest,
                  'paths': 'omitted: longer than this notice allows'}
        line = 'tilewright sandbox: ' + json.dumps(fields, ensure_ascii=True)
    return line


def fresh_child(parent_fd):
    """Create a new uniquely named private child; never adopt one."""
    stamp = time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())
    for _ in range(ATTEMPTS):
        name = f'sandbox-{stamp}-{secrets.token_hex(8)}'
        try:
            os.mkdir(name, mode=0o700, dir_fd=parent_fd)
        except FileExistsError:
            continue
        return name
    raise ValueError('could not make a fresh sandbox name')


def close_others(kept):
    """Close every open descriptor above 2 but `kept`, as /proc/self/fd
    lists them: whatever their number, above a lowered open-file limit
    too, and without a loop over the whole limit. The runner needs /proc
    anyway (it reaches its directories as /proc/self/fd/N); if the list
    cannot be read, nothing is started."""
    kept = set(kept)
    try:
        names = os.listdir('/proc/self/fd')
    except OSError as error:
        raise ValueError('cannot list /proc/self/fd to close inherited descriptors') from error
    for name in names:
        fd = int(name)
        # The listing's own descriptor is listed too, and already closed.
        if fd > 2 and fd not in kept:
            close_quietly(fd)


def open_world(path):
    """The world's directory, opened as the copy reads it: a directory,
    not a link. The copy reads from this descriptor, not the path again."""
    try:
        return provision.directory(path)
    except OSError as error:
        raise ValueError('the world must be an existing readable directory, not a link') from error


def copy_from(world_fd, destination):
    """The prototype helper's copy, from the descriptor checked before the
    sandbox was made. Its copy_tree opens its source with
    provision.directory; for this one call that opens a duplicate of
    `world_fd` instead, so a path changed after the check is never read."""
    opener = provision.directory
    provision.directory = lambda _source: os.dup(world_fd)
    try:
        return provision.copy_tree(None, destination)
    finally:
        provision.directory = opener


def close_quietly(fd):
    """Close `fd`; after close_others it may already be closed."""
    try:
        os.close(fd)
    except OSError:
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--destination-parent', type=Path, required=True,
                        help='existing directory on persistent local storage; each start makes a new child')
    parser.add_argument('--world', type=Path, required=True, help='existing world to copy as data')
    parser.add_argument('--actor', required=True, help='agent:<name>, the actor of every sandbox step')
    parser.add_argument('--client-label', help='untrusted label recorded in proposals, at most 256 bytes')
    parser.add_argument('--editor', type=Path, help='trusted built editor executable; omit to provision only')
    parser.add_argument('--mcp', action='store_true',
                        help='start the MCP editing server (sandbox-mcp) instead of the line runner; needs --editor')
    args = parser.parse_args()
    if args.mcp and args.editor is None:
        raise ValueError('--mcp starts the MCP editing server and needs --editor')
    actor = sandbox_actor(args.actor)
    label = client_label(args.client_label)
    if sys.platform != 'linux':
        raise ValueError('sandbox editing is supported only on Linux')
    executable = args.editor.resolve(strict=True) if args.editor else None
    # The world must open as the copy reads it, a directory and not a
    # link, before anything is created; the copy reads this descriptor.
    world_fd = open_world(args.world)
    held = [world_fd]
    try:
        parent = args.destination_parent.resolve(strict=True)
        # A sandbox inside the world it copies would copy itself.
        if parent.is_relative_to(args.world.resolve(strict=True)):
            raise ValueError('the destination parent may not be the world or inside it')
        parent_fd = provision.directory(parent)
    except BaseException:
        os.close(world_fd)
        raise
    try:
        provision.persistent_mount(parent_fd)
        name = fresh_child(parent_fd)
        task_fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent_fd)
        held.append(task_fd)
        ids = {'session': provision.identity(task_fd)}
        children = {}
        for child in CHILDREN:
            os.mkdir(child, mode=0o700, dir_fd=task_fd)
            fd = os.open(child, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=task_fd)
            held.append(fd)
            children[child] = fd
            ids[child] = provision.identity(fd)
        if len({(v['device'], v['inode']) for v in ids.values()}) != 4:
            raise ValueError('provisioned directory identity collision')
        copied = copy_from(world_fd, children['world'])
        os.fsync(task_fd)
        os.fsync(parent_fd)
        receipt = {'format': 'tilewright.sandbox-provision/1', 'platform': sys.platform,
                   'roots': ids, 'copy': copied, 'actor': actor, 'client_label': label,
                   'scope': 'fresh distinct retained roots for one agent run; nothing is reused or deleted'}
        data = (json.dumps(receipt, indent=2, ensure_ascii=True) + '\n').encode('ascii')
        receipt_fd = os.open('provision.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                             0o600, dir_fd=task_fd)
        try:
            view = memoryview(data)
            while view:
                view = view[os.write(receipt_fd, view):]
            os.fsync(receipt_fd)
        finally:
            os.close(receipt_fd)
        os.fsync(task_fd)
        digest = hashlib.sha256(data).hexdigest()
        print(notice(parent / name, parent / name / 'provision.json', digest), file=sys.stderr, flush=True)
        # Revalidate the same retained FDs at use, after copying and syncing.
        for child, fd in children.items():
            if provision.identity(fd) != ids[child]:
                raise ValueError('provisioned directory identity or mount changed before launch')
        if provision.identity(task_fd) != ids['session']:
            raise ValueError('session identity or mount changed before launch')
        if executable is None:
            return 0
        command = [str(executable), 'sandbox-mcp' if args.mcp else 'sandbox',
                   '--world-fd', str(children['world']),
                   '--recovery-fd', str(children['recovery']),
                   '--proposals-fd', str(children['proposals']),
                   '--actor', actor, '--receipt-sha256', digest]
        if label is not None:
            command += ['--client-label', label]
        # Only the standard streams and the three children cross into the
        # runner. The helper's own descriptors are close-on-exec; any other
        # descriptor it inherited from whoever started it is closed here.
        for fd in children.values():
            os.set_inheritable(fd, True)
        close_others(children.values())
        sys.stdout.flush()
        os.execv(executable, command)
    finally:
        for fd in reversed(held):
            close_quietly(fd)
        close_quietly(parent_fd)


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as error:
        # Escaped: an error can quote a path, and stderr may be a terminal.
        detail = json.dumps(str(error), ensure_ascii=True)[:MAX_NOTICE]
        raise SystemExit(f'sandbox provisioning refused; preserve any created state for inspection: {detail}') from error
