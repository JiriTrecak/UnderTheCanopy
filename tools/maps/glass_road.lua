-- The Glass Road (Act I, Mission Three; docs/story.md). Authored with the map by
-- tools/maps/glass_road.py, which fills in the caravans' spill spots below.
--
-- Every run starts fresh (no library, no globals kept): what the script remembers lives in
-- mission variables. Beats:
--   opening   Aurelith and Oleander walk into the forward camp; Korrath waits, they speak,
--             he kneels and joins.
--   road      the caravans start on the Glass Road; lines as the Queen finds things;
--             Korrath notices the rubble of the old tunnel; breaking it opens the tunnel.
--   gate      at the station's gate Coilkeeper Vosk refuses her from the wall; the waves start.
--   war       the station's Great Bastion falls: the beetles stop.
--   closing   the Queen at the condenser; Oleander's line; victory.
-- Lose: the camp's Nest falls, or the Queen dies with no Royal Chamber to revive her.
--
-- TODO(cinematics): when map cameras, fades, letterbox, animations and skip land, give the
-- opening, gate, tunnel and closing scenes their shots (marked "shot:" below), Korrath's
-- kneel (animation), the condenser's rising violet light, and switch the engineer to
-- mission.clicks("engineer").

local SECOND = 40
local SPILLS = { --@SPILLS@
}
local ROUTE = { { 36, 216 }, { 100, 216 }, { 168, 216 }, { 168, 160 }, { 216, 160 }, { 276, 160 },
  { 340, 160 }, { 340, 124 } }
local CARAVANS = 6
local AMBUSHES = 3

local who = {
  queen = { "Aurelith", "unit.ants.queen" },
  oleander = { "Oleander", "unit.ants.royal-alchemist" },
  korrath = { "Korrath", "unit.ants.marshal" },
  vosk = { "Coilkeeper Vosk", "unit.beetles.coilkeeper" },
  engineer = { "Beetle engineer", "unit.beetles.worker" },
}

local function get(key, default)
  local v = mission.get(key)
  if v == nil then return default end
  return v
end

local function line(id, text, seconds, cinematic)
  local w = who[id]
  local actor = nil
  if mission.alive(id) then actor = id end
  mission.say(w[1], w[2], text, seconds, cinematic, actor)
end

-- Scenes and talks: lists of steps run in order. A scene holds the player's commands and
-- its lines pause the game; a talk is lines over play.
--   { "say", who, text, seconds }      a line (waits until it is spoken)
--   { "move", id, x, y }               an order (alive units only)
--   { "face", id, x, y }
--   { "wait", seconds }
--   { "arrive", id, x, y, seconds }    waits until the unit stands there, or the time is up
--   { "begin", x, y } / { "end" }      the scene's camera
--   { "do", name }                     a beat (see `beats`)
local SCENES = {
  opening = {
    { "begin", 120, 350 }, -- shot: a slow push from the camp's south edge onto Korrath
    { "move", "queen", 121, 341 }, { "move", "oleander", 127, 345 },
    { "face", "korrath", 124, 350 },
    { "arrive", "queen", 121, 341, 9 },
    { "face", "queen", 118, 330 }, { "face", "oleander", 118, 330 }, { "face", "korrath", 121, 341 },
    { "say", "korrath", "Majesty. Your mother set me to hold the end of this road. I hear her daughter means to walk it.", 6 },
    { "say", "queen", "The beetles keep substance in glass, General, and it never changes them. I mean to learn how.", 6 },
    { "say", "oleander", "They keep a condenser at the far end of the road. Whatever they know is in that machine.", 5 },
    { "say", "korrath", "And when they will not share it?", 3 },
    { "say", "queen", "Then I will take it. No brood of ours will rot in the dark again.", 5 },
    { "say", "korrath", "Your mother would have called it madness.", 4 },
    { "say", "queen", "My mother is not here.", 3 },
    { "wait", 1 }, -- shot: close on Korrath; he kneels (animation)
    { "say", "korrath", "...Then my mandibles are yours, my Queen.", 4 },
    { "end" },
    { "do", "road" },
    { "say", "oleander", "The road runs north of the camp. Its outposts will not let us pass quietly.", 5, true },
  },
  gate = {
    { "begin", 340, 130 }, -- shot: low under the wall, looking up at Vosk on the gate
    { "move", "vosk", 340, 98 }, { "face", "vosk", 340, 160 },
    { "wait", 1 },
    { "say", "queen", "Keeper of the glass! I am Aurelith, Queen of the ants. I would speak with you.", 5 },
    { "say", "vosk", "Coilkeeper Vosk. I know what you are, little queen. Your broods drink the light raw, and it eats them from the inside.", 7 },
    { "say", "queen", "Then teach us your glass, and they will never drink again.", 4 },
    { "say", "vosk", "Glass is not for those who drink.", 4 },
    { "say", "vosk", "Turn your column around. The next ant on my road goes home in pieces.", 5 },
    { "say", "korrath", "They're opening the lodge, Majesty. Here they come.", 4 },
    { "end" },
    { "do", "war" },
    { "say", "oleander", "The condenser is behind that wall. Break their Great Bastion and the station is ours.", 6, true },
  },
  tunnel = {
    { "begin", 304, 66 }, -- shot: the old mound pushing up out of the yard behind the west wall
    { "wait", 1 },
    { "say", "korrath", "It runs under the road and comes up behind their west wall. They have forgotten it was ever dug.", 6 },
    { "say", "queen", "Then we have not.", 3 },
    { "end" },
  },
  closing = {
    { "begin", 340, 70 }, -- shot: around the condenser, the Queen's hands on the glass
    { "move", "queen", 340, 72 }, { "move", "oleander", 347, 75 }, { "move", "korrath", 333, 76 },
    { "arrive", "queen", 340, 72, 6 },
    { "face", "queen", 340, 60 }, { "face", "oleander", 340, 60 }, { "face", "korrath", 340, 72 },
    { "say", "queen", "It's warm. It hums... like something breathing in its sleep.", 5 },
    { "say", "oleander", "Now we can learn what it is.", 4 },
    { "wait", 2 }, -- shot: a violet light rises in the glass; fade out
    { "do", "win" },
  },
}

