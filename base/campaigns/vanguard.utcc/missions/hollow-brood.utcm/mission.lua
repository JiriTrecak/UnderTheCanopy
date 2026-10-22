-- The Hollow Brood (Prologue, Mission One; docs/story.md).
--
-- Queen Aurelith's column walks the root path from the south-west to the Hollow, a sunken
-- basin in the north-east whose brood has gone silent. Along the way: caches, a mushroom
-- circle, the ford, a dead sentry, the lost patrol in the webs, the sick pool, the
-- terrace, the survivors behind the rubble, the Moonlit Hollow (secret) and the frog.
-- Mother Ashka waits in the Hollow. Win when she is put to rest; lose with the Queen.
--
-- Players: 1 the Queen; 8 the feral brood (enemy, passive posts; white: bled pale); 3 the survivors (ally,
-- dormant); 4 Tiv's lost patrol (ally, dormant). Scenes follow docs/campaign.md "Scenes":
-- they branch only on variables set before they play.

local function near(region)
  return mission.in_region("queen", region) or mission.in_region("tiv", region)
end

local function all_dead(ids)
  for i = 1, #ids do
    if mission.alive(ids[i]) then
      return false
    end
  end
  return true
end

-- A line outside the scenes, once, when nobody else is speaking. True once said.
local function line(flag, unit, text, name)
  if mission.get(flag) then
    return true
  end
  if mission.dialogue_busy() or not mission.alive(unit) then
    return false
  end
  mission.set(flag, true)
  mission.speak(unit, text, nil, name)
  return true
end

-- What a scene may branch on, fixed as it starts.
local function play(name)
  mission.set("tiv_ok", mission.alive("tiv"))
  local x, y = mission.position("queen")
  mission.set("qx", x)
  mission.set("qy", y)
  mission.play(name)
end

-- A shot on a unit where it stands now: a saved camera's angle, panned onto it (a walk in a
-- scene ends short when the army stands in its way).
local function shot_on(camera, unit)
  local x, y = mission.position(unit)
  mission.camera_to(camera, 0)
  mission.camera_pan(x, y, 0)
end

local function column(x, y)
  mission.move("queen", x, y)
  mission.move("raider.1", x - 5, y + 3)
  mission.move("raider.2", x - 2, y + 7)
  mission.move("raider.3", x - 9, y + 6)
  mission.move("raider.4", x - 6, y + 10)
  mission.move("needler.1", x - 12, y + 10)
  mission.move("needler.2", x - 9, y + 14)
end

function on_start()
  mission.fade_out(0)
  play("opening")
end

