-- Founding (docs/story.md, Mission 2). Each callback runs in a fresh Lua state: what lasts
-- is a mission variable (mission.get / mission.set). Scenes (docs/campaign.md "Scenes") run
-- from their top every tick and pass over the statements already done, so they branch only
-- on variables set before they start, and each ends in the state the player should find
-- (Esc skips the rest at once).

local P1 = "player.1"
local QUEEN = "unit.ants.queen"
local ALCHEMIST = "unit.ants.royal-alchemist"
local SECOND = 40 -- ticks
local NEST = {202, 242}

-- The column that arrives in the opening, and where each stands in the glade: the warriors
-- gather on the open ground at the mouth of the mossy ring, Tiv (raider.1) in front.
local COLUMN = {
  {"raider.1", 226, 276}, {"raider.2", 221, 281}, {"raider.3", 218, 274}, {"raider.4", 214, 280},
  {"needler.1", 217, 285}, {"needler.2", 211, 276},
  {"worker.1", 186, 222}, {"worker.2", 180, 226}, {"worker.3", 190, 228}, {"worker.4", 168, 256},
  {"worker.5", 172, 262},
}
local RISE = {236, 271}
local ADDRESS = {222, 279} -- where she turns to speak to her column
local MEET = {queen = {182, 238}, oleander = {166, 238}, porters = {{159, 232}, {158, 244}}}
local BURROWS_NE = {"burrow.ne.1", "burrow.ne.2"}
local BURROWS_NW = {"burrow.nw"}

-- Places that say something the first time the Queen (or Oleander, once she has joined)
-- comes near: region, speaker, portrait, line.
local FINDS = {
  {"sap", "Aurelith", QUEEN, "Sap from an old root, warm as a brood chamber. Bring the wounded here."},
  {"merchant", "Pill-Bug Peddler", "building.neutral.merchant", "Salves! Flasks! Wraps for cold mornings! Amber only, Majesty. I'm a simple bug."},
  {"bog", "Aurelith", QUEEN, "Toads. Mind their tongues, and keep the workers clear until they're gone."},
  {"roots", "Aurelith", QUEEN, "The roots here are awake. They guard that amber like their own sap."},
  {"hornets", "Aurelith", QUEEN, "Hornets. Leave them be until we're stronger, or until we need what they hoard."},
  {"falls", "Aurelith", QUEEN, "Behind the water... someone hid this here, and sealed it in amber."},
  {"bridge", "Aurelith", QUEEN, "One root across the ravine, and their den on the far side. Whoever holds this root holds the west."},
  {"ridge", "Aurelith", QUEEN, "Their burrows. This is where the waves hatch. Break them, and the glade is ours."},
  {"spring", "Aurelith", QUEEN, "Substance, welling up raw at our door. We leave it be. For now."},
  {"nook", "Aurelith", QUEEN, "A dew spring, walled in behind the rubble. Someone wanted it to themselves."},
  {"mushrooms", "Aurelith", QUEEN, "Caplings. Let the young warriors learn their work on something that bites back gently."},
  {"den", "Aurelith", QUEEN, "Look at the pool they drink from. That glow is what took Ashka's brood."},
}

local function any_alive(ids)
  for i = 1, #ids do
    if mission.alive(ids[i]) then
      return true
    end
  end
  return false
end

function on_start()
  mission.fade_out(0)
  mission.animate("alate-sleeper", "sleep", true) -- until the sneeze wakes it
  mission.play("opening")
end

-- The opening: the column comes up the south road into the glade; the Queen climbs the
-- mossy rise and plants her standard.
function scenes.opening()
  mission.cinematic(true)
  mission.camera_to("arrival", 0)
  mission.move("queen", RISE[1], RISE[2])
  for i = 1, #COLUMN do
    mission.move(COLUMN[i][1], COLUMN[i][2], COLUMN[i][3])
  end
  mission.fade_in(2.5)
  mission.camera_pan(205, 312, 6) -- running up the road towards the glade
  mission.wait(2)
  mission.speak("raider.1", "My Queen, the glade! Just as the scouts said.", nil, "Tiv")
  mission.camera_to("glade-mouth", 3)
  mission.wait_arrived("queen")
  mission.face("queen", ADDRESS[1], ADDRESS[2])
  mission.wait_arrived("raider.1")
  mission.face("raider.1", RISE[1], RISE[2])
  mission.camera_to("rise-front", 0)
  mission.wait_facing("queen")
  mission.animate("queen", "spell")
  mission.speak("queen", "Here. We start again, and we start clean.", nil, "Aurelith")
  mission.camera_to("rise-over", 0)
  mission.speak("raider.1", "And if the brood that ate the Hollow follows us here?", nil, "Tiv")
  mission.camera_to("rise-front", 0)
  mission.speak("queen", "Then they find us ready. Workers, to the amber. Raise a Brood Mound and a Soldier Burrow.", nil, "Aurelith")
  mission.camera_to("glade-wide", 3)
  mission.wait(1.5)
  mission.begin_objective("colony")
  mission.cinematic(false)
  mission.camera_reset(1.5, NEST[1], NEST[2] + 16)
