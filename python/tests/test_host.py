"""python3 -m unittest discover -s python/tests (from the repo root)."""
import glob
import os
import sys
import unittest
import warnings
import xml.etree.ElementTree as ET
from unittest.mock import Mock

ROOT = os.path.join(os.path.dirname(__file__), "..", "..")
sys.path.insert(0, os.path.join(ROOT, "python"))
from choren_host import py2fix  # noqa: E402

SHARE = os.environ.get("CHOREGRAPHE_SHARE", os.path.join(ROOT, "Choregraphe 2.8.8/Choregraphe.app/Contents/Resources/choregraphe-2.8.8-mac/share/choregraphe"))


def scripts(pattern):
    for f in glob.glob(pattern, recursive=True):
        for el in ET.parse(f).iter():
            if el.tag.split("}")[-1] == "content" and "GeneratedClass" in (el.text or ""):
                yield f, el.text


class Py2Fix(unittest.TestCase):
    def test_rewrites(self):
        src = 'print "a", x\nprint\ntry:\n    pass\nexcept Exception, e:\n    raise ValueError, "no"\nf = qi.async(g, delay=1)\nd.has_key(k)\ns = "print x except A, b"\nn = 0777 + 10L\n'
        out = py2fix.fix(src)
        compile(out, "t", "exec")
        self.assertIn('print("a", x)', out)
        self.assertIn("except Exception as e:", out)
        self.assertIn('raise ValueError("no")', out)
        self.assertIn(".async_(", out)
        self.assertIn('s = "print x except A, b"', out)
        self.assertIn("0o777 + 10", out)

    def test_tabs_and_first_line_indent(self):
        compile(py2fix.fix("    class A:\n\tdef f(self):\n\t    return 1\n"), "t", "exec")

    @unittest.skipUnless(os.path.isdir(SHARE), "Choregraphe install not available")
    def test_every_library_script_compiles(self):
        warnings.simplefilter("ignore", SyntaxWarning)
        failed = []
        count = 0
        for f, src in scripts(os.path.join(SHARE, "**/*.xar")):
            count += 1
            try:
                compile(py2fix.fix(src), f, "exec")
            except SyntaxError as e:
                # These two deprecated boxes miss a comma in a dict literal; Python 2 rejects them too.
                if "deprecated/Speech/Creation/" not in f:
                    failed.append("%s: %s" % (f.split("box/")[-1], e))
        self.assertGreater(count, 500)
        self.assertEqual(failed, [])

    def test_choren_library_compiles_unchanged(self):
        for f, src in scripts(os.path.join(ROOT, "library/choren/**/box.xar")):
            compile(src.lstrip(), f, "exec")

    def test_say_unload_stops_pending_speech(self):
        f, src = next(scripts(os.path.join(ROOT, "library/choren/Speech/Creation/Say/box.xar")))
        namespace = {"GeneratedClass": object}
        exec(compile(src.lstrip(), f, "exec"), namespace)
        box = namespace["MyClass"]()
        box.tts = Mock(spec_set=["stopAll"])
        box.pending = object()
        box.onUnload()
        self.assertIsNone(box.pending)
        box.onUnload()
        box.tts.stopAll.assert_called_once_with()

    def test_prelude_matches_naoqi_globals(self):
        sys.path.insert(0, os.path.join(ROOT, "python/choren_host/shims"))
        from choren_host import host

        namespace = {}
        exec(host.PRELUDE, namespace)
        exec("time.sleep(0); ALProxy; qi.Session; os.path; sys.path", namespace)


if __name__ == "__main__":
    unittest.main()
