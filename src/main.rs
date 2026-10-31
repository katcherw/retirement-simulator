/**************************************************************************
* retirement-simulator
*
* Command line program: reads the config file, runs the simulations, and
* prints the results as text or JSON.
**************************************************************************/

use chrono::NaiveDate;
use num_format::{Locale, ToFormattedString};
use std::env;
use std::fs;
use std::process;

use retirement_simulator::report::{self, Report, ScanSummary, SimulationReport};
use retirement_simulator::{runner, Input};

fn parse_input_file(fname: &str, start_date: NaiveDate) -> Result<Input, String> {
    let file_str = fs::read_to_string(fname).map_err(|e| format!("Can't read {fname}: {e}"))?;
    Input::from_yaml(&file_str, start_date)
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

fn print_simulation_results(simulation: &SimulationReport) {
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

    for year in simulation.years.iter() {
        let mut row: Vec<String> = Vec::new();

        row.push(year.year_index.to_string());
        row.push(year.date.format("%Y").to_string());
        row.push(year.age.to_string());
        row.push(num_with_commas(year.balance as u64));
        row.push(format!("{:.0}", year.monthly_expenses));
        row.push(format!("{:.0}", year.monthly_income));
        row.push(format!("{:.0}", year.monthly_taxes));
        row.push(format!("{:.0}%", year.tax_rate));
        row.push(format!("{:.2}%", year.withdrawal_rate));
        row.push(format!("{:.2}%", year.annualized_return));
        if !retire_printed && year.retired {
            row.push("Retired!".to_string());
            retire_printed = true;
        } else {
            row.push("".to_string());
        }

        table.push(row);
    }

    println!("{}", format_table(table));

    println!("Average return: {:.2}%", simulation.average_return);
}

fn print_scan_summary(summary: &ScanSummary) {
    println!(
        "Successful runs: {} of {} ({:.1}%)",
        summary.num_successful, summary.num_simulations, summary.success_rate
    );

    println!();
    println!("Ending balance in today's dollars:");
    let labels = [
        "Worst",
        "10th percentile",
        "25th percentile",
        "Median",
        "75th percentile",
        "90th percentile",
        "Best",
    ];
    for (label, ending) in labels.iter().zip(summary.ending_balances.iter()) {
        println!(
            "    {:<16} {:>14}",
            label,
            format!("${}", num_with_commas(ending.balance as u64))
        );
    }

    println!();
    let Some(failures) = &summary.failures else {
        println!("The money never ran out.");
        return;
    };
    // year, with the ages of the retirees still alive then
    let describe = |point: &report::FailurePoint| {
        let ages: Vec<String> = point
            .ages
            .iter()
            .map(|retiree| format!("{} age {}", retiree.name, retiree.age))
            .collect();
        format!("{} ({})", point.date.format("%Y"), ages.join(", "))
    };
    println!(
        "The money ran out in {} of {} runs ({:.1}%):",
        failures.count, summary.num_simulations, failures.percent
    );
    println!("    {:<16} {}", "Earliest", describe(&failures.earliest));
    println!("    {:<16} {}", "Median", describe(&failures.median));
    println!("    {:<16} {}", "Latest", describe(&failures.latest));
}

fn print_historical_result_details(historical: &report::HistoricalReport) {
    let scenarios = &historical.scenarios;
    if historical.num_wrapped > 0 {
        println!();
        println!(
            "{} of {} scenarios ran past {} and continued with the returns from\n\
             {} on, so their later years are not a real historical sequence.",
            historical.num_wrapped,
            scenarios.len(),
            historical.last_year,
            historical.first_year
        );
    }

    println!();
    println!("Scenarios (sorted by worst to best):");
    for scenario in scenarios.iter() {
        println!(
            "    years {}, ending balance ${}",
            scenario.description,
            num_with_commas(scenario.ending_balance as u64)
        );
    }

    println!();
    println!("Worst result was years {}", scenarios[0].description);
}

fn print_report(report: &Report) {
    println!("-= Simulation using uniform returns =-");
    println!();
    if report.uniform.succeeded {
        println!("Retirement succeeded!");
    } else {
        println!("Retirement failed");
    }
    print_simulation_results(&report.uniform);

    println!();
    println!("-= Historical simulation =-");
    println!();
    print_scan_summary(&report.historical.summary);
    print_historical_result_details(&report.historical);

    println!();
    println!("-= Monte Carlo Simulation =-");
    println!();
    let seed = report.monte_carlo.seed;
    println!("Random seed: {seed} (run with --seed {seed} to repeat these results)");
    println!();
    print_scan_summary(&report.monte_carlo.summary);

    println!();
    println!("Worst scenario:");
    print_simulation_results(&report.monte_carlo.worst_scenario);
}

///////////////////////////////////////////////////////////////////////////
// Command line
///////////////////////////////////////////////////////////////////////////

#[derive(Debug, PartialEq)]
struct Args {
    input_file: String,
    // random seed for the monte carlo simulation, to repeat a previous run
    seed: Option<u64>,
    // print the results as JSON instead of text
    json: bool,
}

const USAGE: &str = "Usage: retirement-simulator [--seed <number>] [--json] <input file>
Example: retirement-simulator retirement.yaml

Options:
    --seed <number>   Random seed for the Monte Carlo simulation. Running again
                      with the same seed gives the same results.
    --json            Print the results as JSON instead of text.";

// args excludes the program name
fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut input_file = None;
    let mut seed = None;
    let mut json = false;

    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--seed" {
            let value = args.next().ok_or("--seed needs a number")?;
            let value = value
                .parse::<u64>()
                .map_err(|_| format!("Invalid seed: {value}"))?;
            seed = Some(value);
        } else if arg == "--json" {
            json = true;
        } else if arg.starts_with('-') {
            return Err(format!("Unknown option: {arg}"));
        } else if input_file.is_some() {
            return Err("Only one input file can be given".to_string());
        } else {
            input_file = Some(arg.clone());
        }
    }

    Ok(Args {
        input_file: input_file.ok_or("No input file given")?,
        seed,
        json,
    })
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let args = parse_args(&args).unwrap_or_else(|e| {
        println!("{e}");
        println!();
        println!("{USAGE}");
        process::exit(1);
    });

    if !args.json {
        println!("Retirement Simulator!!!");
        println!("Version {}", env!("CARGO_PKG_VERSION"));
        println!();
    }

    let today = chrono::Local::now().date_naive();
    let input = parse_input_file(&args.input_file, today).unwrap_or_else(|e| {
        println!("{e}");
        process::exit(1);
    });

    // pick a random seed if none was given, and print it so the run can be
    // repeated. Kept to 32 bits so it's easy to retype.
    let seed = args.seed.unwrap_or_else(|| rand::random::<u32>() as u64);
    let results = runner::run_all(&input, seed).unwrap_or_else(|e| {
        println!("{e}");
        process::exit(1);
    });
    let report = report::build(&results);

    if args.json {
        let json = serde_json::to_string_pretty(&report).unwrap_or_else(|e| {
            println!("Can't convert results to JSON: {e}");
            process::exit(1);
        });
        println!("{json}");
    } else {
        print_report(&report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn test_parse_args() {
        assert_eq!(
            parse_args(&args(&["input.yaml"])).unwrap(),
            Args {
                input_file: "input.yaml".to_string(),
                seed: None,
                json: false,
            }
        );
        assert_eq!(
            parse_args(&args(&["--seed", "42", "input.yaml"])).unwrap(),
            Args {
                input_file: "input.yaml".to_string(),
                seed: Some(42),
                json: false,
            }
        );
        assert_eq!(
            parse_args(&args(&["input.yaml", "--json", "--seed", "42"])).unwrap(),
            Args {
                input_file: "input.yaml".to_string(),
                seed: Some(42),
                json: true,
            }
        );
    }

    #[test]
    fn test_parse_args_errors() {
        let error = |values: &[&str]| parse_args(&args(values)).unwrap_err();
        assert!(error(&[]).contains("No input file"));
        assert!(error(&["input.yaml", "--seed"]).contains("needs a number"));
        assert!(error(&["input.yaml", "--seed", "abc"]).contains("Invalid seed"));
        assert!(error(&["input.yaml", "--seed", "-1"]).contains("Invalid seed"));
        assert!(error(&["--verbose", "input.yaml"]).contains("Unknown option"));
        assert!(error(&["a.yaml", "b.yaml"]).contains("Only one input file"));
    }

    #[test]
    fn test_missing_input_file() {
        let start_date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        assert!(parse_input_file("does-not-exist.yaml", start_date)
            .unwrap_err()
            .contains("Can't read"));
    }
}
