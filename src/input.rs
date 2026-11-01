/**************************************************************************
* input.rs
*
* The retirement plan entered by the user: parsed from YAML (or JSON, which
* is a subset of YAML) and validated.
**************************************************************************/

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::portfolio::{Allocation, Forecasts, Portfolio};
use crate::tax_rates::{TaxRates, TAX_YEAR};
use crate::{monte_carlo, utils};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retiree {
    pub name: String,
    #[serde(with = "date_format")]
    pub date_of_birth: NaiveDate,
    pub retirement_age: u32,
    // the old misspelling is still accepted so existing files keep working
    #[serde(alias = "life_expectency")]
    pub life_expectancy: u32,
    #[serde(rename = "wage_annual_salary")]
    pub salary_annual: f64,
    pub retirement_contribution_percent: f64,
    // age in whole years when this retiree claims social security (62-70)
    pub social_security_age: u32,
    // the monthly benefit at full retirement age from the SSA web site (the
    // primary insurance amount). Benefits at other ages are calculated from it.
    pub social_security_amount_full: f64,
    pub pension_age: u32,
    pub pension_monthly_income: f64,
    pub other_monthly_retirement_income: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expenses {
    pub monthly: f64,
    // percentage of expenses that remain after one spouse dies
    pub survivor_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub retirees: Vec<Retiree>,
    pub portfolio: Portfolio,
    pub expenses: Expenses,
    // built in, not part of the input file. Tests replace them with simpler
    // rates.
    #[serde(skip, default = "TaxRates::current")]
    pub tax_rates: TaxRates,
    // first month of the simulations. Not part of the input file: today when
    // running the program, a fixed date in tests so results don't depend on
    // when they run.
    #[serde(skip)]
    pub start_date: NaiveDate,
}

// dates are entered as mm/dd/yyyy. yyyy-mm-dd, which is what web forms send,
// is also accepted.
mod date_format {
    use chrono::NaiveDate;
    use serde::{de, Deserialize, Deserializer, Serializer};

    const FORMAT: &str = "%m/%d/%Y";

    pub fn serialize<S: Serializer>(date: &NaiveDate, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&date.format(FORMAT).to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<NaiveDate, D::Error> {
        let text = String::deserialize(deserializer)?;
        NaiveDate::parse_from_str(&text, FORMAT)
            .or_else(|_| NaiveDate::parse_from_str(&text, "%Y-%m-%d"))
            .map_err(|_| de::Error::custom(format!("invalid date {text}, expected mm/dd/yyyy")))
    }
}

impl Input {
    // parses and validates a plan. JSON is also accepted, since it is a
    // subset of YAML.
    pub fn from_yaml(text: &str, start_date: NaiveDate) -> Result<Input, String> {
        let mut input: Input =
            serde_yaml_ng::from_str(text).map_err(|e| explain_parse_error(text, e))?;
        input.start_date = start_date;
        input.validate()?;
        Ok(input)
    }

    // the plan in the same format as the input file, e.g. to save a plan
    // entered on the web site
    pub fn to_yaml(&self) -> Result<String, String> {
        serde_yaml_ng::to_string(self).map_err(|e| e.to_string())
    }

    // checks that the values make sense, so the simulations can't run
    // forever, overflow, or produce NaN balances
    pub fn validate(&self) -> Result<(), String> {
        if self.retirees.is_empty() || self.retirees.len() > MAX_RETIREES {
            return Err(format!(
                "there must be one or two retirees (currently {})",
                self.retirees.len()
            ));
        }
        for retiree in self.retirees.iter() {
            validate_retiree(retiree, &self.start_date)?;
        }
        if self
            .retirees
            .iter()
            .all(|r| utils::get_age(&r.date_of_birth, &self.start_date) > r.life_expectancy)
        {
            return Err("every retiree is already past their life expectancy".to_string());
        }

        let portfolio = &self.portfolio;
        check_amount(portfolio.pre_tax_balance, "portfolio balance")?;
        check_amount(portfolio.roth_balance, "portfolio roth_balance")?;
        validate_allocation(
            &portfolio.pre_retirement_allocation,
            "pre_retirement_allocation",
        )?;
        validate_allocation(
            &portfolio.post_retirement_allocation,
            "post_retirement_allocation",
        )?;
        validate_forecasts(&portfolio.forecasts)?;

        check_amount(self.expenses.monthly, "expenses monthly")?;
        check_percent(self.expenses.survivor_percent, "expenses survivor_percent")?;

        Ok(())
    }
}

// limits on the input, generous enough for any real plan
pub const MAX_RETIREES: usize = 2;
pub const MAX_AGE: u32 = 120;
pub const MIN_SOCIAL_SECURITY_AGE: u32 = 62;
// any single balance, income or expense amount
pub const MAX_AMOUNT: f64 = 1.0e12;
// expected returns are real (after inflation) annual percentages. At -100%
// or below, compounding breaks down (a negative growth factor).
pub const MIN_EXPECTED_RETURN: f64 = -50.0;
pub const MAX_EXPECTED_RETURN: f64 = 50.0;
pub const MAX_STANDARD_DEVIATION: f64 = 50.0;

fn validate_retiree(retiree: &Retiree, start_date: &NaiveDate) -> Result<(), String> {
    let name = &retiree.name;
    if retiree.date_of_birth > *start_date {
        return Err(format!("{name}: date_of_birth is in the future"));
    }
    let ages = [
        (retiree.retirement_age, "retirement_age"),
        (retiree.life_expectancy, "life_expectancy"),
        (retiree.social_security_age, "social_security_age"),
        (retiree.pension_age, "pension_age"),
    ];
    for (age, field) in ages {
        if age > MAX_AGE {
            return Err(format!(
                "{name}: {field} can't be more than {MAX_AGE} (currently {age})"
            ));
        }
    }
    if retiree.social_security_age < MIN_SOCIAL_SECURITY_AGE {
        return Err(format!(
            "{name}: social_security_age must be at least {MIN_SOCIAL_SECURITY_AGE} \
             (currently {})",
            retiree.social_security_age
        ));
    }
    let amounts = [
        (retiree.salary_annual, "wage_annual_salary"),
        (retiree.pension_monthly_income, "pension_monthly_income"),
        (
            retiree.other_monthly_retirement_income,
            "other_monthly_retirement_income",
        ),
        (
            retiree.social_security_amount_full,
            "social_security_amount_full",
        ),
    ];
    for (amount, field) in amounts {
        check_amount(amount, &format!("{name}: {field}"))?;
    }
    check_percent(
        retiree.retirement_contribution_percent,
        &format!("{name}: retirement_contribution_percent"),
    )
}

// checks that a value is a number in the range, inclusive. NaN fails, since
// it isn't in any range.
fn check_range(value: f64, min: f64, max: f64, name: &str) -> Result<(), String> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "{name} must be between {min} and {max} (currently {value})"
        ))
    }
}

