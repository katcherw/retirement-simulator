extern crate yaml_rust;
extern crate chrono;
use yaml_rust::{YamlLoader, YamlEmitter};
use chrono::NaiveDate;
use std::env;
use std::fs;
use std::process;

use crate::historical_scan::run_historical_scan;
use crate::portfolio::Portfolio;

mod simulate;
mod historical_scan;
mod utils;
mod portfolio;

#[derive(Debug)]
struct Retiree {
    name: String,
    date_of_birth: NaiveDate,
    retirement_age: u32,
    life_expectency: u32,
    salary_annual: f32,
    take_home_pay_annual: f32,
    retirement_contribution_percent: f32,
    hsa_contribution_annual: f32,
    social_security_age: u32,
    social_security_amount_early: f32,
    social_security_amount_full: f32,
    social_security_amount_delayed: f32,
}
    
#[derive(Debug)]
struct Expenses {
    monthly: f32,
}

#[derive(Debug, Copy, Clone)]
pub struct TaxLevel {
    income: f32,
    rate: f32,
}
    
#[derive(Debug)]
pub struct TaxRates {
    standard_deduction: f32,
    tax_levels: Vec<TaxLevel>,
}

#[derive(Debug)]
struct Input {
    retirees: Vec<Retiree>,
    portfolio: Portfolio,
    expenses: Expenses,
    tax_rates: TaxRates,
}

fn parse_string(yaml: &yaml_rust::Yaml, field_name: &str) -> Result<String, String> {
    let value = yaml[field_name].as_str()
        .ok_or("Invalid value: ".to_string() + field_name)?;
    
    Ok(value.to_string())
}

fn parse_u32(yaml: &yaml_rust::Yaml, field_name: &str) -> Result<u32, String> {
    let value = yaml[field_name].as_i64()
        .ok_or("Invalid value: ".to_string() + field_name)?;
    
    Ok(value as u32)
}
    
fn parse_f32(yaml: &yaml_rust::Yaml, field_name: &str) -> Result<f32, String> {
    let value = yaml[field_name].as_f64()
        .ok_or("Invalid value: ".to_string() + field_name)?;
    
    Ok(value as f32)
}
    
fn parse_portfolio(input_yaml: &yaml_rust::Yaml) -> Result<Portfolio, String> {
    let block = &input_yaml["portfolio"];
    if block.is_badvalue() {
        return Err("portfolio block missing".to_string());
    }

    let balance = parse_f32(block, "balance")?;
    
    let us_equity_allocation = parse_f32(block, "us_equity_allocation")?;
    let international_equity_allocation = parse_f32(block, "international_equity_allocation")?;
    let bond_allocation = parse_f32(block, "bond_allocation")?;
    
    let us_equity_expected_returns = parse_f32(block, "us_equity_expected_returns")?;
    let us_equity_standard_deviation = parse_f32(block, "us_equity_standard_deviation")?;
    let international_equity_expected_returns = parse_f32(block, "international_equity_expected_returns")?;
    let international_equity_standard_deviation = parse_f32(block, "international_equity_standard_deviation")?;
    let bonds_expected_returns = parse_f32(block, "bonds_expected_returns")?;
    let bonds_standard_deviation = parse_f32(block, "bonds_standard_deviation")?;
    let expected_inflation = parse_f32(block, "expected_inflation")?;

    let portfolio = Portfolio {
        balance,
        us_equity_allocation,
        international_equity_allocation,
        bond_allocation,
        us_equity_expected_returns,
        us_equity_standard_deviation,
        international_equity_expected_returns,
        international_equity_standard_deviation,
        bonds_expected_returns,
        bonds_standard_deviation,
        expected_inflation,
    };
    
    Ok(portfolio)
}

fn parse_expenses(input_yaml: &yaml_rust::Yaml) -> Result<Expenses, String> {
    let block = &input_yaml["expenses"];
    if block.is_badvalue() {
        return Err("expenses block missing".to_string());
    }

    let monthly = parse_f32(block, "monthly")?;

    let expenses = Expenses {
        monthly,
    };
    
    Ok(expenses)
}

