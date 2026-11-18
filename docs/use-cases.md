# What Tarafold is for

Tarafold fits when you have a table of numbers and want three things:

1. **a formula** that explains one outcome, readable enough to check;
2. **settings** that bring the outcome to a target, within limits you set;
3. **the next tests to run**, chosen to settle what the data cannot yet tell apart.

It works best when the inputs are things you actually set (temperature, dosage, price,
spend) and a compact relationship plausibly exists. Worked examples with data and real
output: [case studies](case-studies/README.md).

## Good fits

| Area | Typical question | What you get |
|---|---|---|
| Manufacturing | Which temperature, pressure, and cycle time keep defects under 1% at the lowest energy? | Formulas for defects and energy, settings meeting both goals, the trade-off between them |
| R&D: chemistry, materials, food, formulation | How does yield depend on composition and conditions? | The equation, a recipe for a target, the next lab runs |
| Lab science | Is there a law behind these measurements? | Candidate equations from simple to accurate, with grades; experiments that tell them apart |
| Agriculture | How do fertilizer and irrigation relate to yield? | Response curves, settings for a target yield, next field trials |
| Energy and facilities | How does consumption depend on load, weather, and setpoints? | A formula for Excel or SQL; setpoints for a budget |
| Engineering simulation | A simulator takes hours per run | A fast equation that stands in for it inside the simulated range |
| Pricing and marketing | How do price and spend relate to orders and revenue? | A chained funnel formula; spend and price for a revenue goal (as hypotheses to test) |
| Quality and operations | Which settings relate to scrap? | Ranked inputs, an equation, settings to try |

## What each feature is for

| Need | Feature |
|---|---|
| Understand an outcome | `tarafold run data.csv --target y` |
| See which inputs matter | `tarafold relate data.csv -t y`, the Relationships view |
| Hit a number | `--goal 2.0 --control a,b --bounds a=0..10` |
| Several goals with trade-offs | `--require "defect<=0.01" --prefer min:cost --prefer max:yield` |
| Respect limits | `--constrain "a + b <= 100"`, `--integer`, `--step`, `--max-change` |
| Only act when confident | `--robust 0.9` (joint probability over the bootstrap ensemble) |
| Decide the next experiments | `--suggest 8`, `tarafold suggest <dir>` |
| Use the formula elsewhere | `tarafold export <dir> --to excel\|sql\|python\|js\|latex\|json` |
| Share results | `report.html` (one offline file), `tarafold serve` |
| Lab data from designed experiments | `--data-source experimental --designed a,b` |
| Uneven measurement quality | `--weights w` |

## Poor fits

- **No compact formula**: images, text, hundreds of weak signals (for example customer
  churn). The report then shows the equation losing to the flexible baseline model.
- **Very few rows.** Ingest needs 30; useful results usually need a few hundred.
- **Observational data with hidden drivers.** If price was raised in months when demand was
  high anyway, an equation mixes both effects. Tarafold labels such results as association,
  not effect, and its candidates should be tested before use.
- **Far outside the data.** Predictions outside the observed range are extrapolations and
  marked as such.
- **Categorical or multi-table data as is.** Tarafold reads one numeric table. Join tables
  and encode categories first (see the enzyme case study).

## A pilot in five steps

1. Pick one process with one number to improve, settings you control, and a few hundred
   past rows.
2. `tarafold run data.csv --target kpi --control a,b,c --goal <value> --suggest 8 --open`
3. Review the formula, grade, and warnings with someone who knows the process.
4. Run the suggested tests, append the results, rerun. Two or three rounds are typical.
5. When the formula keeps predicting new tests, export it to where decisions are made.
