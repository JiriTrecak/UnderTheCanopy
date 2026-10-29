// Everything the unit exploration workbench (index.html) shows. Add to it freely: findings,
// ideas, images (in images/), answers and decisions. Statuses: idea, open, agreed, rejected.
// Agreed decisions also go to docs/decisions.md; this page is the scratch record.
window.EXPLORE = {
  lede: "A throwaway workbench for redesigning every race's units: readability first, an identity per race and a role you can see. Started 2026-10-29.",

  standard: {
    why: "Warcraft III and StarCraft II units read at a glance because Blizzard designed them for the RTS camera first: unique silhouettes from above, pushed proportions, bold colour, punchy animation and effects that never hide the army. Our current ants share one body plan and blur together (see Ants: Pictures).",
    rules: [
      ["Silhouette first", "Every unit has a body plan of its own and passes a black-shape test at game zoom, from above and at game angle, on one sheet with the whole army. Zergling and Drone differ by body, not by decoration."],
      ["Push proportions", "Big heads and signatures, thick limbs, fewer and fatter legs where the design allows. Nothing thin carries the read."],
      ["One role signature", "One oversized feature that says the role, readable from above: the Footman's shield, the Rifleman's gun, the Peasant's sack."],
      ["A size ladder", "Worker < soldiers < elites < heroes < the centrepiece, which dwarfs everything (an Ultralisk to a Hydralisk)."],
      ["Colour", "A saturated race palette with clear light and dark values, and a large team-colour area on top (back plates, shoulders, short capes, wing membranes) on every unit."],
      ["Animation", "Punchy, quick attacks; two or three move-cycle variants so groups never march in lockstep."],
      ["Effects", "Tight and quick; never covering the units."],
      ["Grounded in a true fact", "Every unit grows from one true, surprising fact about its real animal (a door-head is born a living door): it gives the lore, the look and later the abilities."],
      ["Built for the top", "Identity on the back and sides, things that spread horizontally; hunched, forward-leaning posture; short garments, no long robes. Concepts are drawn from the RTS camera (~55–60° down) and checked small."]
    ]
  },

  history: [
    {date: "2026-10-29", text: "Materials by race (Jiri): metal is the beetles' (technology), ivory and bone the bees' (the blood and holy order), and the ants wear only what the colony grows, gathers or secretes: chitin, amber, resin, leaf, silk, clay, fungus."},
    {date: "2026-10-18", text: "Substance is the forest's magic, a glowing liquid (never crystals). Ants take it in and mutate; bees are a blood and holy order that transform through substance and ritual; beetles never take it in and use it only through technology."},
    {date: "2026-10-19", text: "The first ant concept set was rejected: every unit a plain six-legged ant, hard to tell apart. Ants should mix ant-like and humanoid bodies. Their magic is bio-magic (alive, luminous, blossoms, amber), never corruption."},
    {date: "2026-10-20", text: "Humanoid ant concepts were rejected as unreadable from above (upright, front-facing, robed figures become the same cone). Basic soldiers became ant creatures (the Raider as the ants' Zergling, the Needler Hydralisk-inspired); humanoid was kept for specialists and heroes."},
    {date: "2026-10-29", text: "The current units and story are a tech demo; the story will be completely reworked. Ants drink raw substance and mutate: the race's defining trait."},
    {date: "2026-10-29", text: "The roster lineup in the Combat Lab (Ants: Pictures) shows the problem: shared body plans, little or no team colour on several units, and a centrepiece barely bigger than the heroes. Jiri: redesign every unit, readable on its own, with something unique that explains its role."}
  ],

  races: [
    {
      id: "ants", name: "Ants", status: "open",
      identity: "Mobility, numbers and mutations (Jiri, 2026-10-29): they move through tunnels, win by many, and drink raw substance to mutate, weirder as the missions go on.",
      substance: "Taken in directly: it mutates them (Burster, Honeypot, Reaver Queen). Bio-magic: luminous, blooming, amber.",
      palette: "Sand-and-bronze chitin, ivory mandibles, royal-blue silk and plates, gold for royalty, amber; violet liquid substance. Materials: only what the colony grows, gathers or secretes (chitin, amber, resin, leaf, silk, clay, fungus); never metal.",
      body: "Regular castes are bipedal real ants, each with its real trait exaggerated until it reads from the top camera; mutants are wild, non-bipedal creatures swollen and glowing with substance, weirder as the missions go on.",
      roleMap: {
        "title": "Zerg roles against real ants",
        "note": "2026-10-29. Each role in SC2's Zerg army (Legacy of the Void) and the real ants whose actual behaviour fits it. Names and facts from general knowledge: check the specifics (acid range, propaganda chemicals) against sources before a design leans on them.",
        "rows": [
          {
            "unit": "Drone",
            "role": "Worker",
            "ants": "Leafcutter (Atta): harvests and farms fungus. Harvester ant (Pogonomyrmex): collects seeds.",
            "fit": "Strong"
          },
          {
            "unit": "Zergling",
            "role": "Cheap, fast swarm melee",
            "ants": "Fire ant (Solenopsis): swarms and stings en masse. Saharan silver ant (Cataglyphis): among the fastest runners.",
            "fit": "Strong"
          },
          {
            "unit": "Baneling (morph)",
            "role": "Suicide splash",
            "ants": "Exploding ant (Colobopsis explodens): ruptures to spray toxic glue.",
            "fit": "Perfect"
          },
          {
            "unit": "Roach",
            "role": "Armoured frontline, holds the line",
            "ants": "Door-head (Colobopsis majors): blocks the way. Turtle ant (Cephalotes): armour plates.",
            "fit": "Strong"
          },
          {
            "unit": "Ravager (morph)",
            "role": "Artillery",
            "ants": "Formica wood ant: sprays acid at a distance (short in reality; needs exaggerating).",
            "fit": "Medium"
          },
          {
            "unit": "Hydralisk",
            "role": "Ranged damage, hits air",
            "ants": "Formica wood ant: acid from the abdomen. Spiny ant (Polyrhachis): hooked spines, though it does not shoot them.",
            "fit": "Medium"
          },
          {
            "unit": "Lurker (morph)",
            "role": "Burrowed ambush, area denial",
            "ants": "Trap-jaw (Odontomachus): waits with jaws locked open. Dracula ant (Mystrium): the fastest snap known, lives underground.",
            "fit": "Strong"
          },
          {
            "unit": "Ultralisk",
            "role": "Huge melee tank, cleaves",
            "ants": "Giant Amazon ant (Dinoponera): among the largest ants. Army-ant soldier (Eciton): huge sickle mandibles.",
            "fit": "Strong"
          },
          {
            "unit": "Queen",
            "role": "Base defence, heals, spawns",
            "ants": "Nurse ants: tend the brood and feed nestmates mouth to mouth. The queen herself.",
            "fit": "Strong"
          },
          {
            "unit": "Infestor",
            "role": "Mind control, roots",
            "ants": "Slave-making ants (Polyergus): raid nests and steal brood; some raiders release 'propaganda' chemicals that turn defenders on each other.",
            "fit": "Strong"
          },
          {
            "unit": "Swarm Host",
            "role": "Siege by spawning swarms",
            "ants": "Army and driver ants (Eciton, Dorylus): pour out of a living bivouac in raiding columns.",
            "fit": "Medium"
          },
          {
            "unit": "Viper",
            "role": "Caster: pulls, blinds",
            "ants": "Weaver ant (Oecophylla): pulls in living chains (Abduct). Acrobat ant (Crematogaster): smears venom.",
            "fit": "Medium"
          },
          {
            "unit": "Overlord / Overseer",
            "role": "Supply, scouting, detection",
            "ants": "Replete: living storage, as supply. Bulldog ant (Myrmecia): exceptional eyesight, for detection.",
            "fit": "Weak"
          },
          {
            "unit": "Mutalisk",
            "role": "Fast air harass",
            "ants": "Alates (winged ants). Gliding ants (Cephalotes): steer their fall back to the tree.",
            "fit": "Weak"
          },
          {
            "unit": "Corruptor, Brood Lord",
            "role": "Air against air, air siege",
            "ants": "No ant does this.",
            "fit": "Gap"
          }
        ],
        "findings": [
          "Every ground role has a real ant that does it in nature; several are near-perfect (Exploding ant as the Baneling, Door-head as the Roach, Trap-jaw as the Lurker, slave-makers as the Infestor).",
          "Air is the gap: ants barely fly, and only to mate. Air roles go to partners (non-ant species) or to mutations: an ant that grows wings from raw substance.",
          "The Door-head sits in the Roach's role: the armoured frontline."
        ]
      },
      rosters: [
        {
          "title": "Chitin, round 3 (2026-10-29)",
          "status": "open",
          "note": "Jiri chose grown chitin (material A). Changes: the Soldier dual-wields two long, plain broadswords with no shield (three blade options); the Hunter has no spit effect (the engine will draw it), even legs and stands in line; the Door-head keeps its shield in one hand and gets an oversized fist in the other, a shield-and-fist brawler. Worker and Queen unchanged.",
          "groups": [
            {
              "units": [
                {
                  "name": "Option 1: plain blades",
                  "kind": "sheet",
                  "size": "sheet",
                  "status": "open",
                  "role": "Worker, Soldier, Hunter, Door-head, Queen.",
                  "signature": "—",
                  "notes": "Clean, long, straight broadswords: the simplest, boldest read. The Door-head's fist only slightly enlarged.",
                  "images": [
                    {
                      "file": "images/ant-army-chitin-v3-1-plain.webp",
                      "caption": "Option 1: plain blades"
                    }
                  ]
                },
                {
                  "name": "Option 2: spined blades",
                  "kind": "sheet",
                  "size": "sheet",
                  "status": "open",
                  "role": "Worker, Soldier, Hunter, Door-head, Queen.",
                  "signature": "—",
                  "notes": "Serrated chitin spikes along the blades: the most aggressive and the most distinct from above. The Door-head's fist clearly oversized.",
                  "images": [
                    {
                      "file": "images/ant-army-chitin-v3-2-spined.webp",
                      "caption": "Option 2: spined blades"
                    }
                  ]
                },
                {
                  "name": "Option 3: thorned guards",
                  "kind": "sheet",
                  "size": "sheet",
                  "status": "open",
                  "role": "Worker, Soldier, Hunter, Door-head, Queen.",
                  "signature": "—",
                  "notes": "Plain blades with thorns at the guards: subtle, close to option 1. The Door-head's fist clearly oversized.",
                  "images": [
                    {
                      "file": "images/ant-army-chitin-v3-3-thorned.webp",
                      "caption": "Option 3: thorned guards"
                    }
                  ]
                }
              ]
            }
          ]
        },
        {
          "title": "No metal: four grown materials (2026-10-29)",
          "status": "open",
          "note": "Jiri: metal is the beetles', ivory and bone the bees' (the holy order); the ants wear what the colony grows, gathers or secretes. The Knights round 2 sheet (variant 2) repainted with only the materials changed.",
          "groups": [
            {
              "units": [
                {
                  "name": "A. Grown chitin",
                  "kind": "materials",
                  "size": "sheet",
                  "status": "agreed",
                  "role": "Their own exoskeleton thickened into armour, polished like lacquer; blue as pigmented chitin and silk.",
                  "signature": "—",
                  "notes": "The most ant-like; strong blue on top; cohesive. Recommended, with C's silk stitching as the accent.",
                  "images": [
                    {
                      "file": "images/ant-army-material-a-chitin.webp",
                      "caption": "A. Grown chitin"
                    }
                  ]
                },
                {
                  "name": "B. Amber and resin",
                  "kind": "materials",
                  "size": "sheet",
                  "status": "open",
                  "role": "Translucent golden amber plates over chitin, blue silk.",
                  "signature": "—",
                  "notes": "Beautiful, but gold swamps the blue (weaker team colour) and the Worker's amber load stops standing out.",
                  "images": [
                    {
                      "file": "images/ant-army-material-b-amber.webp",
                      "caption": "B. Amber and resin"
                    }
                  ]
                },
                {
                  "name": "C. Leaf and silk",
                  "kind": "materials",
                  "size": "sheet",
                  "status": "open",
                  "role": "Leaf armour stitched with white weaver silk, seed-husk shields, a thorn sword.",
                  "signature": "—",
                  "notes": "The most natural and charming; clearly not metal.",
                  "images": [
                    {
                      "file": "images/ant-army-material-c-leaf-silk.webp",
                      "caption": "C. Leaf and silk"
                    }
                  ]
                },
                {
                  "name": "D. Clay and fungus",
                  "kind": "materials",
                  "size": "sheet",
                  "status": "open",
                  "role": "Fired sand-clay plates like their spire buildings, blue glaze, fungus leather.",
                  "signature": "—",
                  "notes": "Ties units to buildings; close to A up close.",
                  "images": [
                    {
                      "file": "images/ant-army-material-d-clay-fungus.webp",
                      "caption": "D. Clay and fungus"
                    }
                  ]
                }
              ]
            }
          ]
        },
        {
          "title": "Knights, round 2 (2026-10-29)",
          "status": "open",
          "note": "Jiri chose take 1 (Knights): the Queen is great; the Soldier and Hunter were nearly the same, with no defining traits. Kept the Worker, Door-head and Queen; the Soldier keeps its mandibles and shield and takes a greatsword; the Hunter follows Jiri's sac silhouette (a big round toothed venom sac carried high over a small body) in knight armour, biological, spitting acid, no gun. Three variants. The Hunter came out six-legged (the silhouette reference copied too literally); the sword is a longsword more than a greatsword.",
          "groups": [
            {
              "units": [
                {
                  "name": "Variant 1",
                  "kind": "sheet",
                  "size": "sheet",
                  "status": "open",
                  "role": "Worker, Soldier, Hunter, Door-head, Queen.",
                  "signature": "—",
                  "images": [
                    {
                      "file": "images/ant-army-knights-v2-1.webp",
                      "caption": "Knights round 2, variant 1"
                    }
                  ]
                },
                {
                  "name": "Variant 2",
                  "kind": "sheet",
                  "size": "sheet",
                  "status": "open",
                  "role": "Worker, Soldier, Hunter, Door-head, Queen.",
                  "signature": "—",
                  "images": [
                    {
                      "file": "images/ant-army-knights-v2-2.webp",
                      "caption": "Knights round 2, variant 2"
                    }
                  ]
                },
                {
                  "name": "Variant 3",
                  "kind": "sheet",
                  "size": "sheet",
                  "status": "open",
                  "role": "Worker, Soldier, Hunter, Door-head, Queen.",
                  "signature": "—",
                  "images": [
                    {
                      "file": "images/ant-army-knights-v2-3.webp",
                      "caption": "Knights round 2, variant 3"
                    }
                  ]
                }
              ]
            }
          ]
        },
        {
          "title": "Ten takes on the first five (2026-10-29)",
          "status": "open",
          "note": "Worker, Soldier (fire ant), Hunter (wood ant, shoots up), Door-head and Queen, one sheet per take, from most human-like (1) to fully Zerg-like (10): gpt-image-2.5 Sunburst with Jiri's WC3 and SC2 references and today's ants, under the readability rules and the real-ant traits. Seen across all ten: the amber Worker and the round Door-head read everywhere; the model drifted to six legs in several takes; in the creature takes the Soldier and Hunter blur.",
          "groups": [
            {
              "units": [
                {
                  "name": "1. Knights",
                  "kind": "take",
                  "size": "sheet",
                  "status": "agreed",
                  "role": "Human-like: a disciplined legion like Warcraft's Humans; plate-like chitin, tabards, shields, a sorceress-queen.",
                  "signature": "—",
                  "notes": "The most readable set: upright, a distinct silhouette per unit, lots of blue on top; the Door-head's tower shield reads instantly.",
                  "images": [
                    {
                      "file": "images/ant-army-01-knights.webp",
                      "caption": "Take 1: Knights"
                    }
                  ]
                },
                {
                  "name": "2. Brutes",
                  "kind": "take",
                  "size": "sheet",
                  "status": "open",
                  "role": "Human-like and brutish like the Orcish Horde: huge shoulders, mandibles as tusks, a war-matriarch queen.",
                  "signature": "—",
                  "notes": "Chunky and strong; the Door-head turned into a crab-like shell; less bipedal than asked.",
                  "images": [
                    {
                      "file": "images/ant-army-02-brutes.webp",
                      "caption": "Take 2: Brutes"
                    }
                  ]
                },
                {
                  "name": "3. Wardens",
                  "kind": "take",
                  "size": "sheet",
                  "status": "open",
                  "role": "Graceful like the Night Elves: leaf cloaks, moonlit glows, a winged priestess-queen.",
                  "signature": "—",
                  "notes": "The winged Queen is a standout; the Soldier and Hunter drifted to six legs.",
                  "images": [
                    {
                      "file": "images/ant-army-03-wardens.webp",
                      "caption": "Take 3: Wardens"
                    }
                  ]
                },
                {
                  "name": "4. Tribal",
                  "kind": "take",
                  "size": "sheet",
                  "status": "open",
                  "role": "Clay-painted chitin, woven grass, leaf banners, a shamanic mother.",
                  "signature": "—",
                  "notes": "Warm and handmade; the Queen's headdress and the Door-head's banner read well; Soldier and Hunter crab-like.",
                  "images": [
                    {
                      "file": "images/ant-army-04-tribal.webp",
                      "caption": "Take 4: Tribal"
                    }
                  ]
                },
                {
                  "name": "5. Samurai",
                  "kind": "take",
                  "size": "sheet",
                  "status": "open",
                  "role": "Lacquered chitin armour, tall back banners as team colour, an empress.",
                  "signature": "—",
                  "notes": "Tall back banners are the best team-colour idea: from above you see the banner first.",
                  "images": [
                    {
                      "file": "images/ant-army-05-samurai.webp",
                      "caption": "Take 5: Samurai"
                    }
                  ]
                },
                {
                  "name": "6. Four-armed",
                  "kind": "take",
                  "size": "sheet",
                  "status": "open",
                  "role": "Hunched between human and insect, four arms, like heroic Nerubians.",
                  "signature": "—",
                  "notes": "The four-armed Queen is striking; the castes are hunched and creature-like rather than upright.",
                  "images": [
                    {
                      "file": "images/ant-army-06-four-armed.webp",
                      "caption": "Take 6: Four-armed"
                    }
                  ]
                },
                {
                  "name": "7. Cute",
                  "kind": "take",
                  "size": "sheet",
                  "status": "open",
                  "role": "Hearthstone-style big heads and eyes, mascot-like.",
                  "signature": "—",
                  "notes": "Charming and round; the Door-head became turtle-like; low and six-legged.",
                  "images": [
                    {
                      "file": "images/ant-army-07-cute.webp",
                      "caption": "Take 7: Cute"
                    }
                  ]
                },
                {
                  "name": "8. Bio-magic",
                  "kind": "take",
                  "size": "sheet",
                  "status": "open",
                  "role": "Glowing with raw liquid substance: blue-violet sacs, beautiful rather than corrupted.",
                  "signature": "—",
                  "notes": "The glow reads from far; the units blur into one colour; fits the mutants better than the basics.",
                  "images": [
                    {
                      "file": "images/ant-army-08-bio-magic.webp",
                      "caption": "Take 8: Bio-magic"
                    }
                  ]
                },
                {
                  "name": "9. Half-creature",
                  "kind": "take",
                  "size": "sheet",
                  "status": "open",
                  "role": "Raised fore-bodies like a mantis or centaur, Hydralisk-adjacent.",
                  "signature": "—",
                  "notes": "Soldier and Hunter hard to tell apart; spidery.",
                  "images": [
                    {
                      "file": "images/ant-army-09-half-creature.webp",
                      "caption": "Take 9: Half-creature"
                    }
                  ]
                },
                {
                  "name": "10. Zerg",
                  "kind": "take",
                  "size": "sheet",
                  "status": "open",
                  "role": "Low, six-legged organic monsters, still ants.",
                  "signature": "—",
                  "notes": "Menacing, but Soldier and Hunter blur: today's problem again.",
                  "images": [
                    {
                      "file": "images/ant-army-10-zerg.webp",
                      "caption": "Take 10: Zerg"
                    }
                  ]
                }
              ]
            }
          ]
        },
        {
          title: "Proposal 1: real castes, partners, wild mutations", status: "parked",
          note: "Parked (Jiri, 2026-10-29): only the Door-head is kept; the rest is set aside for now. 2026-10-29. The roster disrupted and rebuilt from real ants: regular castes you recognise, two partner species that are not ants, and mutations from drinking raw substance (a Zerg-style morph in the field: Substance and time, a vulnerable cocoon). 12 units plus 3–4 heroes, the ballpark of WC3 Undead (10 trained, 2 morphs, 4 heroes) and SC2 Zerg (11 hatched, 5 morphs).",
          groups: [
            {title: "Regular castes", note: "grounded, readable, each a real ant with one iconic trait", units: [
              {name: "Leafcutter", kind: "worker", size: "small", status: "parked", role: "Worker: gathers and builds.", nature: "Atta leafcutters carry cut leaf pieces home to farm fungus; tiny minims ride on the leaf to fend off parasitic flies.", lore: "Farmers: they never eat the leaves, they feed them to the fungus garden they tend, the oldest agriculture on earth, millions of years before anyone else.", signature: "A leaf held high like a parasol, a minim riding on it: from above, a moving leaf.", replaces: "Worker"},
              {name: "Trap-jaw", kind: "light melee", size: "small", status: "parked", role: "Fast striker; leaps.", nature: "Odontomachus locks its jaws open at 180° and snaps them shut, among the fastest movements in nature; snapping against the ground launches it into the air.", lore: "Jaws so fast they fling their owner through the air; a trap-jaw is never where you last saw it.", signature: "Jaws held wide open, a flat T in front of the head.", replaces: "Raider"},
              {name: "Wood ant", kind: "ranged", size: "small", status: "parked", role: "Ranged: sprays acid.", nature: "Formica wood ants spray formic acid from the tip of the abdomen, curled forward under the body.", lore: "Carries its weapon inside its own body: an acid cannon it swings forward between its legs.", signature: "The abdomen curled forward like a cannon, a mist of acid.", replaces: "Needler"},
              {name: "Door-head", kind: "tank", size: "medium", status: "agreed", role: "Tank: the armoured frontline.", nature: "Colobopsis door-head majors have a flat, plug-shaped head that seals the nest entrance.", lore: "Born a living door: a head shaped to fit the nest's entrance. Gatekeepers for life, who know every nestmate by touch and step aside only for family.", signature: "A head like a round shield or a manhole cover.", body: "Bipedal: squat and broad under a head the size of a round tower shield, braced like one.", replaces: "Major", notes: "From the top camera the one round disc on the field; in cutscenes the disc reads as a wide hat pulled low over a small, cute face (big eyes, little mandibles): a stoic, lovable gatekeeper and a natural mascot."},
              {name: "Weaver", kind: "support", size: "medium", status: "parked", role: "Builder and binder: repairs, raises leaf walls, snares.", nature: "Oecophylla weaver ants pull leaves together in living chains and stitch them with silk from larvae they hold in their jaws.", lore: "They sew with their own children: the larvae are the thread, held in the jaws and squeezed for silk.", signature: "A glowing larva held like a glue gun, a silk thread.", replaces: "Gardener and Herald (roles to split)"},
              {name: "Army-ant soldier", kind: "elite melee", size: "medium", status: "parked", role: "Elite melee.", nature: "Eciton soldiers carry huge sickle-shaped mandibles; army ants link their bodies into living bridges and bivouacs.", lore: "Jaws so great it cannot feed itself, so others feed it: a pure warrior. Its kind builds bridges and camps from their own bodies.", signature: "Two great sickles curving forward; a living-bridge ability.", replaces: "Bullet Ant"},
              {name: "Alate", kind: "flyer", size: "medium", status: "parked", role: "Flying scout and harasser.", nature: "Winged males and princesses leave the nest on mating flights.", lore: "Born with wings to leave: every new colony begins with one flight.", signature: "Wings held wide.", replaces: "Alate"}
            ]},
            {title: "Partners", note: "not ants; a player knows them on sight", units: [
              {name: "Aphid herd", kind: "livestock", size: "small", status: "parked", role: "Healer or income: slow, defenceless, escorted.", nature: "Ants guard aphids and 'milk' them for honeydew, even carry them to better plants.", lore: "The colony's cattle: guarded, moved to better pastures and milked for honeydew.", signature: "Round, soft and green, in a cluster."},
              {name: "Blue caterpillar", kind: "beast", size: "large", status: "parked", role: "Heavy unit or siege, guarded by ants (perhaps ridden).", nature: "Some blue-butterfly caterpillars are tended and guarded by ants for their sweet secretions.", lore: "A guest the colony feeds and guards for the sweetness it gives; some of its kind repay that by eating the brood.", signature: "A long, segmented soft body with ants on its back."}
            ]},
            {title: "Mutations", note: "drunk raw substance: swollen creatures glowing violet, weirder as the missions go on", units: [
              {name: "Replete", kind: "artillery", size: "large", status: "parked", from: "Leafcutter", role: "Lobs glowing globs over long range.", nature: "Honeypot ants (Myrmecocystus) are workers swollen into living food stores, hanging from the nest ceiling.", lore: "A living larder: swells until it cannot walk, hangs from the ceiling and feeds the colony in hard times.", signature: "A huge glowing abdomen with a little ant in front.", replaces: "Honeypot"},
              {name: "Exploder", kind: "bomb", size: "medium", status: "parked", from: "Trap-jaw", role: "Runs in and bursts for heavy splash.", nature: "Exploding ants (Colobopsis explodens) rupture their own bodies to spray a sticky toxic glue on attackers.", lore: "It dies by bursting, spraying the enemy with glue, to save the nest.", signature: "A swollen, sloshing, glowing body.", replaces: "Burster"},
              {name: "Colossus", kind: "centrepiece", size: "huge", status: "parked", from: "Door-head", role: "The army's giant.", signature: "Two to three times a hero; the door-head grown monstrous.", replaces: "Reaver Queen"},
              {name: "Later, in missions", kind: "progression", size: "?", status: "parked", role: "Weirder mutations found mission by mission: fused swarms, many-headed things, a walking nest.", signature: "Each wilder than the last."}
            ]}
          ]
        }
      ],
      images: [
        {file: "images/ants-today-1.webp", caption: "Today: blue (player 1) and red (player 2), every ant unit, Combat Lab.", date: "2026-10-29"},
        {file: "images/ants-today-2.webp", caption: "Today, blue up close: Queen, Reaver Queen, General; Royal Alchemist, Burster, Honeypot, Bullet Ant; Needler, Herald, Major, Alate; Raider, Worker, Gardener, Youngling, Villager.", date: "2026-10-29"},
        {file: "images/ants-today-3.webp", caption: "Today, red up close. The Bullet Ant and the Honeypot show no team colour; the Raider and the Gardener very little.", date: "2026-10-29"}
      ],
      units: [
        {id: "unit.ants.settler", name: "Worker", size: "small", status: "open", role: "Gathers amber, builds; travels the tunnels.", today: "Small ant, some team colour on the chest.", signature: "Idea: a big amber chunk carried on its back (the Peasant's sack)."},
        {id: "unit.ants.warrior", name: "Raider", size: "small", status: "open", role: "The colony's Zergling: cheap, fast melee, hatched in pairs, swarms.", today: "Stocky biting ant; barely any team colour, so owners blur.", signature: "Idea: huge forward mandibles and swept-back blade fins.", notes: "The most common unit: its team colour matters most."},
        {id: "unit.ants.archer", name: "Needler", size: "medium", status: "open", role: "Ranged: chitin needles at ground and air.", today: "Reads fairly well.", signature: "Idea: a long needle stinger aimed forward, a quiver of spines on the back."},
        {id: "unit.ants.queensguard", name: "Major", size: "medium", status: "open", role: "Armoured tank; guards small ants beside it.", today: "Reads well.", signature: "Idea: an armoured head as wide as a shield."},
        {id: "unit.ants.banneret", name: "Herald", size: "medium", status: "open", role: "Pheromone caster: speed and rally auras.", today: "Reads well.", signature: "Idea: a tall banner or feather crown."},
        {id: "unit.ants.nectarist", name: "Gardener", size: "small", status: "open", role: "Healer: fungus circles, spore clouds.", today: "Mushroom cap; little team colour.", signature: "Idea: a blooming flower or fungus garden it tends."},
        {id: "unit.ants.winglancer", name: "Alate", size: "medium", status: "open", role: "Flying scout and harasser; can land and found a colony.", today: "Reads well.", signature: "Idea: wings, held wide."},
        {id: "unit.ants.thornblade", name: "Bullet Ant", size: "large", status: "open", role: "Elite duellist against heroes and large units.", today: "Black ant, no team colour.", signature: "Idea: a glowing, oversized stinger."},
        {id: "unit.ants.burster", name: "Burster", size: "large", status: "open", role: "Mutated suicide bomber: bursts for heavy splash.", today: "Reads well: a colour-filled sac.", signature: "Idea: the swollen glowing sac itself."},
        {id: "unit.ants.bombardier", name: "Honeypot", size: "large", status: "open", role: "Mutated artillery: lobs nectar globs far; cannot fire up close.", today: "Orange bulb, no team colour.", signature: "Idea: a huge replete abdomen with a launch arm."},
        {id: "unit.ants.reaver-queen", name: "Reaver Queen", size: "huge", status: "open", role: "The ultimate monster: devours, rampages, molts.", today: "Team colour only along the back; barely bigger than the heroes.", signature: "Idea: a massive body with scythes, two to three times a hero."},
        {id: "unit.ants.queen", name: "The Queen", size: "large", hero: true, status: "open", role: "Hero: healer and leader.", today: "Reads well (hero glow)."},
        {id: "unit.ants.royal-alchemist", name: "Royal Alchemist", size: "large", hero: true, status: "open", role: "Hero: substance alchemist.", today: "Reads well (hero glow)."},
        {id: "unit.ants.marshal", name: "General", size: "large", hero: true, status: "open", role: "Hero: army-ant marshal with twin blades.", today: "Reads well (hero glow)."}
      ]
    },
    {
      id: "beetles", name: "Beetles", status: "open",
      identity: "Engineers and soldiers of brass and steam: disciplined, mechanical, proud of their machines.",
      substance: "Never taken in; used only through technology, glowing blue to violet inside tanks, coils and canisters.",
      palette: "Glossy black chitin, dark riveted steel, brass, royal-blue plates; orange furnace fire, white steam, violet only in glass.",
      body: "Mostly upright, armoured beetle soldiers with props and machines.",
      units: [
        {id: "unit.beetles.worker", name: "Tinker", size: "small", status: "open", role: "Gathers, builds, repairs."},
        {id: "unit.beetles.shieldbearer", name: "Shieldbearer", size: "small", status: "open", role: "Front-line tank with a tower shield and piston hammer."},
        {id: "unit.beetles.bolter", name: "Bolter", size: "small", status: "open", role: "Rapid repeater against ground and air."},
        {id: "unit.beetles.breacher", name: "Breacher", size: "small", status: "open", role: "Siege drill: breaks buildings and armour."},
        {id: "unit.beetles.arcwright", name: "Arcwright", size: "small", status: "open", role: "Chain-lightning specialist."},
        {id: "unit.beetles.mender", name: "Mender", size: "small", status: "open", role: "Medic and mechanic."},
        {id: "unit.beetles.warden", name: "Warden", size: "small", status: "open", role: "Barrier projector against missiles."},
        {id: "unit.beetles.bombard", name: "Bombard", size: "large", status: "open", role: "Six-legged mortar crawler."},
        {id: "unit.beetles.steamwing", name: "Steamwing", size: "large", status: "open", role: "Boiler-winged flyer; scout and anti-air."},
        {id: "unit.beetles.juggernaut", name: "Juggernaut", size: "huge", status: "open", role: "The walking fortress."},
        {id: "unit.beetles.hornbreaker", name: "Hornbreaker", size: "large", hero: true, status: "open", role: "Hero: warlord with a Hercules horn."},
        {id: "unit.beetles.forgemaster", name: "Forgemaster", size: "large", hero: true, status: "open", role: "Hero: engineer with turrets."},
        {id: "unit.beetles.coilkeeper", name: "Coilkeeper", size: "large", hero: true, status: "open", role: "Hero: substance savant."}
      ]
    },
    {
      id: "bees", name: "Bees", status: "idea",
      identity: "A blood and holy order: queen, rite and sacrifice; they transform through substance and ritual.",
      substance: "Taken in through ritual, which transforms them (royal jelly makes a queen in nature).",
      palette: "Not set. Ideas: gold and black, wax white, honey amber, a holy light.",
      body: "Not set.",
      units: []
    }
  ],

  armies: {
    "races": [
      "Human",
      "Orc",
      "Night Elf",
      "Undead",
      "Terran",
      "Zerg",
      "Protoss"
    ],
    "roles": [
      {
        "role": "Worker / economy",
        "verdict": "every race",
        "cover": [
          "S",
          "S",
          "S",
          "S",
          "S",
          "S",
          "S"
        ],
        "how": "Peasant, Peon, Wisp, Acolyte and Ghoul; SCV, Drone, Probe. Each gathers in its race's own way."
      },
      {
        "role": "Supply",
        "verdict": "every race",
        "cover": [
          "S",
          "S",
          "S",
          "S",
          "S",
          "S",
          "S"
        ],
        "how": "Farm, Burrow (shelters peons and shoots), Moon Well (also heals), Ziggurat (also a tower); Supply Depot (lowers), Overlord (flies), Pylon (also powers)."
      },
      {
        "role": "Cheap frontline",
        "verdict": "every race",
        "cover": [
          "S",
          "S",
          "M",
          "S",
          "S",
          "S",
          "S"
        ],
        "how": "Footman, Grunt, Huntress, Ghoul; Marine, Zergling, Zealot. Night Elves lean on ranged."
      },
      {
        "role": "Ranged damage",
        "verdict": "every race",
        "cover": [
          "S",
          "S",
          "S",
          "S",
          "S",
          "S",
          "S"
        ],
        "how": "Rifleman, Troll Headhunter, Archer, Crypt Fiend; Marine, Hydralisk, Stalker."
      },
      {
        "role": "Anti-air",
        "verdict": "every race",
        "cover": [
          "S",
          "M",
          "S",
          "S",
          "S",
          "S",
          "S"
        ],
        "how": "Every race can shoot air, but some lean on one unit for it."
      },
      {
        "role": "Siege",
        "verdict": "every race",
        "cover": [
          "S",
          "S",
          "S",
          "S",
          "S",
          "S",
          "S"
        ],
        "how": "Mortar Team, Catapult, Glaive Thrower, Meat Wagon; Siege Tank, Lurker and Brood Lord, Colossus and Tempest."
      },
      {
        "role": "Healing",
        "verdict": "uneven: a weakness",
        "cover": [
          "S",
          "M",
          "M",
          "W",
          "S",
          "M",
          "W"
        ],
        "how": "Priest; Healing Ward; Druid of the Claw, Moon Well; Undead regenerate only on blight. Medivac; Queen's transfuse, burrow regen; Protoss only Shield Batteries."
      },
      {
        "role": "Disable / control",
        "verdict": "every race, in its own way",
        "cover": [
          "S",
          "S",
          "S",
          "S",
          "S",
          "S",
          "S"
        ],
        "how": "Sorceress (slow, polymorph), Shaman (purge), Dryad, Banshee (curse, possession); Ghost, Infestor and Viper, High Templar and Sentry."
      },
      {
        "role": "Buffs",
        "verdict": "uneven",
        "cover": [
          "M",
          "S",
          "M",
          "M",
          "W",
          "W",
          "M"
        ],
        "how": "Inner Fire; Bloodlust (Orc's signature); Roar; Unholy Frenzy. SC2 has few buffs: Stim is self-inflicted."
      },
      {
        "role": "Air army",
        "verdict": "every race",
        "cover": [
          "S",
          "M",
          "S",
          "S",
          "S",
          "S",
          "S"
        ],
        "how": "Gryphon and Dragonhawk; Wind Rider and Bat Rider; Hippogryph, Faerie Dragon, Chimaera; Gargoyle, Frost Wyrm. SC2: Viking, Mutalisk, Void Ray and Carrier."
      },
      {
        "role": "Heavy top tier",
        "verdict": "every race",
        "cover": [
          "S",
          "S",
          "S",
          "S",
          "S",
          "S",
          "S"
        ],
        "how": "Knight, Tauren, Mountain Giant, Abomination; Thor, Ultralisk, Archon or Carrier."
      },
      {
        "role": "Static defence",
        "verdict": "uneven: a weakness",
        "cover": [
          "S",
          "S",
          "M",
          "M",
          "S",
          "W",
          "M"
        ],
        "how": "Human towers and Call to Arms; Orc Burrows; Ancient Protectors; Ziggurats. Terran bunkers and turrets; Zerg spines (slow to place); Protoss cannons need power."
      },
      {
        "role": "Mobility",
        "verdict": "uneven: a strength",
        "cover": [
          "M",
          "M",
          "S",
          "M",
          "S",
          "S",
          "S"
        ],
        "how": "Night Elves: Shadowmeld, Blink. Terran: drops, lift-off. Zerg: speed, creep, Nydus. Protoss: Warp-in, Blink, Recall."
      },
      {
        "role": "Detection",
        "verdict": "every race",
        "cover": [
          "S",
          "S",
          "S",
          "S",
          "S",
          "S",
          "S"
        ],
        "how": "Magic Sentry, Sentry Ward, Owl and Faerie Fire, Shade; Scan and Raven, Overseer, Observer."
      }
    ],
    "identity": [
      {
        "race": "Human",
        "strength": "Versatile and defensive: the best casters (Priest, Sorceress), towers, Militia (workers take up arms).",
        "weakness": "Fragile, slow units early; expensive Knights.",
        "unique": "Militia; Call to Arms; Spell Breakers steal buffs."
      },
      {
        "race": "Orc",
        "strength": "Brute strength: the most hit points, Bloodlust, early aggression.",
        "weakness": "Little healing; weak to dispels and kiting.",
        "unique": "Burrows that shelter peons and shoot; Pillage (gold from buildings); Kodo devour."
      },
      {
        "race": "Night Elf",
        "strength": "Mobile and night-bound: archers, Shadowmeld at night, moon wells.",
        "weakness": "Low hit points; slow, costly tech.",
        "unique": "Buildings that walk and fight (Ancients); wisps; night bonuses."
      },
      {
        "race": "Undead",
        "strength": "Attrition on blight: corpses raise the dead, fast regeneration on blight.",
        "weakness": "Heals only on blight; the base is tied to blight.",
        "unique": "Blight (build only on it); corpses as a resource; Acolytes summon buildings."
      },
      {
        "race": "Terran",
        "strength": "Positional and flexible: siege, healing, drops, upgrades.",
        "weakness": "Weak when caught out of position (an unsieged tank).",
        "unique": "Buildings lift off; add-ons; the MULE; repair."
      },
      {
        "race": "Zerg",
        "strength": "The swarm: cheap units, fast remax from larva, creep, morphs.",
        "weakness": "Lower unit quality; weak static defence; dies to splash.",
        "unique": "Larva production; creep; morphs; burrow; flying supply."
      },
      {
        "race": "Protoss",
        "strength": "Quality: expensive, strong units, shields, devastating splash.",
        "weakness": "Few units: every loss costs; almost no healing.",
        "unique": "Shields; warp-in; Pylon power; Chrono Boost; workers only start buildings."
      }
    ],
    "intro": "2026-10-29. Warcraft III's four races and StarCraft II's three, compared role by role: every army covers the same core jobs, but each is deliberately weak at one or two, and that weakness is part of its identity. From general knowledge of both games: check a detail on a wiki before leaning on it.",
    "lessons": [
      "Every army covers about a dozen jobs: worker, supply, cheap frontline, ranged damage, anti-air, siege, healing, control, air, a heavy top tier, defence and detection. The ants need an answer to each, even a weak one.",
      "Identity comes from one signature mechanic that reshapes both the base and the battle: blight, creep, power fields, walking buildings, the night. For the ants: drinking raw substance and mutating.",
      "Each race is deliberately bad at one or two jobs (Protoss and Undead at healing, Zerg at static defence, Orcs at dispels, Night Elves at hit points). The weakness forces a style of play.",
      "The ants' natural weakness is the air: real ants barely fly. Like Zerg's weak static defence, it can be embraced, with air answered by partner species or by mutation.",
      "The ants' identity (agreed, 2026-10-29): mobility through tunnels, numbers, and mutations. Mutations and movement are enough to set them apart; no other special mechanic is needed."
    ]
  },

  research: {
    intro: "Real creatures that prey on, parasitise, serve or ally with ants, beetles and bees, as seeds for units, neutral camps, abilities and story. First pass from general knowledge (2026-10-29): verify against sources before a design leans on a detail.",
    races: [
      {name: "Ants", groups: [
        {title: "Enemies and parasites", items: [
          {creature: "Antlion larva", fact: "Digs a sand pit and waits at the bottom; flicks sand at ants that slip until they fall in.", hook: "A pit-trap neutral or a terrain hazard; a burrowed ambusher unit."},
          {creature: "Phorid flies", fact: "Lay an egg in a fire ant; the larva eats its way into the head, which falls off.", hook: "A dreaded neutral swarm; a parasite ability."},
          {creature: "Zombie-ant fungus (Ophiocordyceps)", fact: "Infects carpenter ants, drives them to climb and bite a leaf in a death grip, then sprouts from the head.", hook: "The dark side of substance mutation; a campaign omen (the blight)."},
          {creature: "Army ants", fact: "Nomadic hordes that raid other ant colonies and wasp nests for brood.", hook: "A rival ant faction or a raiding wave; the General's lineage."},
          {creature: "Slave-making ants (Polyergus)", fact: "Raid other colonies for pupae, which grow up working for their captors.", hook: "A rival colony; a convert or capture ability."},
          {creature: "Ant-eating spiders", fact: "Zodariid spiders specialise in hunting ants; Myrmarachne jumping spiders mimic ants.", hook: "Neutral hunters; a disguised infiltrator."},
          {creature: "Assassin bug (Acanthaspis)", fact: "Wears the corpses of its ant prey on its back as camouflage.", hook: "A grisly neutral or boss."}
        ]},
        {title: "Allies and partners", items: [
          {creature: "Aphids and scale insects", fact: "Ants guard and 'milk' them for honeydew, even carry them to better plants.", hook: "Livestock: an income building or herd; a reason to defend pastures."},
          {creature: "Fungus gardens (leafcutters)", fact: "Leafcutter ants farm a fungus on cut leaves, their main food.", hook: "The Gardener's fungus; a farming building."},
          {creature: "Acacia trees", fact: "Shelter ants in swollen thorns and feed them; the ants defend the tree fiercely.", hook: "A living fortress or a tree that fights beside its colony."},
          {creature: "Blue butterfly caterpillars", fact: "Some are guarded by ants for their sweet secretions; the large blue tricks ants into carrying it home, then eats their brood.", hook: "A friend-or-traitor ally; a story twist."},
          {creature: "Ant crickets and rove beetles", fact: "Live inside nests by mimicking the colony's smell and stealing food.", hook: "Myrmecophiles: hidden guests, smugglers or spies."}
        ]}
      ]},
      {name: "Beetles", groups: [
        {title: "Enemies and parasites", items: [
          {creature: "Robber flies", fact: "Catch beetles and other insects in flight and inject a paralysing saliva.", hook: "An air threat to Steamwings; a neutral flyer."},
          {creature: "Parasitoid wasps and flies", fact: "Scoliid wasps lay eggs on scarab grubs; tachinid flies parasitise adult beetles.", hook: "A neutral threat to beetle larvae and nurseries."},
          {creature: "Toads, shrews and birds", fact: "Major beetle eaters; at insect scale, giants.", hook: "Boss-sized neutral monsters (the toads already exist)."}
        ]},
        {title: "Traits and partners", items: [
          {creature: "Bombardier beetle", fact: "Mixes chemicals in a chamber and fires a near-boiling spray from its abdomen.", hook: "Steam and pressure weapons: the Bolter and the Bombard."},
          {creature: "Stag and rhinoceros beetles", fact: "Males fight with huge mandibles or horns, lifting and throwing rivals.", hook: "The Hornbreaker; heavy melee signatures."},
          {creature: "Fireflies (beetles)", fact: "Glow with chemical light to signal.", hook: "Lanterns and signal towers; light without substance."},
          {creature: "Dung beetles", fact: "Roll balls of dung and steer by the sun and even the Milky Way.", hook: "Navigators and astronomers; the Coilkeeper's science."},
          {creature: "Ambrosia beetles", fact: "Farm fungus in tunnels bored into wood.", hook: "Engineering in timber; a workshop."},
          {creature: "Phoretic mites", fact: "Ride on beetles to travel between habitats.", hook: "Crew and riders on beetle machines."}
        ]}
      ]},
      {name: "Bees", groups: [
        {title: "Enemies and parasites", items: [
          {creature: "Hornets", fact: "Giant hornets raid hives and butcher defenders; Japanese honeybees answer by balling a hornet and cooking it with body heat.", hook: "The bees' arch-enemy; a sacrifice rite that kills by heat."},
          {creature: "Beewolf (Philanthus)", fact: "A wasp that hunts honeybees and stocks its burrow with them.", hook: "A neutral hunter of bee units."},
          {creature: "Crab spiders", fact: "Wait camouflaged on flowers to ambush visiting bees.", hook: "Flower ambushes on the map."},
          {creature: "Death's-head hawkmoth", fact: "Enters hives to steal honey.", hook: "A thief neutral; an omen."},
          {creature: "Varroa mites and wax moths", fact: "Varroa feed on bees and spread disease; wax moth larvae tunnel through combs.", hook: "Plague and decay; the blight's face among bees."}
        ]},
        {title: "Rites and partners", items: [
          {creature: "Flowers", fact: "Pollination: bees feed on nectar and pollen, flowers get carried pollen.", hook: "Bees bound to flowers: buildings that bloom, fields to defend."},
          {creature: "Royal jelly", fact: "Larvae fed only royal jelly become queens.", hook: "Transformation by rite and food: the bees' tech and upgrades."},
          {creature: "The waggle dance", fact: "Foragers dance to tell the hive where flowers are.", hook: "Rituals that buff, reveal or command."}
        ]}
      ]}
    ],
    between: [
      {between: "Beetles against bees", fact: "The small hive beetle infests honeybee hives; blister-beetle larvae ride bees into their nests and eat the stores.", hook: "A natural feud, and beetle saboteurs in bee lands."},
      {between: "Ants against bees and wasps", fact: "Army ants raid wasp and bee nests for brood; ants steal honey from hives.", hook: "Raids for honey; a reason for the two to war."},
      {between: "Ants and beetles", fact: "Many beetles live inside ant nests as guests, thieves or predators (rove beetles, Lomechusa).", hook: "Beetle spies or traders among ants; uneasy neighbours."},
      {between: "Ants and bees, kin", fact: "Ants, bees and wasps are all Hymenoptera: ants and bees descend from wasp-like ancestors.", hook: "Old kinship: rivals who are cousins."}
    ]
  },

  questions: [
    {
      title: "Bipedal ants?", status: "agreed", date: "2026-10-29",
      text: "Jiri is considering making the ants less ant-like and more bipedal: more room to explore, and default soldiers that look better in cutscenes. Against it: on 2026-10-20 upright humanoid ants were rejected as unreadable from above.",
      options: [
        ["Bipedal throughout", "Every caste upright: the most room for props, faces and cutscene acting, and humanoid rigs and animations. Risk: from above they all become the same cone unless every unit is built for the top (hunched, broad shoulders, things held wide)."],
        ["Bipedal soldiers, creature specialists", "Workers and soldiers upright like Grunts and Footmen; mutated and monstrous units (Burster, Honeypot, Reaver Queen) stay creatures. Two body families make a natural size and role split."],
        ["Mixed by role (today's rule)", "Creature soldiers, humanoid specialists and heroes."]
      ],
      answer: "Agreed (Jiri, 2026-10-29): the regular castes are bipedal AND their real trait is exaggerated until it reads from the top camera (the Grunt's way: upright, with shoulders, tusks and axe blown out of proportion). Mutants are the wild, non-bipedal creatures. There is no trade-off between the two.",
      recommendation: "Bipedal soldiers, creature mutants: upright ants built for the top camera, hunched, with a broad back and four arms (two to fight, two to carry or brace) as the race's signature; the substance-mutated units stay creatures, which also shows what substance does to them. To be proven on a roster sheet before any model."
    },
    {title: "Do the heroes get redesigned too?", status: "open", date: "2026-10-29", text: "Or do they stay as the bar the rest should meet?"},
    {title: "Are the units' roles fixed?", status: "open", date: "2026-10-29", text: "Redesign the looks only, or rethink what each unit does now that its look must explain its role?"}
  ],

  log: [
    {date: "2026-10-29", text: "Grown chitin chosen. Round 3: the Soldier dual-wields broadswords (three blade options), the Hunter loses its spit and stands in line, the Door-head becomes a shield-and-fist brawler."},
    {date: "2026-10-29", text: "No metal for ants: four material sets painted on the Knights sheet (chitin, amber, leaf and silk, clay and fungus)."},
    {date: "2026-10-29", text: "Jiri chose the Knights take: the Queen great, the Soldier and Hunter too alike. Round 2: Soldier with mandibles, shield and greatsword; Hunter with a raised biological venom sac (no gun). The Hunter came out six-legged."},
    {date: "2026-10-29", text: "Ten takes painted on the first five ants (Worker, Soldier, Hunter, Door-head, Queen), human-like to Zerg-like (Ants: Ten takes)."},
    {date: "2026-10-29", text: "Agreed: the ants' racial identity is mobility (tunnels), numbers and mutations. No 'only they can' mechanic beyond those; the Door-head is simply a tank (no choke-sealing)."},
    {date: "2026-10-29", text: "Armies tab: Warcraft III's and StarCraft II's seven races compared role by role, what makes each itself, and what it means for the ants (air as their natural weakness)."},
    {date: "2026-10-29", text: "Proposal 1 parked: only the Door-head is kept. Cross-referenced Zerg's roles against real ants (Ants: Zerg roles against real ants): the ground roles are covered, air is the gap."},
    {date: "2026-10-29", text: "Agreed: regular castes are bipedal with their real trait exaggerated (no trade-off with readability). Every unit is built on one true fact about its real animal, now its Lore line. The Door-head is the first anchor: a round disc from above, a hat over a cute face in cutscenes."},
    {date: "2026-10-29", text: "Ants only for now. Everything today is a tech demo and the story will be reworked, so neither constrains the roster. Jiri: regular basic units, wild mutations (weirder and weirder through the missions), the roster disrupted and drawn from real nature, with 2–3 units that are not ants. Proposal 1 written (Ants tab)."},
    {date: "2026-10-29", text: "Workbench started: readability standard, both races' units, a bee placeholder, a first pass of real enemies and allies, the bipedal question; the ant roster lineup from the Combat Lab (labs/ant-roster.lab.json)."}
  ]
};