fn check_amount(value: f64, name: &str) -> Result<(), String> {
    if value < 0.0 {
        return Err(format!("{name} can't be negative"));
    }
    check_range(value, 0.0, MAX_AMOUNT, name)
}

fn check_percent(value: f64, name: &str) -> Result<(), String> {
    if value < 0.0 {
        return Err(format!("{name} can't be negative"));
    }
    check_range(value, 0.0, 100.0, name)
}

fn validate_forecasts(forecasts: &Forecasts) -> Result<(), String> {
    let expected_returns = [
        (
            forecasts.us_equity_expected_returns,
            "us_equity_expected_returns",
        ),
        (
            forecasts.international_equity_expected_returns,
            "international_equity_expected_returns",
        ),
        (forecasts.bonds_expected_returns, "bonds_expected_returns"),
    ];
    for (value, name) in expected_returns {
        check_range(value, MIN_EXPECTED_RETURN, MAX_EXPECTED_RETURN, name)?;
    }
    let standard_deviations = [
        (
            forecasts.us_equity_standard_deviation,
            "us_equity_standard_deviation",
        ),
        (
            forecasts.international_equity_standard_deviation,
            "international_equity_standard_deviation",
        ),
        (
            forecasts.bonds_standard_deviation,
            "bonds_standard_deviation",
        ),
    ];
    for (value, name) in standard_deviations {
        if value < 0.0 {
            return Err("standard deviations can't be negative".to_string());
        }
        check_range(value, 0.0, MAX_STANDARD_DEVIATION, name)?;
    }
    // checks the correlations
    monte_carlo::ReturnsSampler::new(forecasts)?;
    Ok(())
}

