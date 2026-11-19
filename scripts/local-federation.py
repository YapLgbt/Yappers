#!/usr/bin/env python3
"""Own two persistent macOS launchd homes. Never print private invitations or keys."""
import argparse
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import sqlite3
from collections import deque
import subprocess
import time
import tempfile

REPO = Path(__file__).resolve().parent.parent
ROOT = Path.home() / 'Library/Application Support/Yappers Federation'
AGENTS = Path.home() / 'Library/LaunchAgents'
DOMAIN = f'gui/{os.getuid()}'
LABELS = {home: f'app.yappers.home-{home}' for home in ('a', 'b')}
BINARY = ROOT / 'bin/yappers-server'


def run(*args, check=True, quiet=False):
    return subprocess.run([str(a) for a in args], check=check, text=True,
                          stdout=subprocess.DEVNULL if quiet else subprocess.PIPE,
                          stderr=subprocess.PIPE)


def private_json(path, value):
    descriptor, temporary = tempfile.mkstemp(prefix='.config-', suffix='.tmp', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'w') as stream:
            json.dump(value, stream, indent=2)
            stream.write('\n')
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        parent = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(parent)
        finally:
            os.close(parent)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def service(home):
    return f'{DOMAIN}/{LABELS[home]}'


def running(home):
    result = run('launchctl', 'print', service(home), check=False)
    return result.returncode == 0 and 'state = running' in result.stdout


def stop(home):
    result = run('launchctl', 'bootout', service(home), check=False)
    if result.returncode and running(home):
        raise RuntimeError(f'Could not stop home {home.upper()}; inspect launchctl permissions.')


def start(home):
    path = AGENTS / f'{LABELS[home]}.plist'
    if not running(home):
        # Remove a loaded failed job before bootstrapping the corrected configuration.
        run('launchctl', 'bootout', service(home), check=False)
        run('launchctl', 'bootstrap', DOMAIN, path)


def ready(home, since):
    directory = ROOT / home
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        address = directory / 'address.json'
        join = directory / 'join-0.json'
        if running(home) and all(p.exists() and p.stat().st_mtime >= since - 1 for p in (address, join)):
            return json.loads(address.read_text())
        time.sleep(.25)
    raise RuntimeError(f'Home {home.upper()} did not become ready. Check {directory}/stderr.log')


def up(update=False):
    ROOT.mkdir(parents=True, exist_ok=True, mode=0o700)
    ROOT.chmod(0o700)
    (ROOT / 'bin').mkdir(exist_ok=True, mode=0o700)
    AGENTS.mkdir(parents=True, exist_ok=True)
    source = REPO / 'build/rust/debug/yappers-server'
    if update or not BINARY.exists():
        cargo = Path.home() / '.cargo/bin/cargo'
        subprocess.run([str(cargo), 'build', '--locked', '-p', 'yappers-backend'], cwd=REPO, check=True)
    if not BINARY.exists() or update and hashlib.sha256(source.read_bytes()).digest() != hashlib.sha256(BINARY.read_bytes()).digest():
        for home in LABELS:
            stop(home)
        temporary = BINARY.with_suffix('.new')
        shutil.copy2(source, temporary)
        temporary.chmod(0o700)
        os.replace(temporary, BINARY)
    for home in LABELS:
        directory = ROOT / home
        directory.mkdir(exist_ok=True, mode=0o700)
        directory.chmod(0o700)
        config = directory / 'config.json'
        if not config.exists():
            run(BINARY, 'init', '--directory', directory, '--network', 'n0', '--auto-federate')
        # launchd owns the foreground executable; no terminal or shell is kept alive.
        definition = {
            'Label': LABELS[home], 'ProgramArguments': [str(BINARY), 'run', '--config', str(config)],
            'WorkingDirectory': str(directory), 'RunAtLoad': True, 'KeepAlive': True,
            'ThrottleInterval': 10, 'ExitTimeOut': 25, 'Umask': 0o077,
            'StandardOutPath': str(directory / 'stdout.log'),
            'StandardErrorPath': str(directory / 'stderr.log'),
            'ProcessType': 'Background',
        }
        with (AGENTS / f'{LABELS[home]}.plist').open('wb') as stream:
            plistlib.dump(definition, stream)
    # The dedicated testing pair shares one private feed, with a different capability per home.
    a_path, b_path = ROOT / 'a/config.json', ROOT / 'b/config.json'
    a, b = json.loads(a_path.read_text()), json.loads(b_path.read_text())
    community = next(iter(a['communities']))
    if community not in b['communities']:
        if (ROOT / 'b/events.sqlite').exists():
            raise RuntimeError('Refusing to replace an existing B community/database.')
        b['communities'] = {community: next(iter(b['communities'].values()))}
        private_json(b_path, b)
    addresses = {}
    for home in LABELS:
        since = time.time() if not running(home) else 0
        start(home)
        addresses[home] = ready(home, since)
    # First enrollment only: public endpoint addresses carry no membership capabilities.
    changed = []
    for home, other in (('a', 'b'), ('b', 'a')):
        path = ROOT / home / 'config.json'
        config = json.loads(path.read_text())
        if not config.get('peers'):
            config['peers'] = [{'address': addresses[other], 'communities': [community]}]
            config['autoFederation'] = {'enabled': True, 'seeds': [addresses[other]], 'blockedHosts': []}
            private_json(path, config)
            changed.append(home)
    for home in changed:
        stop(home)
        since = time.time()
        start(home)
        ready(home, since)
    status()
    print('Connect desktop: ./yap invite a --copy')
    print('Connect iPhone:  ./yap invite b --copy')