function on_tick()
  if not mission.alive("queen") then
    mission.lose()
    return
  end
  if mission.playing() then
    return
  end
  if not mission.get("opened") then
    return
  end

  -- A Reaver Queen starves without prey (Ravenous); the deep pool feeds Ashka until the
  -- Queen comes for her.
  if not mission.get("ashka_seen") and mission.tick() % 200 == 0 and mission.alive("ashka") then
    mission.recover("ashka")
  end

  -- The end: Mother Ashka falls.
  if mission.get("ashka_seen") then
    if mission.alive("ashka") then
      local x, y = mission.position("ashka")
      mission.set("ax", x)
      mission.set("ay", y)
    elseif not mission.get("closing") then
      mission.set("closing", true)
      play("closing")
      return
    end
  elseif not mission.alive("ashka") and not mission.get("closing") then
    -- Killed before she was ever seen up close.
    local x, y = mission.position("queen")
    mission.set("ax", x)
    mission.set("ay", y)
    mission.set("closing", true)
    play("closing")
    return
  end

  -- 1. The caches by the road.
  if not mission.get("caches") and mission.tick() > (mission.get("opened_at") or 0) + 40 then
    line("caches", "tiv", "Hollow caches, my Queen. Their foragers hid what they could spare. Break them open.", "Tiv")
  end

  -- 2. The mushroom circle.
  if not mission.get("mushrooms") and mission.get("caches") and mission.in_region("queen", "mushrooms") then
    line("mushrooms", "tiv", "Caplings. They sleep in rings like that, and wake hungry.", "Tiv")
  end

  -- 4. The dead sentry: one of the Hollow's watch staggers out of the ferns and falls.
  if not mission.get("sentry_seen") and mission.in_region("queen", "sentry") then
    mission.set("sentry_seen", true)
    play("sentry")
    return
  end

  -- 5. The lost patrol.
  if not mission.get("patrol_told") and (near("junction") or near("webs")) then
    if line("patrol_line", "tiv", "My Queen... my brother Kesh took a patrol north-west, toward the old webs. They never came back.", "Tiv")
        or not mission.alive("tiv") then
      mission.set("patrol_told", true)
      mission.begin_objective("patrol")
    end
  end
  if not mission.get("patrol_woken") and all_dead({ "webs.0", "webs.1", "webs.2", "webs.spitter.0" }) then
    mission.set("patrol_woken", true)
    mission.ai_wake(4)
    if not mission.give_item("queen", "item.scout-whistle") then
      mission.spawn("whistle")
    end
    return
  end
  if mission.get("patrol_woken") and not mission.get("patrol_freed") then
    mission.set("patrol_freed", true)
    play("patrol")
    return
  end

  -- 6. The sick pool.
  if not mission.get("sick") and near("sick") then
    if line("sick", "queen", "It shouldn't glow like that.", "Aurelith") then
      mission.set("sick_at", mission.tick())
    end
  end
  if mission.get("sick_at") and not mission.get("sick2") and mission.tick() > mission.get("sick_at") + 140 then
    line("sick2", "tiv", "Those are Hollow ants drinking, my Queen. Or they were.", "Tiv")
  end

  -- Places along the way.
  if not mission.get("spring") and near("spring") then
    line("spring", "tiv", "A sap spring. Rest here if the road wears on you.", "Tiv")
  end
  if not mission.get("husk") and near("husk") then
    line("husk", "queen", "A beetle's husk... and their crates. They come this far from their glass roads?", "Aurelith")
  end
  if not mission.get("hornets") and near("hornets") then
    line("hornets", "tiv", "Hornets. Unless you want the whole nest on us, my Queen, let them be.", "Tiv")
  end
  -- 9. Secret: the Moonlit Hollow.
  if not mission.get("glade") and mission.in_region("queen", "glade") then
    line("glade", "queen", "An old broodkeepers' shrine... and its lantern still burns. Let the wisp sleep.", "Aurelith")
  end

  -- 8. The rim: the Hollow comes into view once its picket is down, or the rubble breaks.
  if not mission.get("rim") then
    if (near("rim") and all_dead({ "feral.rim.1", "feral.rim.2" })) or not mission.alive("rubble") then
      mission.set("rim", true)
      play("rim")
      return
    end
  elseif not mission.get("found") and not mission.alive("rubble") then
    if not mission.get("survivors_woken") then
      mission.set("survivors_woken", true)
      mission.ai_wake(3)
      return
    end
    mission.set("found", true)
    play("survivors")
    return
  end

  -- 11. Mother Ashka, once the way down is clear.
  if not mission.get("ashka_seen") and mission.alive("ashka") then
    if (near("hollow") and all_dead({ "feral.gate.1", "feral.gate.2", "feral.gate.3" })) or near("nest") then
      mission.set("ashka_seen", true)
      local x, y = mission.position("ashka")
      mission.set("ax", x)
      mission.set("ay", y)
      play("ashka")
      return
    end
  end

  -- 10. The easter egg: the frog on the lily pool, clicked ten times.
  if not mission.get("frog") and mission.alive("frog") and mission.clicks("frog") >= 10 then
    mission.set("frog", true)
    play("frog")
  end
end

-- The opening: fade in on the column already running along the root path; Tiv runs up.
function scenes.opening()
  mission.cinematic(true)
  mission.camera_to("open-road", 0)
  column(86, 318)
  mission.fade_in(2.5)
  mission.camera_to("open-follow", 6)
  mission.wait(1.5)
  mission.move("tiv", 97, 311)
  mission.wait_arrived("queen")
  mission.wait_arrived("tiv")
  mission.face("queen", 97, 311)
  mission.face("tiv", 86, 318)
  mission.camera_to("over-queen", 0)
  mission.wait_facing()
  mission.speak("tiv", "My Queen! The Hollow's watch-lights went dark three nights ago.", nil, "Tiv")
  mission.speak("tiv", "We sent scouts. Not one came back.", nil, "Tiv")
  mission.camera_to("over-tiv", 0)
  mission.speak("queen", "Mother Ashka would never let her lights go dark.", nil, "Aurelith")
  mission.animate("queen", "spell")
  mission.speak("queen", "Then we go and ask them why.", nil, "Aurelith")
  mission.camera_to("open-wide", 3)
  column(102, 309)
  mission.move("tiv", 110, 304)
  mission.wait(2.5)
  mission.begin_objective("reach")
  mission.set("opened", true)
  mission.set("opened_at", mission.tick())
  mission.cinematic(false)
  mission.camera_reset(1.5, 100, 310)
end

