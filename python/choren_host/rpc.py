"""JSON-lines channel to the Rust runtime. `out` is the original stdout; `print` goes to stderr."""
import itertools
import json
import threading

out = None  # set by host.main
_lock = threading.Lock()
_pending = {}
_ids = itertools.count(1)
modules = {}  # module name -> object, for ALMemory.subscribeToEvent callbacks
memory_subscribers = {}  # key -> [callback]


def send(msg):
    line = json.dumps(msg, default=str)
    with _lock:
        out.write(line + "\n")
        out.flush()


def request(op, **kw):
    """Sends a request and blocks this thread until the runtime replies."""
    rid = next(_ids)
    event = threading.Event()
    _pending[rid] = [event, None]
    send(dict(op=op, id=rid, **kw))
    event.wait()
    reply = _pending.pop(rid)[1]
    if "error" in reply:
        raise RuntimeError(reply["error"])
    return reply.get("ok")


def resolve(msg):
    slot = _pending.get(msg.get("id"))
    if slot:
        slot[1] = msg
        slot[0].set()


def call(service, method, args):
    return request("call", service=service, method=method, args=list(args))


def log(level, message, category="behavior.box"):
    send({"op": "log", "level": level, "category": category, "message": message})
