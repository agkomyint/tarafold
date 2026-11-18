# Case studies

Five worked examples, each with a script that generates the data, the exact commands, and
the output Tarafold v0.1.0 produced. They are **synthetic**: the data comes from a hidden
equation so every result can be checked against the truth. They are not customer stories.

| Case | Field | What it shows | Result in one line |
|---|---|---|---|
| [Enzyme kinetics](enzyme-kinetics/README.md) | Lab science | 5 related tables, instrument calibration, 6 decoy columns, a law outside Tarafold's test set | Right 3 inputs; R² 0.998; all 5 goal candidates within 1% under the true law; graded `medium`, not `high` |
| [Marketing funnel](marketing-funnel/README.md) | Business | 4 chained targets, multi-goal trade-off | Every link of the funnel recovered exactly; revenue-maximizing price ≈ 33 = 1/0.03 |
| [Injection molding](injection-molding/README.md) | Manufacturing | Hard limit plus cost objective, whole-number input, approximate formula | Energy formula recovered; defect formula approximated; settings sit on the limit (within 0.0005); a margin fixes it |
| [Crop trials](crop-trials/README.md) | Agriculture | Closed loop: goal outside past trials, 4 seasons of suggested plots | Goal reached under the true response in seasons 1, 2 and 4; grades stay honest about the formula |
| [Building energy](building-energy/README.md) | Facilities | Observational data, correlated inputs, export to Excel/SQL, extrapolation | `0.012 · area · (setpoint − outdoor)` recovered; refuses to go outside observed setpoints until told to |

Run any of them with the scripts in each folder and an installed `tarafold`.
