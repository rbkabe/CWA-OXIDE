"""
Builds left-hand (offhand) versions of the one-handed lightsaber hilt models.

    python tools/cwa_offhand_hilts.py "C:\\Games\\Clone_Wars\\Clone_Wars"

The client only ships two offhand hilt models (Ventress_LEFT and AhsokaV2_LEFT).
Comparing AhsokaV2.adr with AhsokaV2_LEFT.adr shows the left version is the same
model with two sections changed:

  section 15 (attach point):   R_WIELD        -> L_WIELD
  section 18 (slot bindings):  PrimaryLSShape -> SecondaryLSShape
                               Weapon         -> WeaponSecondary

so for every Wield_LightsaberHiltStandard_* / Wield_LightsaberHiltReverse_* model
in the Assets*.pack files this writes <name>_LEFT.adr as a loose file in the game
folder (the client prefers loose files over packed ones).  Models that already
have a packed _LEFT version are skipped.  Re-running is safe.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cwa_aliases import dump, find_in_packs, parse  # noqa: E402

import glob
import struct


def list_hilts(game_dir):
    names = set()
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
                    names.add(data[p + 4:p + 4 + n].decode("latin1"))
                    p += 4 + n + 12
                if nxt == 0:
                    break
                off = nxt
    return {
        n for n in names
        if n.endswith(".adr")
        and (n.startswith("Wield_LightsaberHiltStandard_") or n.startswith("Wield_LightsaberHiltReverse_"))
    }


def make_left(adr):
    top = parse(adr)
    for item in top:
        if item[0] == 15:
            assert b"R_WIELD\0" in item[1], item[1]
            item[1] = item[1].replace(b"\x08R_WIELD\0", b"\x08L_WIELD\0")
        elif item[0] == 18:
            sub = parse(item[1])
            for s in sub:
                if s[1] == b"PrimaryLSShape\0":
                    s[1] = b"SecondaryLSShape\0"
                elif s[1] == b"Weapon\0":
                    s[1] = b"WeaponSecondary\0"
            item[1] = dump(sub)
    return dump(top)


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    game_dir = sys.argv[1]
    hilts = list_hilts(game_dir)
    todo = sorted(n for n in hilts if not n.endswith("_LEFT.adr") and n[:-4] + "_LEFT.adr" not in hilts)
    originals = find_in_packs(game_dir, set(todo))
    for n in todo:
        out = make_left(originals[n])
        left = n[:-4] + "_LEFT.adr"
        with open(os.path.join(game_dir, left), "wb") as f:
            f.write(out)
        print("wrote", left)
    print(f"{len(todo)} offhand hilt models written")


if __name__ == "__main__":
    main()
