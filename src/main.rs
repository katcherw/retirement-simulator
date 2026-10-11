/**************************************************************************
* retirement-simulator
*
* Parses config file and runs simulations.
**************************************************************************/

use chrono::NaiveDate;
use num_format::{Locale, ToFormattedString};
use std::env;
use std::fs;
use std::process;
use yaml_rust::YamlLoader;

use crate::historical_scan::HistoricalScan;
use crate::monte_carlo::MonteCarloScan;
use crate::portfolio::Portfolio;

mod historical_scan;
mod monte_carlo;
mod portfolio;
mod scan;
mod simulate;
mod utils;

///////////////////////////////////////////////////////////////////////////
// Parsing input
///////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
struct Retiree {
    name: String,
    date_of_birth: NaiveDate,
    retirement_age: u32,
    life_expectency: u32,
    salary_annual: f32,
    retirement_contribution_percent: f32,
    social_security_age: u32,
    pension_age: u32,
    pension_monthly_income: f32,
    other_monthly_retirement_income: f32,
    social_security_amount_early: f32,
    social_security_amount_full: f32,
    social_security_amount_delayed: f32,
}

#[derive(Debug)]
struct Expenses {
    monthly: f32,
    // percentage of expenses that remain after one spouse dies
    survivor_percent: f32,
}

// a tax bracket: income is the lower bound of taxable income (after the
// standard deduction) that is taxed at rate (percentage)
#[derive(Debug, Copy, Clone)]
pub struct TaxLevel {
    income: f32,
    rate: f32,
}

// tax brackets and standard deduction for one filing status
#[derive(Debug)]
pub struct TaxTable {
    standard_deduction: f32,
    tax_levels: Vec<TaxLevel>,
}

