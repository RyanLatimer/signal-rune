#!/usr/bin/env python3
"""Bundle on the target OS. Requires a built binary and JAVA_HOME (Java 25)."""
import hashlib
import os
from pathlib import Path
import plistlib
import platform as host_platform
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from extend_engine import extend

VERSION = '0.14.10'
SHA256 = '706a57d9faae7525cf732936efdabf210146e3c83f9e8ae7e4e62f5946121c33'
root = Path(__file__).resolve().parents[1]
dist = root / 'dist'
dist.mkdir(exist_ok=True)
platform = {'darwin': 'macos-arm64', 'win32': 'windows-x64', 'linux': 'linux-x64'}[sys.platform]
expected = 'arm64' if sys.platform == 'darwin' else 'x86_64'
actual = host_platform.machine().lower().replace('amd64', 'x86_64').replace('aarch64', 'arm64')
if actual != expected:
    raise SystemExit(f'Unexpected build architecture: {actual}; expected {expected}')
name = f'signal-rune-{platform}'
stage = dist / name
if stage.exists():
    shutil.rmtree(stage)
stage.mkdir()
if sys.platform == 'darwin':
    app = stage / 'Signal Rune.app'
    content = app / 'Contents'
    binary_dir = content / 'MacOS'
    binary_dir.mkdir(parents=True)
    with (content / 'Info.plist').open('wb') as f:
        plistlib.dump({'CFBundleName': 'Signal Rune', 'CFBundleDisplayName': 'Signal Rune',
                      'CFBundleIdentifier': 'org.signal-rune.desktop', 'CFBundleExecutable': 'signal-rune',
                      'CFBundleVersion': '1', 'CFBundleShortVersionString': '0.1.0',
                      'CFBundlePackageType': 'APPL', 'NSHighResolutionCapable': True,
                      'LSMinimumSystemVersion': '14.0'}, f)
else:
    binary_dir = stage
exe = 'signal-rune.exe' if sys.platform == 'win32' else 'signal-rune'
shutil.copy2(root / 'target' / 'release' / exe, binary_dir / exe)
archive = dist / f'signal-cli-{VERSION}.tar.gz'
if not archive.exists():
    urllib.request.urlretrieve(f'https://github.com/AsamK/signal-cli/releases/download/v{VERSION}/signal-cli-{VERSION}.tar.gz', archive)
if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
    raise SystemExit('signal-cli archive checksum mismatch')
temporary = dist / 'engine-extract'
if temporary.exists():
    shutil.rmtree(temporary)
with tarfile.open(archive) as tar:
    tar.extractall(temporary, filter='data')
runtime = (content / 'Resources' / 'runtime') if sys.platform == 'darwin' else (binary_dir / 'runtime')
runtime.parent.mkdir(parents=True, exist_ok=True)
shutil.copytree(temporary / f'signal-cli-{VERSION}', runtime)
extend(runtime, root / 'engine-extension')
java_home = Path(os.environ['JAVA_HOME'])
shutil.copytree(java_home, runtime / 'jre', symlinks=True)
# Bundle all upstream notices, including Java and Signal engine dependency licenses.
for filename in ['README.md', 'LICENSE', 'THIRD_PARTY.md']:
    shutil.copy2(root / filename, stage / filename)
shutil.copytree(root / 'docs', stage / 'docs')
shutil.copytree(root / 'licenses', stage / 'licenses')
shutil.copytree(root / 'engine-extension', stage / 'engine-extension')
env = dict(os.environ, JAVA_HOME=str(runtime / 'jre'))
launcher = runtime / 'bin' / ('signal-cli.bat' if sys.platform == 'win32' else 'signal-cli')
subprocess.run([str(launcher), '--version'], env=env, check=True, timeout=120)
help_output = subprocess.run([str(launcher), 'getClientState', '--help'], env=env, check=True, capture_output=True, text=True, timeout=120).stdout
if 'getClientState' not in help_output:
    raise RuntimeError('Bundled engine extension smoke test failed')
if sys.platform == 'darwin':
    subprocess.run(['codesign', '--force', '--sign', '-', str(app)], check=True)
    subprocess.run(['codesign', '--verify', '--strict', str(app)], check=True)
    subprocess.run(['hdiutil', 'create', '-volname', 'Signal Rune', '-srcfolder', str(stage), '-ov', '-format', 'UDZO', str(dist / f'{name}.dmg')], check=True)
elif sys.platform == 'win32':
    shutil.make_archive(str(dist / name), 'zip', dist, name)
else:
    (stage / 'signal-rune.desktop').write_text('[Desktop Entry]\nName=Signal Rune\nType=Application\nExec=signal-rune\nCategories=Network;InstantMessaging;\nTerminal=false\n')
    with tarfile.open(dist / f'{name}.tar.gz', 'w:gz') as tar:
        tar.add(stage, arcname=name)
print(f'Packaged {name}')
