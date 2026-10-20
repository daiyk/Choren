"""Small clean-room subset of `almath`."""
import math

PI = math.pi
TO_RAD = math.pi / 180.0
TO_DEG = 180.0 / math.pi


class Pose2D(object):
    def __init__(self, x=0.0, y=0.0, theta=0.0):
        if isinstance(x, (list, tuple)):
            x, y, theta = x
        self.x, self.y, self.theta = x, y, theta

    def toVector(self):
        return [self.x, self.y, self.theta]


def clip(value, lo, hi):
    return max(lo, min(hi, value))


def modulo2PI(angle):
    return math.atan2(math.sin(angle), math.cos(angle))