#[derive(Debug)]
pub struct TaxRates {
    married_filing_jointly: TaxTable,
    single: TaxTable,
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

#[derive(Debug)]
struct Input {
    retirees: Vec<Retiree>,
    portfolio: Portfolio,
    expenses: Expenses,
    tax_rates: TaxRates,
}

fn parse_string(yaml: &yaml_rust::Yaml, field_name: &str) -> Result<String, String> {
    let value = yaml[field_name]
        .as_str()
        .ok_or("Invalid value: ".to_string() + field_name)?;

    Ok(value.to_string())
}

fn parse_u32(yaml: &yaml_rust::Yaml, field_name: &str) -> Result<u32, String> {
    let value = yaml[field_name]
        .as_i64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or("Invalid value: ".to_string() + field_name)?;

    Ok(value)
}

// accepts both floating point (3140.0) and integer (3140) values
fn parse_f32(yaml: &yaml_rust::Yaml, field_name: &str) -> Result<f32, String> {
    let value = yaml[field_name]
        .as_f64()
        .or_else(|| yaml[field_name].as_i64().map(|v| v as f64))
        .ok_or("Invalid value: ".to_string() + field_name)?;

    Ok(value as f32)
}

fn parse_allocation(input_yaml: &yaml_rust::Yaml) -> Result<portfolio::Allocation, String> {
    let us_equities = parse_f32(input_yaml, "us_equities")?;
    let international = parse_f32(input_yaml, "international")?;
    let bonds = parse_f32(input_yaml, "bonds")?;

    let allocation = portfolio::Allocation {
        us_equities,
        international,
        bonds,
    };

    Ok(allocation)
}

fn parse_portfolio(input_yaml: &yaml_rust::Yaml) -> Result<Portfolio, String> {
    let block = &input_yaml["portfolio"];
    if block.is_badvalue() {
        return Err("portfolio block missing".to_string());
    }

    let balance = parse_f32(block, "balance")?;

    let pre_retirement_block = &block["pre-retirement_allocation"];
    if pre_retirement_block.is_badvalue() {
        return Err("pre-retirement portfolio block missing".to_string());
    }
    let pre_retirement_allocation = parse_allocation(pre_retirement_block)?;

    let post_retirement_block = &block["post-retirement_allocation"];
    if post_retirement_block.is_badvalue() {
        return Err("post-retirement portfolio block missing".to_string());
    }
    let post_retirement_allocation = parse_allocation(post_retirement_block)?;

    let us_equity_expected_returns = parse_f32(block, "us_equity_expected_returns")?;
    let us_equity_standard_deviation = parse_f32(block, "us_equity_standard_deviation")?;
    let international_equity_expected_returns =
        parse_f32(block, "international_equity_expected_returns")?;
    let international_equity_standard_deviation =
        parse_f32(block, "international_equity_standard_deviation")?;
    let bonds_expected_returns = parse_f32(block, "bonds_expected_returns")?;
    let bonds_standard_deviation = parse_f32(block, "bonds_standard_deviation")?;

    let portfolio = Portfolio {
        balance,
        pre_retirement_allocation,
        post_retirement_allocation,
        us_equity_expected_returns,
        us_equity_standard_deviation,
        international_equity_expected_returns,
        international_equity_standard_deviation,
        bonds_expected_returns,
        bonds_standard_deviation,
    };

    Ok(portfolio)
}

fn parse_expenses(input_yaml: &yaml_rust::Yaml) -> Result<Expenses, String> {
    let block = &input_yaml["expenses"];
    if block.is_badvalue() {
        return Err("expenses block missing".to_string());
    }

    let monthly = parse_f32(block, "monthly")?;

    // optional: defaults to no change in expenses after a spouse dies
    let survivor_percent = if block["survivor_percent"].is_badvalue() {
        100.0
    } else {
        parse_f32(block, "survivor_percent")?
    };

    let expenses = Expenses {
        monthly,
        survivor_percent,
    };

    Ok(expenses)
}

fn parse_retiree(input_yaml: &yaml_rust::Yaml) -> Result<Retiree, String> {
    let name = parse_string(input_yaml, "name")?;
    let life_expectency = parse_u32(input_yaml, "life_expectency")?;
    let retirement_age = parse_u32(input_yaml, "retirement_age")?;

    let salary_annual = parse_f32(input_yaml, "wage_annual_salary")?;
    let retirement_contribution_percent = parse_f32(input_yaml, "retirement_contribution_percent")?;
    let social_security_age = parse_u32(input_yaml, "social_security_age")?;
    let pension_age = parse_u32(input_yaml, "pension_age")?;
    let pension_monthly_income = parse_f32(input_yaml, "pension_monthly_income")?;
    let other_monthly_retirement_income = parse_f32(input_yaml, "other_monthly_retirement_income")?;
    let social_security_amount_early = parse_f32(input_yaml, "social_security_amount_early")?;
    let social_security_amount_full = parse_f32(input_yaml, "social_security_amount_full")?;
    let social_security_amount_delayed = parse_f32(input_yaml, "social_security_amount_delayed")?;

    let date_of_birth = parse_string(input_yaml, "date_of_birth")?;
    let date_of_birth =
        NaiveDate::parse_from_str(&date_of_birth, "%m/%d/%Y").map_err(|_| "Invalid date")?;

    let retiree = Retiree {
        name,
        date_of_birth,
        life_expectency,
        retirement_age,
        salary_annual,
        retirement_contribution_percent,
        social_security_age,
        pension_age,
        pension_monthly_income,
        other_monthly_retirement_income,
        social_security_amount_early,
        social_security_amount_full,
        social_security_amount_delayed,
    };

    Ok(retiree)
}

fn parse_retirees(input_yaml: &yaml_rust::Yaml) -> Result<Vec<Retiree>, String> {
    let mut retirees = Vec::new();
    let block = &input_yaml["retirees"];
    if block.is_badvalue() {
        return Err("retirees block missing".to_string());
    }

    let vec = block.as_vec().ok_or("no retirees found")?;
    for element in vec {
        let retiree = parse_retiree(element);
        match retiree {
            Ok(v) => retirees.push(v),
            Err(e) => return Err(e),
        };
    }

    Ok(retirees)
}

fn parse_tax_rate(input_yaml: &yaml_rust::Yaml) -> Result<TaxLevel, String> {
    let income = parse_f32(input_yaml, "income")?;
    let rate = parse_f32(input_yaml, "rate")?;

    let tax_rate = TaxLevel { income, rate };

    Ok(tax_rate)
}

fn parse_tax_table(input_yaml: &yaml_rust::Yaml, name: &str) -> Result<TaxTable, String> {
    let mut tax_levels = Vec::new();
    let block = &input_yaml[name];
    if block.is_badvalue() {
        return Err(format!("tax_rates {name} block missing"));
    }

    let standard_deduction =
        parse_f32(block, "standard_deduction").map_err(|e| format!("{e} in tax_rates {name}"))?;

    let block = &block["levels"];
    if block.is_badvalue() {
        return Err(format!("levels block missing in tax_rates {name}"));
    }

    let vec = block
        .as_vec()
        .ok_or(format!("no tax rates found in {name}"))?;
    for element in vec {
        tax_levels.push(parse_tax_rate(element).map_err(|e| format!("{e} in tax_rates {name}"))?);
    }

    // each level is the lower bound of a bracket, so sort by income and make
    // sure the brackets cover all income starting from 0
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
    if tax_levels
        .iter()
        .any(|level| level.rate < 0.0 || level.rate >= 100.0)
    {
        return Err(format!("tax rates in {name} must be between 0.0 and 100.0"));
    }

    let tax_table = TaxTable {
        standard_deduction,
        tax_levels,
    };

    Ok(tax_table)
}

fn parse_tax_rates(input_yaml: &yaml_rust::Yaml) -> Result<TaxRates, String> {
    let block = &input_yaml["tax_rates"];
    if block.is_badvalue() {
        return Err("tax_rates block missing".to_string());
    }
    if !block["levels"].is_badvalue() {
        return Err(
            "tax_rates must now contain separate married_filing_jointly and single \
                    blocks (see input.yaml)"
                .to_string(),
        );
    }

    let tax_rates = TaxRates {
        married_filing_jointly: parse_tax_table(block, "married_filing_jointly")?,
        single: parse_tax_table(block, "single")?,
    };

    Ok(tax_rates)
}

fn validate_allocation(allocation: &portfolio::Allocation, name: &str) -> Result<(), String> {
    let values = [
        allocation.us_equities,
        allocation.international,
        allocation.bonds,
    ];
    if values.iter().any(|&v| v < 0.0) {
        return Err(format!("{name} percentages can't be negative"));
    }
    let total: f32 = values.iter().sum();
    if (total - 100.0).abs() > 0.01 {
        return Err(format!("{name} must add up to 100.0 (currently {total})"));
    }
    Ok(())
}

fn validate_input(input: &Input) -> Result<(), String> {
    if input.retirees.is_empty() {
        return Err("at least one retiree is required".to_string());
    }
    let today = chrono::Utc::now().naive_utc().date();
    if input
        .retirees
        .iter()
        .all(|r| utils::get_age(&r.date_of_birth, &today) > r.life_expectency)
    {
        return Err("every retiree is already past their life expectancy".to_string());
    }

    let portfolio = &input.portfolio;
    validate_allocation(
        &portfolio.pre_retirement_allocation,
        "pre-retirement_allocation",
    )?;
    validate_allocation(
        &portfolio.post_retirement_allocation,
        "post-retirement_allocation",
    )?;
    let standard_deviations = [
        portfolio.us_equity_standard_deviation,
        portfolio.international_equity_standard_deviation,
        portfolio.bonds_standard_deviation,
    ];
    if standard_deviations.iter().any(|&v| v < 0.0) {
        return Err("standard deviations can't be negative".to_string());
    }

    if input.expenses.monthly < 0.0 || input.expenses.survivor_percent < 0.0 {
        return Err("expenses can't be negative".to_string());
    }

    Ok(())
}

fn parse_input_str(file_str: &str) -> Result<Input, String> {
    let docs = YamlLoader::load_from_str(file_str).map_err(|e| format!("Invalid YAML: {e}"))?;
    let doc = docs.first().ok_or("input file is empty")?;

    let portfolio = parse_portfolio(doc)?;
    let expenses = parse_expenses(doc)?;
    let retirees = parse_retirees(doc)?;
    let tax_rates = parse_tax_rates(doc)?;

    let input = Input {
        retirees,
        portfolio,
        expenses,
        tax_rates,
    };

    validate_input(&input)?;
    Ok(input)
}

fn parse_input_file(fname: &str) -> Result<Input, String> {
    let file_str = fs::read_to_string(fname).map_err(|e| format!("Can't read {fname}: {e}"))?;
    parse_input_str(&file_str)
}

///////////////////////////////////////////////////////////////////////////
// Output results
///////////////////////////////////////////////////////////////////////////

fn format_table(table: Vec<Vec<String>>) -> String {
    if table.is_empty() {
        return "".to_string();
    }

    // find max len of each column
    let mut col_size: Vec<usize> = vec![0; table[0].len()];
    for row in table.iter() {
        for (i, cell) in row.iter().enumerate() {
            if cell.len() > col_size[i] {
                col_size[i] = cell.len();
            }
        }
    }

    // format table
    let mut str = String::new();
    for row in table.iter() {
        for (i, cell) in row.iter().enumerate() {
            str.push_str(&format!("{:>width$} ", cell, width = col_size[i]));
        }
        str.push('\n');
    }

    str
}

fn num_with_commas(num: u64) -> String {
    num.to_formatted_string(&Locale::en)
}

fn print_simulation_results(simulation_results: &simulate::SimulationResults) {
    let mut retire_printed = false;

    let mut table: Vec<Vec<String>> = Vec::new();

    let heading = vec![
        "".to_string(),
        "Year".to_string(),
        "Age".to_string(),
        "Balance".to_string(),
        "Expenses".to_string(),
        "Income".to_string(),
        "Tax".to_string(),
        "Rate".to_string(),
        "Draw".to_string(),
        "Yield".to_string(),
        "".to_string(),
    ];
    table.push(heading);

    for (i, monthly_snapshot) in simulation_results.monthly_snapshot.iter().enumerate() {
        if (i % 12) == 0 {
            let mut row: Vec<String> = Vec::new();

            let age = utils::get_age(
                &simulation_results.retirees[0].date_of_birth,
                &monthly_snapshot.date,
            );
            row.push((i / 12).to_string());
            row.push(monthly_snapshot.date.format("%Y").to_string());
            row.push(age.to_string());
            row.push(num_with_commas(monthly_snapshot.balance as u64));
            row.push(format!("{:.0}", monthly_snapshot.expenses));
            row.push(format!("{:.0}", monthly_snapshot.income));
            row.push(format!("{:.0}", monthly_snapshot.taxes));
            row.push(format!("{:.0}%", monthly_snapshot.tax_rate));
            row.push(format!("{:.2}%", monthly_snapshot.withdrawal_rate * 100.0));
            row.push(format!("{:.2}%", monthly_snapshot.annualized_return));
            if !retire_printed && (monthly_snapshot.date >= simulation_results.retirement_date) {
                row.push("Retired!".to_string());
                retire_printed = true;
            } else {
                row.push("".to_string());
            }

            table.push(row);
        }
    }

    println!("{}", format_table(table));

    println!("Average return: {:.2}%", simulation_results.average_return);
}

///////////////////////////////////////////////////////////////////////////
// Running simulations
///////////////////////////////////////////////////////////////////////////

fn run_scan<S: scan::Scannable>(
    input: &Input,
    scanner: &mut S,
) -> Result<scan::ScanResults, String> {
    let results = scanner.run_scan(input)?;

    println!(
        "Successful runs: {} of {} ({:.1}%)",
        results.num_successful,
        results.num_simulations,
        results.num_successful as f32 / (results.num_simulations as f32) * 100.0
    );
    println!(
        "Lowest ending balance: ${}",
        num_with_commas(results.min_balance as u64)
    );
    println!(
        "Highest ending balance: ${}",
        num_with_commas(results.max_balance as u64)
    );

    Ok(results)
}

fn print_historical_result_details(results: &scan::ScanResults) {
    println!();
    println!("Scenarios (sorted by worst to best):");
    for index in results.sorted_indices.iter() {
        println!(
            "    years {} to {}, ending balance ${}",
            results.scenario_results[*index].starting_year,
            results.scenario_results[*index].ending_year,
            num_with_commas(
                results.scenario_results[*index]
                    .simulation_results
                    .monthly_snapshot
                    .last()
                    .unwrap()
                    .balance as u64
            )
        );
    }

    let worst_index = results.sorted_indices[0];
    println!();
    println!(
        "Worst result was years {} to {}",
        results.scenario_results[worst_index].starting_year,
        results.scenario_results[worst_index].ending_year
    );
}

fn main() {
    println!("Retirement Simulator!!!");
    println!("Version {}", env!("CARGO_PKG_VERSION"));
    println!();

    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("Usage: retirement-simulator <input file>");
        println!("Example: retirement-simulator retirement.yaml");
        return;
    }

