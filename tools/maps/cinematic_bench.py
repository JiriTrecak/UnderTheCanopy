"""Authors the cinematic bench, a small mission map whose script plays an opening scene in
the Warcraft III manner (docs/campaign.md "Scenes"), through the editor's MCP tools.

The mission fades in on Queen Aurelith and two Raiders already running along a root path
through the frozen forest. Tiv, a young scout, runs up to meet them in a clearing; they
stop, turn to each other and speak, letterboxed, the camera cutting between two
over-the-shoulder angles, then pulls back wide; the player gets the game back looking at
the Queen. Escape skips the scene. Clicking Tiv five times is the bench's easter egg.

Run with your own server up (`CANOPY_MCP_PORT=7461 ./target/release/canopy mcp --data "$PWD"`):
`CANOPY_MCP_PORT=7461 python3 tools/maps/cinematic_bench.py`. It creates the map, so remove the package first
to rebuild it; `--mission-only` writes the mission (script, cameras) again."""
import json, os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'cinematic-bench'
SIZE = 192
BORDER = 16
c = mcp.Client()

# Where the scene happens (cells): the Queen's run, the meeting, Tiv's run.
QUEEN = (36.0, 160.0)
MEET = (84.0, 112.0)
TIV = (132.0, 64.0)
TIV_MEET = (94.0, 102.0)
ESCORT = [((30.0, 164.0), (78.0, 118.0)), ((38.0, 167.0), (81.0, 121.0))]

SCRIPT = f'''-- The cinematic bench: an opening scene (docs/campaign.md "Scenes").

function on_start()
  mission.fade_out(0)
  mission.play("opening")
end

function on_tick()
  if mission.playing() then return end
  -- The easter egg: poke Tiv five times.
  if mission.clicks("tiv") >= 5 and not mission.get("poked") then
    mission.set("poked", true)
    mission.say("Tiv", "unit.ants.warrior", "My Queen, I am standing right here.", 3, false, "tiv")
  end
end

function scenes.opening()
  mission.cinematic(true)
  mission.camera_to("road", 0)
  -- Already running when the picture comes up.
  mission.move("aurelith", {MEET[0]:g}, {MEET[1]:g})
  mission.move("escort.1", {ESCORT[0][1][0]:g}, {ESCORT[0][1][1]:g})
  mission.move("escort.2", {ESCORT[1][1][0]:g}, {ESCORT[1][1][1]:g})
  mission.fade_in(2.5)
  mission.camera_to("follow", 6)
  mission.wait(1.5)
  mission.move("tiv", {TIV_MEET[0]:g}, {TIV_MEET[1]:g})
  mission.wait_arrived("aurelith")
  mission.wait_arrived("tiv")
  mission.face("aurelith", {TIV_MEET[0]:g}, {TIV_MEET[1]:g})
  mission.face("tiv", {MEET[0]:g}, {MEET[1]:g})
  mission.camera_to("over-queen", 0)
  mission.wait_facing()
  mission.speak("tiv", "My Queen! The Hollow's watch-lights went dark three nights ago, and its scouts never came back.", nil, "Tiv")
  mission.camera_to("over-tiv", 0)
  mission.animate("aurelith", "spell")
  mission.speak("aurelith", "Then we go and ask them why.", nil, "Aurelith")
  mission.camera_to("wide", 2.5)
  mission.wait(2)
  mission.begin_objective("hollow")
  mission.cinematic(false)
  mission.camera_reset(1.5, {MEET[0]:g}, {MEET[1]:g})
end
'''

CAMERAS = [
    # Low on the root path ahead of them, the Queen running into the picture.
    {'id': 'road', 'x': 54, 'y': 142, 'height': 2, 'distance': 40, 'pitch': 14, 'yaw': 135, 'fov': 38},
    # Rising and swinging round as she runs on into the clearing.
    {'id': 'follow', 'x': 80, 'y': 115, 'height': 2, 'distance': 46, 'pitch': 26, 'yaw': 165, 'fov': 38},
    # Over the Queen's shoulder onto Tiv, and back.
    {'id': 'over-queen', 'x': 91.5, 'y': 104.5, 'height': 2.5, 'distance': 26,
     'pitch': 9, 'yaw': 300, 'fov': 36},
    {'id': 'over-tiv', 'x': 86.5, 'y': 109.5, 'height': 4, 'distance': 26,
     'pitch': 8, 'yaw': 120, 'fov': 36, 'roll': 1.5},
    # Wide from up the road ahead of them, the way they came behind.
    {'id': 'wide', 'x': 89, 'y': 107, 'height': 0, 'distance': 62, 'pitch': 32, 'yaw': 135, 'fov': 40},
]


