#!/usr/bin/env python3
"""Compile the small, source-available GPL extension against pinned signal-cli jars.
No network calls or account access. Requires a Java 25 JDK only at build time.
"""
import os
from pathlib import Path
import subprocess
import tempfile

def extend(runtime: Path, source: Path):
    java_home = Path(os.environ['JAVA_HOME'])
    suffix = '.exe' if os.name == 'nt' else ''
    javac = java_home / 'bin' / ('javac' + suffix)
    jar = java_home / 'bin' / ('jar' + suffix)
    target = runtime / 'lib' / 'signal-cli-0.14.10.jar'
    if not target.exists():
        raise RuntimeError('Expected the checksum-verified signal-cli 0.14.10 distribution')
    files = sorted(source.rglob('*.java'))
    if not files:
        raise RuntimeError('Engine extension source is missing')
    with tempfile.TemporaryDirectory(prefix='signal-rune-engine-') as temporary:
        subprocess.run([str(javac), '--release', '25', '-encoding', 'UTF-8', '-cp', str(runtime / 'lib' / '*'), '-d', temporary, *map(str, files)], check=True)
        subprocess.run([str(jar), '--update', '--file', str(target), '-C', temporary, '.'], check=True)
    test_source = source.parent / 'engine-extension-tests' / 'EngineExtensionTest.java'
    if test_source.exists():
        with tempfile.TemporaryDirectory(prefix='signal-rune-engine-tests-') as temporary:
            classpath = str(runtime / 'lib' / '*')
            subprocess.run([str(javac), '--release', '25', '-cp', classpath, '-d', temporary, *map(str, sorted(test_source.parent.glob("*.java")))], check=True)
            for test in sorted(test_source.parent.glob('*Test.java')):
                subprocess.run([str(java_home / 'bin' / ('java' + suffix)), '-ea', '-cp', temporary + os.pathsep + classpath, test.stem], check=True, timeout=60)


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('runtime', type=Path)
    args = parser.parse_args()
    extend(args.runtime.resolve(), Path(__file__).resolve().parents[1] / 'engine-extension')