local TALKS = {
  caravan = { { "korrath", "Carts on the road, Majesty. The beetles feed that station from the west. Cut the supply and the glass goes thirsty.", 7 } },
  ambush1 = { { "korrath", "That's one cart that won't reach the glass.", 4 } },
  ambush2 = { { "oleander", "Salves, flasks, sealed jars... they live well, these beetles.", 5 } },
  ambush3 = { { "korrath", "Three caravans. The station will feel that, Majesty.", 5 } },
  rubble = {
    { "korrath", "Majesty, that rubble. That's ant work. Old ant work.", 5 },
    { "queen", "A tunnel? This far from any nest?", 4 },
    { "korrath", "Our grandmothers dug toward the glass long before the beetles paved over it. Break it open.", 6 },
  },
  outpost = { { "korrath", "Hornwatch towers. Beetle guns never tire. Bring the whole column, not a handful.", 6 } },
  toll = { { "oleander", "An old toll house. The beetles would charge the rain to cross their road.", 5 } },
  bog = { { "oleander", "Toads in the bog... and something bottled under the reeds. Beetle-made.", 5 } },
  briar = { { "korrath", "Amber under those thorns. Clear the briars and we can raise a second nest here.", 6 } },
  grove = { { "oleander", "Careful. That is the Hollow Stag. Older than the road, older than any of us.", 6 } },
  glade = { { "queen", "A beetle surveyor... he never made it home. Look at what he carried.", 5 } },
  fallen = { { "vosk", "Take it, then. Drink from it. And see what answers.", 6 } },
  snore = { { "engineer", "Hnnnk... zzz... pressure's fine... zzz...", 3 } },
  woken = {
    { "engineer", "Wha- ants?! I wasn't sleeping! I was listening to the boiler!", 5 },
    { "engineer", "Fine, fine. Take this, and don't tell the Coilkeeper you saw me.", 5 },
  },
}

-- Places the Queen finds, each spoken once: talk, region.
local FINDS = { { "outpost", "outpost-w" }, { "toll", "toll" }, { "bog", "bog" }, { "briar", "briar" },
  { "grove", "grove" }, { "glade", "glade" } }

local beats = {}

local function start_scene(name)
  mission.set("scene", name)
  mission.set("step", 1)
  mission.set("until", 0)
end

local function start_talk(name)
  if mission.get("talk") ~= nil or mission.dialogue_busy() then return false end
  mission.set("talk", name)
  mission.set("line", 1)
  return true
end

