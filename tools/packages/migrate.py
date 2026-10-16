#!/usr/bin/env python3
"""One-time migration of today's data into the base package (docs/packages.md, D53).

Run from the repository root, one step at a time; each step reads the old files, writes
the new ones and deletes what it replaced:

    python3 tools/packages/migrate.py data     # content/ -> base/ (definitions, rules, abilities, effects)
    python3 tools/packages/migrate.py assets   # art/assets, assets/ registries -> base/assets, scenery, landscape

Key order of every document is kept as authored. Deleted with the old formats once M0 is
done (git history keeps it).
"""

import collections
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


def save(full, value):
    with open(full, "w", encoding="utf-8") as f:
        json.dump(value, f, indent=2, ensure_ascii=False)
        f.write("\n")


def base_files(*folders):
    out = []
    for folder in folders:
        for root, _, files in os.walk(os.path.join(BASE, folder)):
            out += [os.path.join(root, f) for f in files if f.endswith(".json")]
    return sorted(out)


def asset_folder(id_):
    """base/assets/<path>: the id without `asset.`/`models.`, dots as folders."""
    rest = id_
    for prefix in ("asset.models.", "asset."):
        if rest.startswith(prefix):
            rest = rest[len(prefix):]
            break
    if rest.startswith("texture."):
        rest = "textures." + rest[len("texture."):]
    if rest.startswith("image."):
        rest = "interface." + rest[len("image."):]
    return "assets/" + rest.replace(".", "/")


