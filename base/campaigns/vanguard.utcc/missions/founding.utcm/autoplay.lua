-- Founding, played by its plan (docs/campaign.md "Autoplay"): the colony is the skirmish
-- AI's, held to the mission's tech. The plan takes the Queen's column down the south-west
-- gullies to the Hollow's stragglers, wakes the sleeping Alate, clears the hornets'
-- plateau for their hoard, then sends the army against the ridge burrows and across the
-- root bridge into the den.

hero{"queen"}
economy(true)

-- The stragglers, at once (the colony's AI would take the Queen creeping): down the south
-- road, into the sap spring's hollow, west along the gully to their snail shell. They
-- join, and everyone comes home past the sleeping Alate: three clicks and it sneezes.
go(196, 340)
go(140, 348)
go(70, 352)
wait_objective("strays", 30)
go(140, 348)
go(156, 214)
click("alate-sleeper", 3)
go(204, 268)

-- The hornets' hoard: over the stream and up onto their plateau, once the army has grown.
wait_clock(8)
mass(10, 120)
go(228, 124)
go(222, 70)
wait_objective("hoard", 90)
pickup()
go(222, 150)

-- The ridge burrows; once they are seen broken, across the root bridge into the den.
local function gone(ids)
  return function()
    for i = 1, #ids do
      if alive(ids[i]) ~= false then return false end
    end
    return true
  end
end
wait_clock(12)
push(292, 150)
wait_clock(14)
push(320, 84)
wait_until(gone({"burrow.ne.1", "burrow.ne.2"}), 9 * 60)
push(96, 196)
go(96, 196)
wait(30)
push(86, 92)
wait_objective("burrows")
