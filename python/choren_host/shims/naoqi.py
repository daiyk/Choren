"""Clean-room subset of the legacy `naoqi` module (ALProxy, ALModule, ALBroker)."""
from choren_host import rpc


class _Post(object):
    def __init__(self, name):
        self._name = name

    def __getattr__(self, method):
        if method.startswith("__"):
            raise AttributeError(method)
        return lambda *args: rpc.request("post", service=self._name, method=method, args=list(args))


class ALProxy(object):
    def __init__(self, name, *address):
        self._name = name
        self.post = _Post(name)

    def __getattr__(self, method):
        if method.startswith("__"):
            raise AttributeError(method)
        return lambda *args: rpc.call(self._name, method, args)

    def pCall(self, method, *args):
        return getattr(self.post, method)(*args)

    def wait(self, task, timeout=0):
        return rpc.request("wait", task=task, timeout=timeout)

    def stop(self, task):
        rpc.send({"op": "stopTask", "task": task})

    def isRunning(self, task):
        return rpc.request("isRunning", task=task)

    @staticmethod
    def initProxies(*a):
        pass


class ALModule(object):
    def __init__(self, name):
        self._module_name = name
        rpc.modules[name] = self

    def getName(self):
        return self._module_name

    def BIND_PYTHON(self, *a):
        pass


class ALBroker(object):
    def __init__(self, *a):
        pass

    def shutdown(self):
        pass
