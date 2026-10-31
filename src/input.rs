/**************************************************************************
* input.rs
*
* The retirement plan entered by the user: parsed from YAML (or JSON, which
* is a subset of YAML) and validated.
**************************************************************************/

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::portfolio::{Allocation, Portfolio};
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
    pub social_security_age: u32,
    pub pension_age: u32,
    pub pension_monthly_income: f64,
    pub other_monthly_retirement_income: f64,
    pub social_security_amount_early: f64,
    pub social_security_amount_full: f64,
    pub social_security_amount_delayed: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expenses {
    pub monthly: f64,
    // percentage of expenses that remain after one spouse dies
    pub survivor_percent: f64,
}

// a tax bracket: income is the lower bound of taxable income (after the
// standard deduction) that is taxed at rate (percentage)
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxLevel {
    pub income: f64,
    pub rate: f64,
}

// tax brackets, deductions, and social security thresholds for one filing
// status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxTable {
    pub standard_deduction: f64,
    // added to the standard deduction for each retiree 65 or older
    pub additional_deduction_65_or_older: f64,
    // provisional income (other income plus half of social security) above
    // which up to 50%, then up to 85%, of social security is taxable. These
    // are fixed by law and not adjusted for inflation.
    pub social_security_base: f64,
    pub social_security_adjusted_base: f64,
    // sorted by income once validated
    #[serde(rename = "levels")]
    pub tax_levels: Vec<TaxLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxRates {
    // used to shrink the social security thresholds in today's dollars
    pub expected_inflation: f64,
    pub married_filing_jointly: TaxTable,
    pub single: TaxTable,
}

