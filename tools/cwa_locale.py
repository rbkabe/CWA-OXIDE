"""
Rebuilds the Clone Wars Adventures client string table (locale/en_us_data.dat +
en_us_data.dir) with custom strings added.

    python tools/cwa_locale.py "C:\\Games\\Clone_Wars\\Clone_Wars"

Custom strings live in config/custom_strings.tsv (next to this repo's config):

    # name_id <TAB> text            -> stored as "Global.Text.<name_id>" (ucdt)
    62000	Bianca

The script is idempotent: it always starts from the pristine copies
(en_us_data.dat.orig / .dir.orig, created on first run) so re-running it with an
edited TSV just regenerates the files.  Run it with the game closed.

Format notes (reverse engineered from CloneWars.exe, loader at 0xcab820):
  * .dat: UTF-8 with BOM, CRLF lines "hash<TAB>type<TAB>text".
  * .dir: "## Key:<TAB>value" header lines, then "hash<TAB>offset<TAB>length<TAB>d",
    one per string.  offset/length point at the line in .dat (length excludes CRLF).
  * The loader reads the header keys Count (must equal the number of entries),
    Format and CidOutput.  MD5Checksum / TextLength are not checked.
  * Entries are looked up by hash, so .dir must be sorted by hash, AND each
    entry's offset must be strictly greater than the previous one (error 0x3404
    otherwise) -> the .dat lines must be stored in the same hash order.
  * hash = Bob Jenkins lookup2("Global.Text.<id>", initval 0).  Name ids above
    65535 can't be used by item definitions (u16 lookup on the client).
"""
import os
import re
import shutil
import sys


def _mix(a, b, c):
    m = 0xFFFFFFFF
    a = (a - b - c) & m; a ^= c >> 13
    b = (b - c - a) & m; b ^= (a << 8) & m
    c = (c - a - b) & m; c ^= b >> 13
    a = (a - b - c) & m; a ^= c >> 12
    b = (b - c - a) & m; b ^= (a << 16) & m
    c = (c - a - b) & m; c ^= b >> 5
    a = (a - b - c) & m; a ^= c >> 3
    b = (b - c - a) & m; b ^= (a << 10) & m
    c = (c - a - b) & m; c ^= b >> 15
    return a, b, c


def lookup2(key: str, init: int = 0) -> int:
    k = key.encode("utf-8")
    n = len(k)
    a = b = 0x9E3779B9
    c = init
    i = 0
    rem = n
    while rem >= 12:
        a = (a + int.from_bytes(k[i:i + 4], "little")) & 0xFFFFFFFF
        b = (b + int.from_bytes(k[i + 4:i + 8], "little")) & 0xFFFFFFFF
        c = (c + int.from_bytes(k[i + 8:i + 12], "little")) & 0xFFFFFFFF
        a, b, c = _mix(a, b, c)
        i += 12
        rem -= 12
    c = (c + n) & 0xFFFFFFFF
    t = k[i:] + b"\0" * 12
    a = (a + int.from_bytes(t[0:4], "little")) & 0xFFFFFFFF
    b = (b + int.from_bytes(t[4:8], "little")) & 0xFFFFFFFF
    c = (c + (int.from_bytes(t[8:11], "little") << 8)) & 0xFFFFFFFF
    return _mix(a, b, c)[2]


def text_hash(name_id: int) -> int:
    return lookup2("Global.Text.%d" % name_id)


assert text_hash(50591) == 1067817091  # "Kowakian Sith Robe"


def load_custom(tsv_path):
    out = []
    if not os.path.exists(tsv_path):
        return out
    with open(tsv_path, encoding="utf-8") as f:
        for ln, line in enumerate(f, 1):
            line = line.rstrip("\r\n")
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            parts = line.split("\t", 1)
            if len(parts) != 2 or not parts[0].strip().isdigit():
                sys.exit(f"{tsv_path}:{ln}: expected 'name_id<TAB>text'")
            out.append((int(parts[0]), parts[1]))
    return out


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    game_dir = sys.argv[1]
    here = os.path.dirname(os.path.abspath(__file__))
    tsv = sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, "..", "config", "custom_strings.tsv")

    loc = os.path.join(game_dir, "locale")
    dat_p, dir_p = os.path.join(loc, "en_us_data.dat"), os.path.join(loc, "en_us_data.dir")
    for p in (dat_p, dir_p):
        if not os.path.exists(p + ".orig"):
            shutil.copyfile(p, p + ".orig")
            print("saved pristine copy", p + ".orig")

    dat = open(dat_p + ".orig", "rb").read()
    dir_lines = open(dir_p + ".orig", "rb").read().split(b"\r\n")
    header = [l for l in dir_lines if l.startswith(b"##")]

    # Indexed entries from the original files, in file order: [hash, rest-of-line].
    # A few hashes legitimately appear twice (collisions), so this is a list.
    entries = []
    indexed = set()
    for l in dir_lines:
        if not l[:1].isdigit():
            continue
        h, off, ln, _kind = l.split(b"\t")
        end = dat.index(b"\r\n", int(off))
        line = dat[int(off):end]          # full line; may carry extra fields past `ln`
        hh, rest = line.split(b"\t", 1)
        assert int(hh) == int(h), (h, line[:40])
        # keep the indexed length relative to the line (collision lines index a prefix)
        entries.append([int(h), rest, int(ln) - len(hh) - 1])
        indexed.add(int(h))

    # Lines that exist in .dat but were never indexed (appended by hand) — keep them.
    for line in dat.lstrip(b"\xef\xbb\xbf").split(b"\r\n"):
        parts = line.split(b"\t", 1)
        if len(parts) == 2 and parts[0].isdigit() and int(parts[0]) not in indexed:
            entries.append([int(parts[0]), parts[1], len(parts[1])])
            indexed.add(int(parts[0]))

    custom = load_custom(tsv)
    for name_id, text in custom:
        h = text_hash(name_id)
        new_rest = b"ucdt\t" + text.encode("utf-8")
        existing = [e for e in entries if e[0] == h and e[1].split(b"\t", 1)[0].endswith(b"cdt")]
        if existing:
            print(f"note: name_id {name_id} replaces existing text "
                  f"{existing[0][1].decode('utf-8', 'replace')!r}")
            existing[0][1] = new_rest
            existing[0][2] = len(new_rest)
        else:
            entries.append([h, new_rest, len(new_rest)])

    # Stable sort: hash order, colliding entries keep their original relative order.
    entries.sort(key=lambda e: e[0])
    out_dat = bytearray(b"\xef\xbb\xbf")
    out_dir = []
    for h, rest, rest_len in entries:
        line = b"%d\t%s" % (h, rest)
        out_dir.append(b"%d\t%d\t%d\td" % (h, len(out_dat), len(b"%d" % h) + 1 + rest_len))
        out_dat += line + b"\r\n"

    header = [re.sub(rb"^## Count:\t\d+", b"## Count:\t%d" % len(out_dir), h) for h in header]
    open(dat_p, "wb").write(bytes(out_dat))
    open(dir_p, "wb").write(b"\r\n".join(header + out_dir) + b"\r\n")
    print(f"wrote {len(out_dir)} strings ({len(custom)} custom) to {loc}")


if __name__ == "__main__":
    main()
