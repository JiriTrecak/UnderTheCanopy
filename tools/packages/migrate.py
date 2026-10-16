#!/usr/bin/env python3
"""One-time migration of today's data into the base package (docs/packages.md, D53).

Run from the repository root, one step at a time; each step reads the old files, writes
the new ones and deletes what it replaced:

    python3 tools/packages/migrate.py data     # content/ -> base/ (definitions, rules, abilities, effects)

Key order of every document is kept as authored. Deleted with the old formats once M0 is
done (git history keeps it).
"""

import json
import os
import shutil
import sys

ROOT = os.getcwd()
BASE = os.path.join(ROOT, "base")


def read(path):
    with open(os.path.join(ROOT, path), encoding="utf-8") as f:
        return json.load(f)


def write(path, value):
    full = os.path.join(BASE, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    if os.path.exists(full):
        raise SystemExit(f"refusing to overwrite {path}")
    with open(full, "w", encoding="utf-8") as f:
        json.dump(value, f, indent=2, ensure_ascii=False)
        f.write("\n")


def last(id_):
    return id_.split(".")[-1]


def without(d, *keys):
    return {k: v for k, v in d.items() if k not in keys}


def step_data():
    game = read("content/game.json")
    rules = game["rules"]
    races = rules["races"]
    roster_race = {m: r for r, race in races.items() for m in race["roster"]}

    if not os.path.exists(os.path.join(BASE, "package.json")):
        write("package.json", {
            "format": 1,
            "id": "base",
            "kind": "base",
            "name": "Under the Canopy",
            "version": "0.4.0",
            "description": "The game: every race, unit, ability, asset and shipped map.",
        })

    # Behaviour sets: behaviors/<name>.json
    for s in game["behaviorSets"]:
        write(f"behaviors/{last(s['id']).removeprefix('behavior-set.')}.json", s)

    # Build overrides move into their definitions as `command`.
    overrides = dict(game["actions"]["overrides"])
    commands = {}
    for key, value in list(overrides.items()):
        kind, _, target = key.partition(":")
        if kind in ("build", "produce"):
            commands[target] = value
            del overrides[key]
    if overrides:
        raise SystemExit(f"unhandled command overrides: {list(overrides)}")

    # Definitions: one file each, the kind given by the folder.
    folders = {"unit": "units", "building": "buildings", "item": "items", "resource": "resources"}
    for d in game["definitions"]:
        kind = d["kind"]
        body = without(d, "kind")
        if d["id"] in commands:
            body["command"] = commands.pop(d["id"])
        name = d["id"].split(".", 2)[-1] if kind != "item" else last(d["id"])
        if kind in ("unit", "building"):
            scope = d["id"].split(".")[1]
            sub = f"{scope}/heroes" if d.get("hero") else scope
            path = f"{folders[kind]}/{sub}/{last(d['id'])}.json"
        elif kind == "item":
            if d.get("currency"):
                path = f"items/resources/{last(d['id'])}.json"
            else:
                path = f"items/artifacts/tier-{d.get('itemTier', 0)}/{last(d['id'])}.json"
        else:
            path = f"resources/{last(d['id'])}.json"
        write(path, body)
    if commands:
        raise SystemExit(f"overrides without a definition: {list(commands)}")

    # Rules split.
    write("rules/game.json", {
        "defaultRace": rules["defaultRace"],
        "maxUnits": rules["maxUnits"],
        "maxBuildings": rules["maxBuildings"],
        "maxSupply": rules["maxSupply"],
        "supplyIcon": rules["supplyIcon"],
        "constructionHpPermille": rules["constructionHpPermille"],
        "constructionRefundPermille": rules["constructionRefundPermille"],
        "repairTicks": rules["repairTicks"],
    })
    write("rules/combat.json", {
        "armorTypes": rules["armorTypes"],
        "damageTypes": rules["damageTypes"],
        "damageMultipliers": rules["damageMultipliers"],
        "armorCoefficient": rules["armorCoefficient"],
    })
    write("rules/experience.json", rules["experience"])
    write("rules/commands.json", without(game["actions"], "overrides"))

    # Races: the global starting setup is the default race's own.
    for rid, race in races.items():
        body = {"id": rid}
        body.update(race)
        if rid == rules["defaultRace"] and "startingSetup" not in race:
            # Keep the authored position: after description, before roster.
            ordered = {}
            for k, v in body.items():
                if k == "roster":
                    ordered["startingSetup"] = rules["startingSetup"]
                ordered[k] = v
            body = ordered
        write(f"races/{rid}.json", body)

    for rid, research in rules["research"].items():
        body = {"id": rid}
        body.update(research)
        write(f"research/{rid.split('.')[1]}/{last(rid)}.json", body)
    for pid, pool in rules["lootPools"].items():
        body = {"id": pid}
        body.update(pool)
        write(f"loot-pools/{last(pid)}.json", body)
    for cid, campaign in rules["campaigns"].items():
        root = f"campaigns/{cid}.utcc"
        write(f"{root}/package.json", {
            "format": 1,
            "id": cid,
            "kind": "campaign",
            "name": campaign["name"],
            "version": "0.4.0",
            "dependencies": {"base": ">=0.4"},
        })
        write(f"{root}/campaign.json", {"race": campaign["race"]})

    # Abilities: one file each with the presentation inline, foldered by owner.
    library = read("content/abilities/published.json")["library"]
    presentations = {p["id"]: p for p in library["presentations"]}
    defs = {d["id"]: d for d in game["definitions"]}
    owner = {}
    for d in game["definitions"]:
        for b in (d.get("behaviors", {}).get("abilities") or {}).get("bindings", []):
            owner.setdefault(b["ability"], d)
    ability_dir = {}
    for a in library["abilities"]:
        o = owner.get(a["id"])
        if o is None:
            folder = "abilities/unbound"
        elif o.get("hero"):
            folder = f"abilities/heroes/{last(o['id'])}"
        else:
            scope = o["id"].split(".")[1]
            folder = f"abilities/{scope}/{last(o['id'])}"
        ability_dir[a["id"]] = folder
        p = without(presentations[a["presentation"]], "schemaVersion", "id")
        body = {}
        for k, v in a.items():
            if k == "schemaVersion":
                continue
            body[k] = p if k == "presentation" else v
        write(f"{folder}/{last(a['id'])}.json", body)

    # Effects: with the ability that first binds them, weapons and construction apart.
    effects = read("content/effects/published.json")["effects"]
    effect_dir = {}
    for a in library["abilities"]:
        p = presentations[a["presentation"]]
        for b in p.get("effects", []):
            effect_dir.setdefault(b["effect"], ability_dir[a["id"]].replace("abilities/", "effects/", 1))
    for e in effects:
        id_ = e["id"]
        parts = id_.split(".")
        if parts[1] == "weapon" or id_ not in effect_dir and parts[1] == "ants" and "explosion" in id_:
            folder, name = "effects/weapons", "-".join(parts[2:])
        elif parts[1] == "construction":
            folder, name = "effects/construction", "-".join(parts[2:])
        else:
            folder = effect_dir.get(id_, "effects/other")
            name = "-".join(parts[2:])
        write(f"{folder}/{name}.json", without(e, "schemaVersion"))

    for gone in ["content/game.json", "content/abilities", "content/effects", "content/neutral-camps.json"]:
        full = os.path.join(ROOT, gone)
        if os.path.isdir(full):
            shutil.rmtree(full)
        elif os.path.exists(full):
            os.remove(full)
    # Warcraft importer settings are tool input, not game content.
    if os.path.isdir(os.path.join(ROOT, "content/import")):
        os.makedirs(os.path.join(ROOT, "tools/warcraft-import"), exist_ok=True)
        shutil.move(os.path.join(ROOT, "content/import/warcraft"), os.path.join(ROOT, "tools/warcraft-import/settings"))
        shutil.rmtree(os.path.join(ROOT, "content/import"))
    if os.path.isdir(os.path.join(ROOT, "content")) and not os.listdir(os.path.join(ROOT, "content")):
        os.rmdir(os.path.join(ROOT, "content"))


STEPS = {"data": step_data}

if __name__ == "__main__":
    if len(sys.argv) != 2 or sys.argv[1] not in STEPS:
        raise SystemExit(f"usage: migrate.py {'|'.join(STEPS)}")
    STEPS[sys.argv[1]]()
