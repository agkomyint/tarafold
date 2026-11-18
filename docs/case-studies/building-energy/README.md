# Case study: heating energy in an office portfolio

**Field:** facilities management. **Shows:** observational data with correlated inputs,
fixing conditions, export to Excel and SQL, and how Tarafold treats extrapolation.

## The situation

1,500 daily meter readings from 40 office buildings: floor area, outdoor temperature,
thermostat setpoint, occupancy, and energy use. Managers raise the setpoint on cold days, so
setpoint and outdoor temperature are correlated, as in real logs. The question: what drives
consumption, and which setpoint keeps a 2,000 m² building at 400 kWh on a 2 °C day?

Hidden truth (2% meter noise):
`energy_kwh = 30 + 0.012 · area · (setpoint − outdoor) + 0.08 · occupancy`.

## Steps

```sh
python make_data.py work
tarafold run work/meters.csv --target energy_kwh --control setpoint_C \
  --fix floor_area_m2=2000 --fix outdoor_C=2 --fix occupancy=150 --goal 400 --out work/report
```

## What Tarafold found

`energy_kwh = 42.4 − 0.012 · (outdoor − setpoint) · area`, test R² 0.998, grade `high`,
stability 9/10.

- The physics is recovered: heat loss proportional to floor area times the indoor–outdoor
  difference, with the right coefficient 0.012.
- Occupancy (0.08 kWh per person, about 12 kWh on average) was absorbed into the constant
  (42.4 ≈ 30 + 12). At 2% noise its effect is close to the noise level.
- `building_id` was not used, correctly.
- Evidence is `functional_fit` with the observational note: the setpoint was not set
  deliberately here, so the equation describes association.

## The goal and extrapolation

The first run returned no candidate (exit code 5). The closest prediction was 436 kWh at
setpoint 18.4 °C, the lowest setpoint in the data, and Tarafold reported that bound as
binding. Reaching 400 kWh needs a setpoint below anything observed.

Allowing a wider range on purpose:

```sh
tarafold optimize work/report --control setpoint_C --fix floor_area_m2=2000 --fix outdoor_C=2 \
  --fix occupancy=150 --bounds setpoint_C=16..22 --goal 400
```

gives setpoint 16.9 °C, predicted 400 kWh [369, 431], marked **outside data domain**. Under
the true relationship it is 399.7 kWh, but Tarafold cannot know that; the label is the point.

## Using the formula

```sh
tarafold export work/report --to excel
# =(42.403670644487526-(0.011996693606493993*((outdoor_C-setpoint_C)*floor_area_m2)))
tarafold export work/report --to sql
# SELECT (42.40... - (0.01199... * ((CAST("outdoor_C" AS DOUBLE PRECISION) - ...)))) AS "predicted_energy_kwh" FROM "data";
```

The Excel formula refers to named cells (`outdoor_C`, `setpoint_C`, `floor_area_m2`), and
the header in the exported file lists the training range of each input.

## Takeaways

- Tarafold stays inside the data unless you widen the bounds, and labels it when you do.
- Small effects at the noise level can disappear into a constant; check the warnings and
  the screening table before concluding an input does not matter.
- Export puts the formula where budgets are made, with its validity range attached.