end

-- After the second wave: Oleander comes along the west road with her porters. The Queen
-- meets her when she is near the glade, else speaks from afar ("queen-near", set before).
function scenes.oleander()
  mission.ai_waves(2, false)
  mission.cinematic(true)
  mission.fade_out(0.6)
  mission.wait(0.6)
  mission.ai_wake(3)
  mission.camera_to("west-road", 0)
  mission.move("oleander", MEET.oleander[1], MEET.oleander[2])
  mission.move("porter.1", MEET.porters[1][1], MEET.porters[1][2])
  mission.move("porter.2", MEET.porters[2][1], MEET.porters[2][2])
  if mission.get("queen-near") then
    mission.move("queen", MEET.queen[1], MEET.queen[2])
  end
  mission.fade_in(1.2)
  mission.wait(1.5)
  -- The shot tracks her down the road (the west is unexplored: her own sight lights it).
  mission.camera_pan(MEET.oleander[1] - 6, MEET.oleander[2], 11)
  mission.speak("oleander", "Majesty! Hold your stings. It's only me, and my stills.", nil, "Oleander")
  mission.wait(5) -- the shot follows her the rest of the way
  mission.wait_arrived("oleander")
  if mission.get("queen-near") then
    mission.wait_arrived("queen")
    mission.face("queen", MEET.oleander[1], MEET.oleander[2])
    mission.face("oleander", MEET.queen[1], MEET.queen[2])
    mission.camera_to("meet-over-queen", 0)
    mission.wait_facing()
  else
    mission.face("oleander", NEST[1], NEST[2])
    mission.camera_to("meet-over-queen", 0)
  end
  mission.speak("oleander", "I had your letter. I've studied substance all my life. I know what it did to Ashka's brood.", nil, "Oleander")
  if mission.get("queen-near") then
    mission.camera_to("meet-over-oleander", 0)
    mission.speak("queen", "Then tell me it can be held without it changing us.", nil, "Aurelith")
  else
    mission.say("Aurelith", QUEEN, "Then tell me it can be held without it changing us.", 5, true)
    mission.wait_line()
  end
  mission.camera_to("meet-over-queen", 0)
  mission.speak("oleander", "Not by ants. Not yet. But in the east there are beetles who keep it in glass, and are not changed at all.", nil, "Oleander")
  if mission.get("queen-near") then
    mission.camera_to("meet-over-oleander", 0)
    mission.animate("queen", "spell")
    mission.speak("queen", "Then we will learn their glass. Stay with us, Oleander.", nil, "Aurelith")
  else
    mission.say("Aurelith", QUEEN, "Then we will learn their glass. Stay with us, Oleander.", 5, true)
    mission.wait_line()
  end
  -- She answers on her own shot, and the camera goes to the spring as she names it.
  mission.camera_to("meet-over-queen", 0)
  mission.say("Oleander", ALCHEMIST, "Gladly. And you have a spring at your door, raw and bright. Siphon it, carefully, and I can begin.", 7.5, false, "oleander")
  mission.wait(2)
  mission.camera_to("spring", 3)
  mission.wait_line()
  mission.transfer("oleander", P1)
  mission.transform("porter.1", "unit.ants.settler", P1)
  mission.transform("porter.2", "unit.ants.settler", P1)
  mission.set("oleander-joined", true)
  mission.ai_waves(2, true)
  mission.begin_objective("spring")
  mission.cinematic(false)
  mission.camera_reset(1.5, MEET.oleander[1], MEET.oleander[2])
end

-- A speaker where the fighting left them, turned to the camera ("<key>x"/"<key>y" set as the
-- scene starts while they live; on_tick sets them).
local function close_on(unit, key, camera)
  local x, y = mission.get(key .. "x"), mission.get(key .. "y")
  mission.face(unit, x, y + 10)
  mission.camera_to(camera, 0)
  mission.camera_pan(x, y, 0)
  mission.wait_facing(unit)
end

-- Victory: both burrow sites have fallen.
function scenes.victory()
  mission.cinematic(true)
  if mission.get("qx") then
    close_on("queen", "q", "victory-close")
    mission.speak("queen", "The burrows are broken. The glade is ours.", nil, "Aurelith")
  else
    mission.camera_to("glade-wide", 0)
    mission.say("Aurelith", QUEEN, "The burrows are broken. The glade is ours.", 4, false)
    mission.wait_line()
  end
  if mission.get("ox") then
    close_on("oleander", "o", "victory-close")
    mission.speak("oleander", "Then the beetles are next, Majesty. Their glass is waiting.", nil, "Oleander")
  end
  mission.camera_to("victory", 6)
  mission.wait(4)
  mission.fade_out(2)
  mission.wait(2)
  mission.win()
