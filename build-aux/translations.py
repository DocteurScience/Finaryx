#!/usr/bin/env python3
"""Translation table of Finaryx: po/translations.csv <-> po/*.po.

The table has one row per message (three rows for a message with plural
forms, one per form) and one column per language. Translators fill it in a
spreadsheet; this script turns it into the gettext files that Meson builds.

    meson compile -C build finaryx-pot           # refresh po/finaryx.pot
    python3 build-aux/translations.py update     # add new messages to the table
    python3 build-aux/translations.py po         # write po/<lang>.po and po/LINGUAS
    python3 build-aux/translations.py check      # placeholders, coverage (meson test)
    python3 build-aux/translations.py stats      # progress per language

Columns: msgid, msgid_plural, form, comment, then one per language.
- Source language: French (msgid). Leave a cell empty to keep the French.
- Plural rows: "form" is 0, 1 or 2; each language uses the forms of its
  plural rule (LANGUAGES below: "1 | 2+" means form 0 for 1, form 1 for 2
  and more).
- Keep the placeholders ({n}, {month}…) and the markup (<b>…</b>).
"""

import csv
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PO = ROOT / "po"
TABLE = PO / "translations.csv"
POT = PO / "finaryx.pot"
LOCALE_RS = ROOT / "core" / "src" / "locale.rs"

# code: (name, Plural-Forms, forms in plain words)
LANGUAGES = {
    "en": ("English", "nplurals=2; plural=(n != 1);", "1 | 0, 2+"),
    "de": ("Deutsch", "nplurals=2; plural=(n != 1);", "1 | 0, 2+"),
    "es": ("Español", "nplurals=2; plural=(n != 1);", "1 | 0, 2+"),
    "it": ("Italiano", "nplurals=2; plural=(n != 1);", "1 | 0, 2+"),
    "pt_BR": ("Português (Brasil)", "nplurals=2; plural=(n > 1);", "0, 1 | 2+"),
    "pt_PT": ("Português (Portugal)", "nplurals=2; plural=(n != 1);", "1 | 0, 2+"),
    "nl": ("Nederlands", "nplurals=2; plural=(n != 1);", "1 | 0, 2+"),
    "pl": (
        "Polski",
        "nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);",
        "1 | 2–4, 22–24… | 0, 5–21…",
    ),
    "cs": ("Čeština", "nplurals=3; plural=(n==1) ? 0 : (n>=2 && n<=4) ? 1 : 2;", "1 | 2–4 | 0, 5+"),
    "sv": ("Svenska", "nplurals=2; plural=(n != 1);", "1 | 0, 2+"),
    "da": ("Dansk", "nplurals=2; plural=(n != 1);", "1 | 0, 2+"),
    "nb": ("Norsk bokmål", "nplurals=2; plural=(n != 1);", "1 | 0, 2+"),
    "fi": ("Suomi", "nplurals=2; plural=(n != 1);", "1 | 0, 2+"),
    "uk": (
        "Українська",
        "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);",
        "1, 21… | 2–4, 22–24… | 0, 5–20…",
    ),
    "tr": ("Türkçe", "nplurals=2; plural=(n != 1);", "1 | 0, 2+"),
    "ja": ("日本語", "nplurals=1; plural=0;", "all"),
    "zh_CN": ("简体中文", "nplurals=1; plural=0;", "all"),
}
FIXED = ["msgid", "msgid_plural", "form", "comment"]
# Message whose translation is the language code (see core/src/locale.rs).
CODE_MSGID = "fr"
PLACEHOLDER = re.compile(r"\{[a-z_]+\}")
TAG = re.compile(r"</?[a-z]+>")


def nplurals(code: str) -> int:
    return int(re.search(r"nplurals=(\d)", LANGUAGES[code][1]).group(1))


def unquote(chunks: str) -> str:
    """Joins the quoted lines of a .po field and decodes its escapes."""
    text = "".join(re.findall(r'"((?:[^"\\]|\\.)*)"', chunks))
    escapes = {"n": "\n", "t": "\t", '"': '"', "\\": "\\"}
    return re.sub(r"\\(.)", lambda m: escapes.get(m.group(1), m.group(1)), text)


def read_pot() -> list[dict]:
    """Messages of the template, in order, without the header."""
    entries = []
    for block in POT.read_text(encoding="utf-8").split("\n\n"):
        msgid = re.search(r'^msgid ((?:".*"\n?)+)', block, re.M)
        if not msgid:
            continue
        text = unquote(msgid.group(1))
        if not text:
            continue  # header
        plural = re.search(r'^msgid_plural ((?:".*"\n?)+)', block, re.M)
        comments = [l[3:] for l in block.splitlines() if l.startswith("#. ")]
        refs = [l[3:] for l in block.splitlines() if l.startswith("#: ")]
        entries.append(
            {
                "msgid": text,
                "msgid_plural": unquote(plural.group(1)) if plural else "",
                "comment": " ".join(comments + refs[:1]),
            }
        )
    return entries


def read_table() -> list[dict]:
    if not TABLE.exists():
        return []
    with TABLE.open(encoding="utf-8", newline="") as f:
        return list(csv.DictReader(f))