// a friendlier message for some common mistakes, otherwise serde's error,
// which names the field and line
fn explain_parse_error(text: &str, error: serde_yaml_ng::Error) -> String {
    match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(text) {
        Ok(serde_yaml_ng::Value::Null) => "input file is empty".to_string(),
        // older input files had their own tax rates
        Ok(value) if !value["tax_rates"].is_null() => format!(
            "tax_rates is no longer part of the input file: the {TAX_YEAR} federal \
             tax rates are built in. Remove the tax_rates block."
        ),
        // older input files also had social security amounts at 62 and 70
        Ok(value) if has_old_social_security_amounts(&value) => {
            "social_security_amount_early and social_security_amount_delayed are no \
             longer used: the benefit at social_security_age is calculated from \
             social_security_amount_full, the amount at full retirement age. Remove \
             them."
                .to_string()
        }
        // older input files had the forecasts directly in the portfolio block
        Ok(value) if has_forecasts_in_portfolio(&value) => {
            "the expected returns, standard deviations, and correlations now go in \
             a forecasts block inside the portfolio block (see input.yaml), and are \
             optional"
                .to_string()
        }
        _ => format!("Invalid input: {error}"),
    }
}

fn has_old_social_security_amounts(value: &serde_yaml_ng::Value) -> bool {
    let Some(retirees) = value["retirees"].as_sequence() else {
        return false;
    };
    retirees.iter().any(|retiree| {
        retiree.as_mapping().is_some_and(|retiree| {
            retiree.keys().filter_map(|key| key.as_str()).any(|key| {
                key == "social_security_amount_early" || key == "social_security_amount_delayed"
            })
        })
    })
}

fn has_forecasts_in_portfolio(value: &serde_yaml_ng::Value) -> bool {
    let Some(portfolio) = value["portfolio"].as_mapping() else {
        return false;
    };
    portfolio.keys().filter_map(|key| key.as_str()).any(|key| {
        key.ends_with("_expected_returns")
            || key.ends_with("_standard_deviation")
            || key.ends_with("_correlation")
    })
}