-- One of the Hollow's watch staggers out of the ferns towards them and falls.
function scenes.sentry()
  mission.cinematic(true)
  mission.camera_to("sentry", 1.2)
  mission.move("queen", 189, 243)
  if mission.get("tiv_ok") then
    mission.move("tiv", 201, 238)
  end
  mission.spawn("sentry")
  mission.move("sentry", 195, 237)
  mission.wait_arrived("queen")
  mission.face("queen", 195, 237)
  mission.wait_arrived("sentry")
  if mission.get("tiv_ok") then
    mission.wait_arrived("tiv")
    mission.face("tiv", 195, 237)
  end
  shot_on("sentry-low", "queen")
  mission.damage("sentry", 5000)
  mission.speak("queen", "This was one of Ashka's. Look at its shell.", nil, "Aurelith")
  if mission.get("tiv_ok") then
    shot_on("sentry-tiv", "tiv")
    mission.speak("tiv", "Split open... as if something grew inside it.", nil, "Tiv")
    shot_on("sentry-low", "queen")
  end
  mission.speak("queen", "Stay close. Whatever did this is still out here.", nil, "Aurelith")
  mission.cinematic(false)
  mission.camera_reset(1.2)
end

-- The Queen comes to the webs' mouth; Tiv runs to his brother; Kesh turns from one to the other.
function scenes.patrol()
  mission.cinematic(true)
  mission.camera_to("webs", 1.0)
  mission.move("queen", 99, 198)
  mission.face("patrol.kesh", 99, 198)
  mission.face("patrol.2", 99, 198)
  mission.wait(0.8)
  if mission.get("tiv_ok") then
    mission.move("tiv", 90, 199)
    mission.face("patrol.kesh", 90, 199)
    mission.camera_to("webs-kesh", 0)
    mission.speak("patrol.kesh", "Tiv? Tiv! I knew you'd come.", nil, "Kesh")
    mission.wait_arrived("tiv")
    mission.face("tiv", 84, 192)
    shot_on("webs-tiv", "tiv")
    mission.speak("tiv", "Hold still, brother. These webs stink.", nil, "Tiv")
    mission.face("patrol.kesh", 99, 198)
  else
    mission.camera_to("webs-kesh", 0)
    mission.speak("patrol.kesh", "Light... the Queen's light. You found us.", nil, "Kesh")
  end
  mission.camera_to("webs-kesh", 0)
  mission.speak("patrol.kesh", "We tracked the Hollow's foragers in here, my Queen. The webs were waiting.", nil, "Kesh")
  mission.face("queen", 84, 192)
  mission.speak("patrol.kesh", "Take my whistle. It has called us home through worse.", nil, "Kesh")
  shot_on("webs-queen", "queen")
  mission.speak("queen", "Then walk home with us, Kesh. We go to the Hollow.", nil, "Aurelith")
  mission.transfer("patrol.kesh", "player.1")
  mission.transfer("patrol.2", "player.1")
  if mission.get("patrol_told") then
    mission.complete_objective("patrol")
  end
  mission.cinematic(false)
  mission.camera_reset(1.2)
end

function scenes.rim()
  mission.cinematic(true)
  mission.camera_to("rim-wide", 1.5)
  mission.move("queen", 300, 114)
  if mission.get("tiv_ok") then
    mission.move("tiv", 302, 121)
  end
  mission.wait(1.0)
  mission.speak("queen", "The Hollow. Its nest still glows...", nil, "Aurelith")
  mission.face("queen", 304, 80)
  shot_on("rim-queen", "queen")
  mission.speak("queen", "...but nothing down there moves the way it should.", nil, "Aurelith")
  if mission.get("tiv_ok") then
    mission.face("tiv", 330, 145) -- towards the fallen tunnel
    shot_on("rim-tiv", "tiv")
    mission.speak("tiv", "Listen! Digging, my Queen. Behind the fallen tunnel, east of here.", nil, "Tiv")
    shot_on("rim-queen", "queen")
  else
    mission.speak("queen", "Listen. Someone is digging behind that fallen tunnel.", nil, "Aurelith")
  end
  mission.speak("queen", "Survivors. Break through to them.", nil, "Aurelith")
  mission.complete_objective("reach")
  mission.begin_objective("survivors")
  mission.cinematic(false)
  mission.camera_reset(1.2, 314, 130)
end

