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
-- gather on the open ground south-east of the mossy ring, Tiv (raider.1) in front, so she
-- speaks to them looking south-east and the shots on her look back over the glade to the
-- Nest (ground the column has seen: what it has not is still dark).
local COLUMN = {
  {"raider.1", 250, 286}, {"raider.2", 256, 292}, {"raider.3", 248, 296}, {"raider.4", 260, 284},
  {"needler.1", 258, 300}, {"needler.2", 266, 292},
  {"worker.1", 186, 222}, {"worker.2", 180, 226}, {"worker.3", 190, 228}, {"worker.4", 168, 256},
  {"worker.5", 172, 262},
}
local RISE = {236, 271}
local ADDRESS = {254, 292} -- where she turns to speak to her column
-- Singles from a little higher than the presets' own eye: under the canopy the sky is
-- dark, so a shot looks down onto the glade behind its speaker.
local LOOK_DOWN = { height = 15 }
local MEET = {queen = {182, 238}, oleander = {166, 238}, porters = {{159, 232}, {158, 244}}}
local BURROWS_NE = {"burrow.ne.1", "burrow.ne.2"}
local BURROWS_NW = {"burrow.nw"}
-- The optional ways: the Hollow's stragglers in the south-west gullies (player 4, kneeling
-- by their snail shell until the Queen finds them) and the hornets' hoard on their plateau.
local STRAYS = {"stray.1", "stray.2", "stray.3"}
local HORNETS = {"hornets.1", "hornets.2", "hornets.3"}
local HOARD = 300 -- amber

