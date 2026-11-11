/**************************************************************************
* defaults.rs
*
* Built-in defaults. The market forecasts are used for any forecast a plan
* leaves out. The plan defaults are for programs that build a plan, such as
* a web site: they fill in what a person doesn't enter, and are kept here so
* there is one place for every default.
*
* The forecasts are from Fidelity's long-term asset class assumptions
**************************************************************************/

use serde::Serialize;

use crate::portfolio::Allocation;

// Long-term forecasts from Fidelity Investments: real (after inflation)
// geometric average returns and standard deviations, as percentages.
pub const US_EQUITY_EXPECTED_RETURNS: f64 = 3.2;
pub const US_EQUITY_STANDARD_DEVIATION: f64 = 15.0;
pub const INTERNATIONAL_EQUITY_EXPECTED_RETURNS: f64 = 4.1;
pub const INTERNATIONAL_EQUITY_STANDARD_DEVIATION: f64 = 16.0;
pub const BONDS_EXPECTED_RETURNS: f64 = 2.5;
pub const BONDS_STANDARD_DEVIATION: f64 = 6.0;

// correlations of annual returns (-1.0 to 1.0). US and international stocks
// have historically been strongly correlated. The stock and bond correlation
// has varied widely, so 0.3 is a cautious middle ground.
pub const US_INTERNATIONAL_CORRELATION: f64 = 0.8;
pub const US_BONDS_CORRELATION: f64 = 0.3;
pub const INTERNATIONAL_BONDS_CORRELATION: f64 = 0.3;

// income paid each year as a percentage of the balance (nominal), used to
// tax the taxable account every year: approximate yields in 2026 of a US
// total stock market fund, an international stock fund, and a US aggregate
// bond fund.
pub const US_EQUITY_DIVIDEND_YIELD: f64 = 1.3;
pub const INTERNATIONAL_EQUITY_DIVIDEND_YIELD: f64 = 3.0;
pub const BONDS_INTEREST_YIELD: f64 = 4.0;

// full retirement age for social security for anyone born in 1960 or later
// (https://www.ssa.gov/benefits/retirement/planner/agereduction.html), used
// for both retiring and claiming
pub const RETIREMENT_AGE: u32 = 67;
pub const SOCIAL_SECURITY_AGE: u32 = 67;
// longer than average, so plans err on the safe side
pub const LIFE_EXPECTANCY: u32 = 90;
pub const RETIREMENT_CONTRIBUTION_PERCENT: f64 = 10.0;
// 60% stocks (two thirds US) and 40% bonds, before and after retirement
pub const ALLOCATION: Allocation = Allocation {
    us_equities: 40.0,
    international: 20.0,
    bonds: 40.0,
};

// Average monthly spending of retired households, in today's dollars, from
// the Bureau of Labor Statistics Consumer Expenditure Surveys, 2023-2024:
// tables 3600 and 3620, consumer units of one and two people by age of
// reference person, column "65 years and older"
// (https://www.bls.gov/cex/tables/cross-tab/mean.htm).
//
//   one person:  43,178 average annual expenditures - 1,090 retirement,
//                pensions, and social security = 42,088 a year
//   two people:  73,716 - 3,551 = 70,165 a year
//
// Retirement contributions and social security tax are left out because the
// simulation doesn't spend on them once retired. Income taxes aren't part of
// the survey's expenditures; the simulation adds them. The amounts are
// converted to today's dollars with the consumer price index (CPI-U, BLS
// series CUUR0000SA0): 336.940 in September 2026 / 309.195, the 2023-2024
// average, = 1.0897, and rounded to the nearest $100 a month.
pub const MONTHLY_EXPENSES_ONE_PERSON: f64 = 3800.0;
pub const MONTHLY_EXPENSES_TWO_PEOPLE: f64 = 6400.0;
pub const SURVIVOR_PERCENT: f64 = 100.0;

// the defaults for a new plan, for a program such as a web site that builds
// one from a person's answers. Anything not here starts at 0 (pensions,
// other income, Roth and taxable savings) or is the person's own (date of
// birth, salary, savings). The forecasts aren't here: a plan leaves them out
// to use the built-in ones, so it gets the new ones when they're updated.
#[derive(Debug, Clone, Serialize)]
pub struct PlanDefaults {
    pub retirement_age: u32,
    pub life_expectancy: u32,
    pub retirement_contribution_percent: f64,
    pub social_security_age: u32,
    pub pre_retirement_allocation: Allocation,
    pub post_retirement_allocation: Allocation,
    pub monthly_expenses_one_person: f64,
    pub monthly_expenses_two_people: f64,
    pub survivor_percent: f64,
}

