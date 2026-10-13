"""
Adds mesh emitters to Clone Wars Adventures composite effects.

    python tools/cwa_effects.py "C:\\Games\\Clone_Wars\\Clone_Wars"

Reads config/custom_effects.yaml:

    mesh_emitters:                 # new entries in ActorMeshEmitterDefinitions.xml
      - id: 1274                   # unused emitter id
        copy_of: 1172              # existing emitter to copy (model, bone, scale...)
        name: Emote_Starfighter_Orbit_2
        set: {heading: 2.094395, pitch: -0.349066}   # attributes to add/override
    composite_effects:             # existing ActorCompositeEffectDefinitions.xml entries
      - id: 2109
        add_mesh: [1274, 1275]     # mesh emitters to add (start at time 0)

Composite effects are what the server plays (PlayCompositeEffect /
AddCompositeEffectTag); each MESH entry spawns one model.  For example the
Proton Torpedoes mind trick uses three copies of one torpedo emitter at
different headings to get three torpedoes orbiting the head.

The script always starts from the ORIGINAL xml files inside the Assets*.pack
files, applies the config, and writes both files as loose files in the game
folder (the client prefers loose files over packed ones).  Re-running is safe.
Run it with the game closed.
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cwa_aliases import find_in_packs, load_config  # noqa: E402

MESH_FILE = "ActorMeshEmitterDefinitions.xml"
COMPOSITE_FILE = "ActorCompositeEffectDefinitions.xml"


def fmt(value):
    return "%.6f" % value if isinstance(value, float) else str(value)


def add_mesh_emitters(xml, entries):
    for e in entries:
        new_id, src = e["id"], e["copy_of"]
        if re.search(rf'<EffectDefinition [^>]*\bid="{new_id}"', xml):
            sys.exit(f"mesh emitter id {new_id} is already used")
        m = re.search(rf'(\s*)<EffectDefinition [^>]*\bid="{src}"[^>]*/>', xml)
        if not m:
            sys.exit(f"mesh emitter {src} (copy_of) not found")
        line = m.group(0).strip()
        line = re.sub(r'\bid="\d+"', f'id="{new_id}"', line, count=1)
        if e.get("name"):
            line = re.sub(r'\bname="[^"]*"', f'name="{e["name"]}"', line, count=1)
        for attr, value in (e.get("set") or {}).items():
            if re.search(rf'\b{attr}="', line):
                line = re.sub(rf'\b{attr}="[^"]*"', f'{attr}="{fmt(value)}"', line, count=1)
            else:  # add after boneName (or loadType) like the game's own entries
                anchor = "boneName" if 'boneName="' in line else "loadType"
                line = re.sub(rf'(\b{anchor}="[^"]*")', rf'\1 {attr}="{fmt(value)}"', line, count=1)
        xml = xml[:m.end()] + m.group(1) + line + xml[m.end():]
    return xml


def add_composite_meshes(xml, entries):
    for e in entries:
        cid = e["id"]
        m = re.search(rf'<EffectDefinition [^>]*\bid="{cid}"[^>]*>.*?\n([ \t]*)</EffectDefinition>', xml, re.S)
        if not m:
            sys.exit(f"composite effect {cid} not found")
        block, indent = m.group(0), m.group(1)
        head = block[: block.rindex("\n")]          # everything before the closing tag line
        additions = "".join(
            f'\n{indent}    <Effect triggerName="" effectType="MESH" effectId="{mesh}">'
            f'\n{indent}        <Trigger time="0.000000" eventSlot="0" />'
            f'\n{indent}    </Effect>'
            for mesh in e["add_mesh"]
        )
        new_block = head + additions + "\n" + indent + "</EffectDefinition>"
        count = len(re.findall(r"<Effect ", new_block))
        new_block = re.sub(r'effectCount="\d+"', f'effectCount="{count}"', new_block, count=1)
        xml = xml[: m.start()] + new_block + xml[m.end():]
    return xml


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    game_dir = sys.argv[1]
    here = os.path.dirname(os.path.abspath(__file__))
    cfg_path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, "..", "config", "custom_effects.yaml")
    cfg = load_config(cfg_path) or {}

    originals = find_in_packs(game_dir, {MESH_FILE, COMPOSITE_FILE})
    for name in (MESH_FILE, COMPOSITE_FILE):
        if name not in originals:
            sys.exit(f"{name} not found in the Assets*.pack files")
    mesh = add_mesh_emitters(originals[MESH_FILE].decode("latin1"), cfg.get("mesh_emitters") or [])
    comp = add_composite_meshes(originals[COMPOSITE_FILE].decode("latin1"), cfg.get("composite_effects") or [])
    with open(os.path.join(game_dir, MESH_FILE), "wb") as f:
        f.write(mesh.encode("latin1"))
    with open(os.path.join(game_dir, COMPOSITE_FILE), "wb") as f:
        f.write(comp.encode("latin1"))
    print(f"wrote {MESH_FILE} (+{len(cfg.get('mesh_emitters') or [])} emitters) and "
          f"{COMPOSITE_FILE} ({len(cfg.get('composite_effects') or [])} effects changed)")


if __name__ == "__main__":
    main()
