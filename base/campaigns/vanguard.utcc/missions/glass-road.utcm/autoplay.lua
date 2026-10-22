-- The Glass Road, played by its plan (docs/campaign.md "Autoplay"): the camp is the
-- skirmish AI's, held to the mission's tech. The plan
-- 1. breaks the rubble at the camp's north edge (the old tunnel's secret);
-- 2. takes the road's outposts (their towers watch the narrows before the station) and
--    wakes the engineer by the toll house on the way;
-- 3. brings the Queen to the gate with thirty at her back (Vosk's parley);
-- 4. sends her home while the army breaks the gate and the Great Bastion;
-- 5. walks her to the condenser.

hero{"queen"}
economy(true)

-- 1. The old tunnel, behind the rubble north of the camp.
wait_clock(1)
go(70, 272)
attack("tunnel.rubble")

-- 2. The outposts on the road, and the engineer.
wait_clock(9)
mass(18, 4 * 60)
attack("outpost.west.tower.0")
attack("outpost.west.tower.1")
go(214, 196)
if wait_until(function() return alive("engineer") ~= nil end, 60) then
  click("engineer", 3)
end
mass(18, 3 * 60)
attack("outpost.east.tower.0")
attack("outpost.east.tower.1")

-- 3. The gate: the Queen at its foot with the army.
mass(30, 5 * 60)
go(340, 164)
wait_objective("speak", 60)

-- 4. The Queen goes home while the army breaks in: her fall would lose it all.
keep("queen", 112, 330)
mass(30, 4 * 60)
go(340, 146)
attack("station.gate")
mass(24, 4 * 60)
attack("station.bastion")
go(340, 80)

-- 5. The condenser.
keep("queen")
go(340, 76, "hero")
while objective("condenser") ~= "completed" do
  mass(18, 4 * 60)
  push(372, 64, 12)
  wait(90)
  push(340, 72, 12)
  go(340, 76, "hero")
  wait(30)
end
