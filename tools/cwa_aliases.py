"""
Adds custom texture aliases to Clone Wars Adventures wearable models.

    python tools/cwa_aliases.py "C:\\Games\\Clone_Wars\\Clone_Wars"

Reads config/custom_aliases.yaml (next to this repo's config):

    - adr: Wear_Human_<gender>_Body_CloneBasicArmor.adr   # model(s) to extend
      alias: SGTBoomer                               # new texture_alias name
      texture: Wear_Human_Male_Body_CloneBasicArmor_SGTBoomer_C.dds
      source: custom_armor/SGTBoomer_Body.dds         # optional: file to copy in
                                                     # as `texture` (relative to
                                                     # the config dir or absolute)

For every model listed, the ORIGINAL .adr is read from the client's Assets*.pack
files (never from a previously written loose copy), the aliases are appended, and
the result is written as a loose file in the game folder — the client prefers
loose files over packed ones, so the packs are never modified.  Re-running is
safe: it always rebuilds from the packed original.  Run with the game closed.

.adr format (reverse engineered; round-trips all 2205 Wear_*.adr files):
  sequence of sections: tag (1 byte), length, payload.
  length: 1 byte if < 0x80, else 2 bytes ((b0 & 0x7f) << 8 | b1).
  section 5 = texture aliases: optional first item tag 0xfe = header
  [01, 01, alias count] (absent on most single-alias models),
  then one tag 0x01 item per alias containing:
    0x03 (4 bytes, same for every alias)  0x04 alias name\\0
    0x05 texture file\\0                    0x06 (2 bytes)   [0x07 = default flag]
"""
import os
import shutil
import struct
import sys
import glob


def rlen(d, p):
    b = d[p]
    if b < 0x80:
        return b, p + 1
    return ((b & 0x7F) << 8) | d[p + 1], p + 2


def wlen(n):
    if n < 0x80:
        return bytes([n])
    assert n < 0x8000, n
    return bytes([0x80 | (n >> 8), n & 0xFF])


def parse(d):
    out, p = [], 0
    while p < len(d):
        tag = d[p]
        ln, q = rlen(d, p + 1)
        out.append([tag, d[q:q + ln]])
        p = q + ln
    assert p == len(d)
    return out


def dump(items):
    return b"".join(bytes([t]) + wlen(len(v)) + v for t, v in items)


def find_in_packs(game_dir, names):
    """Return {name: bytes} for the packed copies of `names`."""
    found = {}
    for f in sorted(glob.glob(os.path.join(game_dir, "Assets*.pack"))):
        with open(f, "rb") as fh:
            off = 0
            while True:
                fh.seek(off)
                hdr = fh.read(8)
                if len(hdr) < 8:
                    break
                nxt, cnt = struct.unpack(">II", hdr)
                data = fh.read(cnt * 300)
                p = 0
                for _ in range(cnt):
                    n = struct.unpack_from(">I", data, p)[0]
                    nm = data[p + 4:p + 4 + n].decode("latin1")
                    o, l, _c = struct.unpack_from(">III", data, p + 4 + n)
                    p += 4 + n + 12
                    if nm in names and nm not in found:
                        pos = fh.tell()
                        fh.seek(o)
                        found[nm] = fh.read(l)
                        fh.seek(pos)
                if nxt == 0:
                    break
                off = nxt
    return found


def add_aliases(adr_bytes, aliases):
    top = parse(adr_bytes)
    i5 = next(i for i, (t, _) in enumerate(top) if t == 5)
    items = parse(top[i5][1])
    # Single-alias models (e.g. the Neyo helmet) have no 0xFE count header; give
    # them the standard [01 01 count] one, which 478/478 headered models use.
    if items[0][0] != 0xFE:
        items.insert(0, [0xFE, b"\x01\x01\x00"])
    existing = {}
    template = None
    for t, v in items[1:]:
        sub = parse(v)
        name = next(sv for st, sv in sub if st == 4)[:-1].decode()
        existing[name] = sub
        if template is None:
            template = sub
    for alias, texture in aliases:
        if alias in existing:
            print(f"  note: alias {alias} already exists, replacing its texture")
            for item in existing[alias]:
                if item[0] == 5:
                    item[1] = texture.encode() + b"\0"
            continue
        sub = []
        for st, sv in template:
            if st == 4:
                sub.append([4, alias.encode() + b"\0"])
            elif st == 5:
                sub.append([5, texture.encode() + b"\0"])
            elif st == 7:
                continue  # default-alias flag stays on the original first alias
            else:
                sub.append([st, sv])
        existing[alias] = sub
    header = bytearray(items[0][1])
    header[-1] = len(existing)
    assert len(existing) < 256
    new_items = [[0xFE, bytes(header)]] + [[1, dump(sub)] for sub in existing.values()]
    top[i5][1] = dump(new_items)
    return dump(top)


def load_config(path):
    try:
        import yaml
    except ImportError:
        sys.exit("needs PyYAML: pip install pyyaml")
    with open(path, encoding="utf-8") as f:
        return yaml.safe_load(f) or []


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    game_dir = sys.argv[1]
    here = os.path.dirname(os.path.abspath(__file__))
    cfg_path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, "..", "config", "custom_aliases.yaml")
    cfg_dir = os.path.dirname(os.path.abspath(cfg_path))
    entries = load_config(cfg_path)

    by_adr = {}
    for e in entries:
        # `<gender>` in the model name covers both Male and Female models (like
        # item model_name).  Female wearables reuse the male textures (their
        # aliases point at Wear_Human_Male_*_C.dds), so `texture` stays as-is.
        genders = ("Male", "Female") if "<gender>" in e["adr"] else (None,)
        for g in genders:
            adr = e["adr"].replace("<gender>", g) if g else e["adr"]
            by_adr.setdefault(adr, []).append(e)
    originals = find_in_packs(game_dir, set(by_adr))

    for adr, es in by_adr.items():
        if adr not in originals:
            print(f"!! {adr} not found in any pack — skipped")
            continue
        copied = set()
        for e in es:
            if e["texture"] in copied:
                continue
            copied.add(e["texture"])
            if e.get("source"):
                src = e["source"] if os.path.isabs(e["source"]) else os.path.join(cfg_dir, e["source"])
                dst = os.path.join(game_dir, e["texture"])
                shutil.copyfile(src, dst)
                print(f"  texture {e['texture']} <- {src}")
            elif not os.path.exists(os.path.join(game_dir, e["texture"])):
                print(f"  warning: {e['texture']} is not in the game folder (and no source given)")
        out = add_aliases(originals[adr], [(e["alias"], e["texture"]) for e in es])
        assert parse(out)  # sanity: re-parses
        with open(os.path.join(game_dir, adr), "wb") as f:
            f.write(out)
        print(f"wrote {adr}: +{', '.join(e['alias'] for e in es)}")


if __name__ == "__main__":
    main()