def step_assets():
    records = {r["id"]: r for r in read("assets/manifest.json")["records"]}
    catalogue = {c["id"]: c for c in read("assets/authoring/catalogue.json")}
    sources = {}
    for name in os.listdir(os.path.join(ROOT, "art/assets")):
        path = os.path.join(ROOT, "art/assets", name, "asset.json")
        if os.path.exists(path):
            sources[name] = read(f"art/assets/{name}/asset.json")
    render = {e["id"]: (r["id"], e) for r in records.values() for e in r["render"]}

    def is_model(rid):
        return any(o["path"].endswith(".glb") for o in records[rid]["outputs"])

    # --- New ids for every render id -------------------------------------------------
    new_id = {}
    portraits = {}  # model record -> portrait asset id
    cargo = {}  # (record, file) -> cargo asset id
    for rid, (rec, e) in render.items():
        if "image" in e:
            if is_model(rec):
                pid = "asset.portraits." + rec.removeprefix("asset.models.")
                portraits[rec] = pid
                new_id[rid] = pid
            else:
                new_id[rid] = rec
        elif e.get("sceneryAsset"):
            new_id[rid] = "scenery." + e["sceneryAsset"]
        else:
            file = os.path.basename(e["file"])
            if file != "geometry.glb":
                cid = "asset.models.cargo." + last(rid)
                cargo[(rec, file)] = cid
                new_id[rid] = cid
            else:
                new_id[rid] = rec

    def nid(old):
        if old not in new_id:
            raise SystemExit(f"no new id for {old}")
        return new_id[old]

    # --- Display of each model: from its own entry, else the most used one ------------
    usage = collections.Counter()
    for f in base_files("units", "buildings", "items", "resources"):
        usage[read(os.path.relpath(f, ROOT)).get("asset")] += 1
    display_entry = {}
    for rec, r in records.items():
        entries = [e for e in r["render"] if "file" in e and os.path.basename(e["file"]) == "geometry.glb"]
        if not entries:
            continue
        own = [e for e in entries if e["id"] == rec]
        plain = [e for e in entries if not e.get("sceneryAsset")]
        pick = own or sorted(plain or entries, key=lambda e: (-usage[e["id"]], e["id"]))
        display_entry[rec] = pick[0]
    for (rec, file), cid in cargo.items():
        display_entry[cid] = next(e for e in records[rec]["render"] if e.get("file", "").endswith("/" + file))

    def display(entry):
        d = {}
        for key in ("scale", "character", "healthHeight"):
            if key in entry:
                d[key] = entry[key]
        return d

    # --- Asset folders -----------------------------------------------------------------
    moved = {}  # old pack entry -> new pack entry, for verification
    landscape_kinds = {"landscape-recipe", "water-profile"}
    for aid, a in sorted(sources.items()):
        old = os.path.join(ROOT, "art/assets", aid)
        if a["kind"] in landscape_kinds:
            continue
        folder = os.path.join(BASE, asset_folder(aid))
        os.makedirs(os.path.dirname(folder), exist_ok=True)
        shutil.move(old, folder)
        resources = [without(r, "sha256", "bytes") for r in a["resources"]]
        out = {"id": aid, "name": a["name"], "kind": a["kind"], "tags": a["tags"]}
        rec = records[aid]
        if rec.get("faction"):
            out["race"] = rec["faction"]
        # Carried cargo and portraits leave the model's folder.
        for (crec, file), cid in cargo.items():
            if crec != aid:
                continue
            cfolder = os.path.join(BASE, asset_folder(cid))
            os.makedirs(cfolder, exist_ok=True)
            shutil.move(os.path.join(folder, file), os.path.join(cfolder, "geometry.glb"))
            index = int(file.split("_")[1].split(".")[0])
            resources = [r for r in resources if not (r["role"] == "geometry" and r["index"] == index)]
            save(os.path.join(cfolder, "asset.json"), {
                "id": cid, "name": a["name"] + " cargo " + last(cid), "kind": "prop", "tags": ["cargo"],
                "resources": [{"role": "geometry", "index": 1, "format": "glb"}],
                "transform": a["transform"], "materials": [],
                "display": display(display_entry[cid]),
            })
            moved[f"assets/library/{aid}/{file}"] = f"{cid}/geometry.glb"
        has_image = any(r["role"] == "image" for r in resources)
        if is_model(aid) and has_image:
            image = next(r for r in resources if r["role"] == "image")
            name = f"image.{image['format']}"
            resources = [r for r in resources if r is not image]
            if aid in portraits:
                pid = portraits[aid]
                pfolder = os.path.join(BASE, asset_folder(pid))
                os.makedirs(pfolder, exist_ok=True)
                shutil.move(os.path.join(folder, name), os.path.join(pfolder, name))
                portrait = {"id": pid, "name": a["name"] + " portrait", "kind": "icon", "tags": ["portrait"],
                            "resources": [image]}
                if a.get("provenance", {}).get("licenseNote"):
                    portrait["license"] = a["provenance"]["licenseNote"]
                save(os.path.join(pfolder, "asset.json"), portrait)
                moved[f"assets/library/{aid}/{name}"] = f"{pid}/{name}"
            else:
                # A model's image nothing shows: a stale copy.
                os.remove(os.path.join(folder, name))
        out["resources"] = resources
        if any(r["format"] == "glb" for r in resources):
            out["transform"] = a["transform"]
        if a.get("materials"):
            out["materials"] = a["materials"]
        if a.get("capabilities"):
            out["capabilities"] = a["capabilities"]
        if aid in display_entry and any(r["format"] == "glb" for r in resources):
            d = display(display_entry[aid])
            if d:
                out["display"] = d
        if rec["profile"] == "building":
            out["shading"] = "building"
        for key in ("terrain", "modelClass"):
            if key in a:
                out[key] = a[key]
        if a.get("provenance", {}).get("licenseNote"):
            out["license"] = a["provenance"]["licenseNote"]
        os.remove(os.path.join(folder, "asset.json"))
        save(os.path.join(folder, "asset.json"), out)
        for o in rec["outputs"]:
            file = os.path.basename(o["path"])
            if o["path"] not in moved and os.path.exists(os.path.join(folder, file)):
                moved[o["path"]] = f"{aid}/{file}"

    # --- Scenery definitions -------------------------------------------------------------
    for rec, r in sorted(records.items()):
        for sc in r["scenery"]:
            body = {"id": "scenery." + sc["id"], "name": sc["name"], "category": sc["category"],
                    "type": sc["type"], "model": rec}
            for key in ("editorHidden", "deck", "light", "blocker", "blockers"):
                if key in sc:
                    body[key] = sc[key]
            entry = catalogue.get(rec)
            if entry and entry.get("harvesting") and entry.get("scenery") == sc["id"]:
                body["harvesting"] = {"definition": entry["harvesting"]["definition"]}
            write(f"scenery/{sc['category']}/{sc['id']}.json", body)
    scenery_of = {rec: "scenery." + r["scenery"][0]["id"] for rec, r in records.items() if r["scenery"]}

    # --- Landscape: recipes, water profiles, biomes ------------------------------------
    for aid, a in sorted(sources.items()):
        if a["kind"] not in landscape_kinds:
            continue
        entry = catalogue[aid]
        if a["kind"] == "water-profile":
            write(f"landscape/water/{aid.removeprefix('recipe.').removeprefix('water.').replace('.', '-')}.json",
                  {"id": aid, "name": entry["name"], "water": entry["water"]})
        else:
            recipe = json.loads(json.dumps(entry["recipe"]))
            for species in recipe.get("species", []):
                species["asset"] = scenery_of.get(species["asset"], species["asset"])
            write(f"landscape/recipes/{aid.removeprefix('recipe.').replace('.', '/')}.json",
                  {"id": aid, "name": entry["name"], "recipe": recipe})
        shutil.rmtree(os.path.join(ROOT, "art/assets", aid))
    prototypes = {sc["id"] for r in records.values() for sc in r["scenery"]}

    def biome_refs(value):
        if isinstance(value, dict):
            return {k: biome_refs(v) for k, v in value.items()}
        if isinstance(value, list):
            return [biome_refs(v) for v in value]
        if isinstance(value, str):
            if value in prototypes:
                return "scenery." + value
            return value.replace("assets/library/", "")
        return value

    for name in sorted(os.listdir(os.path.join(ROOT, "config/biomes"))):
        write(f"landscape/biomes/{name}", biome_refs(read(f"config/biomes/{name}")))
        os.remove(os.path.join(ROOT, "config/biomes", name))
    os.rmdir(os.path.join(ROOT, "config/biomes"))

    # --- References ------------------------------------------------------------------------
    unit_fields = ("projectile", "projectileSocket", "impactEffect", "castContact", "cameraAnchor")
    for f in base_files("units", "buildings", "items", "resources"):
        d = read(os.path.relpath(f, ROOT))
        old = d.pop("asset")
        out = {}
        for k, v in d.items():
            if k == "icon":
                out["model"] = nid(old)
                out["icon"] = nid(v)
            else:
                out[k] = v
        rec, e = render[old]
        presentation = {k: e[k] for k in unit_fields if k in e}
        base_display = display(display_entry.get(rec, {})) if not e.get("sceneryAsset") else {}
        for k in ("healthHeight", "character"):
            if k in e and e[k] != base_display.get(k):
                presentation[k] = e[k]
        if presentation:
            out["presentation"] = presentation
        rv = out.get("resourceVisuals")
        if rv:
            rv["variants"] = [nid(v) for v in rv["variants"]]
            for stage in rv["stages"]:
                stage["asset"] = nid(stage["asset"])
        save(f, out)
    for f in base_files("races"):
        race = read(os.path.relpath(f, ROOT))
        c = race.get("construction")
        if c:
            c["scaffold"] = nid(c["scaffold"])
        save(f, race)
    for f in base_files("research"):
        r = read(os.path.relpath(f, ROOT))
        r["icon"] = nid(r["icon"])
        save(f, r)
    combat = read("base/rules/combat.json")
    for t in combat["armorTypes"].values():
        t["icon"] = nid(t["icon"])
    save(os.path.join(BASE, "rules/combat.json"), combat)
    game = read("base/rules/game.json")
    game["supplyIcon"] = nid(game["supplyIcon"])
    save(os.path.join(BASE, "rules/game.json"), game)
    commands = read("base/rules/commands.json")
    for group in ("actions", "categories"):
        for meta in commands.get(group, {}).values():
            meta["icon"] = nid(meta["icon"])
    for meta in commands["navigation"].values():
        meta["icon"] = nid(meta["icon"])
    save(os.path.join(BASE, "rules/commands.json"), commands)
    glow = read("config/glow.json")
    for g in glow["glows"]:
        g["assets"] = [nid(a) for a in g["assets"]]
    save(os.path.join(ROOT, "config/glow.json"), glow)
    theme_path = os.path.join(ROOT, "config/ui/theme.json")
    with open(theme_path, encoding="utf-8") as f:
        theme = f.read()
    with open(theme_path, "w", encoding="utf-8") as f:
        f.write(theme.replace("assets/library/", ""))
    for name in os.listdir(os.path.join(ROOT, "assets/maps/skirmish")):
        path = os.path.join(ROOT, "assets/maps/skirmish", name)
        m = json.load(open(path, encoding="utf-8"))
        for e in m.get("entities", []):
            app = e.get("appearance")
            if app and "asset" in app:
                app["asset"] = nid(app["asset"])
        for o in m["authoring"]["objects"]:
            o["asset"] = scenery_of[o["asset"]]
        with open(path, "w", encoding="utf-8") as f:
            json.dump(m, f, indent=2, ensure_ascii=False)

    # --- Old registries go --------------------------------------------------------------------
    for gone in ["assets/library", "assets/authoring", "assets/manifest.json", "assets/index.md"]:
        full = os.path.join(ROOT, gone)
        if os.path.isdir(full):
            shutil.rmtree(full)
        elif os.path.exists(full):
            os.remove(full)
    if not os.listdir(os.path.join(ROOT, "art/assets")):
        os.rmdir(os.path.join(ROOT, "art/assets"))
    with open(os.path.join(ROOT, "target/asset-entry-renames.json"), "w") as f:
        json.dump(moved, f, indent=1, sort_keys=True)
    with open(os.path.join(ROOT, "target/render-id-renames.json"), "w") as f:
        json.dump(new_id, f, indent=1, sort_keys=True)
    print(f"{len(moved)} pack entries renamed; portraits {len(portraits)}; cargo {len(cargo)}")


STEPS = {"data": step_data, "assets": step_assets}

if __name__ == "__main__":
    if len(sys.argv) != 2 or sys.argv[1] not in STEPS:
        raise SystemExit(f"usage: migrate.py {'|'.join(STEPS)}")
    STEPS[sys.argv[1]]()
