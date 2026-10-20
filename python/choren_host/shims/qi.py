"""Clean-room subset of the `qi` Python SDK backed by Choren's virtual robot."""
import logging as _logging
import threading
import time

from choren_host import rpc

logging = _logging  # qi.logging.info(...) works like the logging module


class Future(object):
    def __init__(self, fn=None, delay_us=0):
        self._done = threading.Event()
        self._value = None
        self._error = None
        self._canceled = False
        if fn is not None:
            self._timer = threading.Timer(delay_us / 1e6, self._run, [fn])
            self._timer.daemon = True
            self._timer.start()

    def _run(self, fn):
        if self._canceled:
            return
        try:
            self._value = fn()
        except Exception as e:
            self._error = str(e)
        self._done.set()

    def _set(self, value=None, error=None):
        self._value, self._error = value, error
        self._done.set()

    def value(self, timeout=None):
        if not self._done.wait(None if timeout is None or timeout < 0 else timeout / 1000.0):
            raise RuntimeError("Future timeout")
        if self._error:
            raise RuntimeError(self._error)
        return self._value

    def wait(self, timeout=None):
        return self._done.wait(None if timeout is None or timeout < 0 else timeout / 1000.0)

    def cancel(self):
        self._canceled = True
        timer = getattr(self, "_timer", None)
        if timer is not None:
            timer.cancel()
        if not self._done.is_set():
            self._error = "canceled"
            self._done.set()

    requestCancellation = cancel

    def isRunning(self):
        return not self._done.is_set()

    def isFinished(self):
        return self._done.is_set()

    def hasError(self, timeout=None):
        self.wait(timeout)
        return bool(self._error) and not self._canceled

    def isCanceled(self):
        return self._canceled

    def error(self, timeout=None):
        self.wait(timeout)
        return self._error

    def addCallback(self, cb):
        threading.Thread(target=lambda: (self._done.wait(), cb(self)), daemon=True).start()

    def then(self, cb):
        return async_(lambda: (self._done.wait(), cb(self))[1])

    andThen = then


def async_(fn, *args, **kwargs):
    delay = kwargs.pop("delay", 0)
    return Future(lambda: fn(*args, **kwargs), delay)


runAsync = async_


class Promise(object):
    def __init__(self, *a):
        self._f = Future()

    def setValue(self, v):
        self._f._set(v)

    def setError(self, e):
        self._f._set(error=str(e))

    def setCanceled(self):
        self._f.cancel()

    def future(self):
        return self._f


class PeriodicTask(object):
    def __init__(self):
        self._cb, self._period, self._stop, self._thread = None, 1.0, threading.Event(), None

    def setCallback(self, cb):
        self._cb = cb

    def setUsPeriod(self, us):
        self._period = us / 1e6

    def start(self, immediate=True):
        self._stop.clear()

        def loop():
            if not immediate:
                self._stop.wait(self._period)
            while not self._stop.is_set():
                self._cb()
                self._stop.wait(self._period)

        self._thread = threading.Thread(target=loop, daemon=True)
        self._thread.start()

    def stop(self):
        self._stop.set()

    asyncStop = stop

    def isRunning(self):
        return self._thread is not None and self._thread.is_alive() and not self._stop.is_set()


class Signal(object):
    def __init__(self, *a):
        self._cbs = {}

    def connect(self, cb):
        sid = len(self._cbs) + 1
        self._cbs[sid] = cb
        return sid

    def disconnect(self, sid):
        self._cbs.pop(sid, None)

    def __call__(self, *args):
        for cb in list(self._cbs.values()):
            cb(*args)


class _Subscriber(object):
    """ALMemory.subscriber(key): a local object whose signal fires on every change of `key`."""

    def __init__(self, key):
        self.signal = Signal()
        rpc.memory_subscribers.setdefault(key, []).append(self.signal)


# (service, task id) of calls started with pCall, so wait/stop/isRunning on those ids go to the
# task instead of to a service method of the same name (ALAudioPlayer.stop(id)). Ids stay known
# after the task ends: isRunning is then False.
_tasks = set()


class ServiceProxy(object):
    def __init__(self, name):
        self._name = name

    def pCall(self, method, *args):
        """ALModule.pCall: starts `method` in the background and returns its task id."""
        task = rpc.request("post", service=self._name, method=method, args=list(args))
        _tasks.add((self._name, task))
        return task

    def wait(self, *args):
        if args and (self._name, args[0]) in _tasks:
            return rpc.request("wait", task=args[0], timeout=args[1] if len(args) > 1 else 0)
        return rpc.call(self._name, "wait", args)

    def stop(self, *args):
        if args and (self._name, args[0]) in _tasks:
            rpc.send({"op": "stopTask", "task": args[0]})
            return None
        return rpc.call(self._name, "stop", args)

    def isRunning(self, *args):
        if args and (self._name, args[0]) in _tasks:
            return rpc.request("isRunning", task=args[0])
        return rpc.call(self._name, "isRunning", args)

    def __getattr__(self, method):
        if method.startswith("__"):
            raise AttributeError(method)
        if self._name == "ALMemory" and method == "subscriber":
            return _Subscriber

        def call(*args, **kwargs):
            if kwargs.pop("_async", False):
                return async_(lambda: rpc.call(self._name, method, args))
            return rpc.call(self._name, method, args)

        call.__name__ = method
        return call


class Session(object):
    _default = None

    @classmethod
    def default(cls):
        if cls._default is None:
            cls._default = Session()
        return cls._default

    def connect(self, *a):
        pass

    def isConnected(self):
        return True

    def service(self, name):
        return ServiceProxy(name)

    def services(self):
        return []

    def registerService(self, *a):
        return 0

    def close(self):
        pass


class Application(object):
    def __init__(self, *a, **k):
        self.session = Session.default()

    def start(self):
        pass

    def run(self):
        while True:
            time.sleep(3600)

    def stop(self):
        pass


def multiThreaded(*a):
    return (lambda cls: cls) if not a or not callable(a[0]) else a[0]


singleThreaded = multiThreaded


def bind(*a, **k):
    return (lambda f: f) if not a or not callable(a[0]) else a[0]


nobind = bind
Void = None