end

local function discoveries()
  if mission.dialogue_busy() then
    return
  end
  local joined = mission.get("oleander-joined")
  for i = 1, #FINDS do
    local find = FINDS[i]
    local key = "found-" .. find[1]
    if not mission.get(key) and (mission.in_region("queen", find[1]) or (joined and mission.in_region("oleander", find[1]))) then
      mission.set(key, true)
      if find[1] == "spring" and joined then
        mission.say("Oleander", ALCHEMIST, "Look at it shine. Raw, and so patient. A Siphon on it, Majesty, and I'll keep it in hand.", 7, false, "oleander")
      else
        mission.say(find[2], find[3], find[4], 7, false)
      end
      return
    end
  end
end

-- The sleeping Alate in its flower: click it, and it sneezes pollen over everyone near it.
local function alate()
  if mission.get("sneezed") or mission.clicks("alate-sleeper") < 3 then
    return
  end
  mission.set("sneezed", true)
  mission.animate("alate-sleeper", "attack")
  mission.say("Sleeping Alate", "unit.ants.winglancer", "Hhh... hhhh... HAA-CHOO!", 4, false, "alate-sleeper")
  if mission.in_region("queen", "alate-near") then
    mission.recover("queen")
  end
  if mission.get("oleander-joined") and mission.in_region("oleander", "alate-near") then
    mission.recover("oleander")
  end
  mission.move("alate-sleeper", 118, 212)
end

function on_tick()
  if mission.get("over") then
    return
  end
  -- Defeat, also while a scene plays.
  if not mission.alive("colony.nest") then
    mission.set("over", true)
    mission.objective("The Nest has fallen.")
    mission.lose()
    return
  end
  if not mission.alive("queen") and mission.count(P1, "building.ants.sanctuary") == 0 then
    mission.set("over", true)
    mission.objective("Aurelith has fallen, and no Royal Chamber stands to revive her.")
    mission.lose()
    return
  end
  if mission.playing() then
    return
  end
  local tick = mission.tick()
  -- The colony: a Brood Mound and a Soldier Burrow.
  if not mission.get("colony-done") and tick > 5 * SECOND and mission.count(P1, "building.ants.house") >= 1
      and mission.count(P1, "building.ants.barracks") >= 1 then
    mission.set("colony-done", true)
    mission.complete_objective("colony")
    mission.begin_objective("burrows")
    mission.say("Aurelith", QUEEN, "The colony stands. Now we take the fight to them: the burrows on the ridge, and the den across the ravine.", 7, false)
    return
  end
  -- The burrows.
  if not mission.get("ne-down") and not any_alive(BURROWS_NE) then
    mission.set("ne-down", true)
    mission.say("Aurelith", QUEEN, "The ridge burrows are broken. Nothing more hatches there.", 6, false)
    return
  end
  if not mission.get("nw-down") and not any_alive(BURROWS_NW) then
    mission.set("nw-down", true)
    mission.say("Aurelith", QUEEN, "The den is empty. Nothing must nest there again.", 6, false)
    return
  end
  if mission.get("ne-down") and mission.get("nw-down") and mission.get("colony-done") then
    mission.complete_objective("burrows")
    mission.set("over", true)
    if mission.alive("queen") then
      local x, y = mission.position("queen")
      mission.set("qx", x)
      mission.set("qy", y)
    end
    if mission.get("oleander-joined") and mission.alive("oleander") then
      local x, y = mission.position("oleander")
      mission.set("ox", x)
      mission.set("oy", y)
    end
    mission.play("victory", false)
    return
  end
  -- The optional substance spring (Oleander sets it when she arrives).
  if mission.get("oleander-joined") and not mission.get("spring-done")
      and mission.count(P1, "building.ants.substance-well") >= 1 and not mission.dialogue_busy() then
    mission.set("spring-done", true)
    mission.complete_objective("spring")
    if mission.alive("oleander") then
      mission.say("Oleander", ALCHEMIST, "There. Drawn, not drunk. That is the whole difference, Majesty, and it is everything.", 7, false, "oleander")
    end
    return
  end
  -- The first wave, then Oleander after the second (waves at 3:00 and 4:40).
  if tick >= 178 * SECOND and not mission.get("warned") then
    mission.set("warned", true)
    mission.say("Aurelith", QUEEN, "Movement on the ridge, and across the ravine. They've smelled the new Nest. To arms!", 6, false)
    return
  end
  if tick >= 345 * SECOND and not mission.get("oleander-called") and not mission.dialogue_busy() then
    mission.set("oleander-called", true)
    mission.set("queen-near", mission.in_region("queen", "glade"))
    mission.play("oleander")
    return
  end
  alate()
  discoveries()
end
