-- The Glass Road (Act I, Mission Three; docs/story.md). Authored with the map by
-- tools/maps/glass_road.py, which fills in the caravans' spill spots below and writes the
-- saved cameras the scenes cut between (mission.cameras).
--
-- Scenes (docs/campaign.md "Scenes"; each skippable, each ends in the state the player
-- should find):
--   opening   fade in on Aurelith and Oleander walking into the forward camp; Korrath waits;
--             the conversation, cut over the shoulders; he salutes and joins.
--   tunnel    the rubble north of the camp broken: the old tunnel comes up behind the
--             station's west wall.
--   gate      at the foot of the station, Coilkeeper Vosk refuses the Queen from the wall;
--             the War Lodge wakes. Its waves march once the station's gate (the road's
--             way into the forecourt) is broken: until then they could not get out.
--   closing   the station fallen, the Queen at the condenser; Oleander's line; victory.
-- Over play (lines in on_tick): the caravans, the places the Queen finds, Korrath and the
-- rubble, the sleeping engineer (click him three times), Vosk when the station falls.
-- Lose: the camp's Nest falls, or the Queen dies with no Royal Chamber to revive her.
--
-- A scene re-runs from its top every run and passes over what it did: it branches only on
-- variables. on_tick runs first and keeps `live_<hero>` current, so a line goes to the
-- living speaker or, if the speaker has fallen, plays from the portrait alone.
--
-- Later (no capability yet): a kneel for Korrath (he salutes with his cast clip), the
-- condenser's own rising light (the closing fades to violet instead).

local SECOND = 40
local SPILLS = { --@SPILLS@
}
local ROUTE = { { 36, 216 }, { 100, 216 }, { 168, 216 }, { 168, 160 }, { 216, 160 }, { 276, 160 },
  { 340, 160 }, { 340, 142 } }  -- the last at the station's gate
local CARAVANS = 6
local AMBUSHES = 3
local HEROES = { "queen", "oleander", "korrath" }

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

-- A line over play (no waiting): with the speaker's live portrait when it stands.
local function line(id, text, seconds)
  local w = who[id]
  local actor = nil
  if mission.alive(id) then actor = id end
  mission.say(w[1], w[2], text, seconds, false, actor)
end

-- A line in a scene: it waits until spoken. Two statements on either branch.
local function talk(id, text)
  local w = who[id]
  if mission.get("live_" .. id) then
    mission.speak(id, text, nil, w[1])
  else
    local seconds = 1 + #text / 15
    if seconds < 2.5 then seconds = 2.5 end
    mission.say(w[1], w[2], text, seconds, false)
  end
  mission.wait_line()
end

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
  gate_down = { { "korrath", "The gate is down, Majesty! Now the lodge comes out to meet us.", 5 } },
  inside = {
    { "vosk", "Ants inside my walls? Seal the yard! Wake the lodge!", 5 },
    { "queen", "We came to ask, Coilkeeper. Now we take.", 4 },
  },
  snore = { { "engineer", "Hnnnk... zzz... pressure's fine... zzz...", 3 } },
  woken = {
    { "engineer", "Wha- ants?! I wasn't sleeping! I was listening to the boiler!", 5 },
    { "engineer", "Fine, fine. Take this, and don't tell the Coilkeeper you saw me.", 5 },
  },
}

-- Places the Queen finds, each spoken once: talk, region.
local FINDS = { { "outpost", "outpost-w" }, { "toll", "toll" }, { "bog", "bog" }, { "briar", "briar" },
  { "grove", "grove" }, { "glade", "glade" } }

local function start_talk(name)
  if mission.get("talk") ~= nil or mission.dialogue_busy() then return false end
  mission.set("talk", name)
  mission.set("line", 1)
  return true
end

-- The War Lodge's waves: at once while the station's gate is open, else once it breaks
-- (a closed gate would hold them in the forecourt).
local function waves()
  if mission.alive("station.gate") then
    mission.set("waves_wait", true)
  else
    mission.ai_waves(2, true)
  end
end

