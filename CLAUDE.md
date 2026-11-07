# Retirement simulator

A Rust retirement simulator: a library (the engine) and a command line
program. The goal is a public web site that runs the engine in the browser.
What the simulator does, and its limitations, are in `README.md`. What's
done and what's next is in `ROADMAP.md`, a local file that isn't committed
yet.

## Checks

Every change must pass these (CI runs them too):

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Checking that results don't change

Before a change, save the output of `cargo run -- --seed 42 input.yaml`.
- For a refactor, the output must be identical afterwards.
- For a new feature, first check that it's identical with the feature turned
  off, then explain the differences it makes.

Simulations start on today's date, so compare runs from the same day.

## Modelling rules

- All amounts are in today's dollars, and returns are real (after
  inflation). Anything fixed in nominal dollars (a pension without a
  cost-of-living increase, the cost basis, the social security tax
  thresholds) shrinks with inflation in the simulation.
- Check tax and benefit figures against an official source (IRS, SSA) or
  several agreeing ones, and give the source (in a code comment or the pull
  request).
- Tax figures are the 2026 amounts, built in (`src/tax_rates.rs`).

## Input and documentation

- When the input format changes, update `input.yaml` and the README to
  match.
- Use plain words, not jargon, in input fields and the README (for example
  `social_security_amount_full`, not "PIA").
- Validate new input fields in `Input::validate`, with limits like the rest.
- The README's "Limitations" section lists what isn't modelled. Keep it up to
  date, but don't add a separate "Limits" section.

## Architecture

`src/lib.rs` is the engine; `src/main.rs` is the command line program (text
or `--json` output). The web site will wrap the same library.

- `input.rs`: the plan (`Input`), parsed from YAML with serde and validated.
- `portfolio.rs`: accounts (pre-tax, Roth, taxable with cost basis),
  allocation, and the market forecasts.
- `simulate.rs`: the month-by-month simulation of one scenario: income,
  expenses, withdrawals, taxes, and returns.
- `social_security.rs`, `rmd.rs`, `tax_rates.rs`: the rules for social
  security, required minimum distributions, and income tax.
- `historical_scan.rs`, `monte_carlo.rs`, `scan.rs`: run many scenarios with
  historical or random returns and collect the results.
- `runner.rs`: `run_all(input, seed)` runs the uniform, historical, and
  Monte Carlo simulations.
- `report.rs`: `report::build` turns the results into the report (yearly
  rows, success rates, percentile balance bands) that both outputs use.
- `returns.csv`: the historical returns, embedded at build time.

## Personal and local files

**Never commit a file with anyone's real financial information in it.** This
is a public repository, and git history keeps a file even after it's
deleted. The only input file ever committed is the sample `input.yaml`,
which must hold made-up numbers. Personal plans such as `retirement.yaml`,
their editor backups and autosaves, and any other file with real balances,
income, ages, or dates of birth stay local. Before every commit, check the
staged files (`git status`, `git diff --cached --stat`) for anything like
this, and stop and ask if in doubt.

Also don't commit these local files if they're present: `HANDOFF.md`,
`analyze-returns.py`, `histretSP.html`, `histretSP_files/`,
`returns.csv.bak1`.
