# Retirement Simulator

This project is a CLI Retirement Simulator built in Rust to model investments,
income, and expenses and simulate the chances for a successful retirement.

There are tons of high quality simulators, why another one? Good
question. Unlike the many good simulators available, this one is free!
Simulators that I've found so far are either too simplistic (like the free ones
available at most brokerages) or hide the assumptions they are making. Either that
or the more full-featured ones require a monthly or annual fee.

This application allows me to truly understand the assumptions and the calculations. 

The other reason is as a CLI program, the simulation values can be entered
and stored locally without your sensitive financial information going to 
somebody else's server.

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

All you need is a Rust development environment installed. To run, simply execute:

```
cargo run input.yaml
```

The configuration is entered in input.yaml. A template is included in the
root directory of this project.

## Configuration Values

A sample configuration file is found in input.yaml.

All dollar amounts are in today's dollars. All dollar amounts and percentages are 
floating numbers and must be entered with a decimal (e.g. 3140.00).

### Retirees

The retirees section contains blocks for either one or two retirees depending whether the simulation is
for a single person or married couple. For a single person, simply delete one of the blocks. The retiree
retiring first should be in the first block.

| Value | Description |
| --- | --- |
| name | Used for reporting purposes. |
| date_of_birth | Entered in mm/dd/yyyy. |
| retirement_age | Age when contributions will end and withdrawals from investments to pay for expenses will begin. |
| life_expectency | Many experts recommend to plan to around 90 so you don't run out of money if all goes well. |
| wage_annual_salary | This is your pre-retirement salary, used only for calculating your investment contributions |
| retirement_contribution_percent | Percentage of salary that you're contributing to your retirement accounts. |
| hsa_contribution_annual | Currently this isn't used, but will be included in the future. |
| social_security_age | Age when you intend to take social security. You can try different ages to find best one to use. Note this doesn't need to be the retirement age. |
| pension_age | Age when you start receiving your pension benefits |
| pension_monthly_income | Monthly income from your pension |
| other_monthly_retirement_income | Any other source of income |

