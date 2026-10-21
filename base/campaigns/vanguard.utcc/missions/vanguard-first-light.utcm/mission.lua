-- A placeholder mission for the campaign screens: the Queen clears a hollow.
local creatures = { "clearing.0", "clearing.1", "clearing.2", "clearing.3", "clearing.4", "clearing.5", "clearing.6", "clearing.7" }

function on_start()
  mission.begin_objective("clear")
end

function on_tick()
  if not mission.alive("queen") then
    mission.lose()
    return
  end
  for i = 1, #creatures do
    if mission.alive(creatures[i]) then
      return
    end
  end
  mission.complete_objective("clear")
  mission.win()
end
