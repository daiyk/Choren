"""The box API: `ALBehavior` and the per-box `GeneratedClass`."""
import logging
import threading

from choren_host import rpc


class _Timeline(object):
    """A timeline box's playhead, run by the robot's runtime."""

    def __init__(self, box, parent):
        self._box = box
        self._parent = parent

    def _do(self, action, value=None):
        return rpc.request("timeline", box=self._box._record.id, parent=self._parent, action=action, value=value)

    def play(self):
        self._do("play")

    def pause(self):
        self._do("pause")

    def stop(self):
        self._do("stop")

    def goTo(self, frame):
        """A frame number, or the name of a behavior keyframe."""
        self._do("goTo", frame)

    def getSize(self):
        return self._do("size")

    def getFPS(self):
        return self._do("fps")

    def setFPS(self, fps):
        self._do("setFPS", fps)


class _BoxLogHandler(logging.Handler):
    def __init__(self, name):
        logging.Handler.__init__(self)
        self.box = name

    def emit(self, record):
        level = {logging.DEBUG: "debug", logging.INFO: "info", logging.WARNING: "warning"}.get(record.levelno, "error")
        rpc.log(level, "[%s] %s" % (self.box, record.getMessage()), "behavior.box")


class ALBehavior(object):
    """Base of every box class. A subclass per box adds one method per output."""

    _record = None

    def __init__(self, *args, **kwargs):
        self.logger = logging.getLogger("behavior.box.%s" % self._record.uri)
        if not self.logger.handlers:
            self.logger.addHandler(_BoxLogHandler(self._record.name))
            self.logger.setLevel(logging.DEBUG)
            self.logger.propagate = False
        rpc.modules[self.getName()] = self

    # --- identity and context
    def getName(self):
        return self._record.uri

    def session(self):
        import qi

        return qi.Session.default()

    def behaviorAbsolutePath(self):
        return self._record.path

    def behaviorRelativePath(self):
        return "."

    def packageUid(self):
        return self._record.uri.split("/")[0]

    def getBrokerName(self):
        return "ChorenVirtualRobot"

    def version(self):
        return "2.8.6"

    # --- parameters
    def getParameter(self, name):
        if name not in self._record.params:
            raise RuntimeError('Parameter "%s" does not exist.' % name)
        return self._record.params[name]

    def setParameter(self, name, value):
        self._record.params[name] = value

    def getParametersList(self):
        return list(self._record.params)

    # --- flow
    def stimulateIO(self, name, value=None):
        rpc.send({"op": "out", "box": self._record.id, "port": name, "value": value})

    def exit(self):
        rpc.send({"op": "exit", "box": self._record.id})

    def _safeCallOfUserMethod(self, name, *args):
        try:
            getattr(self, name)(*args)
            return True
        except Exception as e:  # reported like Choregraphe's _reportError
            import traceback

            if hasattr(self, "onError") and name != "onError":
                try:
                    self.onError(str(e))
                    return False
                except Exception:
                    pass
            rpc.send({"op": "error", "box": self._record.id, "name": self._record.name, "message": "%s\n%s" % (e, traceback.format_exc())})
            return False

    def __onLoad__(self):
        if hasattr(self, "onLoad"):
            self._safeCallOfUserMethod("onLoad")

    def __onUnload__(self):
        if hasattr(self, "onUnload"):
            self._safeCallOfUserMethod("onUnload")

    # --- resources: the runtime acquires a box's declared resources before onStart and
    # releases them when the box stops (crates/choren-naoqi/src/resources.rs)
    def setResources(self, resources, timeout=0):
        # shortcut: resources added from a script are not arbitrated; declare them on the box.
        return True

    def acquireResources(self):
        return bool(rpc.request("acquire", box=self._record.id))

    waitResources = waitResourceFree = acquireResources

    def waitFor(self, *args):
        return True

    def isResourceFree(self, resources):
        if isinstance(resources, str):
            resources = [resources]
        return bool(rpc.request("resourceFree", resources=list(resources)))

    def waitResourcesCallback(self, callback):
        return True

    def releaseResource(self):
        rpc.send({"op": "release", "box": self._record.id})

    releaseResources = releaseResource

    def __onResource__(self, name):
        """Callback on demand: another box wants `name`. Without onResource the box gives it up."""
        if hasattr(self, "onResource"):
            self._safeCallOfUserMethod("onResource", name)
        else:
            self.releaseResource()

    def __onResourceError__(self):
        if hasattr(self, "onResourceError"):
            self._safeCallOfUserMethod("onResourceError")
        for out in self._record.outputs:
            if out.get("nature") == 1:
                self.stimulateIO(out["name"])

    # --- timeline access
    def hasTimeline(self):
        return self._record.timeline

    def getTimeline(self):
        return _Timeline(self, False)

    def hasParentTimeline(self):
        return self._record.parent_timeline

    def getParentTimeline(self):
        return _Timeline(self, True)

    # --- deprecated helpers kept for old boxes
    def log(self, message):
        self.logger.info(message)

    def playTimeline(self, parent=False):
        _Timeline(self, parent).play()

    def stopTimeline(self, parent=False):
        _Timeline(self, parent).stop()

    def exitBehavior(self):
        rpc.send({"op": "stopBehavior"})

    def gotoAndStop(self, frame, parent=False):
        timeline = _Timeline(self, parent)
        timeline.goTo(frame)
        timeline.pause()

    def gotoAndPlay(self, frame, parent=False):
        timeline = _Timeline(self, parent)
        timeline.goTo(frame)
        timeline.play()

    def playTimelineParent(self):
        self.playTimeline(True)

    def stopTimelineParent(self):
        self.stopTimeline(True)

    def gotoAndStopParent(self, frame):
        self.gotoAndStop(frame, True)

    def gotoAndPlayParent(self, frame):
        self.gotoAndPlay(frame, True)

    # --- legacy no-ops
    def connectInput(self, *a):
        pass

    connectOutput = connectParameter = setParentFromName = connectInput

    def addInput(self, *a):
        pass

    addOutput = addParameter = BIND_PYTHON = addInput


def generated_class(record):
    """Builds the box's GeneratedClass: ALBehavior plus one method per output."""

    def output(name):
        def emit(self, p=None):
            self.stimulateIO(name, p)

        emit.__name__ = str(name)
        return emit

    attrs = {"_record": record}
    for out in record.outputs:
        attrs[str(out["name"])] = output(out["name"])
    return type("GeneratedClass", (ALBehavior,), attrs)