    let input = parse_input_file(&args[1]);
    let input = match input {
        Ok(v) => v,
        Err(e) => {
            println!("{e}");
            process::exit(1);
        }
    };

    println!("-= Simulation using uniform returns =-");
    println!();
    let simulation_results = simulate::run_simulation(&input).unwrap_or_else(|err| {
        println!("Error running simulation: {}", err);
        process::exit(1);
    });
    if simulation_results.monthly_snapshot[simulation_results.monthly_snapshot.len() - 1].balance
        == 0.0
    {
        println!("Retirement failed");
    } else {
        println!("Retirement succeeded!");
    }
    print_simulation_results(&simulation_results);

    println!();
    println!("-= Historical simulation =-");
    println!();
    let mut historical_scan = HistoricalScan::new().unwrap_or_else(|err| {
        println!("Error parsing historical returns: {}", err);
        process::exit(1);
    });
    let historical_results = run_scan(&input, &mut historical_scan).unwrap_or_else(|err| {
        println!("Error running historical simulation: {}", err);
        process::exit(1);
    });
    print_historical_result_details(&historical_results);

    println!();
    println!("-= Monte Carlo Simulation =-");
    println!();
    let mut monte_carlo_scan = MonteCarloScan::new();
    let monte_carlo_results = run_scan(&input, &mut monte_carlo_scan).unwrap_or_else(|err| {
        println!("Error running monte carlo simulation: {}", err);
        process::exit(1);
    });