-- Scenes -------------------------------------------------------------------------------

function scenes.opening()
  mission.cinematic(true)
  -- Shot presets (mission.shot) frame each speaker where they stand, so a walk that stops
  -- short of its mark still has its lines on the right faces.
  mission.shot("wide", "camp")
  -- Units placed by the map take orders from the tick after the first.
  mission.wait(0.1)
  -- Already walking into the camp when the picture comes up.
  mission.move("queen", 113, 327)
  mission.move("oleander", 103, 332)
  mission.face("korrath", 113, 340)
  mission.fade_in(3)
  mission.shot("wide", "korrath", { side = "left", seconds = 7 })
  mission.wait_arrived("queen")
  mission.face("queen", 112, 316)
  mission.face("oleander", 112, 317)
  mission.face("korrath", 113, 327)
  mission.wait_facing()
  mission.shot("over", "queen", "korrath")
  talk("korrath", "Majesty. Your mother set me to hold the end of this road. I hear her daughter means to walk it.")
  mission.shot("over", "korrath", "queen")
  talk("queen", "The beetles keep substance in glass, General, and it never changes them. I mean to learn how.")
  mission.shot("single", "oleander")
  talk("oleander", "They keep a condenser at the far end of the road. Whatever they know is in that machine.")
  mission.shot("over", "queen", "korrath")
  talk("korrath", "And when they will not share it?")
  mission.shot("over", "korrath", "queen")
  talk("queen", "Then I will take it. No brood of ours will rot in the dark again.")
  mission.shot("single", "korrath")
  talk("korrath", "Your mother would have called it madness.")
  mission.shot("single", "queen")
  talk("queen", "My mother is not here.")
  mission.shot("single", "korrath")
  mission.wait(1.2)
  mission.animate("korrath", "kneel", true)
  mission.wait(1)
  talk("korrath", "...Then my mandibles are yours, my Queen.")
  mission.animate("korrath", "idle")
  mission.shot("wide", "outpost-w", { seconds = 4 })
  talk("oleander", "The road runs north of the camp. Its outposts will not let us pass quietly.")
  mission.set("phase", "road")
  mission.set("caravan_at", 45 * SECOND) -- counted from the end of the opening, below
  mission.begin_objective("speak")
  mission.begin_objective("caravans")
  mission.cinematic(false)
  mission.camera_reset(1.5, 112, 300)
end

function scenes.tunnel()
  mission.cinematic(true)
  mission.camera_to("tunnel-out", 0)
  mission.wait(1)
  talk("korrath", "It runs under the road and comes up behind their west wall. They have forgotten it was ever dug.")
  talk("queen", "Then we have not.")
  mission.cinematic(false)
  mission.camera_reset(1, 64, 262)
end

function scenes.gate()
  mission.cinematic(true)
  mission.shot("wide", "station")
  mission.move("queen", 340, 154)
  mission.move("oleander", 328, 163)
  mission.move("korrath", 344, 163)
  mission.move("vosk", 340, 98)
  mission.shot("wide", "gate", { seconds = 5 })
  mission.wait(2)
  mission.face("vosk", 340, 154)
  mission.face("queen", 340, 98)
  -- Each line on its speaker, where the walk left them: a single from the front, or over
  -- the listener's shoulder.
  mission.shot("single", "queen")
  talk("queen", "Keeper of the glass! I am Aurelith, Queen of the ants. I would speak with you.")
  mission.shot("over", "queen", "vosk")
  talk("vosk", "Coilkeeper Vosk. I know what you are, little queen. Your broods drink the light raw, and it eats them from the inside.")
  mission.shot("over", "vosk", "queen")
  talk("queen", "Then teach us your glass, and they will never drink again.")
  mission.shot("single", "vosk")
  talk("vosk", "Glass is not for those who drink.")
  mission.shot("over", "queen", "vosk")
  talk("vosk", "Turn your column around. The next ant on my road goes home in pieces.")
  mission.shot("single", "korrath")
  if mission.alive("station.gate") then
    talk("korrath", "They're waking the lodge behind that gate, Majesty. Break it, and they come out.")
  else
    talk("korrath", "They're opening the lodge, Majesty. Here they come.")
  end
  mission.set("phase", "war")
  mission.complete_objective("speak")
  mission.begin_objective("condenser")
  waves()
  mission.cinematic(false)
  mission.move("vosk", 344, 86) -- back from the gate, to his post by the condenser
  mission.camera_reset(1.5, 340, 160)
  talk("oleander", "The condenser is behind that wall. Break their Great Bastion and the station is ours.")