fn parse_retiree(input_yaml: &yaml_rust::Yaml) -> Result<Retiree, String> {
    let name = parse_string(input_yaml, "name")?;
    let life_expectency = parse_u32(input_yaml, "life_expectency")?;
    let retirement_age = parse_u32(input_yaml, "retirement_age")?;

    let salary_annual = parse_f32(input_yaml, "wage_annual_salary")?;
    let take_home_pay_annual = parse_f32(input_yaml, "wage_annual_take_home_pay")?;
    let retirement_contribution_percent = parse_f32(input_yaml, "retirement_contribution_percent")?;
    let hsa_contribution_annual = parse_f32(input_yaml, "hsa_contribution_annual")?;
    let social_security_age = parse_u32(input_yaml, "social_security_age")?;
    let social_security_amount_early = parse_f32(input_yaml, "social_security_amount_early")?;
    let social_security_amount_full = parse_f32(input_yaml, "social_security_amount_full")?;
    let social_security_amount_delayed = parse_f32(input_yaml, "social_security_amount_delayed")?;

    let date_of_birth = parse_string(input_yaml, "date_of_birth")?;
    let date_of_birth = NaiveDate::parse_from_str(&date_of_birth, "%m/%d/%Y").map_err(|_| "Invalid date")?;
    
    let retiree = Retiree {
        name,
        date_of_birth,
        life_expectency,
        retirement_age,
        salary_annual,
        take_home_pay_annual,
        retirement_contribution_percent,
        hsa_contribution_annual,
        social_security_age,
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

    let tax_rate = TaxLevel {
        income,
        rate,
    };

    Ok(tax_rate)
}
    
fn parse_tax_rates(input_yaml: &yaml_rust::Yaml) -> Result<TaxRates, String> {
    let mut tax_levels = Vec::new();
    let block = &input_yaml["tax_rates"];
    if block.is_badvalue() {
        return Err("tax_rates block missing".to_string());
    }

    let standard_deduction = parse_f32(block, "standard_deduction")?;
    println!("standard_deduction {:?}", standard_deduction);

    let block = &block["levels"];
    if block.is_badvalue() {
        return Err("levels block missing".to_string());
    }

    tax_levels.push( TaxLevel {income: 0.0, rate: 0.0});
    let mut vec = block.as_vec().ok_or("no tax rates found")?;
    for element in vec {
        let tax_rate = parse_tax_rate(element);
        match tax_rate {
            Ok(v) => tax_levels.push(v),
            Err(e) => return Err(e),
        };
    }

    //for (i, tax_rate) in tax_rates.iter().enumerate() {
    for i in 1..tax_levels.len() {
        if i < tax_levels.len() - 1 {
            tax_levels[i].income = tax_levels[i + 1].income - 1.0;
        }
        else {
            tax_levels[i].income = f32::MAX;
        }
    }

    let tax_rates = TaxRates {
        standard_deduction,
        tax_levels,
    };

    Ok(tax_rates)
}

fn parse_input_file(fname: &str) -> Result<Input, String> {
    let file_str = fs::read_to_string(fname).unwrap();
    
    let docs = YamlLoader::load_from_str(&file_str).unwrap();
    let doc = &docs[0];

    // Dump the YAML object
    let mut out_str = String::new();
    {
        let mut emitter = YamlEmitter::new(&mut out_str);
        emitter.dump(doc).unwrap(); // dump the YAML object to a String
        println!("{out_str}");
    }

    let portfolio = parse_portfolio(&doc)?;
    let expenses = parse_expenses(&doc)?;
    let retirees = parse_retirees(&doc)?;
    let mut tax_rates = parse_tax_rates(&doc)?;
    tax_rates.tax_levels.sort_unstable_by_key(|e| e.income as u32);
    
    let input = Input {
        retirees,
        portfolio,
        expenses,
        tax_rates,
    };

    Ok(input)
        
}

fn print_simulation_results(simulation_results: &simulate::SimulationResults) {
    let mut retire_printed = false;

    for (i, monthly_snapshot) in simulation_results.monthly_snapshot.iter().enumerate() {
        if (i % 12) == 0 {
            let age = utils::get_age(&simulation_results.retirees[0].date_of_birth, &monthly_snapshot.date);
            print!("{} {} {} {} {} {} {} {} {} {} {}", i, i / 12, monthly_snapshot.date, age,
                   monthly_snapshot.balance, monthly_snapshot.expenses, monthly_snapshot.income,
                   monthly_snapshot.taxes, monthly_snapshot.tax_rate, monthly_snapshot.withdrawal_rate,
                   monthly_snapshot.annualized_return);
            if !retire_printed && (monthly_snapshot.date >= simulation_results.retirement_date) {
                print!(" Retired!");
                retire_printed = true;
            }
            println!();
        }
    }
    println!("Average return: {}%", simulation_results.average_return);
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
        Err(e) => {println!("{e}"); process::exit(1);}
    };
    
    let simulation_results = simulate::run_simulation(&input);
    if simulation_results.is_err() {
        println!("Error running simulation");
        return;
    }
    let simulation_results = simulation_results.unwrap();
    print_simulation_results(&simulation_results);

    let historical_results = run_historical_scan(&input); 
    if historical_results.is_err() {
        println!("Error running simulation");
        return;
    }
    let historical_results = historical_results.unwrap();
        
    println!();
    println!("Historical results:");
    println!("Successful runs: {} of {} ({}%)", historical_results.num_successful,
             historical_results.num_simulations,
             (historical_results.num_successful as f32/historical_results.num_simulations as f32) * 100.0);
    println!("Lowest ending balance: ${}", historical_results.min_balance);
    println!("Highest ending balance: ${}", historical_results.max_balance);
    println!("Failing indices: {:?}", historical_results.indices_failed);

    for index in historical_results.indices_failed {
        println!();
        println!("Failing result for years {} to {}",
                 historical_results.scenario_results[index].starting_year,
                 historical_results.scenario_results[index].ending_year);
         
        print_simulation_results(&historical_results.scenario_results[index].simulation_results);
    }
    
}