    println!();
    println!("Worst year:");
    print_simulation_results(
        &monte_carlo_results.scenario_results[monte_carlo_results.sorted_indices[0]]
            .simulation_results,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_levels(levels_yaml: &str) -> Result<TaxTable, String> {
        let yaml = format!("single:\n  standard_deduction: 32200.0\n  levels:\n{levels_yaml}");
        let docs = YamlLoader::load_from_str(&yaml).unwrap();
        parse_tax_table(&docs[0], "single")
    }

    #[test]
    fn test_parse_tax_rates_sorts_levels() {
        let tax_rates = parse_levels(
            "    - {income: 24800.0, rate: 12.0}\n\
             \x20   - {income: 0.0, rate: 10.0}\n\
             \x20   - {income: 100800.0, rate: 22.0}\n",
        )
        .unwrap();
        assert_eq!(tax_rates.standard_deduction, 32200.0);
        let levels: Vec<(f32, f32)> = tax_rates
            .tax_levels
            .iter()
            .map(|l| (l.income, l.rate))
            .collect();
        assert_eq!(levels, vec![(0.0, 10.0), (24800.0, 12.0), (100800.0, 22.0)]);
    }

    #[test]
    fn test_parse_tax_rates_requires_zero_level() {
        let err = parse_levels("    - {income: 24800.0, rate: 12.0}\n").unwrap_err();
        assert!(err.contains("income 0.0"), "{err}");
    }

    #[test]
    fn test_parse_tax_rates_rejects_duplicates() {
        let err = parse_levels(
            "    - {income: 0.0, rate: 10.0}\n\
             \x20   - {income: 0.0, rate: 12.0}\n",
        )
        .unwrap_err();
        assert!(err.contains("duplicate"), "{err}");
    }

    #[test]
    fn test_parse_tax_rates_rejects_bad_rate() {
        let err = parse_levels("    - {income: 0.0, rate: 120.0}\n").unwrap_err();
        assert!(err.contains("between"), "{err}");
    }

    #[test]
    fn test_parse_sample_input() {
        let input = parse_input_file("input.yaml").unwrap();
        let married = &input.tax_rates.married_filing_jointly;
        let single = &input.tax_rates.single;
        assert_eq!(married.tax_levels.len(), 7);
        assert_eq!(single.tax_levels.len(), 7);
        assert_eq!(married.tax_levels[0].income, 0.0);
        assert_eq!(single.tax_levels[0].income, 0.0);
        assert!(single.standard_deduction < married.standard_deduction);
        assert_eq!(input.expenses.survivor_percent, 100.0);
    }

    #[test]
    fn test_tax_table_for_filing_status() {
        let input = parse_input_file("input.yaml").unwrap();
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
        let docs = YamlLoader::load_from_str(yaml).unwrap();
        let err = parse_tax_rates(&docs[0]).unwrap_err();
        assert!(err.contains("married_filing_jointly and single"), "{err}");
    }

    #[test]
    fn test_parse_requires_both_tax_tables() {
        let sample = fs::read_to_string("input.yaml").unwrap();
        let without_single = sample.split("    single:").next().unwrap();
        let err = parse_input_str(without_single).unwrap_err();
        assert!(err.contains("single block missing"), "{err}");
    }

    // sample input with one piece of text replaced
    fn parse_modified_sample(from: &str, to: &str) -> Result<Input, String> {
        let sample = fs::read_to_string("input.yaml").unwrap();
        assert!(sample.contains(from), "sample input doesn't contain {from}");
        parse_input_str(&sample.replacen(from, to, 1))
    }

    #[test]
    fn test_parse_accepts_integer_amounts() {
        let input = parse_modified_sample("balance: 1200000.0", "balance: 1200000").unwrap();
        assert_eq!(input.portfolio.balance, 1200000.0);
    }

    #[test]
    fn test_parse_rejects_negative_age() {
        let err = parse_modified_sample("retirement_age: 67", "retirement_age: -67").unwrap_err();
        assert!(err.contains("retirement_age"), "{err}");
    }

    #[test]
    fn test_parse_rejects_allocation_not_100() {
        let err = parse_modified_sample("us_equities: 56.0", "us_equities: 50.0").unwrap_err();
        assert!(err.contains("add up to 100"), "{err}");
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

        // defaults to 100 when left out
        let input = parse_modified_sample("survivor_percent: 100.0", "").unwrap();
        assert_eq!(input.expenses.survivor_percent, 100.0);
    }

    #[test]
    fn test_parse_errors_instead_of_panicking() {
        assert!(parse_input_file("does-not-exist.yaml")
            .unwrap_err()
            .contains("Can't read"));
        assert!(parse_input_str("").unwrap_err().contains("empty"));
        assert!(parse_input_str("retirees: [")
            .unwrap_err()
            .contains("Invalid YAML"));
    }
}
