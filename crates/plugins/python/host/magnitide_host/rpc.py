"""The protocol between Magnitide and this host: one JSON object per line.

Requests arrive on stdin as {"id": n, "method": "...", "params": {...}};
replies go to the original stdout as {"id": n, "result": ...} or
{"id": n, "error": "..."}, and events nobody asked for (a GUI plugin's
status message, error, or that it's done) as {"event": "...", ...}. Each
request runs on its own thread, so slow lookups don't hold up others; GUI
work runs on the main thread, in Qt. Anything a plugin prints goes to
stderr, which Magnitide keeps as the host's log, so it can't corrupt the
replies.
"""

import json
import os
import sys
import threading
import traceback
from queue import Queue
from threading import Event

# Replies get a private copy of stdout; everything else printed goes to stderr.
_proto = os.fdopen(os.dup(1), 'w', encoding='utf-8', buffering=1)
os.dup2(2, 1)
sys.stdout = sys.stderr
_write_lock = threading.Lock()

METHODS = {}

# Work for the main thread (Qt's, once it runs); None ends the host.
main_calls = Queue()
# 'app' and 'invoker', once Qt has started (see gui.__init__).
qt = {}


def send(message):
    line = json.dumps(message, ensure_ascii=False, default=str)
    with _write_lock:
        _proto.write(line + '\n')
        _proto.flush()


def event(name, **fields):
    """Something Magnitide didn't ask about: a status message, an error, idle."""
    send({'event': name, **fields})


def method(name):
    """Register a function as the handler of `name`."""

    def register(fn):
        METHODS[name] = fn
        return fn

    return register


def on_main(fn, wait=True):
    """Run fn on the main thread, where Qt lives; return its result."""
    if threading.current_thread() is threading.main_thread():
        return fn()
    done, box = Event(), {}

    def run():
        try:
            box['value'] = fn()
        except BaseException as e:
            box['error'] = e
        finally:
            done.set()

    invoker = qt.get('invoker')
    if invoker is not None:
        invoker.call.emit(run)
    else:
        main_calls.put(run)
    if not wait:
        return None
    done.wait()
    if 'error' in box:
        raise box['error']
    return box.get('value')


def handle(request):
    rid = request.get('id')
    fn = METHODS.get(request.get('method'))
    if fn is None:
        send({'id': rid, 'error': f'unknown method {request.get("method")!r}'})
        return
    try:
        send({'id': rid, 'result': fn(request.get('params') or {})})
    except Exception as e:
        print(traceback.format_exc(), file=sys.stderr, flush=True)
        send({'id': rid, 'error': f'{type(e).__name__}: {e}'.strip()})


def read_requests():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
        except ValueError:
            continue
        if request.get('method') == 'shutdown':
            send({'id': request.get('id'), 'result': None})
            break
        threading.Thread(target=handle, args=(request,), daemon=True).start()
    # stdin closed or shutdown: stop Qt, if it's running, and the main loop.
    app = qt.get('app')
    if app is not None:
        qt['invoker'].call.emit(app.quit)
    main_calls.put(None)


def serve():
    """Read requests on a thread; keep the main thread for Qt, which starts
    the first time a GUI plugin needs it."""
    threading.Thread(target=read_requests, daemon=True).start()
    while True:
        fn = main_calls.get()
        if fn is None:
            break
        fn()
