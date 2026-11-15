/**************************************************************************
 * input.rs
 *
 * The retirement plan entered by the user: parsed from YAML (or JSON, which
 * is a subset of YAML) and validated.
 **************************************************************************/

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::portfolio::{Allocation, Forecasts, Portfolio};
use crate::tax_rates::TaxRates;
use crate::{monte_carlo, social_security, utils};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Retiree {
    pub name: String,
    // mm/dd/yyyy when saved; yyyy-mm-dd is also accepted
    #[cfg_attr(feature = "typescript", ts(type = "string"))]
    #[serde(with = "date_format")]
    pub date_of_birth: NaiveDate,
    pub retirement_age: u32,
    pub life_expectancy: u32,
    #[serde(rename = "wage_annual_salary")]
    pub salary_annual: f64,
    pub retirement_contribution_percent: f64,
    // age in whole years when this retiree claims social security (62-70)
    pub social_security_age: u32,
    // the monthly benefit at full retirement age from the SSA web site (the
    // primary insurance amount). Benefits at other ages are calculated from it.
    // Optional: left out, it's estimated from the salary. Use
    // social_security_full() for the amount to simulate with.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub social_security_amount_full: Option<f64>,
    pub pension_age: u32,
    pub pension_monthly_income: f64,
    // the pension's annual cost-of-living increase (percent). None means it
    // keeps up with inflation; 0 is a fixed pension.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pension_cola_percent: Option<f64>,
    pub other_monthly_retirement_income: f64,
}

impl Retiree {
    // the monthly social security benefit at full retirement age: the
    // amount entered, or else estimated from the salary
    pub fn social_security_full(&self) -> f64 {
        self.social_security_amount_full.unwrap_or_else(|| {
            social_security::estimate_full_amount(self.salary_annual, self.retirement_age)
        })
    }

    pub fn social_security_is_estimated(&self) -> bool {
        self.social_security_amount_full.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Expenses {
    pub monthly: f64,
    // percentage of expenses that remain after one spouse dies
    pub survivor_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
        input.validate().map_err(|errors| describe_first(&errors))?;
        Ok(input)
    }

    // the plan in the same format as the input file, e.g. to save a plan
    // entered on the web site
    pub fn to_yaml(&self) -> Result<String, String> {
        serde_yaml_ng::to_string(self).map_err(|e| e.to_string())
    }

    // checks that the values make sense, so the simulations can't run
    // forever, overflow, or produce NaN balances. Returns every problem
    // found, in the order of the input file.
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Errors::default();
        let count = self.retirees.len();
        if count == 0 || count > MAX_RETIREES {
            errors.add(
                None,
                "retirees",
                Some(format!("must list one or two people (currently {count})")),
            );
        }
        for retiree in self.retirees.iter() {
            validate_retiree(retiree, &self.start_date, &mut errors);
        }
        if !self.retirees.is_empty()
            && self
                .retirees
                .iter()
                .all(|r| utils::get_age(&r.date_of_birth, &self.start_date) > r.life_expectancy)
        {
            errors.add(
                None,
                "retirees",
                Some("are all past their life expectancy already".to_string()),
            );
        }

        let portfolio = &self.portfolio;
        let balances = [
            (Some(portfolio.pre_tax_balance), "pre_tax_balance"),
            (Some(portfolio.roth_balance), "roth_balance"),
            (Some(portfolio.taxable_balance), "taxable_balance"),
            (portfolio.taxable_cost_basis, "taxable_cost_basis"),
        ];
        for (balance, name) in balances {
            if let Some(balance) = balance {
                errors.add(None, &format!("portfolio.{name}"), amount_problem(balance));
            }
        }
        let allocations = [
            (
                &portfolio.pre_retirement_allocation,
                "pre_retirement_allocation",
            ),
            (
                &portfolio.post_retirement_allocation,
                "post_retirement_allocation",
            ),
        ];
        for (allocation, name) in allocations {
            errors.add(
                None,
                &format!("portfolio.{name}"),
                allocation_problem(allocation),
            );
        }
        validate_forecasts(&portfolio.forecasts(), &mut errors);

        errors.add(
            None,
            "expenses.monthly",
            amount_problem(self.expenses.monthly),
        );
        errors.add(
            None,
            "expenses.survivor_percent",
            percent_problem(self.expenses.survivor_percent),
        );

        errors.into_result()
    }
}