def read_json(path):
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise ValueError(f'Expected a JSON object in {path.name}')
    return value


def home_status(home):
    directory = ROOT / home
    result = {'home': home, 'service': 'running' if running(home) else 'stopped',
              'endpoint': None, 'events': None, 'groups': None, 'error': None}
    try:
        address = directory / 'address.json'
        if address.exists():
            result['endpoint'] = read_json(address).get('id')
            if not isinstance(result['endpoint'], str):
                result['endpoint'] = None
                raise ValueError('Invalid endpoint address')
        database = directory / 'events.sqlite'
        if database.exists():
            # Read only: status must never create or migrate a server database.
            with contextlib.closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True, timeout=2)) as db:
                result['events'] = db.execute('SELECT COUNT(*) FROM events').fetchone()[0]
                tables = {r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
                if 'public_groups' in tables:
                    result['groups'] = db.execute('SELECT COUNT(*) FROM public_groups').fetchone()[0]
    except (OSError, ValueError, sqlite3.Error):
        result['error'] = 'Saved address or database could not be read; inspect logs.'
    return result


def status(as_json=False):
    homes = [home_status(home) for home in LABELS]
    if as_json:
        print(json.dumps({'homes': homes, 'dataDirectory': str(ROOT)}, indent=2))
    else:
        print('Yap federation — this Mac')
        for home in homes:
            endpoint = (home['endpoint'] or 'not initialized')[:12]
            events = home['events'] if home['events'] is not None else '—'
            print(f"  {home['home'].upper()}  {home['service']:<8}  events {str(events):<6}  endpoint {endpoint}")
            if home['error']:
                print('     ' + home['error'])
        print('  Manage: ./yap start | stop | restart | doctor | logs')
    return homes


def invite(home, copy=False, show=False):
    path = ROOT / home / 'join-0.json'
    if not path.exists():
        raise RuntimeError('Invitation not ready. Run ./yap start, then ./yap doctor.')
    invitation = read_json(path)
    config = read_json(ROOT / home / 'config.json')
    if invitation.get('accessToken') != config.get('communities', {}).get(invitation.get('community')):
        raise RuntimeError('Invitation does not match this home. Restart it to refresh invitations.')
    value = json.dumps(invitation, separators=(',', ':'))
    if copy:
        subprocess.run(['pbcopy'], input=value, text=True, check=True)
        print(f'Home {home.upper()} invitation copied. In Yap: You → Join community → paste → Join instance.')
        print('For iPhone, paste using Universal Clipboard. Keep the invitation private.')
    elif show:
        # Deliberate opt-in for piping/secure transfer; never part of status or doctor.
        print(value)
    else:
        print(f'Home {home.upper()} private invitation: {path}')
        print(f'Copy: ./yap invite {home} --copy')
        print('In Yap: You → Join community → paste → Join instance.')


def redacted(text):
    secrets = set()
    for home in LABELS:
        path = ROOT / home / 'config.json'
        if path.exists():
            try:
                config = read_json(path)
                secrets.add(config.get('privateKey', ''))
                secrets.update(config.get('communities', {}).values())
            except (OSError, ValueError, AttributeError, TypeError):
                # A corrupt configuration must not cause raw logs to be printed.
                return '[Logs withheld: private configuration could not be read.]'
    for value in sorted(secrets - {''}, key=len, reverse=True):
        text = text.replace(value, '[redacted]')
    return text


def logs(home=None, lines=30):
    for selected in ([home] if home else LABELS):
        for kind in ('stdout', 'stderr'):
            path = ROOT / selected / f'{kind}.log'
            print(f'Home {selected.upper()} {kind}:')
            if path.exists():
                with path.open(errors='replace') as stream:
                    print(redacted(''.join(deque(stream, maxlen=lines))).rstrip() or '(empty)')
            else:
                print('(no log yet)')


def doctor():
    """Probe real authenticated Iroh feed reads without posting or printing feed content."""
    healthy = True
    for home in LABELS:
        item = home_status(home)
        if item['service'] != 'running' or item['error']:
            print(f'FAIL home {home.upper()}: service/database unavailable; try ./yap start or logs {home}')
            healthy = False
            continue
        directory = ROOT / home
        for filename in ('config.json', 'join-0.json', 'events.sqlite'):
            path = directory / filename
            if not path.exists() or path.stat().st_mode & 0o077:
                print(f'FAIL home {home.upper()}: missing or non-private {filename}')
                healthy = False
        try:
            config = read_json(directory / 'config.json')
            invitation = read_json(directory / 'join-0.json')
            if invitation.get('accessToken') != config['communities'].get(invitation.get('community')):
                raise ValueError('stale invitation')
            result = subprocess.run([str(BINARY), 'feed', '--join', str(directory / 'join-0.json'),
                                     '--identity', str(directory / 'health-reader.json')],
                                    capture_output=True, text=True, timeout=30)
            if result.returncode:
                print(f'FAIL home {home.upper()}: Iroh feed request failed; inspect ./yap logs {home}')
                healthy = False
            else:
                feed = json.loads(result.stdout)
                print(f"PASS home {home.upper()}: authenticated Iroh read; {len(feed.get('posts', []))} visible posts; {item['events']} stored events")
        except (OSError, ValueError, KeyError, subprocess.TimeoutExpired):
            print(f'FAIL home {home.upper()}: connection/invitation unavailable; inspect ./yap logs {home}')
            healthy = False
    if healthy:
        print('Both homes accepted an authenticated feed request.')
    return healthy


@contextlib.contextmanager
def management_lock():
    ROOT.mkdir(parents=True, exist_ok=True, mode=0o700)
    ROOT.chmod(0o700)
    descriptor = os.open(ROOT / '.manager.lock', os.O_CREAT | os.O_WRONLY, 0o600)
    with os.fdopen(descriptor, 'w') as stream:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise RuntimeError('Another management command is running. Wait for it to finish.') from None
        yield


def main():
    parser = argparse.ArgumentParser(prog='./yap', description='Manage the two Yap test homes on this Mac.')
    commands = parser.add_subparsers(dest='command')
    for name in ('start', 'up', 'stop', 'down', 'restart', 'update', 'doctor'):
        commands.add_parser(name, help={
            'start': 'Start homes; initialize/build only if missing', 'up': 'Alias for start',
            'stop': 'Stop homes; preserve all keys and data', 'down': 'Alias for stop',
            'restart': 'Restart both homes with saved state', 'update': 'Build and install the current server',
            'doctor': 'Probe both homes over real Iroh without posting',
        }[name])
    state = commands.add_parser('status', help='Read-only service and database summary')
    state.add_argument('--json', action='store_true')
    invitation = commands.add_parser('invite', help='Get private app connection details')
    invitation.add_argument('home', choices=LABELS)
    output = invitation.add_mutually_exclusive_group()
    output.add_argument('--copy', action='store_true', help='Copy privately to clipboard')
    output.add_argument('--show', action='store_true', help='Print private JSON deliberately; do not share publicly')
    log = commands.add_parser('logs', help='Recent logs with configured keys/capabilities redacted')
    log.add_argument('home', nargs='?', choices=LABELS)
    log.add_argument('--lines', type=int, default=30)
    args = parser.parse_args()
    if args.command in (None, 'status'):
        status(getattr(args, 'json', False))
    elif args.command == 'invite':
        invite(args.home, args.copy, args.show)
    elif args.command == 'logs':
        if not 1 <= args.lines <= 1000:
            parser.error('--lines must be between 1 and 1000')
        logs(args.home, args.lines)
    else:
        with management_lock():
            if args.command in ('start', 'up', 'update'):
                up(update=args.command == 'update')
            elif args.command in ('stop', 'down'):
                for home in LABELS:
                    stop(home)
                status()
            elif args.command == 'restart':
                for home in LABELS:
                    stop(home)
                up()
            elif not doctor():
                raise SystemExit(1)


if __name__ == '__main__':
    try:
        main()
    except subprocess.CalledProcessError as error:
        raise SystemExit(redacted(error.stderr or f'Command failed: {error.returncode}'))
    except (OSError, ValueError, RuntimeError) as error:
        raise SystemExit(redacted(str(error)))
