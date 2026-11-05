/**************************************************************************
* simulate.rs
*
* Performs the simulation of a retirement scenario
**************************************************************************/

use crate::portfolio::{Accounts, WithdrawalIncome};
use crate::rmd;
use crate::social_security::{self, HouseholdBenefits};
use crate::utils::*;
use crate::{Input, TaxLevel};
use chrono::{Datelike, NaiveDate};

// stores results of each month of the simulation
#[derive(Debug)]
pub struct MonthlySnapshot {
    pub date: NaiveDate,
    pub balance: f64,
    pub expenses: f64,
    pub income: f64,
    pub tax_rate: f64,
    pub taxes: f64,
    pub withdrawal_rate: f64,
    pub annualized_return: f64,
}

// values collected for each retiree during simulation to make
// reporting easier
#[derive(Debug)]
pub struct RetireeInfo {
    pub name: String,
    pub retirement_date: NaiveDate,
    // first day after the retiree's life expectancy is reached
    pub death_date: NaiveDate,
    // when the retiree claims their own social security benefit
    pub social_security_date: NaiveDate,
    pub date_of_birth: NaiveDate,
}

#[derive(Debug, Default)]
pub struct SimulationResults {
    pub retirement_date: NaiveDate,
    pub retirees: Vec<RetireeInfo>,
    pub monthly_snapshot: Vec<MonthlySnapshot>,
    pub average_return: f64,
}

// returns the annual tax and the marginal tax rate (percentage) for the given
// annual taxable income. Tax levels must be sorted by income, with each
// level's income being the lower bound of its bracket.
fn bracket_taxes(taxable_income: f64, tax_levels: &[TaxLevel]) -> (f64, f64) {
    let mut annual_tax: f64 = 0.0;
    let mut marginal_rate: f64 = 0.0;

    for (i, level) in tax_levels.iter().enumerate() {
        if taxable_income <= level.income {
            break;
        }
        let upper = tax_levels.get(i + 1).map_or(f64::MAX, |next| next.income);
        annual_tax += (taxable_income.min(upper) - level.income) * level.rate / 100.0;
        marginal_rate = level.rate;
    }

    (annual_tax, marginal_rate)
}

// returns the taxable part of annual social security benefits, following the
// IRS worksheet: provisional income is all other income plus half of the
// benefits. Above the base amount, half of the excess is taxable (up to half
// of the benefits). Above the adjusted base amount, 85% of that excess is
// added, up to 85% of the benefits.
fn taxable_social_security(benefits: f64, other_income: f64, base: f64, adjusted_base: f64) -> f64 {
    let provisional_income = other_income + benefits / 2.0;
    if provisional_income <= base {
        0.0
    } else if provisional_income <= adjusted_base {
        f64::min((provisional_income - base) / 2.0, benefits / 2.0)
    } else {
        let below_adjusted_base = f64::min((adjusted_base - base) / 2.0, benefits / 2.0);
        f64::min(
            (provisional_income - adjusted_base) * 0.85 + below_adjusted_base,
            benefits * 0.85,
        )
    }
}

// the tax rules for one month: the filing status's table, with the standard
// deduction and social security thresholds adjusted for the household
struct TaxRules<'a> {
    tax_levels: &'a [TaxLevel],
    capital_gains_levels: &'a [TaxLevel],
    standard_deduction: f64,
    social_security_base: f64,
    social_security_adjusted_base: f64,
}

impl TaxRules<'_> {
    // returns the monthly tax and the marginal tax bracket (percentage) for
    // monthly social security benefits, ordinary income, and long-term
    // capital gains and qualified dividends. The standard deduction comes
    // off ordinary income first, and the capital gains are taxed at their
    // own rates, stacked on top of ordinary income. They also count toward
    // the income that makes social security taxable.
    fn taxes(&self, social_security: f64, ordinary_income: f64, capital_gains: f64) -> (f64, f64) {
        let social_security = social_security * 12.0;
        let ordinary_income = ordinary_income * 12.0;
        let capital_gains = capital_gains * 12.0;
        let other_income = ordinary_income + capital_gains;
        let taxable_income = other_income
            + taxable_social_security(
                social_security,
                other_income,
                self.social_security_base,
                self.social_security_adjusted_base,
            );
        let taxable_income = (taxable_income - self.standard_deduction).max(0.0);
        let taxable_gains = capital_gains.min(taxable_income);
        let taxable_ordinary = taxable_income - taxable_gains;
        let (ordinary_tax, marginal_rate) = bracket_taxes(taxable_ordinary, self.tax_levels);
        let gains_tax = bracket_taxes(taxable_income, self.capital_gains_levels).0
            - bracket_taxes(taxable_ordinary, self.capital_gains_levels).0;
        ((ordinary_tax + gains_tax) / 12.0, marginal_rate)
    }

    // taxes are paid first from any income left over after expenses (the
    // surplus). The rest must be withdrawn, but these withdrawals are also
    // taxed, causing more withdrawals, and more taxes and so on. Extra
    // income can also make more social security taxable, so there is no
    // simple formula. Instead, search for the extra withdrawal that exactly
    // pays the taxes not covered by the surplus.
    //
    // withdrawals (for expenses) and the extra withdrawal come from the
    // taxable account first (only the gain is taxed), then pre-tax (taxed as
    // ordinary income), then Roth (tax free). Returns the monthly tax
    // (including the tax on the extra withdrawal) and the marginal bracket.
    fn taxes_with_withdrawals(
        &self,
        social_security: f64,
        income: WithdrawalIncome,
        withdrawals: f64,
        surplus: f64,
        accounts: &Accounts,
    ) -> (f64, f64) {
        let taxes = |extra: f64| {
            let from_accounts = accounts.withdrawal_income(withdrawals + extra);
            self.taxes(
                social_security,
                income.ordinary + from_accounts.ordinary,
                income.capital_gains + from_accounts.capital_gains,
            )
        };
        let shortfall = |extra: f64| taxes(extra).0 - surplus;
        if shortfall(0.0) <= 0.0 {
            return taxes(0.0);
        }

        // the effective marginal rate is below 100% (tax rates are below 50%,
        // and at most 1.85 times that with social security), so the shortfall
        // grows more slowly than the withdrawal and there is a solution
        let mut low = 0.0;
        let mut high = shortfall(0.0);
        while shortfall(high) > high {
            low = high;
            high *= 2.0;
        }
        for _ in 0..40 {
            let middle = (low + high) / 2.0;
            if shortfall(middle) > middle {
                low = middle;
            } else {
                high = middle;
            }
        }
        taxes(high)
    }
}