// a problem with one value in a plan, for showing beside the input it's
// about
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ValidationError {
    // the retiree's name, for a value that belongs to one retiree
    pub person: Option<String>,
    // the field's path in the plan file: a retiree's field such as
    // "retirement_age", or for example "expenses.monthly" or
    // "portfolio.pre_retirement_allocation"
    pub field: String,
    // what's wrong, in plain words without the person or field name, such as
    // "can't be negative"
    pub message: String,
}

// the way the command line program shows it, for example "John Doe:
// retirement_age can't be more than 120 (currently 121)"
impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(person) = &self.person {
            write!(f, "{person}: ")?;
        }
        write!(f, "{} {}", self.field, self.message)
    }
}

// the first problem, the way the command line program shows it
pub fn describe_first(errors: &[ValidationError]) -> String {
    errors.first().map(|e| e.to_string()).unwrap_or_default()
}

// collects the problems found while validating
#[derive(Default)]
struct Errors(Vec<ValidationError>);

impl Errors {
    // adds the problem with a field, if there is one
    fn add(&mut self, person: Option<&str>, field: &str, problem: Option<String>) {
        if let Some(message) = problem {
            self.0.push(ValidationError {
                person: person.map(str::to_string),
                field: field.to_string(),
                message,
            });
        }
    }

    fn into_result(self) -> Result<(), Vec<ValidationError>> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(self.0)
        }
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
pub const MAX_YIELD: f64 = 20.0;
pub const MAX_PENSION_COLA: f64 = 20.0;

fn validate_retiree(retiree: &Retiree, start_date: &NaiveDate, errors: &mut Errors) {
    let person = Some(retiree.name.as_str());
    let mut add = |field: &str, problem: Option<String>| errors.add(person, field, problem);

    if retiree.date_of_birth > *start_date {
        add("date_of_birth", Some("is in the future".to_string()));
    }
    let ages = [
        (retiree.retirement_age, "retirement_age"),
        (retiree.life_expectancy, "life_expectancy"),
        (retiree.social_security_age, "social_security_age"),
        (retiree.pension_age, "pension_age"),
    ];
    for (age, field) in ages {
        if age > MAX_AGE {
            add(
                field,
                Some(format!("can't be more than {MAX_AGE} (currently {age})")),
            );
        }
    }
    if retiree.social_security_age < MIN_SOCIAL_SECURITY_AGE {
        add(
            "social_security_age",
            Some(format!(
                "must be at least {MIN_SOCIAL_SECURITY_AGE} (currently {})",
                retiree.social_security_age
            )),
        );
    }
    let amounts = [
        (retiree.salary_annual, "wage_annual_salary"),
        (retiree.pension_monthly_income, "pension_monthly_income"),
        (
            retiree.other_monthly_retirement_income,
            "other_monthly_retirement_income",
        ),
    ];
    for (amount, field) in amounts {
        add(field, amount_problem(amount));
    }
    match retiree.social_security_amount_full {
        Some(amount) => add("social_security_amount_full", amount_problem(amount)),
        // never estimate 0 without saying so, e.g. for someone already
        // retired
        None if retiree.social_security_full() == 0.0 => add(
            "social_security_amount_full",
            Some(
                "can't be estimated without wage_annual_salary; enter the amount \
                 from the social security web site, or 0.0 if there is none"
                    .to_string(),
            ),
        ),
        None => {}
    }
    if let Some(cola) = retiree.pension_cola_percent {
        add(
            "pension_cola_percent",
            range_problem(cola, 0.0, MAX_PENSION_COLA),
        );
    }
    add(
        "retirement_contribution_percent",
        percent_problem(retiree.retirement_contribution_percent),
    );
}

