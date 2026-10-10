"""
Adds custom models (actor groups) to Clone Wars Adventures.

    python tools/cwa_models.py "C:\\Games\\Clone_Wars\\Clone_Wars"

Reads config/custom_models.yaml.  Each entry is one line of the client's
Models.txt, plus (optionally) the .agr actor group file it points at:

    # A companion: wraps a creature model so it can be spawned as an NPC.
    - id: 2865                        # model id (use unused ids, e.g. 2920+)
      file: Grp_Companion_Anacondan.agr
      description: "Companion Model: Cinch companion"
      actors:
        - model: Char_Anacondan.adr
          aliases: [Morley]           # texture alias(es), optional

    # A store preview of a full armor set (several stacked pieces).
    - id: 2888
      file: Grp_Clone_ARF_SGTBoomer.agr
      race: 1
      gender: 1
      description: Marketplace Model
      actors:
        - {model: Char_HumanMaleBody.adr, validate: 1, aliases: [CloneTan]}
        - {model: Char_CloneHead.adr, usage: 3, slot: CustomizationHead, aliases: [Whiskers, CloneTan]}
        - {model: Wear_Human_Male_Body_CloneBasicArmor.adr, usage: 2, validate: 1, slot: Body, slots: true, aliases: [SGTBoomer]}

    # Changing a line the game already has: give the id and only what changes.
    - id: 1944
      scale: 1.0

Models.txt fields (defaults for new ids in brackets):
    file, race [0], scale [1.0], description [""], gender [0], age [0],
    descriptor [""], material [1], water_displacement [0.0], teleport_effect [0],
    camera_distance [0.0], camera_angle [0.0]
Actor fields:
    model (required), usage [1] (actorUsage), validate [0] (validatePcNpc;
    0 skips the PC/NPC model check, which is what makes creature models visible
    as companions), slot [none] (EquippedSlotName), aliases [none],
    slots [false] (writes an empty <Slots /> element, as the game's armor parts do)

The script always starts from the ORIGINAL Models.txt inside the Assets*.pack
files, applies every entry, and writes the result plus the .agr files as loose
files in the game folder (the client prefers loose files over packed ones).
Re-running is safe.  Run it with the game closed.  In the server, a companion
uses its model id in COMPANION_MODELS (companion.rs); a store item set uses it
as preview_model_id in item_groups.yaml.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cwa_aliases import find_in_packs, load_config  # noqa: E402

FIELDS = [
    # (yaml key, Models.txt column, default for new ids, formatter)
    ("file", 1, None, str),
    ("race", 2, 0, str),
    ("scale", 3, 1.0, lambda v: "%.6f" % float(v)),
    ("description", 4, "", str),
    ("gender", 5, 0, str),
    ("age", 6, 0, str),
    ("descriptor", 7, "", str),
    ("material", 8, 1, str),
    ("water_displacement", 9, 0.0, lambda v: "%.6f" % float(v)),
    ("teleport_effect", 10, 0, str),
    ("camera_distance", 11, 0.0, lambda v: "%.6f" % float(v)),
    ("camera_angle", 12, 0.0, lambda v: "%.6f" % float(v)),
]
ACTOR_KEYS = {"model", "usage", "validate", "slot", "aliases", "slots"}
ZERO_POS = "0.000000 0.000000 0.000000 1.000000"
ZERO_ORIENT = "0.000000 0.000000 0.000000 0.000000"


def build_agr(actors):
    out = ["<ActorSet>"]
    for a in actors:
        unknown = set(a) - ACTOR_KEYS
        if unknown:
            sys.exit(f"unknown actor field(s) {sorted(unknown)} in {a}")
        attrs = (
            f'name="{a["model"]}" position="{ZERO_POS}" orientation="{ZERO_ORIENT}" '
            f'actorUsage="{a.get("usage", 1)}" validatePcNpc="{a.get("validate", 0)}"'
        )
        if a.get("slot"):
            attrs += f' EquippedSlotName="{a["slot"]}"'
        out.append(f"    <Actor {attrs}>")
        for alias in a.get("aliases") or []:
            out.append(f'        <TextureAlias name="{alias}" />')
        if a.get("slots"):
            out.append("        <Slots />")
        out.append("    </Actor>")
    out.append("</ActorSet>")
    return "\n".join(out) + "\n"


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    game_dir = sys.argv[1]
    here = os.path.dirname(os.path.abspath(__file__))
    cfg_path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, "..", "config", "custom_models.yaml")
    entries = load_config(cfg_path)

    original = find_in_packs(game_dir, {"Models.txt"}).get("Models.txt")
    if original is None:
        sys.exit("Models.txt not found in the Assets*.pack files")
    lines = original.decode("latin1").splitlines()
    index = {l.split("^", 1)[0]: i for i, l in enumerate(lines) if l[:1].isdigit()}

    known = {k for k, *_ in FIELDS} | {"id", "actors"}
    seen = set()
    agr_count = 0
    for e in entries:
        unknown = set(e) - known
        if unknown:
            sys.exit(f"unknown field(s) {sorted(unknown)} in model entry {e.get('id')}")
        mid = str(e["id"])
        if mid in seen:
            sys.exit(f"model id {mid} is listed twice")
        seen.add(mid)
        if mid in index:
            cols = lines[index[mid]].split("^")
        else:
            if not e.get("file"):
                sys.exit(f"new model id {mid} needs a `file`")
            cols = [mid] + [fmt(d) if d is not None else "" for _, _, d, fmt in FIELDS]
        for key, col, _default, fmt in FIELDS:
            if key in e:
                cols[col] = fmt(e[key])
        line = "^".join(cols)
        if mid in index:
            lines[index[mid]] = line
        else:
            index[mid] = len(lines)
            lines.append(line)

        if e.get("actors"):
            with open(os.path.join(game_dir, cols[1]), "w", newline="\n") as f:
                f.write(build_agr(e["actors"]))
            agr_count += 1

    with open(os.path.join(game_dir, "Models.txt"), "wb") as f:
        f.write(("\r\n".join(lines) + "\r\n").encode("latin1"))
    print(f"wrote Models.txt ({len(entries)} custom entries) and {agr_count} .agr files")


if __name__ == "__main__":
    main()