-- Runs steps of a scene from `step` until one has to wait. Returns the next step.
local function run_steps(steps, step, now, cinematic)
  while step <= #steps do
    local s = steps[step]
    local kind = s[1]
    if kind == "say" then
      if mission.dialogue_busy() then return step end
      local w = mission.get("said")
      if w == step then
        mission.set("said", nil)
        step = step + 1
      else
        if s[5] == true or not cinematic then
          line(s[2], s[3], s[4], false)
        else
          line(s[2], s[3], s[4], true)
        end
        mission.set("said", step)
        return step
      end
    elseif kind == "move" then
      if mission.alive(s[2]) then mission.move(s[2], s[3], s[4]) end
      step = step + 1
    elseif kind == "face" then
      if mission.alive(s[2]) then mission.face(s[2], s[3], s[4]) end
      step = step + 1
    elseif kind == "wait" or kind == "arrive" then
      local u = mission.get("until")
      if u == 0 then
        local secs = s[2]
        if kind == "arrive" then secs = s[5] end
        mission.set("until", now + secs * SECOND)
        return step
      end
      local done = now >= u
      if kind == "arrive" and not done then
        done = (not mission.alive(s[2])) or mission.arrived(s[2], s[3], s[4])
      end
      if not done then return step end
      mission.set("until", 0)
      step = step + 1
    elseif kind == "begin" then
      mission.begin_scene(s[2], s[3])
      step = step + 1
    elseif kind == "end" then
      mission.end_scene()
      step = step + 1
    elseif kind == "do" then
      beats[s[2]]()
      step = step + 1
    end
  end
  return step
end

-- The caravans -------------------------------------------------------------------------

local function member(n, role, i)
  return "caravan." .. n .. "." .. role .. "." .. i
end

local function spill(x, z)
  -- The two nearest spots not used yet get a crate each.
  local used = get("spilled", 0)
  for _ = 1, 2 do
    local best, bestd = nil, 0
    for i = 1, #SPILLS do
      local bit = 1 << (i - 1)
      if used & bit == 0 then
        local dx, dz = SPILLS[i][1] - x, SPILLS[i][2] - z
        local d = dx * dx + dz * dz
        if best == nil or d < bestd then best, bestd = i, d end
      end
    end
    if best ~= nil then
      used = used | (1 << (best - 1))
      mission.spawn("spill." .. (best - 1))
    end
  end
  mission.set("spilled", used)
end

local function caravans(now)
  local n = get("caravan", -1)
  local next_at = get("caravan_at", 0)
  if n < 0 or not mission.get("caravan_live") then
    -- Between caravans: the next one sets out from the west edge.
    if now < next_at or n + 1 >= CARAVANS or mission.get("fallen") then return end
    n = n + 1
    for i = 0, 1 do
      mission.spawn(member(n, "cart", i))
      mission.spawn(member(n, "guard", i))
    end
    mission.set("caravan", n)
    mission.set("caravan_live", true)
    mission.set("waypoint", 1)
    mission.set("heading", 1)
    mission.set("hop_at", now + SECOND)
    if n == 0 then start_talk("caravan") end
    return
  end
  -- The carts: the caravan is ambushed when both are gone.
  local lead = nil
  for i = 0, 1 do
    if lead == nil and mission.alive(member(n, "cart", i)) then lead = member(n, "cart", i) end
  end
  if lead == nil then
    mission.set("caravan_live", false)
    mission.set("caravan_at", now + 100 * SECOND)
    spill(get("cx", ROUTE[1][1]), get("cz", ROUTE[1][2]))
    local count = get("ambushed", 0) + 1
    mission.set("ambushed", count)
    if count <= AMBUSHES then mission.set("ambush_talk", count) end
    if count == AMBUSHES then mission.complete_objective("caravans") end
    return
  end
  local x, z = mission.position(lead)
  mission.set("cx", x)
  mission.set("cz", z)
  local wp = get("waypoint", 1)
  local target = ROUTE[wp]
  local dx, dz = x - target[1], z - target[2]
  local hop_at = get("hop_at", 0)
  local close = dx * dx + dz * dz <= 25
  if not close and now < hop_at then return end
  if close then
    -- At a waypoint: the next one; at either end, rest and turn back.
    local heading = get("heading", 1)
    local rest = 0
    if wp + heading > #ROUTE or wp + heading < 1 then
      heading = -heading
      mission.set("heading", heading)
      rest = 14 * SECOND
    end
    wp = wp + heading
    mission.set("waypoint", wp)
    if rest > 0 then
      mission.set("hop_at", now + rest)
      return
    end
    target = ROUTE[wp]
  elseif now < hop_at then
    return
  end
  -- Send the column on (and again when a hop takes too long: a fight, a crowd).
  local ox, oz = 0, 0
  if wp > 1 and ROUTE[wp][1] == ROUTE[wp - 1][1] then ox = 2 else oz = 2 end
  for i = 0, 1 do
    local s = 1 - 2 * i
    if mission.alive(member(n, "cart", i)) then
      mission.move(member(n, "cart", i), target[1] + s * ox, target[2] + s * oz)
    end
    if mission.alive(member(n, "guard", i)) then
      mission.attack_move(member(n, "guard", i), target[1] + s * 2 * ox - 3 * oz, target[2] + s * 2 * oz - 3 * ox)
    end
  end
  mission.set("hop_at", now + 30 * SECOND)