def place(id, definition, at, rotation):
    return {'id': id, 'definition': definition, 'owner': 'player.1',
            'position': {'x': at[0], 'y': at[1]}, 'rotation': rotation}


def blockout():
    c.call('create_map', id=M, name='Bench: Cinematics', size=SIZE, biome='frozen-dream', folder='showcase')
    D = f'maps/showcase/{M}.utcm/map.json'
    c.call('edit', label='The bench\'s cast', commands=[
        # No start: the mission's player begins with what the map places.
        {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': []},
        {'op': 'set', 'document': D, 'pointer': '/camps', 'value': []},
        {'op': 'set', 'document': D, 'pointer': '/playable',
         'value': {'x': BORDER, 'z': BORDER, 'width': SIZE - 2 * BORDER, 'height': SIZE - 2 * BORDER}},
        {'op': 'set', 'document': D, 'pointer': '/entities', 'value': [
            place('aurelith', 'unit.ants.queen', QUEEN, 45.0),
            place('escort.1', 'unit.ants.warrior', ESCORT[0][0], 45.0),
            place('escort.2', 'unit.ants.warrior', ESCORT[1][0], 45.0),
            place('tiv', 'unit.ants.warrior', TIV, 225.0),
        ]},
    ])
    c.call('map_blocks', map=M, op='flat')

    # Frozen forest everywhere but a root path from the south-west to the north-east, a
    # clearing where it meets Tiv's trail, and a mossy rise beside it.
    c.call('map_blocks', map=M, op='paint', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9], kind='wall')
    road = [[20, 176], [40, 156], [60, 136], [84, 112], [112, 86], [136, 60], [160, 30], [176, 16]]
    c.call('map_blocks', map=M, op='paint', points=road, radius=3.5, kind='ground')
    clearing = [[60, 104], [74, 84], [100, 76], [118, 90], [116, 118], [98, 134], [70, 130]]
    c.call('map_blocks', map=M, op='paint', points=clearing, fill=True, kind='ground')
    rise = [[118, 112], [130, 100], [144, 108], [142, 128], [126, 132]]
    c.call('map_blocks', map=M, op='paint', points=rise, fill=True, kind='ground')
    c.call('map_blocks', map=M, op='level', points=rise, fill=True, level=2)
    c.call('map_blocks', map=M, op='round', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9])
    # A frozen stream crossing the path behind the Queen.
    c.call('map_blocks', map=M, op='cells', points=[[24, 120], [44, 128], [58, 150], [70, 170]], radius=2.5, kind='ice')

    SC = f'maps/showcase/{M}.utcm/scenery.json'
    props = [('scenery.frost-pine-a', 64, 100, 0.4, 1.3), ('scenery.frost-pine-b', 110, 124, 1.9, 1.2),
             ('scenery.mega-mushroom', 118, 96, 2.7, 0.35), ('scenery.frost-pine-sapling', 72, 132, 0.9, 1.1),
             ('scenery.frost-pine-a', 46, 128, 2.2, 1.0), ('scenery.frost-pine-sapling', 100, 76, 1.3, 1.0)]
    c.call('edit', label='Trees by the clearing', commands=[
        {'op': 'set', 'document': SC, 'pointer': '/objects', 'value': [
            {'id': f'tree.{i}', 'asset': a, 'x': x, 'z': z, 'yaw': yaw, 'scale': s}
            for i, (a, x, z, yaw, s) in enumerate(props)]}])
    # Without its mission (written next) the check judges it a skirmish map.
    print(c.call('save', force=True))


def write_mission():
    # The mission: the editor does not edit missions yet, so it is written beside the saved
    # map (`mission` in map.json, the script in mission.lua) and checked by the editor.
    PACKAGE = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
                           'base', 'maps', 'showcase', f'{M}.utcm')
    with open(os.path.join(PACKAGE, 'mission.lua'), 'w') as f:
        f.write(SCRIPT)
    path = os.path.join(PACKAGE, 'map.json')
    with open(path) as f:
        document = json.load(f)
    document['playerStarts'] = []
    document['mission'] = {
        'objectives': [{'id': 'hollow', 'title': 'The Hollow Brood',
                        'description': 'Find out why the Hollow Brood went silent'}],
        'regions': [], 'cameras': CAMERAS}
    with open(path, 'w') as f:
        json.dump(document, f, indent=2, ensure_ascii=False)
        f.write('\n')


if '--mission-only' not in sys.argv:
    blockout()
write_mission()
time.sleep(2)
print(c.call('problems'))