pub fn plan_defaults() -> PlanDefaults {
    PlanDefaults {
        retirement_age: RETIREMENT_AGE,
        life_expectancy: LIFE_EXPECTANCY,
        retirement_contribution_percent: RETIREMENT_CONTRIBUTION_PERCENT,
        social_security_age: SOCIAL_SECURITY_AGE,
        pre_retirement_allocation: ALLOCATION,
        post_retirement_allocation: ALLOCATION,
        monthly_expenses_one_person: MONTHLY_EXPENSES_ONE_PERSON,
        monthly_expenses_two_people: MONTHLY_EXPENSES_TWO_PEOPLE,
        survivor_percent: SURVIVOR_PERCENT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::{ForecastChoices, Portfolio};
    use crate::{runner, Expenses, Input, Retiree, TaxRates};
    use chrono::NaiveDate;

    fn start_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()
    }

    // a retiree as a web site would build one: the person's answers plus the
    // defaults
    fn retiree(defaults: &PlanDefaults, name: &str, date_of_birth: NaiveDate) -> Retiree {
        Retiree {
            name: name.to_string(),
            date_of_birth,
            retirement_age: defaults.retirement_age,
            life_expectancy: defaults.life_expectancy,
            salary_annual: 80_000.0,
            retirement_contribution_percent: defaults.retirement_contribution_percent,
            social_security_age: defaults.social_security_age,
            // estimated from the salary
            social_security_amount_full: None,
            pension_age: 0,
            pension_monthly_income: 0.0,
            pension_cola_percent: None,
            other_monthly_retirement_income: 0.0,
        }
    }

    fn plan(retirees: Vec<Retiree>) -> Input {
        let defaults = plan_defaults();
        let monthly = if retirees.len() == 1 {
            defaults.monthly_expenses_one_person
        } else {
            defaults.monthly_expenses_two_people
        };
        Input {
            retirees,
            portfolio: Portfolio {
                pre_tax_balance: 500_000.0,
                roth_balance: 0.0,
                taxable_balance: 0.0,
                taxable_cost_basis: None,
                pre_retirement_allocation: defaults.pre_retirement_allocation,
                post_retirement_allocation: defaults.post_retirement_allocation,
                forecasts: ForecastChoices::default(),
            },
            expenses: Expenses {
                monthly,
                survivor_percent: defaults.survivor_percent,
            },
            tax_rates: TaxRates::current(),
            start_date: start_date(),
        }
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn test_defaults_make_a_valid_plan() {
        let defaults = plan_defaults();
        let single = plan(vec![retiree(&defaults, "You", date(1970, 6, 1))]);
        single.validate().unwrap();

        let couple = plan(vec![
            retiree(&defaults, "You", date(1970, 6, 1)),
            retiree(&defaults, "Spouse", date(1972, 3, 10)),
        ]);
        runner::run_all(&couple, 42).unwrap();

        // and it reads back the same from a saved file
        let yaml = couple.to_yaml().unwrap();
        let reread = Input::from_yaml(&yaml, start_date()).unwrap();
        assert_eq!(reread.to_yaml().unwrap(), yaml);
    }

    #[test]
    fn test_two_people_spend_more_than_one_but_less_than_twice() {
        let defaults = plan_defaults();
        assert!(defaults.monthly_expenses_two_people > defaults.monthly_expenses_one_person);
        assert!(defaults.monthly_expenses_two_people < 2.0 * defaults.monthly_expenses_one_person);
    }

    #[test]
    fn test_defaults_as_json() {
        // the names a web site reads
        let json = serde_json::to_value(plan_defaults()).unwrap();
        assert_eq!(json["retirement_age"], 67);
        assert_eq!(json["post_retirement_allocation"]["bonds"], 40.0);
        // the forecasts are left out, so plans use the built-in ones
        assert!(json.get("forecasts").is_none());
        assert_eq!(json["monthly_expenses_one_person"], 3800.0);
    }
}
