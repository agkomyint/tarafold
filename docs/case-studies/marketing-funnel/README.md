# Case study: a marketing funnel with chained equations

**Field:** e-commerce. **Shows:** several linked targets, multi-goal optimization with a
trade-off set, and why business data needs the association caveat.

## The situation

2,000 rows of weekly data: ad `spend`, `price`, and the funnel `visitors`, `conversion`,
`orders`, `revenue`. Management wants revenue of at least 40,000 while keeping traffic
(and its cost) low, and would like to know the price that maximizes revenue.

Hidden truth (2% noise): `visitors = 120 · spend^0.6`, `conversion = 0.08 · e^(−0.03·price)`,
`orders = visitors · conversion`, `revenue = orders · price`.

## Steps

```sh
tarafold synth business-revenue --rows 2000 --noise 0.02 --out data
tarafold run data/data.csv --target visitors,conversion,orders,revenue \
  --control spend,price --require "revenue>=40000" \
  --prefer "min:visitors" --prefer "max:revenue" --suggest 6 --stability 3 --out report --open
```

Listing the targets in funnel order lets each one use the ones before it, so goals on
revenue are evaluated through the whole chain.

## What Tarafold found

| Target | Selected equation | Test R² | Grade |
|---|---|---|---|
| visitors | `120 · spend^0.6` | 1.0000 | high |
| conversion | `0.08 · e^(−0.03·price)` | 1.0000 | high |
| orders | `visitors · conversion` | 1.0000 | high |
| revenue | `0.9995 · price · orders` | 0.9996 | high |

Every link of the funnel was recovered with the right constants.

## Trade-off set

NSGA-II returned 20 candidates between "just enough revenue" and "maximum revenue". Three
are marked:

| Mark | spend | price | revenue | visitors | P(revenue ≥ 40k) |
|---|---|---|---|---|---|
| best for low traffic | 16,561 | 32.87 | 40,000 | 40,797 | 0.60 |
| knee (balanced) | 28,440 | 33.17 | 55,337 | 56,434 | 1.00 |
| best for revenue | 49,985 | 33.32 | 77,618 | 79,156 | 1.00 |

Every candidate sits at a price near 33. That matches the truth: revenue ∝ price · e^(−0.03·price)
peaks at price = 1/0.03 ≈ 33.3. The cheapest option meets the goal in only 60% of the
bootstrap ensemble, because it sits right on the limit. The other options are safe.

## The caveat

On real business data, price and spend are usually set in response to the market. Then an
equation describes how they moved together, not what a change would do. Tarafold says so in
every report ("association, not the effect of intervening"). Treat the knee point as a
hypothesis for an A/B test, not a decision.

## Takeaways

- Chained targets turn one model into a funnel you can reason about step by step.
- The trade-off set shows the price of each extra unit of revenue in traffic.
- Joint probability flags options that only work if the estimate is exactly right.
