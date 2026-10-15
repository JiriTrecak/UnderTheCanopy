# Art issues found by the cooker

Problems in the published assets that the old editors should fix (the data is mirrored from the old repository, D9). `just cook --verbose` lists the current set. None blocks the game; each is cooked to match what the old game showed.

| Assets | Problem | Effect today | Fix in the old editor |
|---|---|---|---|
| 22 foliage models: `frost-grass`, `frost-snow-grass`, `pl-clover-a`, `pl-clover-sorrel`, `pl-daisy`, `pl-fern-bushy`, `pl-fern-bushy-b`, `pl-fern-sword`, `pl-fern-sword-b`, `pl-flowers-blue`, `pl-flowers-bluebell`, `pl-flowers-trillium`, `pl-flowers-white`, `pl-flowers-yellow`, `pl-grass-lush-a/b/c`, `pl-grass-tall`, `woodland-grass-high-a/b`, `woodland-grass-low`, `woodland-grass-messy` (all `asset.models.environment.*`) | `capabilities.wind` declared, but no node carries the `foliageWind` extras the renderer uses | They don't sway (neither did they in the old game) | Re-export with `foliageWind` on the swaying nodes, or drop `capabilities.wind` |
| `autumn-tree-copper`, `autumn-tree-gold`, `canopy-oak` | Hit/fall/decay clips, but no `capabilities.animations` states | The cooker and renderer find the clips by name, as the old game did | Declare the animation states |
| `asset.placeholder.missing-model` | Same as above | — | Declare states, or drop the clips |
| `asset.models.resources.amber-deposit` | Faction/building profile without team colour | Fine for a neutral deposit; owners of a captured deposit couldn't be told apart | Confirm it is always neutral, or add a team-colour mask |
| Every unit and building | Published `team_mask.png` files are never read: team colour comes from the base colour's alpha, as in the old game | No visible problem | Either keep the alpha convention and stop publishing the masks, or switch the cooker to the masks (decide once) |