impl TaxRates {
    // married filing jointly while two retirees are alive, otherwise single
    pub fn table_for(&self, num_alive: usize) -> &TaxTable {
        if num_alive >= 2 {
            &self.married_filing_jointly
        } else {
            &self.single
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub retirees: Vec<Retiree>,
    pub portfolio: Portfolio,
    pub expenses: Expenses,
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

    // checks that the values make sense, and sorts the tax brackets
    pub fn validate(&mut self) -> Result<(), String> {
        if self.retirees.is_empty() {
            return Err("at least one retiree is required".to_string());
        }
        if self
            .retirees
            .iter()
            .all(|r| utils::get_age(&r.date_of_birth, &self.start_date) > r.life_expectancy)
        {
            return Err("every retiree is already past their life expectancy".to_string());
        }

        let portfolio = &self.portfolio;
        if portfolio.pre_tax_balance < 0.0 || portfolio.roth_balance < 0.0 {
            return Err("portfolio balances can't be negative".to_string());
        }
        validate_allocation(
            &portfolio.pre_retirement_allocation,
            "pre_retirement_allocation",
        )?;
        validate_allocation(
            &portfolio.post_retirement_allocation,
            "post_retirement_allocation",
        )?;
        let standard_deviations = [
            portfolio.us_equity_standard_deviation,
            portfolio.international_equity_standard_deviation,
            portfolio.bonds_standard_deviation,
        ];
        if standard_deviations.iter().any(|&v| v < 0.0) {
            return Err("standard deviations can't be negative".to_string());
        }
        monte_carlo::ReturnsSampler::new(portfolio)?;

        if self.expenses.monthly < 0.0 || self.expenses.survivor_percent < 0.0 {
            return Err("expenses can't be negative".to_string());
        }

        self.tax_rates.validate()
    }
}

// a friendlier message for some common mistakes, otherwise serde's error,
// which names the field and line
fn explain_parse_error(text: &str, error: serde_yaml_ng::Error) -> String {
    match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(text) {
        Ok(serde_yaml_ng::Value::Null) => "input file is empty".to_string(),
        Ok(value) if !value["tax_rates"]["levels"].is_null() => {
            "tax_rates must now contain separate married_filing_jointly and single \
             blocks (see input.yaml)"
                .to_string()
        }
        _ => format!("Invalid input: {error}"),
    }
}

impl TaxRates {
    fn validate(&mut self) -> Result<(), String> {
        if self.expected_inflation <= -100.0 {
            return Err("expected_inflation in tax_rates must be more than -100.0".to_string());
        }
        self.married_filing_jointly
            .validate("married_filing_jointly")?;
        self.single.validate("single")
    }
}

impl TaxTable {
    // name is the filing status, for error messages
    fn validate(&mut self, name: &str) -> Result<(), String> {
        if self.standard_deduction < 0.0 || self.additional_deduction_65_or_older < 0.0 {
            return Err(format!("deductions in tax_rates {name} can't be negative"));
        }
        if self.social_security_base < 0.0
            || self.social_security_adjusted_base < self.social_security_base
        {
            return Err(format!(
                "social_security_adjusted_base in tax_rates {name} must be at least \
                 social_security_base, and both must be 0.0 or more"
            ));
        }

        // each level is the lower bound of a bracket, so sort by income and
        // make sure the brackets cover all income starting from 0
        let tax_levels = &mut self.tax_levels;
        tax_levels.sort_by(|a, b| a.income.total_cmp(&b.income));
        match tax_levels.first() {
            None => return Err(format!("no tax rates found in {name}")),
            Some(first) if first.income != 0.0 => {
                return Err(format!(
                    "first tax level in {name} must have income 0.0 (the lowest bracket)"
                ));
            }
            _ => {}
        }
        for pair in tax_levels.windows(2) {
            if pair[0].income == pair[1].income {
                return Err(format!(
                    "duplicate tax level in {name} for income {}",
                    pair[0].income
                ));
            }
        }
        // each extra dollar of income can make up to 85 cents more of social
        // security taxable, so the tax on a withdrawal can be 1.85 times the
        // rate. Rates must stay low enough that this is less than 100%.
        if tax_levels
            .iter()
            .any(|level| level.rate < 0.0 || level.rate >= 50.0)
        {
            return Err(format!("tax rates in {name} must be between 0.0 and 50.0"));
        }

        Ok(())
    }
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
    let total: f64 = values.iter().sum();
    if (total - 100.0).abs() > 0.01 {
        return Err(format!("{name} must add up to 100.0 (currently {total})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn parse_levels(levels_yaml: &str) -> Result<TaxTable, String> {
        let yaml = format!(
            "standard_deduction: 32200.0\nadditional_deduction_65_or_older: 1650.0\n\
             social_security_base: 32000.0\nsocial_security_adjusted_base: 44000.0\n\
             levels:\n{levels_yaml}"
        );
        let mut table: TaxTable = serde_yaml_ng::from_str(&yaml).unwrap();
        table.validate("single")?;
        Ok(table)
    }

    #[test]
    fn test_parse_tax_rates_sorts_levels() {
        let tax_rates = parse_levels(
            "  - {income: 24800.0, rate: 12.0}\n\
             \x20 - {income: 0.0, rate: 10.0}\n\
             \x20 - {income: 100800.0, rate: 22.0}\n",
        )
        .unwrap();
        assert_eq!(tax_rates.standard_deduction, 32200.0);
        let levels: Vec<(f64, f64)> = tax_rates
            .tax_levels
            .iter()
            .map(|l| (l.income, l.rate))
            .collect();
        assert_eq!(levels, vec![(0.0, 10.0), (24800.0, 12.0), (100800.0, 22.0)]);
    }

    #[test]
    fn test_parse_tax_rates_requires_zero_level() {
        let err = parse_levels("  - {income: 24800.0, rate: 12.0}\n").unwrap_err();
        assert!(err.contains("income 0.0"), "{err}");
    }

    #[test]
    fn test_parse_tax_rates_rejects_duplicates() {
        let err = parse_levels(
            "  - {income: 0.0, rate: 10.0}\n\
             \x20 - {income: 0.0, rate: 12.0}\n",
        )
        .unwrap_err();
        assert!(err.contains("duplicate"), "{err}");
    }

    #[test]
    fn test_parse_tax_rates_rejects_bad_rate() {
        let err = parse_levels("  - {income: 0.0, rate: 120.0}\n").unwrap_err();
        assert!(err.contains("between"), "{err}");
    }

    #[test]
    fn test_parse_sample_input() {
        let input = parse_sample();
        let married = &input.tax_rates.married_filing_jointly;
        let single = &input.tax_rates.single;
        assert_eq!(married.tax_levels.len(), 7);
        assert_eq!(single.tax_levels.len(), 7);
        assert_eq!(married.tax_levels[0].income, 0.0);
        assert_eq!(single.tax_levels[0].income, 0.0);
        assert!(single.standard_deduction < married.standard_deduction);
        assert_eq!(input.expenses.survivor_percent, 100.0);
        assert_eq!(input.retirees[0].life_expectancy, 90);
        assert_eq!(
            input.retirees[0].date_of_birth,
            NaiveDate::from_ymd_opt(1960, 9, 30).unwrap()
        );
        assert_eq!(input.start_date, start_date());
    }

    #[test]
    fn test_tax_table_for_filing_status() {
        let input = parse_sample();
        let rates = &input.tax_rates;
        assert!(std::ptr::eq(
            rates.table_for(2),
            &rates.married_filing_jointly
        ));
        assert!(std::ptr::eq(rates.table_for(1), &rates.single));
    }

    #[test]
    fn test_parse_rejects_old_tax_format() {
        let yaml = "tax_rates:\n  standard_deduction: 32200.0\n  levels:\n    - {income: 0.0, rate: 10.0}\n";
        let err = Input::from_yaml(yaml, start_date()).unwrap_err();
        assert!(err.contains("married_filing_jointly and single"), "{err}");
    }

    #[test]
    fn test_parse_requires_both_tax_tables() {
        let sample = sample();
        let without_single = sample.split("    single:").next().unwrap();
        let err = Input::from_yaml(without_single, start_date()).unwrap_err();
        assert!(err.contains("missing field `single`"), "{err}");
    }

    #[test]
    fn test_parse_accepts_integer_amounts() {
        let input = parse_modified_sample("balance: 900000.0", "balance: 900000").unwrap();
        assert_eq!(input.portfolio.pre_tax_balance, 900000.0);
    }

    #[test]
    fn test_parse_social_security_tax_settings() {
        let input = parse_sample();
        let rates = &input.tax_rates;
        assert_eq!(rates.expected_inflation, 2.5);
        assert_eq!(rates.married_filing_jointly.social_security_base, 32000.0);
        assert_eq!(
            rates.married_filing_jointly.social_security_adjusted_base,
            44000.0
        );
        assert_eq!(rates.single.social_security_base, 25000.0);
        assert_eq!(rates.single.additional_deduction_65_or_older, 2050.0);
    }

    #[test]
    fn test_parse_requires_social_security_tax_settings() {
        let err = parse_modified_sample("expected_inflation: 2.5", "").unwrap_err();
        assert!(err.contains("expected_inflation"), "{err}");
        let err =
            parse_modified_sample("additional_deduction_65_or_older: 2050.0", "").unwrap_err();
        assert!(err.contains("tax_rates.single"), "{err}");
        assert!(err.contains("additional_deduction_65_or_older"), "{err}");
    }

    #[test]
    fn test_parse_rejects_adjusted_base_below_base() {
        let err = parse_modified_sample(
            "social_security_adjusted_base: 44000.0",
            "social_security_adjusted_base: 30000.0",
        )
        .unwrap_err();
        assert!(err.contains("social_security_adjusted_base"), "{err}");
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
        assert_eq!(input.portfolio.us_international_correlation, 0.7);
        assert_eq!(input.portfolio.us_bonds_correlation, 0.3);
        assert_eq!(input.portfolio.international_bonds_correlation, 0.3);
    }

    #[test]
    fn test_parse_requires_each_correlation() {
        let sample = sample();
        for name in [
            "us_international_correlation",
            "us_bonds_correlation",
            "international_bonds_correlation",
        ] {
            let without: Vec<&str> = sample
                .lines()
                .filter(|line| !line.contains(&format!("{name}:")))
                .collect();
            let without = without.join("\n");
            let err = Input::from_yaml(&without, start_date()).unwrap_err();
            assert!(err.contains(name), "{err}");
        }
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

    #[test]
    fn test_validation_uses_start_date() {
        // John Doe (born 1960) and Jane Doe (born 1964) both have a life
        // expectancy of 90, so by 2056 they are both past it
        let late = NaiveDate::from_ymd_opt(2056, 1, 1).unwrap();
        let err = Input::from_yaml(&sample(), late).unwrap_err();
        assert!(err.contains("past their life expectancy"), "{err}");
    }
}