// represents a simulation run
pub struct Simulation<'a> {
    pub simulation_results_: SimulationResults,

    input_: &'a Input,
    current_date_: NaiveDate,
    accounts_: Accounts,
    social_security_: HouseholdBenefits,
    expenses_: f64,
    sum_of_returns_: f64,
    // the social security tax thresholds aren't adjusted for inflation, so
    // they shrink in today's dollars. Starts at 1.0.
    social_security_threshold_scale_: f64,
    // this year's required minimum distribution, per month
    rmd_monthly_: f64,
    // each retiree's pension in today's dollars, as a fraction of the amount
    // entered. Stays 1.0 for pensions that keep up with inflation.
    pension_scale_: Vec<f64>,
}

impl<'a> Simulation<'a> {
    pub fn new(input: &'a Input) -> Result<Self, String> {
        // the household retires (expenses start being paid from the portfolio)
        // when the first retiree retires
        let first_retiree = input
            .retirees
            .iter()
            .min_by_key(|r| add_years(&r.date_of_birth, r.retirement_age))
            .ok_or("at least one retiree is required")?;
        let retirement_date = add_years(&first_retiree.date_of_birth, first_retiree.retirement_age);
        let current_date = input.start_date;

        let mut simulation_results = SimulationResults {
            retirement_date,
            retirees: Vec::new(),
            monthly_snapshot: Vec::new(),
            average_return: 0.0,
        };

        for retiree in input.retirees.iter() {
            let retiree_result = RetireeInfo {
                name: retiree.name.to_string(),
                retirement_date: add_years(&retiree.date_of_birth, retiree.retirement_age),
                death_date: add_years(&retiree.date_of_birth, retiree.life_expectancy + 1),
                social_security_date: add_years(
                    &retiree.date_of_birth,
                    retiree.social_security_age,
                ),
                date_of_birth: retiree.date_of_birth,
            };
            simulation_results.retirees.push(retiree_result);
        }

        let people: Vec<social_security::Person> = input
            .retirees
            .iter()
            .zip(simulation_results.retirees.iter())
            .map(|(retiree, info)| social_security::Person {
                date_of_birth: retiree.date_of_birth,
                pia: retiree.social_security_amount_full,
                claiming_age: retiree.social_security_age,
                death_date: info.death_date,
                annual_earnings: retiree.salary_annual,
                retirement_date: info.retirement_date,
            })
            .collect();

        let expenses = input.expenses.monthly;

        Ok(Self {
            simulation_results_: simulation_results,
            input_: input,
            current_date_: current_date,
            accounts_: input.portfolio.accounts(),
            social_security_: HouseholdBenefits::new(&people),
            expenses_: expenses,
            sum_of_returns_: 0.0,
            social_security_threshold_scale_: 1.0,
            rmd_monthly_: 0.0,
            pension_scale_: vec![1.0; input.retirees.len()],
        })
    }

    // a pension without a full cost-of-living adjustment is a fixed amount
    // from today (as on a pension statement), so inflation shrinks it in
    // today's dollars. Once it starts, its cost-of-living increases offset
    // some of that. Inflation is annual percent.
    fn adjust_pensions_for_inflation(&mut self, date: &NaiveDate, inflation: f64) {
        for (retiree, scale) in self.input_.retirees.iter().zip(&mut self.pension_scale_) {
            let Some(cola) = retiree.pension_cola_percent else {
                continue;
            };
            let mut growth = 1.0 / (1.0 + inflation / 100.0);
            if *date >= add_years(&retiree.date_of_birth, retiree.pension_age) {
                growth *= 1.0 + cola / 100.0;
            }
            *scale *= growth.powf(1.0 / 12.0);
        }
    }

    // returns true if simulation finished. Returns and inflation are annual
    // percentages.
    pub fn run_simulation_one_month(
        &mut self,
        us_equity_expected_returns: f64,
        international_equity_expected_returns: f64,
        bonds_expected_returns: f64,
        inflation: f64,
    ) -> Result<bool, String> {
        let date = self.current_date_;
        let infos = &self.simulation_results_.retirees;
        let alive: Vec<bool> = infos.iter().map(|info| date < info.death_date).collect();
        if !alive.contains(&true) {
            return Ok(true);
        }
        let someone_died = alive.contains(&false);
        let household_retired = date >= self.simulation_results_.retirement_date;

        // pre-retirement contributions, until each retiree's own retirement.
        // Before the household retires, wages are assumed to cover expenses.
        // After that, a retiree who is still working brings in their wages
        // (minus pre-tax contributions) as income.
        let mut wages = 0.0;
        for (i, retiree) in self.input_.retirees.iter().enumerate() {
            if alive[i] && date < infos[i].retirement_date {
                let contribution =
                    retiree.salary_annual * retiree.retirement_contribution_percent / 100.0 / 12.0;
                self.accounts_.contribute(contribution);
                if household_retired {
                    wages += retiree.salary_annual / 12.0 - contribution;
                }
            }
        }

        // required minimum distributions from the pre-tax account, figured
        // each January (or in the first month) from the balance then, and
        // taken evenly over the year. They use the age of the oldest retiree
        // alive. They're taxed as income and spent first; what isn't spent
        // goes to the taxable account.
        if date.month() == 1 || self.simulation_results_.monthly_snapshot.is_empty() {
            let oldest = infos
                .iter()
                .zip(&alive)
                .filter(|&(_, &alive)| alive)
                .map(|(info, _)| info.date_of_birth)
                .min();
            self.rmd_monthly_ = oldest.map_or(0.0, |date_of_birth| {
                rmd::annual_distribution(self.accounts_.pre_tax, &date_of_birth, date.year()) / 12.0
            });
        }
        let rmd = self.accounts_.withdraw_pre_tax(self.rmd_monthly_);

        // social security (own, spousal, and survivor benefits), before or
        // after retirement
        let social_security = self.social_security_.monthly_total(&date);

        // all income other than social security is fully taxable
        let mut income = social_security + wages;
        let mut taxable_income = wages;

        // interest and dividends in the taxable account are reinvested, but
        // taxed every year
        let allocation = self.input_.portfolio.allocation(household_retired);
        let (interest, dividends) = self
            .accounts_
            .investment_income(allocation, &self.input_.portfolio.forecasts);
        self.accounts_.reinvest(interest + dividends);
        taxable_income += interest;

        // pension income, before or after retirement, until the retiree dies
        for (i, retiree) in self.input_.retirees.iter().enumerate() {
            let pension_date = add_years(&retiree.date_of_birth, retiree.pension_age);
            if alive[i] && date >= pension_date {
                let pension = retiree.pension_monthly_income * self.pension_scale_[i];
                income += pension;
                taxable_income += pension;
            }
        }

        // other retirement income, after each retiree's own retirement
        for (i, retiree) in self.input_.retirees.iter().enumerate() {
            if alive[i] && date >= infos[i].retirement_date {
                income += retiree.other_monthly_retirement_income;
                taxable_income += retiree.other_monthly_retirement_income;
            }
        }

        // expenses are paid from the portfolio only after retirement, and are
        // reduced for a surviving spouse
        let expenses = if !household_retired {
            0.0
        } else if someone_died {
            self.expenses_ * self.input_.expenses.survivor_percent / 100.0
        } else {
            self.expenses_
        };
        let withdrawals = f64::max(expenses - income - rmd, 0.0);

        // tax on income and withdrawals, using the filing status for the
        // number of retirees alive
        let num_alive = alive.iter().filter(|&&a| a).count();
        let num_65_or_older = self
            .input_
            .retirees
            .iter()
            .enumerate()
            .filter(|&(i, retiree)| alive[i] && date >= add_years(&retiree.date_of_birth, 65))
            .count();
        let tax_table = self.input_.tax_rates.table_for(num_alive);
        let scale = self.social_security_threshold_scale_;
        let tax_rules = TaxRules {
            tax_levels: &tax_table.tax_levels,
            capital_gains_levels: &tax_table.capital_gains_levels,
            standard_deduction: tax_table.standard_deduction
                + tax_table.additional_deduction_65_or_older * num_65_or_older as f64,
            social_security_base: tax_table.social_security_base * scale,
            social_security_adjusted_base: tax_table.social_security_adjusted_base * scale,
        };
        let surplus = f64::max(income + rmd - expenses, 0.0);
        let (taxes, tax_rate) = tax_rules.taxes_with_withdrawals(
            social_security,
            WithdrawalIncome {
                ordinary: taxable_income + rmd,
                capital_gains: dividends,
            },
            withdrawals,
            surplus,
            &self.accounts_,
        );

        // the withdrawal rate is what the portfolio pays for, net of any
        // distribution put back in the taxable account
        let net_cash_flow = income - expenses - taxes;
        let mut withdrawal_rate = 0.0;
        if net_cash_flow < 0.0 && self.accounts_.balance() + rmd > 0.0 {
            withdrawal_rate = -net_cash_flow * 12.0 / (self.accounts_.balance() + rmd);
        }
        let cash = net_cash_flow + rmd;
        if cash >= 0.0 {
            self.accounts_.deposit(cash);
        } else {
            self.accounts_.withdraw(-cash);
        }

        let annualized_return = self.accounts_.grow(
            allocation,
            us_equity_expected_returns,
            international_equity_expected_returns,
            bonds_expected_returns,
        );
        self.accounts_.adjust_basis_for_inflation(inflation);
        self.adjust_pensions_for_inflation(&date, inflation);
        self.sum_of_returns_ += annualized_return;
        self.simulation_results_.average_return =
            self.sum_of_returns_ / (self.simulation_results_.monthly_snapshot.len() as f64 + 1.0);

        let monthly_balance = MonthlySnapshot {
            date: self.current_date_,
            balance: self.accounts_.balance(),
            expenses,
            income,
            taxes,
            tax_rate,
            withdrawal_rate,
            annualized_return,
        };

        self.simulation_results_
            .monthly_snapshot
            .push(monthly_balance);

        self.current_date_ = self
            .current_date_
            .checked_add_months(chrono::Months::new(1))
            .ok_or_else(|| format!("simulation ran past the last date ({date})"))?;
        self.social_security_threshold_scale_ /= (1.0 + inflation / 100.0).powf(1.0 / 12.0);

        Ok(self.accounts_.balance() == 0.0)
    }
}