// the problem with a value outside the range (inclusive), if any. NaN is
// outside every range.
fn range_problem(value: f64, min: f64, max: f64) -> Option<String> {
    (!(min..=max).contains(&value))
        .then(|| format!("must be between {min} and {max} (currently {value})"))
}

// the problem with a value that must be from 0 to max, if any
fn positive_problem(value: f64, max: f64) -> Option<String> {
    if value < 0.0 {
        return Some("can't be negative".to_string());
    }
    range_problem(value, 0.0, max)
}

fn amount_problem(value: f64) -> Option<String> {
    positive_problem(value, MAX_AMOUNT)
}

fn percent_problem(value: f64) -> Option<String> {
    positive_problem(value, 100.0)
}

fn validate_forecasts(forecasts: &Forecasts, errors: &mut Errors) {
    let found_before = errors.0.len();
    let field = |name: &str| format!("portfolio.forecasts.{name}");

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
        let problem = range_problem(value, MIN_EXPECTED_RETURN, MAX_EXPECTED_RETURN);
        errors.add(None, &field(name), problem);
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
        let problem = positive_problem(value, MAX_STANDARD_DEVIATION);
        errors.add(None, &field(name), problem);
    }
    let yields = [
        (
            forecasts.us_equity_dividend_yield,
            "us_equity_dividend_yield",
        ),
        (
            forecasts.international_equity_dividend_yield,
            "international_equity_dividend_yield",
        ),
        (forecasts.bonds_interest_yield, "bonds_interest_yield"),
    ];
    for (value, name) in yields {
        errors.add(None, &field(name), positive_problem(value, MAX_YIELD));
    }
    let correlations = [
        (
            forecasts.us_international_correlation,
            "us_international_correlation",
        ),
        (forecasts.us_bonds_correlation, "us_bonds_correlation"),
        (
            forecasts.international_bonds_correlation,
            "international_bonds_correlation",
        ),
    ];
    for (value, name) in correlations {
        errors.add(None, &field(name), range_problem(value, -1.0, 1.0));
    }
    // correlations that are each in range can still be inconsistent with
    // each other. Checked only once everything above is fine.
    if errors.0.len() == found_before && monte_carlo::ReturnsSampler::new(forecasts).is_err() {
        errors.add(
            None,
            "portfolio.forecasts",
            Some("has correlations that are inconsistent with each other".to_string()),
        );
    }
}

// a friendlier message for an empty file, otherwise serde's error, which
// names the field and line
fn explain_parse_error(text: &str, error: serde_yaml_ng::Error) -> String {
    match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(text) {
        Ok(serde_yaml_ng::Value::Null) => "input file is empty".to_string(),
        _ => format!("Invalid input: {error}"),
    }
}

