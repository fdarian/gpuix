"""Regenerates content-visibility-animation-oracle.json from content-visibility-animation-oracle.html with headless Chrome:
  sh content-visibility-animation-oracle.sh   (the scenes the tests play; uv supplies playwright)
  uv run --with playwright python content-visibility-animation-oracle.py [scene:mode ...]   (any scenes of the page, into OUT)
Each scene runs in a Chrome of its own (1280x800, DPR 1) with a devtools trace on. Output: {"chrome": version, "scenes":
{name: {"log": [...], "trace": {...}}}}; the trace holds, per window (before / skipped / after, from the console.timeStamp
marks or the log's skip and reveal), the counts of the frame, style and layout events and of the main thread's frames.
Scenes named on the command line replace their entries in the file. CHROME=<path> picks another binary (headless shell by default), OUT=<path> another output file.
"""
import collections, json, os, sys, time
from playwright.sync_api import sync_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
PAGE = os.path.join(HERE, 'content-visibility-animation-oracle.html')
OUT = os.environ.get('OUT') or os.path.join(HERE, 'content-visibility-animation-oracle.json')
ROOT = os.path.expanduser('~/Library/Caches/ms-playwright/chromium_headless_shell-1248')
CATEGORIES = ['devtools.timeline', 'disabled-by-default-devtools.timeline.frame', 'cc', 'viz', 'blink', 'toplevel']
COUNTED = ['BeginMainThreadFrame', 'ProxyMain::BeginMainFrame', 'Commit', 'ActivateLayerTree', 'DrawFrame', 'UpdateLayoutTree',
           'Layout', 'PrePaint', 'Paint', 'UpdateLayer', 'RequestMainThreadFrame', 'ScheduledActionRunner', 'FireAnimationFrame']
TIMED = ['BeginMainThreadFrame', 'ProxyMain::BeginMainFrame', 'Commit', 'UpdateLayoutTree', 'Layout', 'PrePaint', 'Paint', 'FireAnimationFrame']


def find_exe():
    if os.environ.get('CHROME'): return os.environ['CHROME']
    for root, _, files in os.walk(ROOT):
        for f in files:
            if f == 'chrome-headless-shell': return os.path.join(root, f)
    raise SystemExit('no chrome-headless-shell under ' + ROOT)


def run(p, scene):
    b = p.chromium.launch(executable_path=find_exe(), args=['--allow-file-access-from-files', '--no-sandbox'])
    try:
        pg = b.new_page(viewport={'width': 1280, 'height': 800}, device_scale_factor=1)
        pg.goto('about:blank')
        b.start_tracing(page=pg, categories=CATEGORIES)
        pg.goto('file://' + PAGE + '#' + scene)
        # polling must be an interval: the default polls from requestAnimationFrame, which keeps the page producing frames
        pg.wait_for_function('window.__done === true', timeout=20000, polling=100)
        log = pg.evaluate('window.__log')
        version = b.version
        raw = json.loads(b.stop_tracing())
    finally:
        b.close()
    events = raw['traceEvents'] if isinstance(raw, dict) else raw
    marks = sorted(e['ts'] for e in events if e.get('name') == 'TimeStamp' and (e.get('args', {}).get('data') or {}).get('message') == 'mark')
    trace = {}
    if len(marks) >= 2:
        skip, reveal = marks[0], marks[-1]
        margin = 300_000
        spans = {'before': (skip - 500_000, skip), 'skipped': (skip + margin, reveal - margin), 'after': (reveal + margin, reveal + 700_000)}
        for label, (lo, hi) in spans.items():
            inside = [e for e in events if e.get('ph') in ('X', 'B', 'I', 'b', 'n') and lo <= e.get('ts', -1) < hi and e['name'] in COUNTED]
            counts = collections.Counter(e['name'] for e in inside)
            millis = collections.defaultdict(float)
            for e in inside:
                if e.get('dur') and e['name'] in TIMED: millis[e['name']] += e['dur'] / 1000
            trace[label] = {'seconds': round((hi - lo) / 1e6, 2), **{k: counts[k] for k in COUNTED if counts.get(k)}, 'ms': {k: round(v, 1) for k, v in millis.items()}}
    return version, {'log': log, 'trace': trace}


def main():
    sys.path.insert(0, HERE)
    only = sys.argv[1:]
    with sync_playwright() as p:
        b = p.chromium.launch(executable_path=find_exe())
        pg = b.new_page()
        pg.goto('file://' + PAGE + '#list')
        names = pg.evaluate('window.__names')
        b.close()
        found = json.load(open(OUT))['scenes'] if only and os.path.exists(OUT) else {}
        version = None
        for scene in only or names:
            version, found[scene] = run(p, scene)
            print(scene, file=sys.stderr)
    json.dump({'chrome': version, 'scenes': found}, open(OUT, 'w'), separators=(',', ':'))
    print('wrote %d scenes' % (len(only) or len(names)))


main()
