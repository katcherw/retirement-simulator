# Retirement Simulator

This project is a CLI Retirement Simulator built in Rust to model investments,
income, and expenses and simulate the chances for a successful retirement.

There are tons of high quality simulators, why another one? Good question.
Unlike the many good simulators available, this one is free! Simulators that
I've found so far are either too simplistic (like the free ones available at
most brokerages) or hide the assumptions they are making. Either that or the
more full-featured ones require a monthly or annual fee.

This application allows me to truly understand the assumptions and the
calculations.

The other reason is as a CLI program, the simulation values can be entered and
stored locally without your sensitive financial information going to somebody
else's server.

## Features

Runs 3 types of simulations:

* Uniform Returns. This assumes the same invesment returns every year and shows
  the cashflow throughout your lifetime.
* Historical Returns. This will simulate what will happen if you retire at each
  year in history and will calculate the percentage of successful returns. Since
  historical returns were higher than the future forecasts, this could be
  regarded as an optimistic simulation.
* Monte Carlo simulation. This takes the forecasted returns for each asset class
  and performs 1000 simulations, with each year's returns being randomly
  generated.
  
It is quick and easy to try different what-ifs.

## Building and Running

All you need is a Rust development environment installed. To run, simply
execute:

```
cargo run input.yaml
```

The configuration is entered in input.yaml. A template is included in the root
directory of this project.

The Monte Carlo simulation is random, so its results change slightly from run to
run. Each run prints the random seed it used. To repeat a run exactly, for
example to check whether a change to your plan really helped rather than just
getting a luckier draw, pass that seed with `--seed`:

```
cargo run -- --seed 42 input.yaml
```

Simulations start in the current month.

To get the results as JSON instead of text, for use by other programs, add
`--json`:

```
cargo run -- --json input.yaml
```

The JSON has the same results as the text output, plus `balance_bands` for the
historical and Monte Carlo simulations: the balance at the start of each year
at the worst, 10th, 25th, 50th, 75th, 90th percentiles and best, for charting
the range of outcomes over time. Dates are in yyyy-mm-dd format.

The simulation engine is also a Rust library (`src/lib.rs`), which the command
line program uses: `Input::from_yaml` parses and validates a plan,
`runner::run_all` runs all the simulations, and `report::build` analyzes their
results.

## Configuration Values

A sample configuration file is found in input.yaml.

All dollar amounts are in today's dollars. Dollar amounts and percentages may be
entered with or without a decimal (e.g. 3140.00 or 3140). Ages must be whole
numbers.

Misspelled or unknown values are reported as errors, with the line number. The
input may also be written in JSON, using the same names.

### Retirees

The retirees section contains blocks for either one or two retirees depending
whether the simulation is for a single person or married couple. For a single
person, simply delete one of the blocks. The age shown in the output is the age
of the retiree in the first block.

| Value                           | Description                                                                                                                                                                                                                       |
| ---                             | ---                                                                                                                                                                                                                               |
| name                            | Used for reporting purposes.                                                                                                                                                                                                      |
| date_of_birth                   | Entered in mm/dd/yyyy (yyyy-mm-dd is also accepted).                                                                                                                                                                              |
| retirement_age                  | Age when this retiree's wages and contributions end. Withdrawals from investments to pay for expenses begin when the first retiree retires; until the other retiree retires, their salary (minus contributions) counts as income. |
| life_expectancy                 | Many experts recommend to plan to around 90 so you don't run out of money if all goes well.                                                                                                                                       |
| wage_annual_salary              | This is your pre-retirement salary. It is used to calculate your investment contributions, and as income while your spouse is retired and you are still working.                                                                  |
| retirement_contribution_percent | Percentage of salary that you're contributing to your retirement accounts.                                                                                                                                                        |
| social_security_age             | Age (62 to 70) when you intend to claim social security. You can try different ages to find the best one. This doesn't need to be the retirement age. |
| pension_age                     | Age when you start receiving your pension benefits                                                                                                                                                                                |
| pension_monthly_income          | Monthly income from your pension                                                                                                                                                                                                  |
| other_monthly_retirement_income | Any other source of income                                                                                                                                                                                                        |

