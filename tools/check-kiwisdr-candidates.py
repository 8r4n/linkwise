#!/usr/bin/env python3
"""Check candidate HTTP interfaces and metadata; never open an audio stream."""
import concurrent.futures
import datetime
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / 'docs/testing/data'

def check(node):
    result = {'candidate_id': node['candidate_id'], 'label': node['label'], 'url': node['url'],
              'checked_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat()}
    for kind, suffix in [('web', '/'), ('status', '/status')]:
        with tempfile.TemporaryDirectory() as tmp:
            body = Path(tmp) / 'body'
            command = ['curl', '--silent', '--show-error', '--location', '--max-redirs', '3',
                       '--connect-timeout', '8', '--max-time', '20', '--max-filesize', '1048576',
                       '--output', str(body), '--write-out', '%{http_code}\n%{url_effective}',
                       node['url'].rstrip('/') + suffix]
            p = subprocess.run(command, capture_output=True, text=True, timeout=25)
            text = body.read_text(errors='replace') if body.exists() else ''
            lines = p.stdout.splitlines()
            entry = {'curl_exit': p.returncode, 'http_status': int(lines[0]) if lines and lines[0].isdigit() else 0,
                     'effective_url': lines[1] if len(lines)>1 else None, 'error': p.stderr.strip(),
                     'response_bytes': body.stat().st_size if body.exists() else 0}
            if kind == 'status':
                entry['fields'] = dict(line.split('=', 1) for line in text.splitlines() if '=' in line)
                entry['accessible'] = p.returncode == 0 and entry['http_status'] == 200 and entry['fields'].get('status') == 'active' and entry['fields'].get('offline') == 'no'
            else:
                entry['kiwi_interface_detected'] = 'kiwi' in text.lower() and ('<html' in text.lower() or '<!doctype' in text.lower())
                entry['accessible'] = p.returncode == 0 and entry['http_status'] == 200 and entry['kiwi_interface_detected']
            result[kind] = entry
    result['passed'] = result['web']['accessible'] and result['status']['accessible']
    return result

def main():
    nodes = json.loads((DATA/'kiwisdr-candidates.json').read_text())['candidates']
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        results = list(pool.map(check, nodes))
    report = {'scope': 'HTTP web interface and /status only; no audio, reservation or RF test',
              'results': results, 'passed': all(n['passed'] for n in results)}
    (DATA/'kiwisdr-access-check.json').write_text(json.dumps(report, indent=2)+'\n')
    for n in results:
        print(n['candidate_id'], 'PASS' if n['passed'] else 'FAIL', n['web']['http_status'], n['status']['http_status'], n['url'])
    return 0 if report['passed'] else 1

if __name__ == '__main__':
    raise SystemExit(main())
