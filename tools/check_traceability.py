#!/usr/bin/env python3
"""R11: requirement-to-test traceability check.

Every `<crate>::<module path>::<test fn>` reference in the `verification`
column of docs/requirements/requirements.csv must name a test function that
exists in crates/<crate>/src. Rows with status VERIFIED must reference at
least one automated test or an evidence file under docs/evidence/.

Exit 0 = consistent, 1 = broken reference. Run from the repository root.
"""
import csv
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
REGISTER = ROOT / "docs" / "requirements" / "requirements.csv"
TEST_REF = re.compile(r"\b([a-z0-9][a-z0-9-]*)::((?:[a-z0-9_]+::)*)([a-z0-9_]+)\b")


def module_path_of(src: pathlib.Path, path: pathlib.Path) -> str:
    """Rust module path of a source file: main.rs/lib.rs -> '', a/mod.rs -> 'a', a/b.rs -> 'a::b'."""
    parts = list(path.relative_to(src).with_suffix("").parts)
    if parts[-1] in ("main", "lib", "mod"):
        parts = parts[:-1]
    return "::".join(parts)


def test_exists(crate: str, module_path: str, name: str) -> bool:
    """True when crates/<crate>/src has `fn name` at exactly <module_path>.

    The module path is resolved strictly: a file prefix (from the file layout)
    followed by inline `mod X` blocks inside that file. A test that moved to
    another module therefore fails until the register is updated.
    """
    src = ROOT / "crates" / crate / "src"
    if not src.is_dir():
        return False
    wanted = [m for m in module_path.split("::") if m]
    fn = re.compile(rf"\bfn\s+{re.escape(name)}\s*\(")
    for path in src.rglob("*.rs"):
        file_mod = [m for m in module_path_of(src, path).split("::") if m]
        if wanted[: len(file_mod)] != file_mod:
            continue
        inline = wanted[len(file_mod):]
        text = path.read_text(encoding="utf-8")
        if fn.search(text) and all(re.search(rf"\bmod\s+{m}\s*\{{", text) for m in inline):
            return True
    return False


def main() -> int:
    errors, checked = [], 0
    with REGISTER.open(newline="", encoding="utf-8") as handle:
        lines = [line for line in handle if line.strip()]
    for row in csv.DictReader(lines):
        rid = row["requirement_id"]
        refs = TEST_REF.findall(row.get("verification", ""))
        for crate, module_path, name in refs:
            checked += 1
            if not test_exists(crate, module_path, name):
                errors.append(f"{rid}: test not found: {crate}::{module_path}{name}")
        if row.get("status") == "VERIFIED":
            evidence = row.get("evidence", "")
            if not refs and "docs/evidence/" not in evidence:
                errors.append(f"{rid}: VERIFIED without an automated test or docs/evidence/ reference")
            for ref in re.findall(r"docs/evidence/[\w.\-/]+", evidence):
                if not (ROOT / ref).exists():
                    errors.append(f"{rid}: evidence file missing: {ref}")
    for error in errors:
        print(f"[TRACE] FAIL {error}")
    print(f"[TRACE] {checked} test references checked, {len(errors)} problem(s)")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
