/**************************************************************************
* simulate.rs
*
* Performs the simulation of a retirement scenario
**************************************************************************/

use crate::portfolio::Portfolio;
use crate::utils::*;
use crate::{Input, TaxLevel};
use chrono::NaiveDate;

// stores results of each month of the simulation
#[derive(Debug)]
pub struct MonthlySnapshot {
    pub date: NaiveDate,
    pub balance: f32,
    pub expenses: f32,
    pub income: f32,
    pub tax_rate: f32,
    pub taxes: f32,
    pub withdrawal_rate: f32,
    pub annualized_return: f32,
}

// values collected for each retiree during simulation to make
// reporting easier
#[derive(Debug)]
pub struct RetireeInfo {
    pub name: String,
    pub retirement_date: NaiveDate,
    // first day after the retiree's life expectancy is reached
    pub death_date: NaiveDate,
    pub social_security_date: NaiveDate,
    pub date_of_birth: NaiveDate,
    social_security_income: f32,
}

#[derive(Debug, Default)]
pub struct SimulationResults {
    pub retirement_date: NaiveDate,
    pub retirees: Vec<RetireeInfo>,
    pub monthly_snapshot: Vec<MonthlySnapshot>,
    pub average_return: f32,
}

// returns the monthly tax and the marginal tax rate (percentage) for the given
// monthly income. Tax levels must be sorted by income, with each level's income
// being the lower bound of its bracket.
fn get_taxes(monthly_income: f32, standard_deduction: f32, tax_rates: &[TaxLevel]) -> (f32, f32) {
    let taxable_income = (monthly_income * 12.0 - standard_deduction).max(0.0);
    let mut annual_tax: f32 = 0.0;
    let mut marginal_rate: f32 = 0.0;

    for (i, level) in tax_rates.iter().enumerate() {
        if taxable_income <= level.income {
            break;
        }
        let upper = tax_rates.get(i + 1).map_or(f32::MAX, |next| next.income);
        annual_tax += (taxable_income.min(upper) - level.income) * level.rate / 100.0;
        marginal_rate = level.rate;
    }

    (annual_tax / 12.0, marginal_rate)
}

// this is an estimate. The IRS has a big table for retirement income based on
// age and retirement date. This routine uses the values from the last row of
// the table, for younger retirees. The user will enter their personal values
// from the IRS web site and this routine will interpolate the rest. In the future
// the whole table should be entered.
fn get_social_security_monthly_income(
    retirement_age: u32,
    benefit_early: f32,
    benefit_full: f32,
    benefit_delayed: f32,
) -> f32 {
    let min_age = 62;
    let normal_age = 67;
    let max_age = 70;

    if retirement_age >= max_age {
        benefit_delayed
    } else if retirement_age < min_age {
        0.0
    } else if retirement_age >= normal_age {
        benefit_full
            + (benefit_delayed - benefit_full) * (retirement_age - normal_age) as f32
                / (max_age - normal_age) as f32
    } else {
        benefit_early
            + (benefit_full - benefit_early) * (retirement_age - min_age) as f32
                / (normal_age - min_age) as f32
    }
}

// represents a simulation run
pub struct Simulation<'a> {
    pub simulation_results_: SimulationResults,

    input_: &'a Input,
    current_date_: NaiveDate,
    portfolio_: Portfolio,
    expenses_: f32,
    sum_of_returns_: f32,
}