Also in the retirees section is the social security amounts. These depend on
your age and earnings history.  To get these values, go to the [Social Security
Administration web site](https://www.ssa.gov/myaccount), creating an account,
and entering in the values reported for you.

| Value | Description |
| --- | --- |
| social_security_amount_early | Amount if you elect social security at age 62 |
| social_security_amount_full | Amount if you elect social security at age 67 |
| social_security_amount_delayed | Amount if you elect social security at age 70 |

### Portfolio

The portfolio section represents your investment accounts.

| Value | Description |
| --- | --- |
| Balance | Today's balance of all investments |

The next blocks are the `pre-retirement_allocation` block which is your asset
allocation before retirement and `post-retirement_allocation` block which is
your asset allocation after retirement. In each block, this is the percentage
allocated to US equities, international equities, and bonds. Each block should
add up to exactly 100.0.

| Value | Description |
| --- | --- |
| us_equities | Percentage of US stocks |
| international | Percentage of international stocks |
| bonds | Percentage of bonds |

The next group of values is the expected returns and expected standard deviation
of returns. The sample file contains forecasted longterm returns published by
Fidelity Investments. You may change these as you like. All expected returns
are real returns as opposed to nominal returns. In other words, the returns are
the actual returns minus inflation.

### Expenses

The expenses section is your estimated monthly expenses during retirement. A
good way of estimating this amount is using your current take-home pay (after
taxes, retirement contributions, benefits, etc.).

### Tax Rates

This section contains the IRS tax rates and standard deduction. The sample file contains the latest values for married
couples filing jointly. You can change this for singles or values from future years.

## Output

There are 3 sections of the output: Uniform returns, Historical returns, and Monte Carlo simulation.

All output is in today's dollars.

### Uniform Returns

This simulation assumes that the returns for each asset class match the expected
returns in the input file every year (effectively setting the standard deviation to 0).

The output is a table with the following columns:

| Column | Description |
| --- | --- |
| Year | Year that is simulated |
| Age | Age of first retiree |
| Balance | Total balance in all accounts in today's dollars |
| Expenses | The retirement expenses in today's dollars |
| Income | The total income from all sources during retirement |
| Tax | The estimated tax paid on the income and withdrawals from the investment accounts during retirement |
| Rate | The estimated marginal tax rate during retirement |
| Draw | The percentage of assets withdrawn from the investment accounts. A popular rule of thumb aims to keep this at less than 4%. |
| Yield | The annual investment yield for the portfolio |

### Historical Simulation

This simulation uses historical investment data to simulate starting a
retirement at any time in history. First it simulates a retirement starting
in 1928. Then it simulates starting in 1929. And so on. If it runs out of years
to simulate, it starts again in 1928. For example, if the data is available for
1928 to 2023 and the simulation starts in 2022, the simulations first year of
retirement will be 2022, next 2023, next 1928, next 1929, etc.

The output will print the percentage of years that were successful, i.e. the
number of years where the balance didn't go to 0 at any point. 

The simulation results are then sorted from worst to best results and printed.

### Monte Carlo Simulation

The Monte Carlo simulation takes the expected returns and standard deviations
in the input, and simulates each year with a random value. That random value
will be chosen from a normal distribution with the mean and standard deviation 
from the input file. 1000 such simulations will be performed.

The output is the percentage of successful simulations. The worst year's result
will be printed in a format the same as the Uniform Simulation.

## About the Simulation

**Inflation.** All input and output is in terms of today's dollars and real returns. This makes
it easier to interpret the output of future years. For example, if your monthly expenses in 30 years
are $20K, is that high or low? It is hard to interpret amounts that far in the future because of the
compounding of inflation. Instead, the effect of inflation is accounted for by the lower returns
of investments. It is assumed that social security, pensions, and other income are adjusted 
for inflation annually. 

**Rebalancing.** It is assumed that the portfolio is rebalanced continuously.

**Which simulation do I use?** This is up to you. The uniform simulation is unrealistic
but give some insight into your retirement plan. The historical returns can be considered
what other retirement planners call an "optimistic result" because historical returns
have been high compared to what the forecasts are like. Aiming for a 100% success rate
may be a good idea. The default values for the Monte Carlo results are from Fidelity 
Investment's published forecasts, and are pretty dismal compared to historical returns.
Other investment firms also are forecasting low returns. Therefore, these may correspond
to a "pessimistic result". Most planning software recommends a success ratio around
85% to 90% to be a good plan.

**Why only 3 asset classes?** This application only models US stocks, international
stocks, and bonds. There's lots of other asset classes out there: small caps, emerging
markets, REITS, long-term bonds, short-term bonds, TIPS, gold, bitcoin, etc. But the
goal here is not to be a portfolio optimizer but a rough estimate of a retirement plan.
The expected returns are already so speculative that fine tuning the granularity seems
like overkill. The strategy here is to model some rough parameters, and then try 
to optimize the portfolio outside of that. Fine tuning the asset mix is unlikely to 
affect the outcome enough to change the overall plan.

## Limitations

The goal of this application is to get an idea of the big picture of a retirement plan.
There is so much unceratainty about future returns and expenses, that it is futile to 
try to focus on the small details. In that spirit, this application simplified some assumptions.
However, as the application evolves, these limitations may be removed in the future.

**Social security income.** There is no formula to calculate social security benefits. The
Social Security Administration publishes tables to look up the income based on the earnings
history and the age of the retiree. Instead of reproducing this table in code, this application
simply makes an estimate based on the values reported by the Social Security web site. These
values include estimated benefits at ages 62, 27, and 70 (for later retirees). Ages in between
are simply interpolated (averaged) between the reported values.

**Income tax.** This can get very complicated. Social Security is taxed either
at 50% or 85%, depending on total income. This application simply assumes social
security is taxed at 85%. It also assumes all other income (including what is
withdrawn from the retirement accounts) is taxed at the normal income tax
rate. Besides the standard deduction, we don't make any other adjustments to
income or allow for itemization.  State taxes are ignored. We do estimate the
taxes needed to pay for the withdrawals from the retirement accounts and add that
to the withdrawal.

## Disclaimer

I am not a financial advisor and this application is only a tool for you to use that
may or may not be helpful or accurate. Consult a licensed financial advisor before
making any financial decisions.