end

function scenes.closing()
  mission.cinematic(true)
  mission.shot("wide", "station")
  mission.move("queen", 340, 70)
  mission.move("oleander", 348, 74)
  mission.move("korrath", 332, 74)
  mission.shot("wide", "condenser", { seconds = 5 })
  mission.wait_arrived("queen")
  mission.face("queen", 340, 56)
  mission.face("oleander", 340, 56)
  mission.face("korrath", 340, 62)
  mission.shot("single", "queen")
  mission.wait(1)
  mission.animate("queen", "cast")
  talk("queen", "It's warm. It hums... like something breathing in its sleep.")
  mission.shot("single", "oleander")
  talk("oleander", "Now we can learn what it is.")
  mission.camera_to("cond-rise", 5)
  -- The condenser wakes to her: its light rises, and keeps rising into the fade.
  mission.effect_at("condenser_hum", "effect.buildings.alembic-glow", 340, 57)
  mission.light_at("condenser", 340, 57, "#9b6cff", 40, 18, 4, 3)
  mission.wait(2)
  mission.fade_out(3, "#2b1450")
  mission.wait(3)
  mission.complete_objective("condenser")
  mission.win()
end

-- The caravans -------------------------------------------------------------------------

local function member(n, role, i)
  return "caravan." .. n .. "." .. role .. "." .. i
end