function scenes.survivors()
  mission.cinematic(true)
  mission.camera_to("chamber", 1.0)
  mission.move("nurse", 330, 141)
  mission.move("survivor.1", 325, 146)
  mission.move("survivor.2", 335, 146)
  mission.move("survivor.3", 330, 150)
  mission.move("queen", 330, 133)
  mission.wait_arrived("nurse")
  mission.face("nurse", 330, 133)
  mission.face("queen", 330, 141)
  mission.face("survivor.1", 330, 133)
  mission.face("survivor.2", 330, 133)
  mission.face("survivor.3", 330, 133)
  -- They kneel to her where their walks end; the transfers below stand them up.
  mission.animate("nurse", "kneel", true)
  mission.animate("survivor.1", "kneel", true)
  mission.animate("survivor.2", "kneel", true)
  mission.animate("survivor.3", "kneel", true)
  -- Down past the Queen onto the kneeling, and up at her as they see her.
  shot_on("over-nurse", "nurse")
  mission.speak("nurse", "Queen Aurelith... you came. We prayed someone would.", nil, "Nurse Ardi")
  shot_on("survivors-queen", "queen")
  mission.speak("queen", "What happened here?", nil, "Aurelith")
  shot_on("over-nurse", "nurse")
  mission.speak("nurse", "They drank from the deep pool. Then they stopped sleeping.", nil, "Nurse Ardi")
  mission.speak("nurse", "Mother Ashka drank first, and drank the most. She does not know us now.", nil, "Nurse Ardi")
  shot_on("over-survivor", "survivor.1")
  mission.speak("survivor.1", "Let us fight beside you, my Queen. Let us put her to rest.", nil, "Hollow Raider")
  shot_on("survivors-queen", "queen")
  mission.speak("queen", "Then come. All of you who can.", nil, "Aurelith")
  mission.transfer("survivor.1", "player.1")
  mission.transfer("survivor.2", "player.1")
  mission.transfer("survivor.3", "player.1")
  mission.move("nurse", 328, 164)
  mission.complete_objective("survivors")
  mission.begin_objective("ashka")
  mission.cinematic(false)
  mission.camera_reset(1.2, 318, 130)
end

-- Ashka rises from the pool and turns to them, and does not know them.
function scenes.ashka()
  mission.cinematic(true)
  mission.camera_to("hollow-wide", 1.0)
  mission.wait(1.0)
  mission.camera_to("ashka", 0)
  mission.animate("ashka", "spell")
  mission.face("queen", 296, 68)
  if mission.get("tiv_ok") then
    mission.face("tiv", 296, 68)
  end
  mission.wait(0.8)
  mission.move("ashka", 296, 68)
  mission.wait_arrived("ashka")
  mission.face("ashka", mission.get("qx"), mission.get("qy"))
  mission.wait_facing("ashka")
  shot_on("ashka-queen", "queen")
  mission.speak("queen", "Mother Ashka! It is Aurelith, of the Amber Court.", nil, "Aurelith")
  mission.camera_to("ashka-close", 0)
  mission.speak("ashka", "...so loud... the song is so loud...", nil, "Mother Ashka")
  if mission.get("tiv_ok") then
    shot_on("ashka-tiv", "tiv")
    mission.speak("tiv", "She doesn't know you, my Queen.", 3, "Tiv")
  end
  shot_on("ashka-queen", "queen")
  mission.speak("queen", "Then I will help her sleep.", nil, "Aurelith")
  if not mission.get("found") then
    mission.objective("Put Mother Ashka to rest")
  end
  mission.cinematic(false)
  mission.camera_reset(1.0)
  -- Days without sleep have hollowed her: she comes at them already starving.
  mission.damage("ashka", 1000)
  mission.attack_move("ashka", mission.get("qx"), mission.get("qy"))
end

-- Ashka, lucid for a moment as she dies; Aurelith's vow.
function scenes.closing()
  mission.cinematic(true)
  -- From the side, the Queen standing over her; then the Queen past her, swearing.
  mission.camera_to("closing", 0)
  mission.camera_pan(mission.get("ax") + 1.5, mission.get("ay") + 1.5, 0)
  mission.move("queen", mission.get("ax") + 3, mission.get("ay") + 3)
  mission.wait_arrived("queen")
  mission.face("queen", mission.get("ax"), mission.get("ay"))
  mission.wait_facing("queen")
  mission.say("Mother Ashka", "unit.ants.reaver-queen", "It sang to us... we drank... and it gave, and gave...", 6)
  mission.wait_line()
  shot_on("closing-queen", "queen")
  mission.speak("queen", "No ant will drink like this again. I swear it.", nil, "Aurelith")
  mission.wait(1.5)
  mission.fade_out(2.5)
  mission.wait(2.5)
  if mission.get("found") then
    mission.complete_objective("ashka")
  end
  mission.win()
end

-- The tree frog, clicked ten times: it croaks, hops away and leaves a Heartseed.
function scenes.frog()
  mission.speak("frog", "Rrrrribbit.", 2, "Tree Frog")
  mission.animate("frog", "attack")
  mission.wait(0.6)
  mission.move("frog", 100, 290)
  mission.spawn("heartseed")
end