end

-- Beats --------------------------------------------------------------------------------

beats.road = function()
  mission.set("phase", "road")
  mission.begin_objective("speak")
  mission.begin_objective("caravans")
  mission.set("caravan_at", mission.tick() + 45 * SECOND)
end

beats.war = function()
  mission.set("phase", "war")
  mission.complete_objective("speak")
  mission.begin_objective("condenser")
  mission.ai_waves(2, true)
end

beats.win = function()
  mission.complete_objective("condenser")
  mission.win()
end

-- The engineer by the toll house: snores while a hero is near; the Queen wakes him.
local function engineer(now)
  if get("woken", false) or not mission.alive("engineer") then return end
  local ex, ez = mission.position("engineer")
  local qx, qz = mission.position("queen")
  if qx == nil then return end
  local dx, dz = qx - ex, qz - ez
  local d = dx * dx + dz * dz
  if d <= 36 then
    if start_talk("woken") then
      mission.set("woken", true)
      if not mission.give_item("queen", "item.moon-dew") then
        if not mission.give_item("oleander", "item.moon-dew") then
          mission.give_item("korrath", "item.moon-dew")
        end
      end
      mission.face("engineer", qx, qz)
    end
  elseif d <= 400 and now >= get("snore_at", 0) then
    if start_talk("snore") then mission.set("snore_at", now + 14 * SECOND) end
  end
end

local function finds()
  local seen = get("seen", 0)
  for i = 1, #FINDS do
    local bit = 1 << (i - 1)
    if seen & bit == 0 and mission.in_region("queen", FINDS[i][2]) then
      if start_talk(FINDS[i][1]) then mission.set("seen", seen | bit) end
      return
    end
  end
end

local function tunnel()
  if get("tunnel", 0) == 2 then return end
  if not mission.alive("tunnel.rubble") then
    mission.set("tunnel", 2)
    mission.spawn("tunnel.in")
    mission.spawn("tunnel.out")
    start_scene("tunnel")
    return true
  end
  if get("tunnel", 0) == 0 and mission.alive("korrath") and mission.in_region("queen", "rubble") then
    if start_talk("rubble") then mission.set("tunnel", 1) end
  end
end

function on_start()
  mission.set("phase", "opening")
  start_scene("opening")
end

function on_tick()
  local now = mission.tick()
  if not mission.alive("camp.nest") then
    mission.lose()
    return
  end
  if not mission.alive("queen") and mission.count("player.1", "building.ants.sanctuary") == 0 then
    mission.lose()
    return
  end

  -- A scene in progress runs alone.
  local scene = mission.get("scene")
  if scene ~= nil then
    local steps = SCENES[scene]
    local step = run_steps(steps, get("step", 1), now, true)
    if step > #steps then
      mission.set("scene", nil)
    else
      mission.set("step", step)
    end
    return
  end

  -- A talk in progress: its next line once the last is spoken.
  local talk = mission.get("talk")
  if talk ~= nil and not mission.dialogue_busy() then
    local lines = TALKS[talk]
    local i = get("line", 1)
    if i > #lines then
      mission.set("talk", nil)
    else
      line(lines[i][1], lines[i][2], lines[i][3], false)
      mission.set("line", i + 1)
    end
  end

  local phase = get("phase", "opening")
  if phase == "road" or phase == "war" then
    caravans(now)
    local a = mission.get("ambush_talk")
    if a ~= nil and start_talk("ambush" .. a) then mission.set("ambush_talk", nil) end
    engineer(now)
    finds()
    if tunnel() then return end
  end

  if phase == "road" then
    if mission.in_region("queen", "gate") or mission.in_region("queen", "forecourt")
        or mission.in_region("queen", "station") then
      start_scene("gate")
    end
  elseif phase == "war" then
    if not mission.get("fallen") and not mission.alive("station.bastion") then
      mission.set("fallen", true)
      mission.ai_waves(2, false)
      mission.ai_behaviour(2, "passive")
      mission.objective("Bring Aurelith to the condenser.")
      if mission.alive("vosk") then
        mission.set("talk", nil)
        start_talk("fallen")
      end
    end
    if mission.get("fallen") and mission.in_region("queen", "condenser") then
      mission.set("phase", "closing")
      start_scene("closing")
    end
  end
end
