"""Rewrites the Python 2-only syntax found in Choregraphe box scripts so Python 3 can run them.

Line numbers are preserved. String literals are masked first so their contents are never touched.
Integer division and str/bytes semantics cannot be fixed this way and stay Python 3 behaviour.
"""
import re

_STRING = re.compile(r"""(?:[rRuUbB]{0,2})(?:'''[\s\S]*?'''|\"\"\"[\s\S]*?\"\"\"|'(?:\\.|[^'\\\n])*'|"(?:\\.|[^"\\\n])*")""")
_FIXES = [
    (re.compile(r"\bexcept\s+([^,:\n]+?)\s*,\s*(\w+)\s*:"), r"except \1 as \2:"),
    (re.compile(r"^(\s*)raise\s+([\w.]+)\s*,\s*(.+)$", re.M), r"\1raise \2(\3)"),
    (re.compile(r"\.async\("), ".async_("),
    (re.compile(r"\.iteritems\(\)"), ".items()"),
    (re.compile(r"\.iterkeys\(\)"), ".keys()"),
    (re.compile(r"\.itervalues\(\)"), ".values()"),
    (re.compile(r"\.has_key\("), ".__contains__("),
    (re.compile(r"<>"), "!="),
    (re.compile(r"\b(\d+)[lL]\b"), r"\1"),
    (re.compile(r"(?<![\w.])0([0-7]+)\b"), r"0o\1"),
    (re.compile(r"`([^`\n]+)`"), r"repr(\1)"),
]
_PRINT = re.compile(r"^(\s*)print\b(?!\s*\()\s*(>>\s*([^,]+),\s*)?(.*?)(,?)\s*$")
_EXEC = re.compile(r"^(\s*)exec\s+(?!\()(.+)$")


def _indent(line):
    """Python 2 reads a tab as advancing to the next multiple of 8 columns."""
    stripped = line.lstrip(" \t")
    return line[: len(line) - len(stripped)].expandtabs(8) + stripped


def fix(source):
    lines = source.replace("\r\n", "\n").split("\n")
    # Choregraphe's pretty printer can indent the first line of the CDATA.
    while lines and not lines[0].strip():
        lines.pop(0)
    if lines and lines[0][:1] in (" ", "\t"):
        pad = len(lines[0]) - len(lines[0].lstrip())
        if all(not l.strip() or l[:pad].strip() == "" for l in lines):
            lines = [l[pad:] for l in lines]
        else:
            lines[0] = lines[0].lstrip()  # only the first line carries the XML indentation
    source = "\n".join(_indent(l) for l in lines)
    strings = []

    def mask(m):
        strings.append(m.group(0))
        return "\x00%d\x00" % (len(strings) - 1)

    code = _STRING.sub(mask, source)
    for pattern, repl in _FIXES:
        code = pattern.sub(repl, code)
    out = []
    for line in code.split("\n"):
        m = _PRINT.match(line)
        if m and not line.strip().startswith("print("):
            indent, _, target, body, comma = m.groups()
            args = [body] if body else []
            if comma:
                args.append('end=" "')
            if target:
                args.append("file=" + target.strip())
            line = "%sprint(%s)" % (indent, ", ".join(args))
        m = _EXEC.match(line)
        if m:
            line = "%sexec(%s)" % (m.group(1), m.group(2))
        out.append(line)
    code = "\n".join(out)
    return re.sub("\x00(\\d+)\x00", lambda m: strings[int(m.group(1))], code)
