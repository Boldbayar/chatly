import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGETS = ["aarch64-apple-darwin", "x86_64-apple-darwin"]


def metadata(target):
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target],
        cwd=ROOT / "src-tauri",
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(out)


def reachable(m):
    pkgs = {p["id"]: p for p in m["packages"]}
    nodes = {n["id"]: n for n in m["resolve"]["nodes"]}
    root = m["resolve"]["root"]
    seen, stack = set(), [root]
    while stack:
        i = stack.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            if any(k["kind"] != "dev" for k in d["dep_kinds"]):
                stack.append(d["pkg"])
    seen.discard(root)
    return {(pkgs[i]["name"], pkgs[i]["version"]): pkgs[i] for i in seen}

crates = {}
for target in TARGETS:
    crates.update(reachable(metadata(target)))

def norm(lic):
    return (lic or "see crate").replace("/", " OR ").replace("  ", " ")

rows = sorted(crates.values(), key=lambda p: (p["name"].lower(), p["version"]))
out = []
out.append("# Third-party licenses\n")
out.append("Chatly is MIT licensed (see [LICENSE](LICENSE)). It is built from the Rust crates listed below, each under its own license. Every license here is compatible with MIT.\n")
out.append("The source of each crate, including its full license text, is on [crates.io](https://crates.io) at the listed version. If you **distribute a built Chatly.app**, include this file and the license texts. [`cargo about`](https://github.com/EmbarkStudios/cargo-about) can collect the full texts for you.\n")
out.append("Licenses that need a note:\n")
out.append("- **MPL-2.0** (`cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`, `selectors`): file-level copyleft. Chatly uses them unmodified. If you change one of these files, you must publish your changed version of that file under MPL-2.0.")
out.append("- **CDLA-Permissive-2.0** (`webpki-roots`): Mozilla's list of root certificates, used by the Work laptop check. When you redistribute it, include the license text.")
out.append("- **Unicode-3.0** (`icu_*` crates): Unicode data. The copyright notice must be kept.\n")
out.append("This list covers Apple-silicon and Intel Macs, without dev-only dependencies. To regenerate it after changing dependencies, follow [CONTRIBUTING.md](CONTRIBUTING.md#updating-dependencies).\n")
out.append(f"## Crates ({len(rows)})\n")
out.append("| Crate | Version | License |")
out.append("|---|---|---|")
for p in rows:
    name = p["name"]
    out.append(f"| [{name}](https://crates.io/crates/{name}) | {p['version']} | {norm(p.get('license'))} |")
(ROOT / "THIRD_PARTY_LICENSES.md").write_text("\n".join(out) + "\n")
print(f"Wrote THIRD_PARTY_LICENSES.md ({len(rows)} crates)")