impl<'a> Simulation<'a> {
    pub fn new(input: &'a Input) -> Self {
        // the household retires (expenses start being paid from the portfolio)
        // when the first retiree retires
        let first_retiree = input
            .retirees
            .iter()
            .min_by_key(|r| add_years(&r.date_of_birth, r.retirement_age))
            .expect("at least one retiree");
        let retirement_date = add_years(&first_retiree.date_of_birth, first_retiree.retirement_age);
        let current_date: NaiveDate = chrono::Utc::now().naive_utc().date();

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
                death_date: add_years(&retiree.date_of_birth, retiree.life_expectency + 1),
                social_security_date: add_years(
                    &retiree.date_of_birth,
                    retiree.social_security_age,
                ),
                date_of_birth: retiree.date_of_birth,
                social_security_income: get_social_security_monthly_income(
                    retiree.social_security_age,
                    retiree.social_security_amount_early,
                    retiree.social_security_amount_full,
                    retiree.social_security_amount_delayed,
                ),
            };
            simulation_results.retirees.push(retiree_result);
        }

        let portfolio = input.portfolio;
        let expenses = input.expenses.monthly;

        Self {
            simulation_results_: simulation_results,
            input_: input,
            current_date_: current_date,
            portfolio_: portfolio,
            expenses_: expenses,
            sum_of_returns_: 0.0,
        }
    }

    // returns true if simulation finished
    pub fn run_simulation_one_month(
        &mut self,
        us_equity_expected_returns: f32,
        international_equity_expected_returns: f32,
        bonds_expected_returns: f32,
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
                self.portfolio_.deposit(contribution);
                if household_retired {
                    wages += retiree.salary_annual / 12.0 - contribution;
                }
            }
        }

        // social security: before or after retirement. After a retiree dies,
        // the survivor receives the larger of their own benefit and the
        // deceased's benefit (if the deceased had started collecting).
        let survivor_benefit = infos
            .iter()
            .zip(alive.iter())
            .filter(|(info, &is_alive)| !is_alive && info.social_security_date < info.death_date)
            .map(|(info, _)| info.social_security_income)
            .fold(0.0, f32::max);
        let mut income = 0.0;
        for (i, info) in infos.iter().enumerate() {
            if alive[i] {
                let own_benefit = if date > info.social_security_date {
                    info.social_security_income
                } else {
                    0.0
                };
                income += f32::max(own_benefit, survivor_benefit);
            }
        }

        // social security is usually 85% taxable (ignore lower incomes)
        let mut taxable_income = income * 0.85;

        income += wages;
        taxable_income += wages;

        // pension income, before or after retirement, until the retiree dies
        for (i, retiree) in self.input_.retirees.iter().enumerate() {
            let pension_date = add_years(&retiree.date_of_birth, retiree.pension_age);
            if alive[i] && date >= pension_date {
                income += retiree.pension_monthly_income;
                taxable_income += retiree.pension_monthly_income;
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
        let withdrawals = f32::max(expenses - income, 0.0);

        // tax on income and withdrawals. tax rate on ss will be higher, but ignore that for now
        let num_alive = alive.iter().filter(|&&a| a).count();
        let tax_table = self.input_.tax_rates.table_for(num_alive);
        let (base_taxes, tax_rate) = get_taxes(
            withdrawals + taxable_income,
            tax_table.standard_deduction,
            &tax_table.tax_levels,
        );

        // taxes are paid first from any income left over after expenses. The
        // rest must be withdrawn, but these withdrawals cost more taxes,
        // causing more withdrawals, and more taxes and so on. This can be
        // calculated as an infinite power series.
        let surplus = f32::max(income - expenses, 0.0);
        let taxes =
            base_taxes + f32::max(base_taxes - surplus, 0.0) * tax_rate / (100.0 - tax_rate);

        let net_cash_flow = income - expenses - taxes;
        let mut withdrawal_rate = 0.0;
        if net_cash_flow >= 0.0 {
            self.portfolio_.deposit(net_cash_flow);
        } else {
            if self.portfolio_.balance > 0.0 {
                withdrawal_rate = -net_cash_flow * 12.0 / self.portfolio_.balance;
            }
            self.portfolio_.withdraw(-net_cash_flow);
        }

        let annualized_return = self.portfolio_.grow(
            us_equity_expected_returns,
            international_equity_expected_returns,
            bonds_expected_returns,
            self.current_date_ >= self.simulation_results_.retirement_date,
        );
        self.sum_of_returns_ += annualized_return;
        self.simulation_results_.average_return =
            self.sum_of_returns_ / (self.simulation_results_.monthly_snapshot.len() as f32 + 1.0);

        let monthly_balance = MonthlySnapshot {
            date: self.current_date_,
            balance: self.portfolio_.balance,
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
            .unwrap();

        Ok(self.portfolio_.balance == 0.0)
    }
}

pub fn run_simulation(input: &Input) -> Result<SimulationResults, String> {
    let mut simulation = Simulation::new(input);

    loop {
        let is_finished = simulation.run_simulation_one_month(
            input.portfolio.us_equity_expected_returns,
            input.portfolio.international_equity_expected_returns,
            input.portfolio.bonds_expected_returns,
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

    #[test]
    fn test_social_security() {
        let early = 1000.0;
        let full = 2000.0;
        let delayed = 4000.0;

        assert_eq!(
            get_social_security_monthly_income(60, early, full, delayed),
            0.0
        );
        assert_eq!(
            get_social_security_monthly_income(62, early, full, delayed),
            early
        );
        assert_eq!(
            get_social_security_monthly_income(67, early, full, delayed),
            full
        );
        assert_eq!(
            get_social_security_monthly_income(70, early, full, delayed),
            delayed
        );
        assert_eq!(
            get_social_security_monthly_income(71, early, full, delayed),
            delayed
        );
        assert_eq!(
            get_social_security_monthly_income(63, early, full, delayed),
            1200.0
        );
        assert_eq!(
            get_social_security_monthly_income(68, early, full, delayed),
            2000.0 + 2000.0 / 3.0
        );
    }

    // 2026 married filing jointly brackets
    const STANDARD_DEDUCTION: f32 = 32200.0;

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

    // returns (annual tax, marginal rate) for an annual gross income
    fn annual_taxes(annual_income: f32) -> (f32, f32) {
        let (monthly_tax, rate) = get_taxes(annual_income / 12.0, STANDARD_DEDUCTION, &brackets());
        (monthly_tax * 12.0, rate)
    }

    fn assert_close(actual: f32, expected: f32) {
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

    // a retiree who is 60 today, with no social security, pension or other income
    fn test_retiree(retirement_age: u32, salary_annual: f32) -> crate::Retiree {
        let today = chrono::Utc::now().naive_utc().date();
        crate::Retiree {
            name: "Test".to_string(),
            date_of_birth: today
                .checked_sub_months(chrono::Months::new(60 * 12))
                .unwrap(),
            retirement_age,
            life_expectency: 64,
            salary_annual,
            retirement_contribution_percent: 10.0,
            social_security_age: 67,
            pension_age: 0,
            pension_monthly_income: 0.0,
            other_monthly_retirement_income: 0.0,
            social_security_amount_early: 0.0,
            social_security_amount_full: 0.0,
            social_security_amount_delayed: 0.0,
        }
    }

    // input with zero returns and no taxes, so balances are easy to predict
    fn test_input(retirees: Vec<crate::Retiree>, monthly_expenses: f32) -> Input {
        let all_bonds = crate::portfolio::Allocation {
            us_equities: 0.0,
            international: 0.0,
            bonds: 100.0,
        };
        Input {
            retirees,
            portfolio: Portfolio {
                balance: 1_000_000.0,
                pre_retirement_allocation: all_bonds,
                post_retirement_allocation: all_bonds,
                us_equity_expected_returns: 0.0,
                us_equity_standard_deviation: 0.0,
                international_equity_expected_returns: 0.0,
                international_equity_standard_deviation: 0.0,
                bonds_expected_returns: 0.0,
                bonds_standard_deviation: 0.0,
                us_international_correlation: 0.0,
                us_bonds_correlation: 0.0,
                international_bonds_correlation: 0.0,
            },
            expenses: crate::Expenses {
                monthly: monthly_expenses,
                survivor_percent: 100.0,
            },
            tax_rates: crate::TaxRates {
                married_filing_jointly: flat_tax_table(1.0e9, 0.0),
                single: flat_tax_table(1.0e9, 0.0),
            },
        }
    }

    fn flat_tax_table(standard_deduction: f32, rate: f32) -> crate::TaxTable {
        crate::TaxTable {
            standard_deduction,
            tax_levels: vec![TaxLevel { income: 0.0, rate }],
        }
    }

    // flat tax with no standard deduction, the same for every filing status
    fn flat_tax(input: &mut Input, rate: f32) {
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
            assert_eq!(snapshot.balance, 1_000_000.0 + 1000.0 * (i + 1) as f32);
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

    // two retirees, both 60 and retired. The first has a pension and the
    // larger social security benefit, and dies first.
    fn survivor_input(first_life_expectency: u32) -> Input {
        let mut first = test_retiree(60, 0.0);
        first.life_expectency = first_life_expectency;
        first.pension_monthly_income = 500.0;
        first.social_security_age = 62;
        first.social_security_amount_early = 2000.0;
        let mut second = test_retiree(60, 0.0);
        second.life_expectency = 66;
        second.social_security_age = 62;
        second.social_security_amount_early = 1000.0;
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
            .find(|s| s.date > first.social_security_date)
            .unwrap();
        assert_eq!(both_collecting.income, 500.0 + 2000.0 + 1000.0);
        assert_eq!(both_collecting.expenses, 5000.0);

        // pension stops, survivor switches to the larger benefit, expenses drop
        let survivor = results
            .monthly_snapshot
            .iter()
            .find(|s| s.date >= first.death_date)
            .unwrap();
        assert_eq!(survivor.income, 2000.0);
        assert_eq!(survivor.expenses, 2500.0);

        // simulation runs until the survivor dies
        let last = results.monthly_snapshot.last().unwrap();
        assert!(last.date < results.retirees[1].death_date);
        assert!(last.date >= first.death_date);
    }

    #[test]
    fn test_no_survivor_benefit_if_deceased_never_collected() {
        // first retiree dies at 62, before collecting social security
        let input = survivor_input(61);
        let results = run_simulation(&input).unwrap();
        let survivor = results
            .monthly_snapshot
            .iter()
            .find(|s| s.date > results.retirees[1].social_security_date)
            .unwrap();
        assert_eq!(survivor.income, 1000.0);
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
            let (tax, _) = annual_taxes(income as f32);
            assert!(tax >= previous, "tax dropped at income {income}");
            previous = tax;
        }
    }
}
