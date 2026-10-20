"""Choren's Python 3 box host: runs Choregraphe box scripts for the virtual robot.

Clean-room implementation of the box API (GeneratedClass/ALBehavior) and of the `qi`, `naoqi`
and `almath` modules that box scripts import. It talks to the Rust behavior runtime with JSON
lines: requests on stdin, events and service calls on the original stdout.
"""