pub fn run_simulation(input: &Input) -> Result<SimulationResults, String> {
    let mut simulation = Simulation::new(input)?;

    loop {
        let is_finished = simulation.run_simulation_one_month(
            input.portfolio.forecasts.us_equity_expected_returns,
            input
                .portfolio
                .forecasts
                .international_equity_expected_returns,
            input.portfolio.forecasts.bonds_expected_returns,
            input.tax_rates.expected_inflation,
        )?;

        if is_finished {
            break;
        }
    }

    Ok(simulation.simulation_results_)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026 married filing jointly brackets
    const STANDARD_DEDUCTION: f64 = 32200.0;

    fn brackets() -> Vec<TaxLevel> {
        [
            (0.0, 10.0),
            (24800.0, 12.0),
            (100800.0, 22.0),
            (211400.0, 24.0),
            (403550.0, 32.0),
            (512450.0, 35.0),
            (768700.0, 37.0),
        ]
        .iter()
        .map(|&(income, rate)| TaxLevel { income, rate })
        .collect()
    }

    // returns (annual tax, marginal rate) for an annual gross income with no
    // social security
    fn annual_taxes(annual_income: f64) -> (f64, f64) {
        let brackets = brackets();
        let rules = married_rules(&brackets);
        let (monthly_tax, rate) = rules.taxes(0.0, annual_income / 12.0, 0.0);
        (monthly_tax * 12.0, rate)
    }

    fn married_rules(brackets: &[TaxLevel]) -> TaxRules<'_> {
        TaxRules {
            tax_levels: brackets,
            capital_gains_levels: &[],
            standard_deduction: STANDARD_DEDUCTION,
            social_security_base: 32000.0,
            social_security_adjusted_base: 44000.0,
        }
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 0.5,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn test_taxes_below_standard_deduction() {
        assert_eq!(annual_taxes(0.0), (0.0, 0.0));
        assert_eq!(annual_taxes(STANDARD_DEDUCTION), (0.0, 0.0));
        assert_eq!(annual_taxes(20000.0), (0.0, 0.0));
    }

    #[test]
    fn test_taxes_lowest_bracket() {
        let (tax, rate) = annual_taxes(STANDARD_DEDUCTION + 10000.0);
        assert_close(tax, 1000.0);
        assert_eq!(rate, 10.0);
    }

    #[test]
    fn test_taxes_bracket_boundaries() {
        // taxable income exactly at the top of a bracket stays at that rate
        let (tax, rate) = annual_taxes(STANDARD_DEDUCTION + 24800.0);
        assert_close(tax, 2480.0);
        assert_eq!(rate, 10.0);

        let (tax, rate) = annual_taxes(STANDARD_DEDUCTION + 100800.0);
        assert_close(tax, 2480.0 + 9120.0);
        assert_eq!(rate, 12.0);
    }

    #[test]
    fn test_taxes_middle_brackets() {
        // taxable 67,800: 10% of 24,800 + 12% of 43,000
        let (tax, rate) = annual_taxes(100000.0);
        assert_close(tax, 2480.0 + 5160.0);
        assert_eq!(rate, 12.0);

        // taxable 367,800: 2,480 + 9,120 + 24,332 + 24% of 156,400
        let (tax, rate) = annual_taxes(400000.0);
        assert_close(tax, 2480.0 + 9120.0 + 24332.0 + 37536.0);
        assert_eq!(rate, 24.0);
    }

    #[test]
    fn test_taxes_top_bracket() {
        // taxable 967,800: all brackets filled up to 768,700, then 37%
        let (tax, rate) = annual_taxes(1000000.0);
        let below_top = 2480.0 + 9120.0 + 24332.0 + 46116.0 + 34848.0 + 89687.5;
        assert!((tax - (below_top + (967800.0 - 768700.0) * 0.37)).abs() < 5.0);
        assert_eq!(rate, 37.0);
    }

    // fixed start date for the simulations, so results don't depend on when
    // the tests run
    fn start_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()
    }

    // a retiree who is 60 on the start date, with no social security, pension
    // or other income
    fn test_retiree(retirement_age: u32, salary_annual: f64) -> crate::Retiree {
        crate::Retiree {
            name: "Test".to_string(),
            date_of_birth: start_date()
                .checked_sub_months(chrono::Months::new(60 * 12))
                .unwrap(),
            retirement_age,
            life_expectancy: 64,
            salary_annual,
            retirement_contribution_percent: 10.0,
            social_security_age: 67,
            social_security_amount_full: 0.0,
            pension_age: 0,
            pension_monthly_income: 0.0,
            pension_cola_percent: None,
            other_monthly_retirement_income: 0.0,
        }
    }

    // input with zero returns and yields and no taxes, so balances are easy
    // to predict
    fn test_input(retirees: Vec<crate::Retiree>, monthly_expenses: f64) -> Input {
        let all_bonds = crate::portfolio::Allocation {
            us_equities: 0.0,
            international: 0.0,
            bonds: 100.0,
        };
        Input {
            retirees,
            portfolio: crate::portfolio::Portfolio {
                pre_tax_balance: 1_000_000.0,
                roth_balance: 0.0,
                taxable_balance: 0.0,
                taxable_cost_basis: None,
                pre_retirement_allocation: all_bonds,
                post_retirement_allocation: all_bonds,
                forecasts: crate::portfolio::Forecasts {
                    us_equity_expected_returns: 0.0,
                    international_equity_expected_returns: 0.0,
                    bonds_expected_returns: 0.0,
                    us_equity_dividend_yield: 0.0,
                    international_equity_dividend_yield: 0.0,
                    bonds_interest_yield: 0.0,
                    ..Default::default()
                },
            },
            expenses: crate::Expenses {
                monthly: monthly_expenses,
                survivor_percent: 100.0,
            },
            tax_rates: crate::TaxRates {
                expected_inflation: 0.0,
                married_filing_jointly: flat_tax_table(1.0e9, 0.0),
                single: flat_tax_table(1.0e9, 0.0),
            },
            start_date: start_date(),
        }
    }

    #[test]
    fn test_no_retirees_is_an_error() {
        let input = test_input(vec![], 0.0);
        assert!(run_simulation(&input).is_err());
    }

    #[test]
    fn test_simulation_starts_on_start_date() {
        let input = test_input(vec![test_retiree(61, 0.0)], 0.0);
        let results = run_simulation(&input).unwrap();
        let dates: Vec<NaiveDate> = results.monthly_snapshot.iter().map(|s| s.date).collect();
        assert_eq!(dates[0], start_date());
        assert_eq!(dates[1], NaiveDate::from_ymd_opt(2026, 2, 15).unwrap());
        // runs until the retiree's 65th birthday (life expectancy 64)
        assert_eq!(
            *dates.last().unwrap(),
            NaiveDate::from_ymd_opt(2030, 12, 15).unwrap()
        );
    }

    fn flat_tax_table(standard_deduction: f64, rate: f64) -> crate::TaxTable {
        crate::TaxTable {
            standard_deduction,
            additional_deduction_65_or_older: 0.0,
            // social security is never taxed
            social_security_base: 1.0e9,
            social_security_adjusted_base: 1.0e9,
            tax_levels: vec![TaxLevel { income: 0.0, rate }],
            capital_gains_levels: vec![TaxLevel {
                income: 0.0,
                rate: 0.0,
            }],
        }
    }

    // flat tax with no standard deduction, the same for every filing status
    fn flat_tax(input: &mut Input, rate: f64) {
        input.tax_rates.married_filing_jointly = flat_tax_table(0.0, rate);
        input.tax_rates.single = flat_tax_table(0.0, rate);
    }

    #[test]
    fn test_single_retiree_uses_single_rates() {
        let mut retiree = test_retiree(60, 0.0);
        retiree.pension_monthly_income = 1000.0;
        let mut input = test_input(vec![retiree], 0.0);
        input.tax_rates.married_filing_jointly = flat_tax_table(0.0, 10.0);
        input.tax_rates.single = flat_tax_table(0.0, 20.0);
        let results = run_simulation(&input).unwrap();
        let first = &results.monthly_snapshot[0];
        assert_eq!(first.tax_rate, 20.0);
        assert!((first.taxes - 200.0).abs() < 0.01, "{}", first.taxes);
    }

    #[test]
    fn test_survivor_switches_to_single_rates() {
        let mut input = survivor_input(63);
        input.tax_rates.married_filing_jointly = flat_tax_table(0.0, 10.0);
        input.tax_rates.single = flat_tax_table(0.0, 20.0);
        let results = run_simulation(&input).unwrap();
        let death_date = results.retirees[0].death_date;
        assert_eq!(results.monthly_snapshot[0].tax_rate, 10.0);
        let survivor = results
            .monthly_snapshot
            .iter()
            .find(|s| s.date >= death_date)
            .unwrap();
        assert_eq!(survivor.tax_rate, 20.0);
    }

    #[test]
    fn test_pre_retirement_pension_is_saved() {
        let mut retiree = test_retiree(63, 0.0);
        retiree.pension_monthly_income = 1000.0;
        let input = test_input(vec![retiree], 0.0);
        let results = run_simulation(&input).unwrap();

        let pre_retirement: Vec<_> = results
            .monthly_snapshot
            .iter()
            .take_while(|s| s.date < results.retirement_date)
            .collect();
        assert!(pre_retirement.len() > 12);
        for (i, snapshot) in pre_retirement.iter().enumerate() {
            assert_eq!(snapshot.balance, 1_000_000.0 + 1000.0 * (i + 1) as f64);
        }
    }

    #[test]
    fn test_taxes_paid_from_surplus_income() {
        // pension covers expenses and taxes, so nothing is withdrawn and the
        // tax isn't grossed up
        let mut retiree = test_retiree(60, 0.0);
        retiree.pension_monthly_income = 5000.0;
        let mut input = test_input(vec![retiree], 1000.0);
        flat_tax(&mut input, 10.0);
        let results = run_simulation(&input).unwrap();
        let first = &results.monthly_snapshot[0];
        assert!((first.taxes - 500.0).abs() < 0.01, "{}", first.taxes);
        assert_eq!(first.withdrawal_rate, 0.0);
        assert!((first.balance - (1_000_000.0 + 3500.0)).abs() < 0.5);
    }

    #[test]
    fn test_taxes_on_withdrawals_are_grossed_up() {
        // $1000 withdrawn for expenses, tax on $2000 is $200, and the $200
        // withdrawn to pay it is taxed again: 200 / 0.9 = 222.22
        let mut retiree = test_retiree(60, 0.0);
        retiree.pension_monthly_income = 1000.0;
        let mut input = test_input(vec![retiree], 2000.0);
        flat_tax(&mut input, 10.0);
        let results = run_simulation(&input).unwrap();
        let first = &results.monthly_snapshot[0];
        assert!((first.taxes - 222.22).abs() < 0.01, "{}", first.taxes);
        assert!((first.balance - (1_000_000.0 - 1222.22)).abs() < 0.5);
    }

    #[test]
    fn test_roth_withdrawals_are_not_taxed() {
        // $1000 withdrawn for expenses comes from Roth, so only the pension is
        // taxed, and the $100 tax is withdrawn from Roth tax free
        let mut retiree = test_retiree(60, 0.0);
        retiree.pension_monthly_income = 1000.0;
        let mut input = test_input(vec![retiree], 2000.0);
        input.portfolio.pre_tax_balance = 0.0;
        input.portfolio.roth_balance = 1_000_000.0;
        flat_tax(&mut input, 10.0);
        let results = run_simulation(&input).unwrap();
        let first = &results.monthly_snapshot[0];
        assert!((first.taxes - 100.0).abs() < 0.01, "{}", first.taxes);
        assert!((first.balance - (1_000_000.0 - 1100.0)).abs() < 0.01);
    }

    #[test]
    fn test_only_pre_tax_part_of_withdrawal_is_taxed() {
        // $1000 for expenses plus taxes, but only $500 is left in pre-tax:
        // tax is 10% of the $1000 pension and the $500, and the rest comes
        // from Roth
        let mut retiree = test_retiree(60, 0.0);
        retiree.pension_monthly_income = 1000.0;
        let mut input = test_input(vec![retiree], 2000.0);
        input.portfolio.pre_tax_balance = 500.0;
        input.portfolio.roth_balance = 1_000_000.0;
        flat_tax(&mut input, 10.0);
        let results = run_simulation(&input).unwrap();
        let first = &results.monthly_snapshot[0];
        assert!((first.taxes - 150.0).abs() < 0.01, "{}", first.taxes);
        assert!((first.balance - (1_000_500.0 - 1150.0)).abs() < 0.01);
    }

    // two retirees, both 60 and retired, both claiming social security at
    // 62 (70% of the full amount). The first has a pension and the larger
    // benefit (2000 at full retirement age), and dies first. The second's
    // full amount is half the first's, so there's no spousal benefit.
    fn survivor_input(first_life_expectancy: u32) -> Input {
        let mut first = test_retiree(60, 0.0);
        first.life_expectancy = first_life_expectancy;
        first.pension_monthly_income = 500.0;
        first.social_security_age = 62;
        first.social_security_amount_full = 2000.0;
        let mut second = test_retiree(60, 0.0);
        second.life_expectancy = 66;
        second.social_security_age = 62;
        second.social_security_amount_full = 1000.0;
        let mut input = test_input(vec![first, second], 5000.0);
        input.expenses.survivor_percent = 50.0;
        input
    }

    #[test]
    fn test_survivor_gets_larger_social_security_benefit() {
        let input = survivor_input(63);
        let results = run_simulation(&input).unwrap();
        let first = &results.retirees[0];

        let both_collecting = results
            .monthly_snapshot
            .iter()
            .find(|s| s.date >= first.social_security_date)
            .unwrap();
        assert_close(both_collecting.income, 500.0 + 1400.0 + 700.0);
        assert_eq!(both_collecting.expenses, 5000.0);

        // pension stops, survivor switches to the survivor benefit, expenses
        // drop. The first claimed early, so the survivor gets 82.5% of their
        // full amount.
        let survivor = results
            .monthly_snapshot
            .iter()
            .find(|s| s.date >= first.death_date)
            .unwrap();
        assert_close(survivor.income, 1650.0);
        assert_eq!(survivor.expenses, 2500.0);

        // simulation runs until the survivor dies
        let last = results.monthly_snapshot.last().unwrap();
        assert!(last.date < results.retirees[1].death_date);
        assert!(last.date >= first.death_date);
    }

    #[test]
    fn test_survivor_benefit_if_deceased_never_collected() {
        // first retiree dies at 62, before collecting social security. The
        // survivor still gets a survivor benefit from their full amount, reduced
        // for starting at 62 (60 months before survivor FRA).
        let input = survivor_input(61);
        let results = run_simulation(&input).unwrap();
        let survivor = results
            .monthly_snapshot
            .iter()
            .find(|s| s.date >= results.retirees[1].social_security_date)
            .unwrap();
        assert_close(survivor.income, 2000.0 * (1.0 - 0.285 * 60.0 / 84.0));
    }

    #[test]
    fn test_earnings_test_withholds_social_security() {
        // claims at 62 (January 2028) while earning $120,000 until 64: all
        // withheld until retiring, then 70% of PIA, then from FRA at 67
        // recalculated for the 24 months withheld (80%)
        let mut retiree = test_retiree(64, 120000.0);
        retiree.life_expectancy = 70;
        retiree.social_security_age = 62;
        retiree.social_security_amount_full = 2000.0;
        let input = test_input(vec![retiree], 0.0);
        let results = run_simulation(&input).unwrap();
        let income_on = |year, month| {
            let date = NaiveDate::from_ymd_opt(year, month, 15).unwrap();
            let snapshot = results.monthly_snapshot.iter().find(|s| s.date == date);
            snapshot.unwrap().income
        };
        assert_eq!(income_on(2029, 6), 0.0);
        assert_close(income_on(2030, 6), 1400.0);
        assert_close(income_on(2033, 6), 1600.0);
    }

    // 2026 married filing jointly capital gains rates
    fn capital_gains_brackets() -> Vec<TaxLevel> {
        [(0.0, 0.0), (98900.0, 15.0), (613700.0, 20.0)]
            .iter()
            .map(|&(income, rate)| TaxLevel { income, rate })
            .collect()
    }

    // returns the annual tax for annual ordinary income and capital gains,
    // with no social security
    fn annual_taxes_with_gains(ordinary: f64, gains: f64) -> f64 {
        let brackets = brackets();
        let gains_brackets = capital_gains_brackets();
        let rules = TaxRules {
            capital_gains_levels: &gains_brackets,
            ..married_rules(&brackets)
        };
        rules.taxes(0.0, ordinary / 12.0, gains / 12.0).0 * 12.0
    }

    #[test]
    fn test_capital_gains_taxes() {
        // ordinary income uses up the standard deduction; the 50,000 of
        // gains is all under the 98,900 top of the 0% bracket
        assert_close(annual_taxes_with_gains(STANDARD_DEDUCTION, 50000.0), 0.0);
        // the standard deduction comes off ordinary income first: taxable
        // income of 27,800 is all gains
        assert_close(annual_taxes_with_gains(10000.0, 50000.0), 0.0);

        // gains are stacked on top of 67,800 of taxable ordinary income
        // (2,480 + 12% of 43,000): 31,100 at 0%, then 18,900 at 15%
        assert_close(
            annual_taxes_with_gains(100000.0, 50000.0),
            2480.0 + 5160.0 + 2835.0,
        );

        // all three capital gains rates
        assert_close(
            annual_taxes_with_gains(0.0, 1_000_000.0),
            514800.0 * 0.15 + 354100.0 * 0.2,
        );
    }

    // single retiree, retired, with $100,000 in a taxable account that is
    // half gain, and $1000/month of expenses. 10% ordinary tax and 15%
    // capital gains tax, with no deductions.
    fn taxable_account_input() -> Input {
        let mut input = test_input(vec![test_retiree(60, 0.0)], 1000.0);
        input.portfolio.pre_tax_balance = 0.0;
        input.portfolio.taxable_balance = 100_000.0;
        input.portfolio.taxable_cost_basis = Some(50_000.0);
        flat_tax(&mut input, 10.0);
        input.tax_rates.single.capital_gains_levels = vec![TaxLevel {
            income: 0.0,
            rate: 15.0,
        }];
        input
    }

    #[test]
    fn test_only_gains_are_taxed_on_taxable_withdrawals() {
        // withdraw w to cover 1000 plus the tax on half of w: w = 1000 +
        // 0.15 * w / 2
        let results = run_simulation(&taxable_account_input()).unwrap();
        let first = &results.monthly_snapshot[0];
        let withdrawal = 1000.0 / (1.0 - 0.075);
        assert!(
            (first.taxes - (withdrawal - 1000.0)).abs() < 0.01,
            "{}",
            first.taxes
        );
        assert!((first.balance - (100_000.0 - withdrawal)).abs() < 0.01);
    }

    #[test]
    fn test_taxable_account_is_withdrawn_before_pre_tax() {
        let mut input = taxable_account_input();
        input.portfolio.pre_tax_balance = 100_000.0;
        let results = run_simulation(&input).unwrap();
        // same as with no pre-tax balance: only capital gains tax
        let first = &results.monthly_snapshot[0];
        let withdrawal = 1000.0 / (1.0 - 0.075);
        assert!((first.taxes - (withdrawal - 1000.0)).abs() < 0.01);
    }

    #[test]
    fn test_interest_and_dividends_are_taxed() {
        // $120,000 at a 2% bond interest yield and 1% dividend yield: $200
        // of interest a month (10%) and $100 of dividends (15%), with no
        // expenses. No gain in the account, so withdrawing the tax isn't
        // taxed.
        let mut input = taxable_account_input();
        input.expenses.monthly = 0.0;
        input.portfolio.taxable_cost_basis = None;
        input.portfolio.taxable_balance = 120_000.0;
        input.portfolio.pre_retirement_allocation.bonds = 50.0;
        input.portfolio.pre_retirement_allocation.us_equities = 50.0;
        input.portfolio.post_retirement_allocation = input.portfolio.pre_retirement_allocation;
        input.portfolio.forecasts.bonds_interest_yield = 4.0;
        input.portfolio.forecasts.us_equity_dividend_yield = 2.0;
        let results = run_simulation(&input).unwrap();
        let first = &results.monthly_snapshot[0];
        assert!((first.taxes - 35.0).abs() < 0.01, "{}", first.taxes);
        // reinvested, and taxes are paid from the account
        assert!((first.balance - (120_000.0 - 35.0)).abs() < 0.01);
    }

    #[test]
    fn test_surplus_income_goes_to_taxable_account() {
        // a $3000/month pension, $1000 of expenses, no tax: $2000 a month is
        // saved in the taxable account, at cost
        let mut retiree = test_retiree(60, 0.0);
        retiree.pension_monthly_income = 3000.0;
        let input = test_input(vec![retiree], 1000.0);
        let mut simulation = Simulation::new(&input).unwrap();
        simulation
            .run_simulation_one_month(0.0, 0.0, 0.0, 0.0)
            .unwrap();
        assert_eq!(simulation.accounts_.taxable, 2000.0);
        assert_eq!(simulation.accounts_.taxable_basis, 2000.0);
        assert_eq!(simulation.accounts_.pre_tax, 1_000_000.0);
    }

    // a $1000/month pension from 60 (the start), with no expenses or tax
    fn pension_input(cola: Option<f64>, pension_age: u32) -> Input {
        let mut retiree = test_retiree(60, 0.0);
        retiree.pension_age = pension_age;
        retiree.pension_monthly_income = 1000.0;
        retiree.pension_cola_percent = cola;
        test_input(vec![retiree], 0.0)
    }

    // the monthly pension (all of the income) after each month, with the
    // given annual inflation
    fn pension_by_month(input: &Input, inflation: f64, months: usize) -> Vec<f64> {
        let mut simulation = Simulation::new(input).unwrap();
        for _ in 0..months {
            simulation
                .run_simulation_one_month(0.0, 0.0, 0.0, inflation)
                .unwrap();
        }
        let snapshots = &simulation.simulation_results_.monthly_snapshot;
        snapshots.iter().map(|s| s.income).collect()
    }

    #[test]
    fn test_pension_keeps_up_with_inflation_by_default() {
        let pensions = pension_by_month(&pension_input(None, 60), 12.0, 13);
        assert!(pensions.iter().all(|&p| p == 1000.0), "{pensions:?}");
    }

    #[test]
    fn test_fixed_pension_shrinks_with_inflation() {
        // after a year of 12% inflation, $1000 is worth $1000 / 1.12
        let pensions = pension_by_month(&pension_input(Some(0.0), 60), 12.0, 13);
        assert_eq!(pensions[0], 1000.0);
        assert!(
            (pensions[12] - 1000.0 / 1.12).abs() < 0.01,
            "{}",
            pensions[12]
        );
    }

    #[test]
    fn test_pension_cola_offsets_inflation() {
        // a 2% increase with 5% inflation
        let pensions = pension_by_month(&pension_input(Some(2.0), 60), 5.0, 13);
        assert!(
            (pensions[12] - 1000.0 * 1.02 / 1.05).abs() < 0.01,
            "{}",
            pensions[12]
        );
        // and fully, when they're the same
        let pensions = pension_by_month(&pension_input(Some(5.0), 60), 5.0, 13);
        assert!((pensions[12] - 1000.0).abs() < 0.01, "{}", pensions[12]);
    }

    #[test]
    fn test_pension_shrinks_before_it_starts() {
        // starts at 61, a year from now. Inflation shrinks it until then,
        // and the 5% increases only start with it.
        let pensions = pension_by_month(&pension_input(Some(5.0), 61), 5.0, 25);
        assert_eq!(pensions[11], 0.0);
        assert!(
            (pensions[12] - 1000.0 / 1.05).abs() < 0.01,
            "{}",
            pensions[12]
        );
        assert!(
            (pensions[24] - 1000.0 / 1.05).abs() < 0.01,
            "{}",
            pensions[24]
        );
    }

    // a retired retiree with the given birth date, who lives to 90
    fn rmd_retiree(year: i32, month: u32) -> crate::Retiree {
        let mut retiree = test_retiree(60, 0.0);
        retiree.date_of_birth = NaiveDate::from_ymd_opt(year, month, 15).unwrap();
        retiree.life_expectancy = 90;
        retiree
    }

    fn run_months(simulation: &mut Simulation, months: usize) {
        for _ in 0..months {
            simulation
                .run_simulation_one_month(0.0, 0.0, 0.0, 0.0)
                .unwrap();
        }
    }

    #[test]
    fn test_rmd_moves_pre_tax_to_taxable() {
        // 74 in 2026: the distribution period is 25.5. With no expenses or
        // taxes, it all goes to the taxable account.
        let input = test_input(vec![rmd_retiree(1952, 6)], 0.0);
        let mut simulation = Simulation::new(&input).unwrap();
        run_months(&mut simulation, 12);
        let rmd = 1_000_000.0 / 25.5;
        assert!((simulation.accounts_.pre_tax - (1_000_000.0 - rmd)).abs() < 0.01);
        assert!((simulation.accounts_.taxable - rmd).abs() < 0.01);
        assert!((simulation.accounts_.taxable_basis - rmd).abs() < 0.01);

        // next year's is from the balance at the end of 2026, at 75
        run_months(&mut simulation, 1);
        let monthly = (1_000_000.0 - rmd) / 24.6 / 12.0;
        assert!((simulation.accounts_.taxable - rmd - monthly).abs() < 0.01);
    }

    #[test]
    fn test_rmd_starts_at_75_for_born_1960() {
        // 74 in 2034, 75 in 2035
        let input = test_input(vec![rmd_retiree(1960, 3)], 0.0);
        let mut simulation = Simulation::new(&input).unwrap();
        run_months(&mut simulation, 12 * 9);
        assert_eq!(simulation.accounts_.pre_tax, 1_000_000.0);
        run_months(&mut simulation, 1);
        let monthly = 1_000_000.0 / 24.6 / 12.0;
        assert!((simulation.accounts_.pre_tax - (1_000_000.0 - monthly)).abs() < 0.01);
    }

    #[test]
    fn test_rmd_is_taxed_and_spent_first() {
        // a 10% tax on the distribution, which pays the $1000 of expenses
        // before anything is sold in the taxable account. The rest is saved.
        let mut input = test_input(vec![rmd_retiree(1952, 6)], 1000.0);
        flat_tax(&mut input, 10.0);
        input.portfolio.taxable_balance = 100_000.0;
        input.portfolio.taxable_cost_basis = Some(50_000.0);
        let mut simulation = Simulation::new(&input).unwrap();
        run_months(&mut simulation, 1);
        let monthly = 1_000_000.0 / 25.5 / 12.0;
        let first = &simulation.simulation_results_.monthly_snapshot[0];
        assert!(
            (first.taxes - monthly * 0.1).abs() < 0.01,
            "{}",
            first.taxes
        );
        let saved = monthly * 0.9 - 1000.0;
        assert!((simulation.accounts_.taxable - (100_000.0 + saved)).abs() < 0.01);
        assert!((simulation.accounts_.taxable_basis - (50_000.0 + saved)).abs() < 0.01);
        // the withdrawal rate is the net spending from the portfolio: the
        // expenses and tax, not the whole distribution
        let rate = (1000.0 + monthly * 0.1) * 12.0 / 1_100_000.0;
        assert!((first.withdrawal_rate - rate).abs() < 1e-9);
    }

    #[test]
    fn test_rmd_uses_oldest_living_retiree() {
        // the older retiree (74 in 2026) dies in mid 2027. In 2028 the
        // survivor is 72, too young for distributions.
        let mut older = rmd_retiree(1952, 6);
        older.life_expectancy = 74;
        let input = test_input(vec![older, rmd_retiree(1956, 6)], 0.0);
        let mut simulation = Simulation::new(&input).unwrap();
        run_months(&mut simulation, 24);
        let pre_tax = simulation.accounts_.pre_tax;
        assert!(pre_tax < 1_000_000.0 - 1_000_000.0 / 25.5);
        run_months(&mut simulation, 12);
        assert_eq!(simulation.accounts_.pre_tax, pre_tax);
        // and 73 in 2029
        run_months(&mut simulation, 1);
        let monthly = pre_tax / 26.5 / 12.0;
        assert!((simulation.accounts_.pre_tax - (pre_tax - monthly)).abs() < 0.01);
    }

    #[test]
    fn test_inflation_makes_basis_taxable_gain() {
        // no gain to start, but after a month of 12% inflation the basis is
        // worth less in today's dollars
        let mut input = taxable_account_input();
        input.portfolio.taxable_cost_basis = None;
        let mut simulation = Simulation::new(&input).unwrap();
        simulation
            .run_simulation_one_month(0.0, 0.0, 0.0, 12.0)
            .unwrap();
        assert_eq!(
            simulation.simulation_results_.monthly_snapshot[0].taxes,
            0.0
        );
        let basis = simulation.accounts_.taxable_basis;
        let balance = simulation.accounts_.taxable;
        assert!((basis - balance / 1.12f64.powf(1.0 / 12.0)).abs() < 0.01);
        simulation
            .run_simulation_one_month(0.0, 0.0, 0.0, 12.0)
            .unwrap();
        assert!(simulation.simulation_results_.monthly_snapshot[1].taxes > 0.0);
    }

    #[test]
    fn test_household_retires_with_first_retiree() {
        // list the later retiree first to make sure order doesn't matter
        let input = test_input(
            vec![test_retiree(63, 24000.0), test_retiree(61, 12000.0)],
            0.0,
        );
        let results = run_simulation(&input).unwrap();
        assert_eq!(results.retirement_date, results.retirees[1].retirement_date);
    }

    #[test]
    fn test_each_retiree_contributes_until_own_retirement() {
        // first retiree contributes $100/month until 61, second $200/month
        // until 63. Expenses equal the second retiree's take-home pay
        // ($2000 - $200), so nothing is withdrawn until they also retire.
        let input = test_input(
            vec![test_retiree(61, 12000.0), test_retiree(63, 24000.0)],
            1800.0,
        );
        let results = run_simulation(&input).unwrap();
        let first_retirement = results.retirees[0].retirement_date;
        let second_retirement = results.retirees[1].retirement_date;

        let mut expected = 1_000_000.0;
        for snapshot in results.monthly_snapshot.iter() {
            if snapshot.date < first_retirement {
                expected += 100.0;
            }
            if snapshot.date < second_retirement {
                expected += 200.0;
            } else {
                expected -= 1800.0;
            }
            assert!(
                (snapshot.balance - expected).abs() < 1.0,
                "on {}: expected {expected}, got {}",
                snapshot.date,
                snapshot.balance
            );
        }

        // while only the first retiree is retired, wages are counted as income
        let gap = results
            .monthly_snapshot
            .iter()
            .find(|s| s.date >= first_retirement && s.date < second_retirement)
            .unwrap();
        assert_eq!(gap.income, 1800.0);
        assert_eq!(gap.withdrawal_rate, 0.0);
    }

    #[test]
    fn test_taxes_are_monotonic() {
        // more income should never mean less tax
        let mut previous = 0.0;
        for income in (0..2_000_000).step_by(5000) {
            let (tax, _) = annual_taxes(income as f64);
            assert!(tax >= previous, "tax dropped at income {income}");
            previous = tax;
        }
    }

    #[test]
    fn test_social_security_untaxed_below_base() {
        // provisional income 20,000 + 24,000 / 2 = 32,000
        assert_eq!(
            taxable_social_security(24000.0, 20000.0, 32000.0, 44000.0),
            0.0
        );
    }

    #[test]
    fn test_social_security_half_taxed_between_bases() {
        // single: provisional income 20,000 + 10,000 = 30,000, half of the
        // 5,000 over the base is taxable
        assert_close(
            taxable_social_security(20000.0, 20000.0, 25000.0, 34000.0),
            2500.0,
        );
        // limited to half of the benefits
        assert_close(
            taxable_social_security(4000.0, 31000.0, 25000.0, 34000.0),
            2000.0,
        );
    }

    #[test]
    fn test_social_security_85_percent_above_adjusted_base() {
        // married: provisional income 40,000 + 15,000 = 55,000.
        // 85% of 11,000 over the adjusted base, plus half of the 12,000
        // between the bases
        assert_close(
            taxable_social_security(30000.0, 40000.0, 32000.0, 44000.0),
            9350.0 + 6000.0,
        );
        // limited to 85% of the benefits
        assert_close(
            taxable_social_security(40000.0, 200000.0, 32000.0, 44000.0),
            34000.0,
        );
    }

    #[test]
    fn test_taxes_include_taxable_social_security() {
        // 30,000 of social security and 40,000 of other income: 15,350 of
        // social security is taxable, so taxable income is 55,350 - 32,200
        let brackets = brackets();
        let rules = married_rules(&brackets);
        let (tax, rate) = rules.taxes(30000.0 / 12.0, 40000.0 / 12.0, 0.0);
        assert_close(tax * 12.0, 2315.0);
        assert_eq!(rate, 10.0);
    }

    #[test]
    fn test_withdrawals_cover_taxes_with_social_security() {
        // in the range where each extra dollar makes 85 cents of social
        // security taxable, the extra withdrawal must still exactly pay the
        // taxes that the surplus doesn't cover
        let brackets = brackets();
        let rules = married_rules(&brackets);
        let social_security = 3000.0;
        let other_income = 4000.0;
        let surplus = 100.0;
        let (taxes, _) = rules.taxes_with_withdrawals(
            social_security,
            WithdrawalIncome {
                ordinary: other_income,
                capital_gains: 0.0,
            },
            0.0,
            surplus,
            &Accounts {
                pre_tax: f64::MAX,
                ..Default::default()
            },
        );
        let extra = taxes - surplus;
        assert!(extra > 0.0);
        let (expected, _) = rules.taxes(social_security, other_income + extra, 0.0);
        assert!((taxes - expected).abs() < 0.01, "{taxes} vs {expected}");
        // more than the 12% bracket alone would add
        let (base_taxes, _) = rules.taxes(social_security, other_income, 0.0);
        assert!(taxes - base_taxes > (base_taxes - surplus) * 12.0 / 88.0 + 1.0);
    }

    #[test]
    fn test_additional_deduction_at_65() {
        // $1000/month pension, 10% flat tax, $6000 standard deduction plus
        // $2400 at 65: $50/month before, $30/month after
        let mut retiree = test_retiree(60, 0.0);
        retiree.life_expectancy = 66;
        retiree.pension_monthly_income = 1000.0;
        let mut input = test_input(vec![retiree], 0.0);
        input.tax_rates.single = flat_tax_table(6000.0, 10.0);
        input.tax_rates.single.additional_deduction_65_or_older = 2400.0;
        let results = run_simulation(&input).unwrap();
        let birthday_65 = add_years(&input.retirees[0].date_of_birth, 65);
        for snapshot in results.monthly_snapshot.iter() {
            let expected = if snapshot.date >= birthday_65 {
                30.0
            } else {
                50.0
            };
            assert!(
                (snapshot.taxes - expected).abs() < 0.01,
                "on {}: {}",
                snapshot.date,
                snapshot.taxes
            );
        }
    }

    // a single retiree collecting $4000/month of social security from 62
    // (70% of the full amount), with provisional income of 24,000 just under the
    // 25,000 base
    fn social_security_only_input(inflation: f64) -> Input {
        let mut retiree = test_retiree(60, 0.0);
        retiree.social_security_age = 62;
        retiree.social_security_amount_full = 4000.0 / 0.7;
        let mut input = test_input(vec![retiree], 0.0);
        input.tax_rates.expected_inflation = inflation;
        input.tax_rates.single = flat_tax_table(0.0, 10.0);
        input.tax_rates.single.social_security_base = 25000.0;
        input.tax_rates.single.social_security_adjusted_base = 34000.0;
        input
    }

    #[test]
    fn test_social_security_thresholds_shrink_with_inflation() {
        let input = social_security_only_input(0.0);
        let results = run_simulation(&input).unwrap();
        assert!(results.monthly_snapshot.iter().all(|s| s.taxes == 0.0));

        // with 10% inflation the base is below 24,000 in today's dollars
        // within a year, so some social security becomes taxable
        let input = social_security_only_input(10.0);
        let results = run_simulation(&input).unwrap();
        let last = results.monthly_snapshot.last().unwrap();
        assert!(last.taxes > 0.0);
        // both thresholds shrink: after almost 5 years of 10% inflation,
        // provisional income is over the adjusted base
        let months = results.monthly_snapshot.len() - 1;
        let scale = 1.1f64.powf(-(months as f64) / 12.0);
        let (base, adjusted_base) = (25000.0 * scale, 34000.0 * scale);
        assert!(adjusted_base < 24000.0);
        let taxable = taxable_social_security(48000.0, 0.0, base, adjusted_base);
        assert!(
            (last.taxes * 12.0 - taxable * 0.1).abs() < 1.0,
            "{}",
            last.taxes
        );
    }
}
