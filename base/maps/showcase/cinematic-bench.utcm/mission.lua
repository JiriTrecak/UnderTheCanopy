-- The cinematic bench: an opening scene (docs/campaign.md "Scenes").

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
  mission.move("aurelith", 84, 112)
  mission.move("escort.1", 78, 118)
  mission.move("escort.2", 81, 121)
  mission.fade_in(2.5)
  mission.camera_to("follow", 6)
  mission.wait(1.5)
  mission.move("tiv", 94, 102)
  mission.wait_arrived("aurelith")
  mission.wait_arrived("tiv")
  mission.face("aurelith", 94, 102)
  mission.face("tiv", 84, 112)
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
  mission.camera_reset(1.5, 84, 112)
end
