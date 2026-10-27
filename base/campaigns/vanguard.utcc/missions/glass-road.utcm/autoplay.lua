-- The Glass Road, played by its plan (docs/campaign.md "Autoplay"): the camp is the
-- skirmish AI's, held to the mission's tech. The plan
-- 1. breaks the rubble at the camp's north-west edge (the old tunnel's secret);
-- 2. takes the west outpost on the road, wakes the engineer by the toll house, clears the
--    briar amber (optional) and takes the east outpost at the top of the main ramp;
-- 3. brings the Queen to the gate with thirty at her back (Vosk's parley);
-- 4. while the gate still holds the lodge in, takes the Hollow Stag's grove (optional);
-- 5. sends the Queen home while the army breaks the gate and the Great Bastion;
-- 6. brings her to the condenser.

hero{"queen"}
economy(true)

-- 1. The old tunnel, behind the rubble north-west of the camp.
wait_clock(1)
go(72, 296)
attack("tunnel.rubble")

-- 2. The road's west outpost, the engineer, the briar amber, the east outpost.
wait_clock(9)
mass(18, 4 * 60)
attack("outpost.west.tower.0")
attack("outpost.west.tower.1")
go(228, 276)
if wait_until(function() return alive("engineer") ~= nil end, 60) then
  click("engineer", 3)
end
go(330, 246)
go(368, 250)
wait_objective("briar", 90)
mass(18, 3 * 60)
attack("outpost.east.tower.0")
attack("outpost.east.tower.1")

-- 3. The gate: the Queen before it with the army.
mass(30, 5 * 60)
go(340, 166, "walk")
wait_objective("speak", 60)

-- 4. The grove, while the lodge is shut in behind its gate.
mass(30, 3 * 60)
go(300, 360)
go(352, 364)
wait_objective("stag", 120)

-- 5. The Queen goes home while the army breaks in: her fall would lose it all.
keep("queen", 112, 330)
mass(30, 4 * 60)
go(340, 140)
attack("station.gate")
attack("station.gate.1")
while alive("station.bastion") ~= false do
  mass(30, 3 * 60)
  go(340, 98)
  go(362, 78)
  attack("station.bastion")
end
go(340, 84)

-- 6. The condenser: the Queen comes back while the army clears the yard around it.
keep("queen", 340, 78)
while objective("condenser") ~= "completed" do
  mass(18, 4 * 60)
  push(372, 70, 12)
  wait(60)
  push(340, 80, 12)
  wait(30)
end