Also in the retirees section is the social security benefit at full
retirement age (the SSA calls it the primary insurance amount). It depends on
your earnings history. To get it, go to the [Social Security Administration
web site](https://www.ssa.gov/myaccount), create an account, and enter the
amount reported for your full retirement age.

| Value                       | Description |
| ---                         | ---         |
| social_security_amount_full | Monthly benefit at full retirement age. Enter 0.0 for a spouse who never worked: their spousal and survivor benefits come from the other retiree's amount. |

The benefits are then calculated from the full amounts:
- **Your own benefit** at `social_security_age`: reduced if you claim before
  your full retirement age (67 if born in 1960 or later, earlier for older
  retirees), down to 70% at 62. Delayed credits add 8% a year after full
  retirement age, up to 70.
- **Spousal benefit** while both retirees are alive: up to half of the other
  retiree's full amount, minus your own full amount. It's reduced if started
  before your full retirement age (down to 32.5% of the other's full amount at
  62), can't start until both of you have claimed, and doesn't grow after full
  retirement age.
- **Survivor benefit** after the other retiree dies: what they were receiving,
  including delayed credits. If they claimed early, at least 82.5% of their
  full amount. If they died before claiming, their full amount plus any
  delayed credits earned by then. It's reduced if started before your survivor
  full retirement age (down to 71.5% at 60). The survivor receives the larger
  of their own benefit and the survivor benefit.
- **Earnings test**: if you claim before full retirement age while still
  working, $1 of benefits is withheld for every $2 of wages above $24,480 a
  year. In the year you reach full retirement age, $1 for every $3 above
  $65,160, counting only wages before that month. Benefits are withheld from
  January on, and months after you retire are always paid. Your wages also
  withhold the spousal benefit paid on your record. At full retirement age,
  your benefits are recalculated as if you had claimed later by the number of
  months withheld, so withholding mostly delays benefits rather than losing
  them. If your salary is well above the limit, claiming before you retire
  works out the same as claiming when you retire.

### Portfolio

The portfolio section represents your investment accounts.

| Value        | Description                                                                                                                                                                                                                     |
| ---          | ---                                                                                                                                                                                                                             |
| balance      | Today's balance of all accounts except Roth: 401(k), 403(b), traditional IRA, and brokerage accounts. Withdrawals are taxed as ordinary income. Contributions and any income left over after expenses and taxes are added here. |
| roth_balance | Today's balance of Roth IRA, Roth 401(k), and HSA accounts. Withdrawals aren't taxed. Optional; if it's left out, all savings are treated as `balance`.                                                                         |

Withdrawals come from `balance` first, and from `roth_balance` only after
`balance` runs out. Every account holds the same asset allocation (below).

An HSA is treated as a Roth account, which assumes it will be spent on medical
expenses (tax free). Brokerage accounts are included in `balance`, so their
withdrawals are taxed as ordinary income, even though in practice only the
gains would be taxed, at lower capital gains rates. This overstates their taxes,
so results are on the safe side.

The next blocks are the `pre_retirement_allocation` block which is your asset
allocation before retirement and `post_retirement_allocation` block which is
your asset allocation after retirement. In each block, this is the percentage
allocated to US equities, international equities, and bonds. Each block should
add up to exactly 100.0.

| Value         | Description                        |
| ---           | ---                                |
| us_equities   | Percentage of US stocks            |
| international | Percentage of international stocks |
| bonds         | Percentage of bonds                |

The optional `forecasts` block inside the portfolio block has the expected
returns and expected standard deviation of returns, and the correlations below.
Forecasted longterm returns published by Fidelity Investments are built in (see
`src/defaults.rs`), and the sample file spells them out. Leave out the block,
or any value in it, to use the built-in value, or change them as you like. All
expected returns are
real returns as opposed to nominal returns. In other words, the returns are the
actual returns minus inflation.

Expected returns must be geometric (compound annual) averages, which is how
Fidelity and most other firms publish their forecasts. This is the rate money
actually grows at over many years. It is lower than the simple average of the
yearly returns, because volatility drags down compound growth: a +50% year
followed by a -50% year averages 0%, but leaves you with only 75% of your money.

The last group of values in the `forecasts` block is the correlation of annual
returns between the asset classes, used by the Monte Carlo simulation.

| Value                           | Description                                                                   |
| ---                             | ---                                                                           |
| us_international_correlation    | Correlation between US and international stocks. The sample file uses 0.8.    |
| us_bonds_correlation            | Correlation between US stocks and bonds. The sample file uses 0.3.            |
| international_bonds_correlation | Correlation between international stocks and bonds. The sample file uses 0.3. |

Correlations range from -1.0 (always move in opposite directions) to 1.0 (always
move together). US and international stocks have historically been strongly
correlated (about 0.7 to 0.85 depending on the period). The correlation between
stocks and bonds has varied widely, from negative in 2000-2020 to strongly
positive in years like 2022, so the sample value of 0.3 is a cautious middle
ground. Some investment firms, such as J.P. Morgan in its Long-Term Capital
Market Assumptions, publish forecasted correlations along with expected returns.

### Expenses

The expenses section is your estimated monthly expenses during retirement. A
good way of estimating this amount is using your current take-home pay (after
taxes, retirement contributions, benefits, etc.).

| Value            | Description                                                                                                                               |
| ---              | ---                                                                                                                                       |
| monthly          | Monthly expenses during retirement.                                                                                                       |
| survivor_percent | Percentage of `monthly` that remains after one spouse dies (e.g. 70.0). Use 100.0 for no change. For a single retiree this has no effect. |

### Tax Rates

Tax rates aren't part of the configuration: the 2026 federal tax brackets,
standard deductions, and the additional deduction for retirees 65 or older are
built in (see `src/tax_rates.rs`), for two filing statuses. Married filing
jointly is used while two retirees are alive. Single is used for a single
retiree, and for the surviving spouse after one retiree dies. Tax brackets are
adjusted for inflation every year, so in today's dollars they stay about the
same in future years.

Social security is taxed based on provisional income: all other taxable income
(including withdrawals) plus half of the social security benefits. Unlike the
tax brackets, the social security thresholds are set by law and are not
adjusted for inflation ($32,000 and $44,000 married, $25,000 and $34,000
single), so they get smaller every year in today's dollars, assuming 2.5%
inflation. The historical simulation uses the actual inflation for each year
instead. Over time, more retirees pay tax on 85% of their benefits.

## Output

There are 3 sections of the output: Uniform returns, Historical returns, and
Monte Carlo simulation.

All output is in today's dollars.

### Uniform Returns

This simulation assumes that the returns for each asset class match the expected
returns in the input file every year (effectively setting the standard deviation
to 0).

The output is a table with the following columns:

| Column   | Description                                                                                                                                            |
| ---      | ---                                                                                                                                                    |
| Year     | Year that is simulated                                                                                                                                 |
| Age      | Age of first retiree                                                                                                                                   |
| Balance  | Total balance in all accounts in today's dollars                                                                                                       |
| Expenses | The retirement expenses in today's dollars                                                                                                             |
| Income   | The total income from all sources during retirement                                                                                                    |
| Tax      | The estimated tax paid on the income and withdrawals from the investment accounts during retirement                                                    |
| Rate     | The estimated marginal tax bracket during retirement. Withdrawals can be taxed at a higher effective rate when they make more social security taxable. |
| Draw     | The percentage of assets withdrawn from the investment accounts. A popular rule of thumb aims to keep this at less than 4%.                            |
| Yield    | The annual investment yield for the portfolio                                                                                                          |

### Historical Simulation

This simulation uses historical investment data to simulate starting your plan
(today) in each year of history. First it runs your plan with the returns from
1928, 1929, and so on. Then it runs it again starting with the returns from
1929. And so on, for every year of data.

When a scenario runs out of years of data, it continues from 1928. For example,
the data covers 1928 to 2025, so a 30 year plan starting with 2010 uses the
returns from 2010 to 2025, then 1928 to 1941. These later years are not a real
historical sequence, but wrapping around gives every starting year a full
length scenario, so more of history is tested. The output says how many
scenarios wrapped around, and lists their years as, for example, "2010 to 2025,
then 1928 to 1941".

The output will print the percentage of years that were successful, i.e. the
number of years where the balance didn't go to 0 at any point, followed by the
same summary as the Monte Carlo simulation (see below).

The simulation results are then sorted from worst to best results and printed.

### Monte Carlo Simulation

The Monte Carlo simulation takes the expected returns and standard deviations in
the input, and simulates each year with a random value. That random value will
be chosen from a normal distribution with the standard deviation from the input
file. Its mean is the expected return plus half the variance (the standard
deviation squared), which converts the geometric expected return into the
average yearly return needed for the money to compound at the expected rate. For
example, US stocks with an expected return of 3.2% and a standard deviation of
15% are drawn with a mean of 4.3%. The returns of the three asset classes are
drawn together using the correlations from the input file, so that, for example,
a bad year for US stocks is usually also a bad year for international stocks.
1000 such simulations will be performed. The random seed is printed first, so
the same simulations can be repeated with `--seed` (see Building and Running).

The output starts with the percentage of successful simulations, followed by a
summary of all the simulations:

* **Ending balance.** The balance at the end of the plan (in today's dollars)
  for the worst simulation, the 10th, 25th, 50th (median), 75th, and 90th
  percentiles, and the best simulation. For example, the 10th percentile is the
  ending balance that 10% of simulations fell below. Simulations where the money
  ran out count as $0.
* **When the money ran out.** For the simulations that failed, the earliest,
  median, and latest year the money ran out, with the ages of the retirees still
  alive that year. A failure late in life is a very different risk from one
  early in retirement.

Finally, the worst simulation is printed year by year in the same format as the
Uniform Simulation.

## About the Simulation

**Inflation.** All input and output is in terms of today's dollars and real
returns. This makes it easier to interpret the output of future years. For
example, if your monthly expenses in 30 years are $20K, is that high or low? It
is hard to interpret amounts that far in the future because of the compounding
of inflation. Instead, the effect of inflation is accounted for by the lower
returns of investments. It is assumed that social security, pensions, and other
income are adjusted for inflation annually.

**Rebalancing.** It is assumed that the portfolio is rebalanced continuously,
and that every account holds the same asset allocation.

**Which simulation do I use?** This is up to you. The uniform simulation is
unrealistic but give some insight into your retirement plan. The historical
returns can be considered what other retirement planners call an "optimistic
result" because historical returns have been high compared to what the forecasts
are like. Aiming for a 100% success rate may be a good idea. The default values
for the Monte Carlo results are from Fidelity Investment's published forecasts,
and are pretty dismal compared to historical returns. Other investment firms
also are forecasting low returns. Therefore, these may correspond to a
"pessimistic result". Most planning software recommends a success ratio around
85% to 90% to be a good plan.

**Why only 3 asset classes?** This application only models US stocks,
international stocks, and bonds. There's lots of other asset classes out there:
small caps, emerging markets, REITS, long-term bonds, short-term bonds, TIPS,
gold, bitcoin, etc. But the goal here is not to be a portfolio optimizer but a
rough estimate of a retirement plan. The expected returns are already so
speculative that fine tuning the granularity seems like overkill. The strategy
here is to model some rough parameters, and then try to optimize the portfolio
outside of that. Fine tuning the asset mix is unlikely to affect the outcome
enough to change the overall plan.

## Limitations

The goal of this application is to get an idea of the big picture of a
retirement plan. There is so much unceratainty about future returns and expenses
that it is futile to try to focus on the small details. In that spirit, this
application simplified some assumptions. However, as the application evolves,
these limitations may be removed in the future.

**Social security income.** Benefits are calculated from each retiree's full
amount using the SSA rules for full retirement age, early reductions, delayed
credits, spousal benefits, and survivor benefits. Claiming ages are whole
years. A survivor is assumed to start the survivor benefit when the spouse
dies, but not before the earlier of their own `social_security_age` and their
survivor full retirement age. The full amount reported by the SSA assumes you
keep working until you claim. The earnings test limits are the 2026 amounts,
and are kept the same in today's dollars. The earnings test withholds exactly the
amount owed, ending with part of a month; SSA withholds whole monthly checks
and refunds the difference the next year. The government
pension offset, the lump-sum death payment, and benefits for children are not
modelled.

**Income tax.** All income other than social security (including what is
withdrawn from `balance`) is taxed at the normal income tax rate. Roth
withdrawals aren't taxed. Required minimum distributions are not modelled.
Social security is taxed according to the IRS worksheet using provisional
income. Besides the standard deduction and the additional deduction for people
65 and older, we don't make any other adjustments to income or allow for
itemization. The temporary $6,000 senior deduction for 2025 through 2028 is not
included. State taxes are ignored. We do estimate the taxes needed to pay for
the withdrawals from the retirement accounts and add that to the withdrawal,
including the extra social security that becomes taxable because of the
withdrawal. Payroll taxes on wages are ignored. A survivor switches to single
rates immediately, although in practice they can file jointly for the year of
death.

**Death of a spouse.** When a retiree reaches their life expectancy, their
wages, pension, and other income stop. The survivor receives social security
survivor benefits as described above. Pension survivor benefits are not
modelled.

## Disclaimer

I am not a financial advisor and this application is only a tool for you to use
that may or may not be helpful or accurate. Consult a licensed financial advisor
before making any financial decisions.