fn validate_allocation(allocation: &Allocation, name: &str) -> Result<(), String> {
    let values = [
        allocation.us_equities,
        allocation.international,
        allocation.bonds,
    ];
    if values.iter().any(|&v| v < 0.0) {
        return Err(format!("{name} percentages can't be negative"));
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Err(format!("{name} percentages must be numbers"));
    }
    let total: f64 = values.iter().sum();
    if (total - 100.0).abs() > 0.01 {
        return Err(format!("{name} must add up to 100.0 (currently {total})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defaults;
    use std::fs;

    // fixed start date so results don't depend on when the tests run
    fn start_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()
    }

    fn sample() -> String {
        fs::read_to_string("input.yaml").unwrap()
    }

    fn parse_sample() -> Input {
        Input::from_yaml(&sample(), start_date()).unwrap()
    }

    // sample input with one piece of text replaced
    fn parse_modified_sample(from: &str, to: &str) -> Result<Input, String> {
        let sample = sample();
        assert!(sample.contains(from), "sample input doesn't contain {from}");
        Input::from_yaml(&sample.replacen(from, to, 1), start_date())
    }

    #[test]
    fn test_parse_sample_input() {
        let input = parse_sample();
        assert_eq!(input.expenses.survivor_percent, 100.0);
        assert_eq!(input.retirees[0].life_expectancy, 90);
        assert_eq!(
            input.retirees[0].date_of_birth,
            NaiveDate::from_ymd_opt(1960, 9, 30).unwrap()
        );
        assert_eq!(input.start_date, start_date());
    }

    #[test]
    fn test_tax_rates_are_built_in() {
        let input = parse_sample();
        assert_eq!(
            input.tax_rates.single.standard_deduction,
            TaxRates::current().single.standard_deduction
        );
        // and aren't written to a saved plan
        assert!(!input.to_yaml().unwrap().contains("tax_rates"));

        // older input files had their own tax rates
        let yaml = sample() + "\ntax_rates:\n    expected_inflation: 2.5\n";
        let err = Input::from_yaml(&yaml, start_date()).unwrap_err();
        assert!(err.contains("built in"), "{err}");
        let yaml = "tax_rates:\n  standard_deduction: 32200.0\n  levels:\n    - {income: 0.0, rate: 10.0}\n";
        let err = Input::from_yaml(yaml, start_date()).unwrap_err();
        assert!(err.contains("Remove the tax_rates block"), "{err}");
    }

    // the sample input without the optional forecasts block
    fn minimal_sample() -> String {
        let forecasts = [
            "forecasts:",
            "_expected_returns:",
            "_standard_deviation:",
            "_correlation:",
        ];
        let sample = sample();
        let lines: Vec<&str> = sample
            .lines()
            .filter(|line| !forecasts.iter().any(|name| line.contains(name)))
            .collect();
        lines.join("\n")
    }

    #[test]
    fn test_defaults_match_sample() {
        // the sample input spells out the defaults, so leaving them out
        // gives the same plan
        let minimal = minimal_sample();
        assert!(!minimal.contains("bonds_standard_deviation:"), "{minimal}");
        let input = Input::from_yaml(&minimal, start_date()).unwrap();
        assert_eq!(input.to_yaml().unwrap(), parse_sample().to_yaml().unwrap());
    }

    #[test]
    fn test_override_one_default() {
        let minimal = minimal_sample().replace(
            "international: 24.0",
            "international: 24.0\n    forecasts:\n        bonds_expected_returns: 1.0",
        );
        let input = Input::from_yaml(&minimal, start_date()).unwrap();
        let forecasts = &input.portfolio.forecasts;
        assert_eq!(forecasts.bonds_expected_returns, 1.0);
        assert_eq!(
            forecasts.bonds_standard_deviation,
            defaults::BONDS_STANDARD_DEVIATION
        );
    }

    #[test]
    fn test_forecasts_in_old_place() {
        // older input files had the forecasts directly in the portfolio block
        let old = minimal_sample().replace(
            "international: 24.0",
            "international: 24.0\n    bonds_expected_returns: 1.0",
        );
        let err = Input::from_yaml(&old, start_date()).unwrap_err();
        assert!(err.contains("forecasts block"), "{err}");
    }

    #[test]
    fn test_parse_accepts_integer_amounts() {
        let input = parse_modified_sample("balance: 900000.0", "balance: 900000").unwrap();
        assert_eq!(input.portfolio.pre_tax_balance, 900000.0);
    }

    #[test]
    fn test_parse_roth_balance() {
        let input = parse_sample();
        assert_eq!(input.portfolio.roth_balance, 300000.0);

        // optional, defaults to no Roth
        let input = parse_modified_sample("roth_balance: 300000.0", "").unwrap();
        assert_eq!(input.portfolio.roth_balance, 0.0);

        let err =
            parse_modified_sample("roth_balance: 300000.0", "roth_balance: lots").unwrap_err();
        assert!(err.contains("roth_balance"), "{err}");
        let err =
            parse_modified_sample("roth_balance: 300000.0", "roth_balance: -1.0").unwrap_err();
        assert!(err.contains("negative"), "{err}");
    }

    #[test]
    fn test_parse_rejects_negative_age() {
        let err = parse_modified_sample("retirement_age: 67", "retirement_age: -67").unwrap_err();
        assert!(err.contains("retirement_age"), "{err}");
    }

    #[test]
    fn test_parse_rejects_fractional_age() {
        let err = parse_modified_sample("retirement_age: 67", "retirement_age: 67.5").unwrap_err();
        assert!(err.contains("retirement_age"), "{err}");
    }

    #[test]
    fn test_parse_rejects_allocation_not_100() {
        let err = parse_modified_sample("us_equities: 56.0", "us_equities: 50.0").unwrap_err();
        assert!(err.contains("add up to 100"), "{err}");
    }

    #[test]
    fn test_parse_correlations() {
        let input = parse_modified_sample(
            "us_international_correlation: 0.8",
            "us_international_correlation: 0.7",
        )
        .unwrap();
        let forecasts = &input.portfolio.forecasts;
        assert_eq!(forecasts.us_international_correlation, 0.7);
        assert_eq!(forecasts.us_bonds_correlation, 0.3);
        assert_eq!(forecasts.international_bonds_correlation, 0.3);
    }

    #[test]
    fn test_each_correlation_has_a_default() {
        let input = parse_modified_sample(
            "us_international_correlation: 0.8",
            "us_bonds_correlation: 0.1",
        );
        // the sample now has us_bonds_correlation twice
        assert!(input.unwrap_err().contains("duplicate"));

        let input = parse_modified_sample("us_international_correlation: 0.8", "").unwrap();
        assert_eq!(
            input.portfolio.forecasts.us_international_correlation,
            defaults::US_INTERNATIONAL_CORRELATION
        );
    }

    #[test]
    fn test_parse_rejects_inconsistent_correlations() {
        let err = parse_modified_sample("us_bonds_correlation: 0.3", "us_bonds_correlation: -0.9")
            .unwrap_err();
        assert!(err.contains("inconsistent"), "{err}");
    }

    #[test]
    fn test_parse_rejects_negative_standard_deviation() {
        let err = parse_modified_sample(
            "bonds_standard_deviation: 6.0",
            "bonds_standard_deviation: -6.0",
        )
        .unwrap_err();
        assert!(err.contains("standard deviation"), "{err}");
    }

    #[test]
    fn test_parse_survivor_percent() {
        let input =
            parse_modified_sample("survivor_percent: 100.0", "survivor_percent: 70.0").unwrap();
        assert_eq!(input.expenses.survivor_percent, 70.0);

        // required
        let err = parse_modified_sample("survivor_percent: 100.0", "").unwrap_err();
        assert!(err.contains("survivor_percent"), "{err}");
    }

    #[test]
    fn test_parse_errors_instead_of_panicking() {
        assert!(Input::from_yaml("", start_date())
            .unwrap_err()
            .contains("empty"));
        assert!(Input::from_yaml("# only a comment\n", start_date())
            .unwrap_err()
            .contains("empty"));
        assert!(Input::from_yaml("retirees: [", start_date())
            .unwrap_err()
            .contains("Invalid input"));
    }

    #[test]
    fn test_parse_rejects_unknown_fields() {
        // a typo would otherwise be silently ignored
        let err = parse_modified_sample("monthly: 9000.0", "montly: 9000.0").unwrap_err();
        assert!(err.contains("unknown field `montly`"), "{err}");
    }

    #[test]
    fn test_parse_accepts_old_field_names() {
        let input = parse_modified_sample("life_expectancy: 90", "life_expectency: 85").unwrap();
        assert_eq!(input.retirees[0].life_expectancy, 85);
        let input =
            parse_modified_sample("pre_retirement_allocation:", "pre-retirement_allocation:")
                .unwrap();
        assert_eq!(input.portfolio.pre_retirement_allocation.us_equities, 56.0);
    }

    #[test]
    fn test_parse_dates() {
        let input = parse_modified_sample("9/30/1960", "1960-09-30").unwrap();
        assert_eq!(
            input.retirees[0].date_of_birth,
            NaiveDate::from_ymd_opt(1960, 9, 30).unwrap()
        );
        let err = parse_modified_sample("9/30/1960", "30/9/1960").unwrap_err();
        assert!(err.contains("invalid date 30/9/1960"), "{err}");
    }

    #[test]
    fn test_parse_json() {
        let json = serde_json::to_string(&parse_sample()).unwrap();
        let input = Input::from_yaml(&json, start_date()).unwrap();
        assert_eq!(input.portfolio.roth_balance, 300000.0);
        assert_eq!(input.retirees[1].name, "Jane Doe");
    }

    #[test]
    fn test_yaml_round_trip() {
        let yaml = parse_sample().to_yaml().unwrap();
        let input = Input::from_yaml(&yaml, start_date()).unwrap();
        assert_eq!(input.to_yaml().unwrap(), yaml);
        assert!(yaml.contains("date_of_birth: 09/30/1960"), "{yaml}");
    }

    // the sample input after changing it in code, to test validation of
    // values that are awkward to write as text
    fn validate_modified(change: impl FnOnce(&mut Input)) -> Result<(), String> {
        let mut input = parse_sample();
        change(&mut input);
        input.validate()
    }

    #[test]
    fn test_validation_requires_one_or_two_retirees() {
        let err = validate_modified(|input| input.retirees.clear()).unwrap_err();
        assert!(err.contains("one or two retirees"), "{err}");
        let err = validate_modified(|input| {
            let third = input.retirees[0].clone();
            input.retirees.push(third);
        })
        .unwrap_err();
        assert!(err.contains("one or two retirees (currently 3)"), "{err}");
        validate_modified(|input| {
            input.retirees.pop();
        })
        .unwrap();
    }

    #[test]
    fn test_validation_limits_ages() {
        // a huge life expectancy would make the simulation run for ages, or
        // overflow the date arithmetic
        let err = parse_modified_sample("life_expectancy: 90", "life_expectancy: 4000000000")
            .unwrap_err();
        assert!(
            err.contains("life_expectancy can't be more than 120"),
            "{err}"
        );
        parse_modified_sample("life_expectancy: 90", "life_expectancy: 120").unwrap();
        for field in [
            "retirement_age: 67",
            "social_security_age: 67",
            "pension_age: 62",
        ] {
            let name = field.split(':').next().unwrap();
            let err = parse_modified_sample(field, &format!("{name}: 121")).unwrap_err();
            assert!(err.contains(&format!("{name} can't be more than")), "{err}");
        }
    }

    #[test]
    fn test_validation_rejects_future_birth_date() {
        let err = parse_modified_sample("9/30/1960", "9/30/2060").unwrap_err();
        assert!(
            err.contains("John Doe: date_of_birth is in the future"),
            "{err}"
        );
    }

    #[test]
    fn test_validation_limits_expected_returns() {
        let err = parse_modified_sample(
            "bonds_expected_returns: 2.5",
            "bonds_expected_returns: -100.0",
        )
        .unwrap_err();
        assert!(
            err.contains("bonds_expected_returns must be between"),
            "{err}"
        );
        let err = parse_modified_sample(
            "us_equity_expected_returns: 3.2",
            "us_equity_expected_returns: 1000.0",
        )
        .unwrap_err();
        assert!(err.contains("us_equity_expected_returns"), "{err}");
        parse_modified_sample(
            "bonds_expected_returns: 2.5",
            "bonds_expected_returns: -3.0",
        )
        .unwrap();
    }

    #[test]
    fn test_validation_limits_standard_deviations() {
        let err = parse_modified_sample(
            "bonds_standard_deviation: 6.0",
            "bonds_standard_deviation: 500.0",
        )
        .unwrap_err();
        assert!(
            err.contains("bonds_standard_deviation must be between"),
            "{err}"
        );
    }

    #[test]
    fn test_validation_limits_percentages() {
        let err = parse_modified_sample(
            "retirement_contribution_percent: 12.0",
            "retirement_contribution_percent: 150.0",
        )
        .unwrap_err();
        assert!(err.contains("retirement_contribution_percent"), "{err}");
        let err = parse_modified_sample(
            "retirement_contribution_percent: 12.0",
            "retirement_contribution_percent: -1.0",
        )
        .unwrap_err();
        assert!(err.contains("can't be negative"), "{err}");
        let err = parse_modified_sample("survivor_percent: 100.0", "survivor_percent: 101.0")
            .unwrap_err();
        assert!(err.contains("survivor_percent"), "{err}");
    }

    #[test]
    fn test_validation_limits_amounts() {
        let err = parse_modified_sample("monthly: 9000.0", "monthly: -1.0").unwrap_err();
        assert!(err.contains("expenses monthly can't be negative"), "{err}");
        let err = parse_modified_sample("balance: 900000.0", "balance: 1.0e300").unwrap_err();
        assert!(err.contains("portfolio balance must be between"), "{err}");
        let err = parse_modified_sample(
            "pension_monthly_income: 1000.0",
            "pension_monthly_income: -1000.0",
        )
        .unwrap_err();
        assert!(err.contains("John Doe: pension_monthly_income"), "{err}");
    }

    #[test]
    fn test_validation_rejects_nan_and_infinity() {
        // YAML can spell these, and JSON from code could contain them
        for (from, to) in [
            ("balance: 900000.0", "balance: .nan"),
            ("monthly: 9000.0", "monthly: .inf"),
            ("wage_annual_salary: 75000.0", "wage_annual_salary: .nan"),
            ("survivor_percent: 100.0", "survivor_percent: .nan"),
            (
                "bonds_expected_returns: 2.5",
                "bonds_expected_returns: .nan",
            ),
            (
                "bonds_standard_deviation: 6.0",
                "bonds_standard_deviation: .inf",
            ),
            ("us_bonds_correlation: 0.3", "us_bonds_correlation: .nan"),
        ] {
            assert!(
                parse_modified_sample(from, to).is_err(),
                "{to} was accepted"
            );
        }
        let err = validate_modified(|input| {
            input.portfolio.post_retirement_allocation.bonds = f64::NAN;
        })
        .unwrap_err();
        assert!(err.contains("post_retirement_allocation"), "{err}");
    }

    #[test]
    fn test_parse_social_security() {
        let input = parse_sample();
        assert_eq!(input.retirees[0].social_security_amount_full, 3870.0);
        assert_eq!(input.retirees[0].social_security_age, 67);

        let err = parse_modified_sample("social_security_age: 67", "social_security_age: 61")
            .unwrap_err();
        assert!(
            err.contains("social_security_age must be at least 62"),
            "{err}"
        );
        let err = parse_modified_sample(
            "social_security_amount_full: 3870.0",
            "social_security_amount_full: -1.0",
        )
        .unwrap_err();
        assert!(err.contains("social_security_amount_full"), "{err}");
    }

    #[test]
    fn test_old_social_security_amounts() {
        // older input files also had the amounts at 62 and 70
        let old = sample().replacen(
            "social_security_amount_full: 3870.0",
            "social_security_amount_early: 2693.0\n       \
             social_security_amount_full: 3870.0\n       \
             social_security_amount_delayed: 4809.0",
            1,
        );
        let err = Input::from_yaml(&old, start_date()).unwrap_err();
        assert!(err.contains("are no longer used"), "{err}");
    }

    #[test]
    fn test_validation_uses_start_date() {
        // John Doe (born 1960) and Jane Doe (born 1964) both have a life
        // expectancy of 90, so by 2056 they are both past it
        let late = NaiveDate::from_ymd_opt(2056, 1, 1).unwrap();
        let err = Input::from_yaml(&sample(), late).unwrap_err();
        assert!(err.contains("past their life expectancy"), "{err}");
    }
}