-- Places that say something the first time the Queen (or Oleander, once he has joined)
-- comes near: region, speaker, portrait, line.
local FINDS = {
  {"sap", "speaker.aurelith", QUEEN, "line.founding.sap-from-an-old-root"},
  {"merchant", "speaker.pill-bug-peddler", "building.neutral.merchant", "line.founding.salves-flasks-wraps-for-cold"},
  {"bog", "speaker.aurelith", QUEEN, "line.founding.toads-mind-their-tongues-and"},
  {"roots", "speaker.aurelith", QUEEN, "line.founding.the-roots-here-are-awake"},
  {"hornets", "speaker.aurelith", QUEEN, "line.founding.hornets-leave-them-be-until"},
  {"falls", "speaker.aurelith", QUEEN, "line.founding.behind-the-water-someone-hid"},
  {"bridge", "speaker.aurelith", QUEEN, "line.founding.one-root-across-the-ravine"},
  {"ridge", "speaker.aurelith", QUEEN, "line.founding.their-burrows-this-is-where"},
  {"spring", "speaker.aurelith", QUEEN, "line.founding.substance-welling-up-raw-at"},
  {"nook", "speaker.aurelith", QUEEN, "line.founding.a-dew-spring-walled-in"},
  {"mushrooms", "speaker.aurelith", QUEEN, "line.founding.caplings-let-the-young-warriors"},
  {"den", "speaker.aurelith", QUEEN, "line.founding.look-at-the-pool-they"},
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
  for i = 1, #STRAYS do
    mission.animate(STRAYS[i], "kneel", true) -- resting, until the Queen finds them
  end
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
  mission.wait(3)
  mission.shot("two", "raider.1", "queen") -- Tiv running beside her, the glade before them
  mission.speak("raider.1", "line.founding.my-queen-the-glade-just", nil, "speaker.tiv")
  mission.camera_to("glade-mouth", 3)
  mission.wait_arrived("queen")
  mission.face("queen", ADDRESS[1], ADDRESS[2])
  mission.wait_arrived("raider.1")
  mission.face("raider.1", RISE[1], RISE[2])
  mission.wait_facing()
  mission.shot("wide", "queen", { height = 22 }) -- on the rise, the glade behind her
  mission.animate("queen", "spell")
  mission.speak("queen", "line.founding.here-we-start-again-and", nil, "speaker.aurelith")
  mission.shot("over", "queen", "raider.1") -- past her onto Tiv (a single from the ring sees moss)
  mission.speak("raider.1", "line.founding.and-if-the-brood-that", nil, "speaker.tiv")
  mission.shot("single", "queen", LOOK_DOWN)
  mission.speak("queen", "line.founding.then-they-find-us-ready", nil, "speaker.aurelith")
  mission.camera_to("glade-wide", 3)
  mission.wait(1.5)
  mission.begin_objective("colony")
  mission.cinematic(false)
  mission.camera_reset(1.5, NEST[1], NEST[2] + 16)
end

-- After the second wave: Oleander comes along the west road with his porters. The Queen
-- meets him when she is near the glade, else speaks from afar ("queen-near", set before).
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
  -- The shot follows him down the road (the west is unexplored: his own sight lights it).
  mission.shot("wide", "oleander")
  mission.speak("oleander", "line.founding.majesty-hold-your-stings-it", nil, "speaker.oleander")
  mission.wait_arrived("oleander")
  local near = mission.get("queen-near")
  -- His lines on him (she stands a head taller: over her shoulder looks down on him), hers
  -- over his shoulder; from afar, the Queen speaks over him.
  local function on_oleander(kind)
    if near and kind == "two" then
      mission.shot("two", "oleander", "queen")
    else
      mission.shot("single", "oleander", { height = 11 })
    end
  end
  if near then
    mission.wait_arrived("queen")
    mission.face("queen", MEET.oleander[1], MEET.oleander[2])
    mission.face("oleander", MEET.queen[1], MEET.queen[2])
    mission.wait_facing()
  else
    mission.face("oleander", NEST[1], NEST[2])
    mission.wait_facing()
  end
  on_oleander()
  mission.speak("oleander", "line.founding.i-had-your-letter-i", nil, "speaker.oleander")
  if near then
    mission.shot("over", "oleander", "queen")
    mission.speak("queen", "line.founding.then-tell-me-it-can", nil, "speaker.aurelith")
  else
    mission.say("speaker.aurelith", QUEEN, "line.founding.then-tell-me-it-can", 5, true)
    mission.wait_line()
  end
  on_oleander("two")
  mission.speak("oleander", "line.founding.not-by-ants-not-yet", nil, "speaker.oleander")
  if near then
    mission.shot("single", "queen", LOOK_DOWN)
    mission.animate("queen", "spell")
    mission.speak("queen", "line.founding.then-we-will-learn-their", nil, "speaker.aurelith")
  else
    mission.say("speaker.aurelith", QUEEN, "line.founding.then-we-will-learn-their", 5, true)
    mission.wait_line()
  end
  -- He answers on his own shot, and the camera goes to the spring as he names it.
  on_oleander()
  mission.say("speaker.oleander", ALCHEMIST, "line.founding.gladly-and-you-have-a", 7.5, false, "oleander")
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

-- Victory: both burrow sites have fallen. Whoever speaks is framed where the fighting left
-- them ("qx"/"ox" set as the scene starts while they live; on_tick sets them).
function scenes.victory()
  mission.cinematic(true)
  -- Together when they stand close: turned to each other, she over his shoulder.
  local both = false
  if mission.get("qx") and mission.get("ox") then
    local qx, qy, ox, oy = mission.get("qx"), mission.get("qy"), mission.get("ox"), mission.get("oy")
    both = (qx - ox) * (qx - ox) + (qy - oy) * (qy - oy) < 30 * 30
    if both then
      mission.face("queen", ox, oy)
      mission.face("oleander", qx, qy)
      mission.wait_facing()
    end
  end
  if mission.get("qx") then
    if both then
      mission.shot("over", "oleander", "queen")
    else
      mission.shot("single", "queen", LOOK_DOWN)
    end
    mission.speak("queen", "line.founding.the-burrows-are-broken-the", nil, "speaker.aurelith")
  else
    mission.camera_to("glade-wide", 0)
    mission.say("speaker.aurelith", QUEEN, "line.founding.the-burrows-are-broken-the", 4, false)
    mission.wait_line()
  end
  if mission.get("ox") then
    if both then
      mission.shot("two", "oleander", "queen")
    else
      mission.shot("single", "oleander", { height = 11 })
    end
    mission.speak("oleander", "line.founding.then-the-beetles-are-next", nil, "speaker.oleander")
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
        mission.say("speaker.oleander", ALCHEMIST, "line.founding.look-at-it-shine-raw", 7, false, "oleander")
      else
        mission.say(find[2], find[3], find[4], 7, false)
      end
      return
    end
  end
end

-- An optional objective, begun once (it may be begun by its finding or by its time).
local function offer(id)
  local key = "offered-" .. id
  if not mission.get(key) then
    mission.set(key, true)
    mission.begin_objective(id)
  end
end

-- The Hollow's stragglers: offered a little after the colony is founded; when the Queen
-- (or Oleander) reaches them, they stand and join her.
local function stragglers()
  if mission.get("strays-done") then
    return false
  end
  if not mission.get("offered-strays") and mission.tick() >= 75 * SECOND then
    offer("strays")
    return true
  end
  local alive = {}
  for i = 1, #STRAYS do
    if mission.alive(STRAYS[i]) then
      alive[#alive + 1] = STRAYS[i]
    end
  end
  if #alive == 0 then
    mission.set("strays-done", true)
    offer("strays")
    mission.fail_objective("strays")
    return true
  end
  local finder = nil
  if mission.alive("queen") and mission.in_region("queen", "strays") then
    finder = "queen"
  elseif mission.get("oleander-joined") and mission.alive("oleander") and mission.in_region("oleander", "strays") then
    finder = "oleander"
  end
  if not finder then
    return false
  end
  mission.set("strays-done", true)
  offer("strays")
  local x, y = mission.position(finder)
  for i = 1, #alive do
    mission.transfer(alive[i], P1)
    mission.move(alive[i], x + 4 * i - 8, y + 6)
  end
  mission.complete_objective("strays")
  return true
end

-- The hornets' hoard: offered when the Queen first sees their plateau (or once the colony
-- stands); when the last hornet falls, its amber goes to the colony (their standard drops).
local function hoard()
  if mission.get("hoard-done") then
    return false
  end
  if not mission.get("offered-hoard") and (mission.get("found-hornets") or mission.get("colony-done")) then
    offer("hoard")
    return true
  end
  if mission.get("offered-hoard") and not any_alive(HORNETS) then
    mission.set("hoard-done", true)
    mission.give_stock(P1, "item.amber", HOARD)
    mission.complete_objective("hoard")
    return true
  end
  return false
end

-- The sleeping Alate in its flower: click it, and it sneezes pollen over everyone near it.
local function alate()
  if mission.get("sneezed") or mission.clicks("alate-sleeper") < 3 then
    return
  end
  mission.set("sneezed", true)
  mission.animate("alate-sleeper", "attack")
  mission.say("speaker.sleeping-alate", "unit.ants.winglancer", "line.founding.hhh-hhhh-haa-choo", 4, false, "alate-sleeper")
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
    mission.objective("line.founding.the-nest-has-fallen")
    mission.lose()
    return
  end
  if not mission.alive("queen") and mission.count(P1, "building.ants.sanctuary") == 0 then
    mission.set("over", true)
    mission.objective("line.founding.aurelith-has-fallen-and-no")
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
    mission.say("speaker.aurelith", QUEEN, "line.founding.the-colony-stands-now-we", 7, false)
    return
  end
  -- The burrows.
  if not mission.get("ne-down") and not any_alive(BURROWS_NE) then
    mission.set("ne-down", true)
    mission.say("speaker.aurelith", QUEEN, "line.founding.the-ridge-burrows-are-broken", 6, false)
    return
  end
  if not mission.get("nw-down") and not any_alive(BURROWS_NW) then
    mission.set("nw-down", true)
    mission.say("speaker.aurelith", QUEEN, "line.founding.the-den-is-empty-nothing", 6, false)
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
      mission.say("speaker.oleander", ALCHEMIST, "line.founding.there-drawn-not-drunk-that", 7, false, "oleander")
    end
    return
  end
  -- The first wave, then Oleander after the second (waves at 3:00 and 4:40).
  if tick >= 178 * SECOND and not mission.get("warned") then
    mission.set("warned", true)
    mission.say("speaker.aurelith", QUEEN, "line.founding.movement-on-the-ridge-and", 6, false)
    return
  end
  if tick >= 345 * SECOND and not mission.get("oleander-called") and not mission.dialogue_busy() then
    mission.set("oleander-called", true)
    mission.set("queen-near", mission.in_region("queen", "glade"))
    mission.play("oleander")
    return
  end
  if stragglers() or hoard() then
    return
  end
  alate()
  discoveries()
end