local function spill(x, z)
  -- The two nearest spots not used yet get one of its goods each.
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
  if n < 0 or not mission.get("caravan_live") then
    -- Between caravans: the next one sets out from the west edge.
    if now < get("caravan_at", 0) or n + 1 >= CARAVANS or mission.get("fallen") then return end
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
    -- Its guards, carts lost, fall back up the road to the station's gate, fighting
    -- whatever they meet: the gate's guard grows with every escort that survives.
    local gate = ROUTE[#ROUTE]
    for i = 0, 1 do
      if mission.alive(member(n, "guard", i)) then
        mission.attack_move(member(n, "guard", i), gate[1] - 4 + 8 * i, gate[2] + 4)
      end
    end
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
    if wp + heading > #ROUTE or wp + heading < 1 then
      heading = -heading
      mission.set("heading", heading)
      mission.set("waypoint", wp + heading)
      mission.set("hop_at", now + 14 * SECOND)
      return
    end
    wp = wp + heading
    mission.set("waypoint", wp)
    target = ROUTE[wp]
  end
  -- Send the column on (and again when a hop takes too long: a fight, a crowd).
  local ox, oz = 0, 3
  if wp > 1 and ROUTE[wp][1] == ROUTE[wp - 1][1] then ox, oz = 3, 0 end
  local back = 6 * get("heading", 1)
  for i = 0, 1 do
    local s = 1 - 2 * i
    if mission.alive(member(n, "cart", i)) then
      mission.move(member(n, "cart", i), target[1] + s * ox, target[2] + s * oz)
    end
    if mission.alive(member(n, "guard", i)) then
      -- The guards walk a little behind the carts, ready to fight.
      mission.attack_move(member(n, "guard", i), target[1] + s * 2 * ox - back * oz / 2,
        target[2] + s * 2 * oz - back * ox / 2)
    end
  end
  mission.set("hop_at", now + 30 * SECOND)
end

-- Over play ----------------------------------------------------------------------------

-- The engineer asleep by the toll house: snores while a hero is near; three clicks wake him.
local function engineer(now)
  if get("woken", false) or not mission.alive("engineer") then return end
  if mission.clicks("engineer") >= 3 then
    if start_talk("woken") then
      mission.set("woken", true)
      local given = false
      for i = 1, #HEROES do
        if not given and mission.alive(HEROES[i]) then given = mission.give_item(HEROES[i], "item.moon-dew") end
      end
      local qx, qz = mission.position("queen")
      mission.animate("engineer", "idle")
      if qx ~= nil then mission.face("engineer", qx, qz) end
    end
    return
  end
  if now >= get("snore_at", 0) and mission.in_region("queen", "engineer") and start_talk("snore") then
    mission.set("snore_at", now + 14 * SECOND)
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

-- True when a scene starts.
local function tunnel()
  local t = get("tunnel", 0)
  if t == 2 then return false end
  if not mission.alive("tunnel.rubble") then
    mission.set("tunnel", 2)
    mission.spawn("tunnel.in")
    mission.spawn("tunnel.out")
    mission.play("tunnel")
    return true
  end
  if t == 0 and mission.alive("korrath") and mission.in_region("queen", "rubble") and start_talk("rubble") then
    mission.set("tunnel", 1)
  end
  return false
end

function on_start()
  mission.set("phase", "opening")
  for i = 1, #HEROES do mission.set("live_" .. HEROES[i], true) end
  mission.set("live_vosk", true)
  mission.animate("engineer", "sleep", true) -- until three clicks wake him
  mission.fade_out(0)
  mission.play("opening")
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
  for i = 1, #HEROES do mission.set("live_" .. HEROES[i], mission.alive(HEROES[i])) end
  mission.set("live_vosk", mission.alive("vosk"))
  if mission.playing() then return end

  -- A talk in progress: its next line once the last is spoken.
  local talk_name = mission.get("talk")
  if talk_name ~= nil and not mission.dialogue_busy() then
    local lines = TALKS[talk_name]
    local i = get("line", 1)
    if i > #lines then
      mission.set("talk", nil)
    else
      line(lines[i][1], lines[i][2], lines[i][3])
      mission.set("line", i + 1)
    end
  end

  local phase = get("phase", "opening")
  if phase == "opening" then return end
  if get("started", 0) == 0 then
    -- The opening is over (played or skipped): the caravans' clock starts now.
    mission.set("started", now)
    mission.set("caravan_at", now + get("caravan_at", 0))
  end
  caravans(now)
  local a = mission.get("ambush_talk")
  if a ~= nil and start_talk("ambush" .. a) then mission.set("ambush_talk", nil) end
  engineer(now)
  if tunnel() then return end
  finds()

  if phase == "road" then
    if mission.in_region("queen", "gate") or mission.in_region("queen", "forecourt") then
      mission.set("talk", nil)
      mission.play("gate")
    elseif mission.in_region("queen", "station") then
      -- In by the old tunnel, before ever knocking at the gate: no parley.
      mission.set("phase", "war")
      mission.complete_objective("speak")
      mission.begin_objective("condenser")
      waves()
      mission.set("talk", nil)
      start_talk("inside")
    end
  elseif phase == "war" then
    if mission.get("waves_wait") and not mission.alive("station.gate") then
      mission.set("waves_wait", nil)
      if not mission.get("fallen") then
        mission.ai_waves(2, true)
        mission.set("pending_gate", true)
      end
    end
    if mission.get("pending_gate") and start_talk("gate_down") then mission.set("pending_gate", nil) end
    if not mission.get("fallen") and not mission.alive("station.bastion") then
      mission.set("fallen", true)
      mission.ai_waves(2, false)
      mission.ai_behaviour(2, "passive")
      mission.objective("Bring Aurelith to the condenser.")
      if mission.alive("vosk") then
        mission.set("talk", nil)
        mission.set("pending_fallen", true)
      end
    end
    if mission.get("pending_fallen") and start_talk("fallen") then mission.set("pending_fallen", nil) end
    if mission.get("fallen") and mission.in_region("queen", "condenser") then
      mission.set("phase", "closing")
      mission.set("talk", nil)
      mission.play("closing")
    end
  end
end