def write_table(rows: list[dict]) -> None:
    with TABLE.open("w", encoding="utf-8", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=FIXED + list(LANGUAGES), lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow({k: row.get(k, "") for k in FIXED + list(LANGUAGES)})


def key(row: dict) -> tuple:
    return (row["msgid"], row["msgid_plural"], row["form"])


def update() -> int:
    if not POT.exists():
        print("po/finaryx.pot is missing: run `meson compile -C build finaryx-pot`.")
        return 1
    old = {key(r): r for r in read_table()}
    rows = []
    for e in read_pot():
        if e["msgid"] == CODE_MSGID:
            continue
        forms = ["0", "1", "2"] if e["msgid_plural"] else [""]
        for form in forms:
            row = {"msgid": e["msgid"], "msgid_plural": e["msgid_plural"], "form": form}
            previous = old.pop(key(row), {})
            row["comment"] = e["comment"]
            for code in LANGUAGES:
                row[code] = previous.get(code, "")
            rows.append(row)
    write_table(rows)
    lost = sum(1 for r in old.values() if any(r.get(c) for c in LANGUAGES))
    print(f"{TABLE.relative_to(ROOT)}: {len(rows)} rows; {lost} translated rows no longer used")
    return 0


def po_string(text: str) -> str:
    text = text.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")
    return f'"{text}"'


def write_po() -> int:
    rows = read_table()
    messages: dict[tuple, dict] = {}
    for r in rows:
        messages.setdefault((r["msgid"], r["msgid_plural"]), {})[r["form"]] = r
    linguas = []
    for code, (name, plural_forms, _) in LANGUAGES.items():
        if not any(r.get(code) for r in rows):
            (PO / f"{code}.po").unlink(missing_ok=True)
            continue
        linguas.append(code)
        out = [
            "# Finaryx — " + name,
            "# Generated from po/translations.csv by build-aux/translations.py: edit the table.",
            'msgid ""',
            'msgstr ""',
            '"Project-Id-Version: finaryx\\n"',
            '"PO-Revision-Date: \\n"',
            '"Last-Translator: \\n"',
            f'"Language-Team: {name}\\n"',
            f'"Language: {code}\\n"',
            '"MIME-Version: 1.0\\n"',
            '"Content-Type: text/plain; charset=UTF-8\\n"',
            '"Content-Transfer-Encoding: 8bit\\n"',
            f'"Plural-Forms: {plural_forms}\\n"',
            "",
            f"msgid {po_string(CODE_MSGID)}",
            f"msgstr {po_string(code)}",
            "",
        ]
        n = nplurals(code)
        for (msgid, plural), forms in messages.items():
            if plural:
                values = [forms.get(str(i), {}).get(code, "") for i in range(n)]
                if not all(values):
                    values = [""] * n  # incomplete: keep the French
                out.append(f"msgid {po_string(msgid)}")
                out.append(f"msgid_plural {po_string(plural)}")
                for i, v in enumerate(values):
                    out.append(f"msgstr[{i}] {po_string(v)}")
            else:
                out.append(f"msgid {po_string(msgid)}")
                out.append(f"msgstr {po_string(forms[''].get(code, ''))}")
            out.append("")
        (PO / f"{code}.po").write_text("\n".join(out), encoding="utf-8")
    (PO / "LINGUAS").write_text("".join(f"{c}\n" for c in linguas), encoding="utf-8")
    print("po/LINGUAS: " + (" ".join(linguas) or "(none)"))
    return 0


def check() -> int:
    errors = []
    codes = set(re.findall(r'code: "([a-zA-Z_]+)"', LOCALE_RS.read_text(encoding="utf-8")))
    if codes != set(LANGUAGES) | {"fr"}:
        errors.append(f"languages differ from core/src/locale.rs: {sorted(codes ^ (set(LANGUAGES) | {'fr'}))}")
    rows = read_table()
    if POT.exists():
        wanted = {(e["msgid"], e["msgid_plural"]) for e in read_pot() if e["msgid"] != CODE_MSGID}
        have = {(r["msgid"], r["msgid_plural"]) for r in rows}
        if wanted - have:
            errors.append(f"{len(wanted - have)} messages missing from the table: run `update`")
    for r in rows:
        source = r["msgid_plural"] if r["form"] not in ("", "0") else r["msgid"]
        expected = set(PLACEHOLDER.findall(r["msgid"])) | set(PLACEHOLDER.findall(r["msgid_plural"]))
        for code in LANGUAGES:
            text = r.get(code, "")
            if not text:
                continue
            if r["form"] and int(r["form"]) >= nplurals(code):
                errors.append(f"{code}: form {r['form']} unused by this language: {r['msgid'][:50]}")
            found = set(PLACEHOLDER.findall(text))
            # A singular may omit {n} ("Une opération"); nothing else may differ.
            if found - expected or (expected - found - {"{n}"}):
                errors.append(f"{code}: placeholders {sorted(found)} ≠ {sorted(expected)}: {source[:60]}")
            if TAG.findall(text) != TAG.findall(source):
                errors.append(f"{code}: markup differs: {source[:60]}")
    for e in errors:
        print(e)
    print(f"check: {len(errors)} problem(s)")
    return 1 if errors else 0


def stats() -> int:
    rows = read_table()
    messages = {(r["msgid"], r["msgid_plural"]) for r in rows}
    for code, (name, _, forms) in LANGUAGES.items():
        n = nplurals(code)
        done = 0
        for msgid, plural in messages:
            cells = [r.get(code, "") for r in rows if (r["msgid"], r["msgid_plural"]) == (msgid, plural)]
            cells = cells[:n] if plural else cells
            done += all(cells)
        print(f"{code:6} {name:22} {done:4}/{len(messages)}  {100 * done // max(len(messages), 1):3} %  pluriels : {forms}")
    return 0


if __name__ == "__main__":
    commands = {"update": update, "po": write_po, "check": check, "stats": stats}
    if len(sys.argv) != 2 or sys.argv[1] not in commands:
        print(__doc__)
        sys.exit(2)
    sys.exit(commands[sys.argv[1]]())