// the problem with an allocation's percentages, if any
fn allocation_problem(allocation: &Allocation) -> Option<String> {
    let values = [
        allocation.us_equities,
        allocation.international,
        allocation.bonds,
    ];
    if values.iter().any(|&v| v < 0.0) {
        return Some("percentages can't be negative".to_string());
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Some("percentages must be numbers".to_string());
    }
    let total: f64 = values.iter().sum();
    if (total - 100.0).abs() > 0.01 {
        return Some(format!("must add up to 100.0 (currently {total})"));
    }
    None
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
        // changing a commented-out line would change nothing
        let line = sample.lines().find(|line| line.contains(from)).unwrap();
        assert!(
            !line.trim_start().starts_with('#'),
            "{from} is commented out"
        );
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
    }

    // the sample input with its commented-out forecasts block turned on
    fn sample_with_forecasts() -> String {
        let mut in_block = false;
        let lines: Vec<String> = sample()
            .lines()
            .map(|line| {
                if line == "    # forecasts:" {
                    in_block = true;
                } else if !line.starts_with("    #") {
                    in_block = false;
                }
                // "    #     name: value" becomes "        name: value"
                match line.strip_prefix("    #") {
                    Some(rest) if in_block => {
                        format!("    {}", rest.strip_prefix(' ').unwrap_or(rest))
                    }
                    _ => line.to_string(),
                }
            })
            .collect();
        lines.join("\n")
    }

    // the sample input with its forecasts turned on and one piece of text
    // in them replaced
    fn parse_modified_forecasts(from: &str, to: &str) -> Result<Input, String> {
        let sample = sample_with_forecasts();
        assert!(
            sample.contains(from),
            "sample forecasts don't contain {from}"
        );
        Input::from_yaml(&sample.replacen(from, to, 1), start_date())
    }

    fn yaml(forecasts: &Forecasts) -> String {
        serde_yaml_ng::to_string(forecasts).unwrap()
    }

    #[test]
    fn test_sample_shows_the_built_in_forecasts() {
        // the sample leaves the forecasts out, so it uses the built-in ones
        let input = parse_sample();
        assert!(input.portfolio.forecasts.is_empty());
        assert_eq!(
            yaml(&input.portfolio.forecasts()),
            yaml(&Forecasts::default())
        );

        // and its commented-out block shows them all
        let input = Input::from_yaml(&sample_with_forecasts(), start_date()).unwrap();
        let saved = input.to_yaml().unwrap();
        for name in ["us_equity_expected_returns", "bonds_interest_yield"] {
            assert!(saved.contains(name), "{saved}");
        }
        assert_eq!(
            yaml(&input.portfolio.forecasts()),
            yaml(&Forecasts::default())
        );
    }

    #[test]
    fn test_forecasts_left_out_are_not_saved() {
        // otherwise a saved plan would keep this year's built-in forecasts
        let saved = parse_sample().to_yaml().unwrap();
        assert!(!saved.contains("forecasts"), "{saved}");
        let reread = Input::from_yaml(&saved, start_date()).unwrap();
        assert!(reread.portfolio.forecasts.is_empty());

        // a full block is kept as it is
        let input = Input::from_yaml(&sample_with_forecasts(), start_date()).unwrap();
        let saved = input.to_yaml().unwrap();
        let reread = Input::from_yaml(&saved, start_date()).unwrap();
        assert_eq!(reread.portfolio.forecasts, input.portfolio.forecasts);
        assert_eq!(reread.to_yaml().unwrap(), saved);
    }

    #[test]
    fn test_override_one_default() {
        let input = parse_modified_sample(
            "international: 24.0",
            "international: 24.0\n    forecasts:\n        bonds_expected_returns: 1.0",
        )
        .unwrap();
        let forecasts = input.portfolio.forecasts();
        assert_eq!(forecasts.bonds_expected_returns, 1.0);
        assert_eq!(
            forecasts.bonds_standard_deviation,
            defaults::BONDS_STANDARD_DEVIATION
        );
        // only the value given is saved
        let saved = input.to_yaml().unwrap();
        assert!(saved.contains("bonds_expected_returns: 1.0"), "{saved}");
        assert!(!saved.contains("bonds_standard_deviation"), "{saved}");
    }

    #[test]
    fn test_parse_accepts_integer_amounts() {
        let input = parse_modified_sample("balance: 700000.0", "balance: 700000").unwrap();
        assert_eq!(input.portfolio.pre_tax_balance, 700000.0);
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
        let input = parse_modified_forecasts(
            "us_international_correlation: 0.8",
            "us_international_correlation: 0.7",
        )
        .unwrap();
        let forecasts = input.portfolio.forecasts();
        assert_eq!(forecasts.us_international_correlation, 0.7);
        assert_eq!(forecasts.us_bonds_correlation, 0.3);
        assert_eq!(forecasts.international_bonds_correlation, 0.3);
    }

    #[test]
    fn test_each_correlation_has_a_default() {
        let input = parse_modified_forecasts(
            "us_international_correlation: 0.8",
            "us_bonds_correlation: 0.1",
        );
        // the sample now has us_bonds_correlation twice
        assert!(input.unwrap_err().contains("duplicate"));

        let input = parse_modified_forecasts("us_international_correlation: 0.8", "").unwrap();
        assert_eq!(
            input.portfolio.forecasts().us_international_correlation,
            defaults::US_INTERNATIONAL_CORRELATION
        );
    }

    #[test]
    fn test_parse_rejects_inconsistent_correlations() {
        let err =
            parse_modified_forecasts("us_bonds_correlation: 0.3", "us_bonds_correlation: -0.9")
                .unwrap_err();
        assert!(err.contains("inconsistent"), "{err}");
    }

    #[test]
    fn test_parse_rejects_negative_standard_deviation() {
        let err = parse_modified_forecasts(
            "bonds_standard_deviation: 6.0",
            "bonds_standard_deviation: -6.0",
        )
        .unwrap_err();
        assert!(
            err.contains("bonds_standard_deviation can't be negative"),
            "{err}"
        );
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
    // values that are awkward to write as text. Returns the first problem,
    // the way the command line program shows it.
    fn validate_modified(change: impl FnOnce(&mut Input)) -> Result<(), String> {
        validation_errors(change).map_err(|errors| describe_first(&errors))
    }

    // every problem with the sample input after changing it in code
    fn validation_errors(change: impl FnOnce(&mut Input)) -> Result<(), Vec<ValidationError>> {
        let mut input = parse_sample();
        change(&mut input);
        input.validate()
    }

    #[test]
    fn test_validation_requires_one_or_two_retirees() {
        let err = validate_modified(|input| input.retirees.clear()).unwrap_err();
        assert!(
            err.contains("retirees must list one or two people"),
            "{err}"
        );
        let err = validate_modified(|input| {
            let third = input.retirees[0].clone();
            input.retirees.push(third);
        })
        .unwrap_err();
        assert!(err.contains("one or two people (currently 3)"), "{err}");
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
        let err = parse_modified_forecasts(
            "bonds_expected_returns: 2.5",
            "bonds_expected_returns: -100.0",
        )
        .unwrap_err();
        assert!(
            err.contains("bonds_expected_returns must be between"),
            "{err}"
        );
        let err = parse_modified_forecasts(
            "us_equity_expected_returns: 3.2",
            "us_equity_expected_returns: 1000.0",
        )
        .unwrap_err();
        assert!(err.contains("us_equity_expected_returns"), "{err}");
        parse_modified_forecasts(
            "bonds_expected_returns: 2.5",
            "bonds_expected_returns: -3.0",
        )
        .unwrap();
    }

    #[test]
    fn test_validation_limits_standard_deviations() {
        let err = parse_modified_forecasts(
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
        assert!(err.contains("expenses.monthly can't be negative"), "{err}");
        let err = parse_modified_sample("balance: 700000.0", "balance: 1.0e300").unwrap_err();
        assert!(
            err.contains("portfolio.pre_tax_balance must be between"),
            "{err}"
        );
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
            ("balance: 700000.0", "balance: .nan"),
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
                parse_modified_forecasts(from, to).is_err(),
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
        assert_eq!(input.retirees[0].social_security_amount_full, Some(3870.0));
        assert_eq!(input.retirees[0].social_security_full(), 3870.0);
        assert!(!input.retirees[0].social_security_is_estimated());
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
    fn test_social_security_estimated_when_left_out() {
        // John Doe earns $75,000 and retires at 67
        let input = parse_modified_sample("social_security_amount_full: 3870.0", "").unwrap();
        let john = &input.retirees[0];
        assert!(john.social_security_is_estimated());
        assert_eq!(john.social_security_full(), 2777.2);

        // the estimate isn't saved, so it follows changes to the salary.
        // Only Jane Doe's amount is written.
        let saved = input.to_yaml().unwrap();
        assert_eq!(saved.matches("social_security_amount_full").count(), 1);
        let reread = Input::from_yaml(&saved, start_date()).unwrap();
        assert!(reread.retirees[0].social_security_is_estimated());
    }

    #[test]
    fn test_social_security_needs_a_salary_to_estimate() {
        let err = validate_modified(|input| {
            input.retirees[0].social_security_amount_full = None;
            input.retirees[0].salary_annual = 0.0;
        })
        .unwrap_err();
        assert!(
            err.contains("John Doe: social_security_amount_full can't be estimated"),
            "{err}"
        );
        // an amount entered is used as it is, including 0 for someone who
        // never worked
        validate_modified(|input| {
            input.retirees[0].social_security_amount_full = Some(0.0);
            input.retirees[0].salary_annual = 0.0;
        })
        .unwrap();
    }

    #[test]
    fn test_parse_taxable_account() {
        let input = parse_sample();
        assert_eq!(input.portfolio.taxable_balance, 200000.0);
        assert_eq!(input.portfolio.taxable_cost_basis, Some(140000.0));
        // and saved again
        let yaml = input.to_yaml().unwrap();
        assert!(yaml.contains("taxable_cost_basis: 140000.0"), "{yaml}");

        // both optional
        let sample = sample();
        let lines: Vec<&str> = sample
            .lines()
            .filter(|line| !line.contains("taxable_") && !line.contains("# the gain is taxed"))
            .collect();
        let input = Input::from_yaml(&lines.join("\n"), start_date()).unwrap();
        assert_eq!(input.portfolio.taxable_balance, 0.0);
        assert_eq!(input.portfolio.taxable_cost_basis, None);
        assert!(!input.to_yaml().unwrap().contains("taxable_cost_basis"));

        let err = parse_modified_sample("taxable_cost_basis: 140000.0", "taxable_cost_basis: -1.0")
            .unwrap_err();
        assert!(
            err.contains("taxable_cost_basis can't be negative"),
            "{err}"
        );
    }

    #[test]
    fn test_pension_cola() {
        // optional: left out, the pension keeps up with inflation
        let input = parse_sample();
        assert_eq!(input.retirees[1].pension_cola_percent, None);
        assert!(!input
            .to_yaml()
            .unwrap()
            .contains("pension_cola_percent: null"));

        let err = validate_modified(|input| {
            input.retirees[0].pension_cola_percent = Some(-1.0);
        })
        .unwrap_err();
        assert!(err.contains("John Doe: pension_cola_percent"), "{err}");
        let err = validate_modified(|input| {
            input.retirees[0].pension_cola_percent = Some(25.0);
        })
        .unwrap_err();
        assert!(err.contains("John Doe: pension_cola_percent"), "{err}");
    }

    #[test]
    fn test_validation_limits_yields() {
        let err = validate_modified(|input| {
            input.portfolio.forecasts.bonds_interest_yield = Some(25.0);
        })
        .unwrap_err();
        assert!(
            err.contains("bonds_interest_yield must be between"),
            "{err}"
        );
        let err = validate_modified(|input| {
            input.portfolio.forecasts.us_equity_dividend_yield = Some(-1.0);
        })
        .unwrap_err();
        assert!(err.contains("us_equity_dividend_yield"), "{err}");
    }

    #[test]
    fn test_validation_uses_start_date() {
        // John Doe (born 1960) and Jane Doe (born 1964) both have a life
        // expectancy of 90, so by 2056 they are both past it
        let late = NaiveDate::from_ymd_opt(2056, 1, 1).unwrap();
        let err = Input::from_yaml(&sample(), late).unwrap_err();
        assert!(err.contains("past their life expectancy"), "{err}");
    }

    // a change to the sample input, for tables of test cases
    type Change = Box<dyn FnOnce(&mut Input)>;

    // (person, field, message) of each problem, to compare in tests
    fn summarize(errors: &[ValidationError]) -> Vec<(Option<&str>, &str, &str)> {
        errors
            .iter()
            .map(|e| (e.person.as_deref(), e.field.as_str(), e.message.as_str()))
            .collect()
    }

    #[test]
    fn test_validation_errors_name_the_person_and_field() {
        let john = Some("John Doe");
        let cases: Vec<(Change, _)> = vec![
            (
                Box::new(|input: &mut Input| input.retirees.clear()),
                (
                    None,
                    "retirees",
                    "must list one or two people (currently 0)",
                ),
            ),
            (
                Box::new(|input: &mut Input| {
                    input.retirees[0].date_of_birth = NaiveDate::from_ymd_opt(2060, 1, 1).unwrap()
                }),
                (john, "date_of_birth", "is in the future"),
            ),
            (
                Box::new(|input: &mut Input| input.retirees[0].retirement_age = 121),
                (
                    john,
                    "retirement_age",
                    "can't be more than 120 (currently 121)",
                ),
            ),
            (
                Box::new(|input: &mut Input| input.retirees[1].social_security_age = 61),
                (
                    Some("Jane Doe"),
                    "social_security_age",
                    "must be at least 62 (currently 61)",
                ),
            ),
            (
                Box::new(|input: &mut Input| input.retirees[0].salary_annual = -1.0),
                (john, "wage_annual_salary", "can't be negative"),
            ),
            (
                Box::new(|input: &mut Input| input.retirees[0].pension_cola_percent = Some(25.0)),
                (
                    john,
                    "pension_cola_percent",
                    "must be between 0 and 20 (currently 25)",
                ),
            ),
            (
                Box::new(|input: &mut Input| {
                    input.retirees[0].retirement_contribution_percent = 150.0
                }),
                (
                    john,
                    "retirement_contribution_percent",
                    "must be between 0 and 100 (currently 150)",
                ),
            ),
            (
                Box::new(|input: &mut Input| input.portfolio.taxable_cost_basis = Some(-1.0)),
                (None, "portfolio.taxable_cost_basis", "can't be negative"),
            ),
            (
                Box::new(|input: &mut Input| input.portfolio.pre_retirement_allocation.bonds = 0.0),
                (
                    None,
                    "portfolio.pre_retirement_allocation",
                    "must add up to 100.0 (currently 80)",
                ),
            ),
            (
                Box::new(|input: &mut Input| {
                    input.portfolio.forecasts.bonds_expected_returns = Some(-100.0)
                }),
                (
                    None,
                    "portfolio.forecasts.bonds_expected_returns",
                    "must be between -50 and 50 (currently -100)",
                ),
            ),
            (
                Box::new(|input: &mut Input| {
                    input.portfolio.forecasts.bonds_standard_deviation = Some(-6.0)
                }),
                (
                    None,
                    "portfolio.forecasts.bonds_standard_deviation",
                    "can't be negative",
                ),
            ),
            (
                Box::new(|input: &mut Input| {
                    input.portfolio.forecasts.us_bonds_correlation = Some(1.5)
                }),
                (
                    None,
                    "portfolio.forecasts.us_bonds_correlation",
                    "must be between -1 and 1 (currently 1.5)",
                ),
            ),
            (
                Box::new(|input: &mut Input| {
                    input.portfolio.forecasts.us_bonds_correlation = Some(-0.9)
                }),
                (
                    None,
                    "portfolio.forecasts",
                    "has correlations that are inconsistent with each other",
                ),
            ),
            (
                Box::new(|input: &mut Input| input.expenses.monthly = f64::INFINITY),
                (
                    None,
                    "expenses.monthly",
                    "must be between 0 and 1000000000000 (currently inf)",
                ),
            ),
        ];
        for (change, expected) in cases {
            let errors = validation_errors(change).unwrap_err();
            assert_eq!(summarize(&errors), vec![expected]);
        }
    }

    #[test]
    fn test_validation_collects_every_problem() {
        let errors = validation_errors(|input| {
            input.expenses.survivor_percent = 101.0;
            input.portfolio.roth_balance = -1.0;
            input.retirees[1].life_expectancy = 121;
            input.retirees[0].pension_monthly_income = f64::NAN;
        })
        .unwrap_err();
        // in the order of the input file
        assert_eq!(
            summarize(&errors),
            vec![
                (
                    Some("John Doe"),
                    "pension_monthly_income",
                    "must be between 0 and 1000000000000 (currently NaN)"
                ),
                (
                    Some("Jane Doe"),
                    "life_expectancy",
                    "can't be more than 120 (currently 121)"
                ),
                (None, "portfolio.roth_balance", "can't be negative"),
                (
                    None,
                    "expenses.survivor_percent",
                    "must be between 0 and 100 (currently 101)"
                ),
            ]
        );
    }

    #[test]
    fn test_inconsistent_correlations_checked_only_when_the_rest_is_fine() {
        // the correlation check can't be trusted with a bad standard
        // deviation, so only the standard deviation is reported
        let errors = validation_errors(|input| {
            input.portfolio.forecasts.us_bonds_correlation = Some(-0.9);
            input.portfolio.forecasts.bonds_standard_deviation = Some(500.0);
        })
        .unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].field,
            "portfolio.forecasts.bonds_standard_deviation"
        );
    }

    #[test]
    fn test_first_error_text() {
        // the command line program shows the first problem, with the person
        // and field in front
        let cases: Vec<(Change, &str)> = vec![
            (
                Box::new(|input: &mut Input| {
                    input.retirees[0].retirement_age = 121;
                    input.expenses.monthly = -1.0;
                }),
                "John Doe: retirement_age can't be more than 120 (currently 121)",
            ),
            (
                Box::new(|input: &mut Input| input.portfolio.pre_tax_balance = -1.0),
                "portfolio.pre_tax_balance can't be negative",
            ),
            (
                Box::new(|input: &mut Input| {
                    input.portfolio.post_retirement_allocation.bonds = f64::NAN
                }),
                "portfolio.post_retirement_allocation percentages must be numbers",
            ),
            (
                Box::new(|input: &mut Input| {
                    input.portfolio.forecasts.bonds_interest_yield = Some(25.0)
                }),
                "portfolio.forecasts.bonds_interest_yield must be between 0 and 20 (currently 25)",
            ),
            (
                Box::new(|input: &mut Input| {
                    input.portfolio.forecasts.bonds_standard_deviation = Some(-6.0)
                }),
                "portfolio.forecasts.bonds_standard_deviation can't be negative",
            ),
            (
                Box::new(|input: &mut Input| input.expenses.survivor_percent = -1.0),
                "expenses.survivor_percent can't be negative",
            ),
            (
                Box::new(|input: &mut Input| {
                    input.retirees[0].social_security_amount_full = None;
                    input.retirees[0].salary_annual = 0.0;
                }),
                "John Doe: social_security_amount_full can't be estimated without \
                 wage_annual_salary; enter the amount from the social security web site, \
                 or 0.0 if there is none",
            ),
        ];
        for (change, expected) in cases {
            assert_eq!(validate_modified(change).unwrap_err(), expected);
        }
        // and so does the runner, for a plan built in code
        let mut input = parse_sample();
        input.retirees.clear();
        let err = crate::runner::run_all(&input, 42).err().unwrap();
        assert_eq!(err, "retirees must list one or two people (currently 0)");
    }

    #[test]
    fn test_validation_errors_as_json() {
        let errors = validation_errors(|input| {
            input.retirees[0].retirement_age = 121;
            input.expenses.monthly = -1.0;
        })
        .unwrap_err();
        let json = serde_json::to_value(&errors).unwrap();
        assert_eq!(
            json,
            serde_json::json!([
                {
                    "person": "John Doe",
                    "field": "retirement_age",
                    "message": "can't be more than 120 (currently 121)"
                },
                {
                    "person": null,
                    "field": "expenses.monthly",
                    "message": "can't be negative"
                }
            ])
        );
    }
}
