"""
Adds custom icons to Clone Wars Adventures.

    python tools/cwa_icons.py "C:\\Games\\Clone_Wars\\Clone_Wars"

Reads config/custom_icons.yaml:

    - icon_set_id: 3895                       # what items/groups use (icon_set_id)
      image_id: 7089                          # internal image id (any unused id)
      file: Icon_Set_Wear_SGTBoomer_64.dds    # file name in the game folder

Optional per entry:
    source:      file to copy in as `file` (relative to the config dir or
                 absolute).  Default: custom_icons/<file> if it exists.  Icons
                 the game already has inside its packs need no file.
    description: ImageSets.txt description.  Default: `file` without ".dds".
    image_type:  ImageSetMappings image type.  Default 6 (the 64x64 icon size
                 used by every item icon).

An icon takes three client tables: Images.txt (image id -> .dds file),
ImageSets.txt (icon set id -> description) and ImageSetMappings.txt (icon set
id + image type -> image id).  The script always starts from the ORIGINAL
tables inside the Assets*.pack files, adds or replaces the listed ids, copies
the .dds files, and writes everything as loose files in the game folder (the
client prefers loose files over packed ones).  Re-running is safe.  Run it with
the game closed.  Icons are 64x64 DDS (DXT3, like the game's own).
"""
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cwa_aliases import find_in_packs, load_config  # noqa: E402

TABLES = ("Images.txt", "ImageSets.txt", "ImageSetMappings.txt")
KEYS = {"icon_set_id", "image_id", "file", "source", "description", "image_type"}


def upsert(lines, key_cols, new_lines):
    """Replace lines whose first `key_cols` columns match, append the rest."""
    key = lambda l: tuple(l.split("^")[:key_cols])
    index = {key(l): i for i, l in enumerate(lines) if l[:1].isdigit()}
    for nl in new_lines:
        k = key(nl)
        if k in index:
            lines[index[k]] = nl
        else:
            index[k] = len(lines)
            lines.append(nl)


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    game_dir = sys.argv[1]
    here = os.path.dirname(os.path.abspath(__file__))
    cfg_path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, "..", "config", "custom_icons.yaml")
    cfg_dir = os.path.dirname(os.path.abspath(cfg_path))
    entries = load_config(cfg_path)

    originals = find_in_packs(game_dir, set(TABLES))
    for t in TABLES:
        if t not in originals:
            sys.exit(f"{t} not found in the Assets*.pack files")
    tables = {t: originals[t].decode("latin1").splitlines() for t in TABLES}

    images, sets, mappings = [], [], []
    seen_sets, seen_images = set(), set()
    missing = []
    for e in entries:
        unknown = set(e) - KEYS
        if unknown:
            sys.exit(f"unknown field(s) {sorted(unknown)} in icon entry {e}")
        set_id, image_id, file = e["icon_set_id"], e["image_id"], e["file"]
        if set_id in seen_sets or image_id in seen_images:
            sys.exit(f"icon_set_id {set_id} / image_id {image_id} is listed twice")
        seen_sets.add(set_id)
        seen_images.add(image_id)
        description = e.get("description") or os.path.splitext(file)[0]
        images.append(f"{image_id}^{file}")
        sets.append(f"{set_id}^{description}")
        mappings.append(f"{set_id}^{e.get('image_type', 6)}^{image_id}")

        source = e.get("source") or os.path.join("custom_icons", file)
        source = source if os.path.isabs(source) else os.path.join(cfg_dir, source)
        dest = os.path.join(game_dir, file)
        if os.path.exists(source):
            shutil.copyfile(source, dest)
        elif not os.path.exists(dest):
            missing.append(file)

    # Some icons the game ships inside its packs but never registered (e.g. the
    # Hodgepodge droid's); those need no loose copy.
    packed = find_in_packs(game_dir, set(missing)) if missing else {}
    for file in missing:
        if file not in packed:
            print(f"  warning: {file} is not in the game folder, the packs or config/custom_icons")

    upsert(tables["Images.txt"], 1, images)
    upsert(tables["ImageSets.txt"], 1, sets)
    upsert(tables["ImageSetMappings.txt"], 2, mappings)
    for t in TABLES:
        with open(os.path.join(game_dir, t), "wb") as f:
            f.write(("\r\n".join(tables[t]) + "\r\n").encode("latin1"))
    print(f"wrote {', '.join(TABLES)} with {len(entries)} custom icons")


if __name__ == "__main__":
    main()
