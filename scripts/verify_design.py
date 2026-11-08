#!/usr/bin/env python3
"""Render the actual native app. Verify critical controls, warnings and layout bounds."""
import argparse
import json
from pathlib import Path
import subprocess
import cv2

parser = argparse.ArgumentParser()
parser.add_argument('--binary', default='target/debug/signal-rune')
args = parser.parse_args()
out = Path('target/design')
out.mkdir(parents=True, exist_ok=True)
results = []
scenes = ['chat-dark', 'chat-light', 'link-dark', 'link-light', 'settings', 'new-message', 'empty', 'error', 'long-message', 'qr-preview', 'typing', 'receipts', 'pinned', 'archived']
for scene in scenes:
    for width, height in [(1120, 760), (760, 560), (1440, 900)]:
        name = f'{scene}-{width}x{height}'
        tree = out / f'{name}.json'
        run = subprocess.run([args.binary, '--scene', scene, '--size', f'{width}x{height}', '--scale', '1', '--at', '1', '--snap', str(out / f'{name}.png'), '--tree', str(tree)], capture_output=True, text=True, timeout=60)
        if run.returncode:
            raise SystemExit(run.stderr)
        data = json.loads(tree.read_text())
        failures = list(data.get('warnings', []))
        elements = {e['key']: e for e in data['elements'] if 'key' in e}
        critical = ['settings']
        if scene.startswith('chat') or scene in ['long-message', 'typing', 'receipts', 'pinned']:
            critical += ['search', 'new-message', 'composer', 'send', 'conversation', 'pin-conversation', 'archive-conversation']
        if scene == 'typing':
            critical += ['typing-indicator']
        if scene == 'new-message':
            critical = ['new-chat-dialog', 'recipient', 'start-chat']
        if scene == 'settings':
            critical = ['back', 'theme-toggle']
        if scene.startswith('link') or scene == 'qr-preview':
            critical += ['link-device', 'qr-placeholder' if scene != 'qr-preview' else 'link-qr']
        for key in critical:
            if key not in elements:
                failures.append(f'Missing {key}')
                continue
            x, y, w, h = elements[key]['rect']
            if w <= 0 or h <= 0 or x < -1 or y < -1 or x+w > width+1 or y+h > height+1:
                failures.append(f'{key} outside window: {[x,y,w,h]}')
        if scene == 'long-message':
            for e in data['elements']:
                if e.get('key','').startswith('message-'):
                    x, y, w, h = e['rect']
                    if x < 377 or x+w > width+1:
                        failures.append(f'Message overflows conversation: {e["rect"]}')
        if scene == 'qr-preview':
            decoded, _, _ = cv2.QRCodeDetector().detectAndDecode(cv2.imread(str(out / f'{name}.png')))
            expected = 'https://example.invalid/signal-rune-design-fixture?payload=' + 'sample-' * 16
            if decoded != expected:
                failures.append('Rendered QR code did not decode to the expected fixture')
        results.append({'scene': scene, 'size': [width,height], 'failures': failures})
(out / 'report.json').write_text(json.dumps(results, indent=2))
failures = [r for r in results if r['failures']]
for result in failures:
    print(result)
print(f'{len(results)} native renders; {len(failures)} failed layout checks')
raise SystemExit(bool(failures))
