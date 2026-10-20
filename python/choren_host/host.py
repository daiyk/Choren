"""Main loop: loads box scripts, dispatches inputs on a thread pool, answers runtime events."""
import builtins
import functools
import io
import json
import os
import sys
import threading
from concurrent.futures import ThreadPoolExecutor, wait

from choren_host import py2fix, rpc

HERE = os.path.dirname(os.path.abspath(__file__))


class Record(object):
    """One loaded script box."""

    def __init__(self, msg):
        self.id = msg["box"]
        self.name = msg.get("name", "box")
        self.uri = msg.get("uri", self.name)
        self.script = msg.get("script", "")
        self.inputs = msg.get("inputs", [])
        self.outputs = msg.get("outputs", [])
        self.params = dict(msg.get("params") or {})
        self.path = msg.get("path", "")
        self.timeline = bool(msg.get("timeline"))
        self.parent_timeline = bool(msg.get("parentTimeline"))
        self.instance = None
        self.loaded = threading.Event()


boxes = {}
unloads = []  # pending onUnload calls, awaited on shutdown
pool = ThreadPoolExecutor(max_workers=64, thread_name_prefix="box")


def _install_compat():
    """Python 2 names that box scripts use, provided once for the whole host."""
    import configparser, http.client, pickle, queue, urllib.parse, urllib.request, _thread

    aliases = {"urllib2": urllib.request, "urlparse": urllib.parse, "httplib": http.client, "Queue": queue,
               "StringIO": io, "cStringIO": io, "ConfigParser": configparser, "cPickle": pickle, "thread": _thread}
    for name, module in aliases.items():
        sys.modules.setdefault(name, module)
    extra = {"unicode": str, "basestring": str, "long": int, "xrange": range, "raw_input": input, "unichr": chr,
             "reduce": functools.reduce, "cmp": lambda a, b: (a > b) - (a < b)}
    for name, value in extra.items():
        if not hasattr(builtins, name):
            setattr(builtins, name, value)


# NAOqi's Python bridge (libpythonbridge) runs box code in __main__ after "import os, sys, time" and
# "from naoqi import *", so scripts use these names without importing them (e.g. time.sleep).
PRELUDE = "import os, sys, time, inspect, logging, weakref, qi, motion\nfrom functools import partial\nfrom naoqi import *\n"


def load(record):
    from choren_host.behavior import ALBehavior, generated_class

    try:
        source = py2fix.fix(record.script)
        namespace = {"__name__": "box_%s" % record.id}
        exec(PRELUDE, namespace)
        namespace.update(GeneratedClass=generated_class(record), ALBehavior=ALBehavior)
        exec(compile(source, "<%s>" % record.name, "exec"), namespace)
        cls = namespace.get("MyClass")
        if cls is None:
            raise RuntimeError("The script does not define MyClass")
        record.instance = cls()
        record.instance.__onLoad__()
    except Exception as e:
        import traceback

        rpc.send({"op": "error", "box": record.id, "name": record.name, "message": "Loading failed with the error: %s\n%s" % (e, traceback.format_exc())})
    finally:
        record.loaded.set()


def run_input(record, port, value, has_value):
    record.loaded.wait()
    inst = record.instance
    if inst is None:
        return
    name = "onInput_" + port
    method = getattr(inst, name, None)
    if method is None:
        return
    code = getattr(getattr(method, "__func__", method), "__code__", None)
    if code is not None and code.co_argcount >= 2:
        inst._safeCallOfUserMethod(name, value)
    else:
        inst._safeCallOfUserMethod(name)


def call_box(record, method, *args):
    record.loaded.wait()
    if record.instance is not None:
        getattr(record.instance, method)(*args)


def unload(record):
    record.loaded.wait()
    if record.instance is not None:
        record.instance.__onUnload__()
        rpc.modules.pop(record.instance.getName(), None)


def main():
    rpc.out = os.fdopen(os.dup(1), "w", buffering=1, encoding="utf-8")
    os.dup2(2, 1)  # print() and C-level output go to stderr, i.e. the robot log
    sys.stdout = sys.stderr
    sys.path.insert(0, os.path.join(HERE, "shims"))
    _install_compat()
    for line in sys.stdin:
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        op = msg.get("op")
        if op == "reply":
            rpc.resolve(msg)
        elif op == "load":
            record = Record(msg)
            boxes[record.id] = record
            pool.submit(load, record)
        elif op == "input" and msg["box"] in boxes:
            pool.submit(run_input, boxes[msg["box"]], msg["port"], msg.get("value"), msg.get("hasValue"))
        elif op == "unload" and msg["box"] in boxes:
            unloads.append(pool.submit(unload, boxes.pop(msg["box"])))
        elif op == "resource" and msg["box"] in boxes:
            pool.submit(call_box, boxes[msg["box"]], "__onResource__", msg.get("resource"))
        elif op == "resourceError" and msg["box"] in boxes:
            pool.submit(call_box, boxes[msg["box"]], "__onResourceError__")
        elif op == "param" and msg["box"] in boxes:
            boxes[msg["box"]].params[msg["name"]] = msg.get("value")
        elif op == "mem":
            for callback in list(rpc.memory_subscribers.get(msg["key"], [])):
                pool.submit(callback, msg.get("value"))
        elif op == "module":
            target = rpc.modules.get(msg.get("module"))
            method = getattr(target, msg.get("method", ""), None)
            if method is not None:
                pool.submit(method, *msg.get("args", []))
        elif op == "shutdown":
            # onUnload callbacks may still be calling the robot (stopMove…): keep answering their
            # RPC replies here while a helper waits for them, within the runtime's kill timeout.
            def finish():
                wait(unloads, timeout=1.8)
                os._exit(0)

            threading.Thread(target=finish, daemon=True).start()
    os._exit(0)
