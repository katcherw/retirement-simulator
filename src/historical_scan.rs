use chrono::Datelike;

use crate::{Input, simulate};
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use core::cmp::Ordering;

#[derive(Debug, Default)]
struct HistoricalReturnsOneYear {
    year: u32,
    inflation: f32,

    // all returns are real returns (after inflation)
    sp500return: f32,
    tbill3month: f32,
    tbill10year: f32,
    corpBonds: f32,
    realEstate: f32,
    international: Option<f32>,
}

struct HistoricalReturns {
    annual_returns: Vec<HistoricalReturnsOneYear>,
    averages: HistoricalReturnsOneYear,
}

fn str_to_u32(s: &str) -> Result<u32, String> {
    s.trim().parse::<u32>().map_err(|v| (format!("Invalid integer: {}", v)))
}

fn str_to_f32(s: &str) -> Result<f32, String> {
    s.trim().parse::<f32>().map_err(|v| (format!("Invalid floating point: {}", v)))
}

fn str_to_f32_optional(s: &str) -> Option<f32> {
    match s.trim().parse::<f32>() {
        Ok(v) => Some(v),
        Err(_) => None,
    }
}

fn calculate_average(vals: &[f32]) -> f32 {
    let mut total = 0f32;

    for val in vals {
        total += val;
    }

    if vals.len() > 0 { total / vals.len() as f32} else { 0.0 }
}

fn calculate_average_optional(vals: &[Option<f32>]) -> f32 {
    let mut total = 0f32;

    for val in vals {
        total += val.unwrap_or(0.0);
    }

    if vals.len() > 0 { total / vals.len() as f32} else { 0.0 }
}

fn calculate_averages(returns: &[HistoricalReturnsOneYear]) -> HistoricalReturnsOneYear {
    let mut totals = HistoricalReturnsOneYear::default();
    totals.international = Some(0.0);
    let mut international_count = 0.0;
    
    for ret in returns.iter() {
        totals.inflation += ret.inflation;
        totals.sp500return += ret.sp500return;
        totals.tbill3month += ret.tbill3month;
        totals.tbill10year += ret.tbill10year;
        totals.corpBonds += ret.corpBonds;
        totals.realEstate += ret.realEstate;
        if ret.international.is_some() {
            totals.international = Some(totals.international.unwrap() +
                ret.international.unwrap());
            international_count += 1.0;
        }
    };

    let mut averages = HistoricalReturnsOneYear::default();
    let count = returns.len() as f32;
    if count > 0.0 {
        averages.inflation = totals.inflation / count;
        averages.sp500return = totals.sp500return / count;
        averages.tbill3month = totals.tbill3month / count;
        averages.tbill10year = totals.tbill10year / count;
        averages.corpBonds = totals.corpBonds / count;
        averages.realEstate = totals.realEstate / count;
        averages.international = Some(if international_count > 0.0
            { totals.international.unwrap() / international_count } else { 0.0 });
    };

    averages    
}

fn parse_returns() -> Result<HistoricalReturns, String> {

    let mut annual_returns: Vec<HistoricalReturnsOneYear> = Vec::new();

    let fname = Path::new("returns.csv");
    let file = File::open(fname).map_err(|_| "Can't open returns.csv")?;
    let reader = io::BufReader::new(file);

    for (i, line) in reader.lines().enumerate() {
        if i < 2 {
            continue;
        }
        let line = line.map_err(|v| format!("Can't read line from returns.csv: {}", v.to_string()))?;
        let toks: Vec<&str> = line.split(',').collect();
        if toks.len() < 14 {
            return Err(format!("Can't parse line [{}]", line));
        }

        let year = str_to_u32(toks[0])?;
        let inflation = str_to_f32(toks[8])? * 100.0;
        let sp500return = str_to_f32(toks[9])? * 100.0;
        let tbill3month = str_to_f32(toks[10])? * 100.0;
        let tbill10year = str_to_f32(toks[11])? * 100.0;
        let corpBonds = str_to_f32(toks[12])? * 100.0;
        let realEstate = str_to_f32(toks[13])? * 100.0;
        let mut international = str_to_f32_optional(toks[14]);
        if let Some(v) = international {
            international = Some(v * 100.0);
        }

        let returns = HistoricalReturnsOneYear {
            year,
            inflation,
            sp500return,
            tbill3month,
            tbill10year,
            corpBonds,
            realEstate,
            international,
        };

        annual_returns.push(returns);
    }

    let averages = calculate_averages(&annual_returns);
    let historical_returns = HistoricalReturns {
        annual_returns,
        averages
    };
    Ok(historical_returns)
}

#[derive(Debug)]
pub struct HistoricalScenario {
    pub simulation_results: simulate::SimulationResults,
    pub starting_year: u32,
    pub ending_year: u32,
}

fn run_scenario(starting_index: usize, 
                historical_returns: &HistoricalReturns,
                input: &Input) -> Result<HistoricalScenario, String> {
    let mut simulation = simulate::Simulation::new(input);
    let mut index = starting_index;

    'outer: loop {
        for month in 0..12 {
            let international = historical_returns.annual_returns[index].international.unwrap_or(
                historical_returns.annual_returns[index].sp500return);
            let is_finished = simulation.run_simulation_one_month(
                historical_returns.annual_returns[index].sp500return,
                international,
                historical_returns.annual_returns[index].tbill10year)?;
            if is_finished {
                break 'outer;
            }
        }
        index += 1;
        if index >= historical_returns.annual_returns.len() {
            index = 0;
        }
    }

    Ok(HistoricalScenario {
        simulation_results: simulation.simulation_results_,
        starting_year: historical_returns.annual_returns[starting_index].year,
        ending_year: historical_returns.annual_returns[index].year,
    })
}

struct FailedInfo {
    index: usize,
    num_months: usize,
    ending_balance: f32,
}
    
#[derive(Debug)]
pub struct HistoricalScanResults {
    pub scenario_results: Vec<HistoricalScenario>,
    pub num_simulations: u32,
    pub num_successful: u32,
    pub min_balance: f32,
    pub max_balance: f32,
    pub indices_failed: Vec<usize>,
}

pub fn run_historical_scan(input: &Input) -> Result<HistoricalScanResults, String> {
    let historical_returns = parse_returns()?;
    println!("Averages: {:?}", historical_returns.averages);

    let mut results = HistoricalScanResults {
        scenario_results: Vec::new(),
        num_simulations: 0,
        num_successful: 0,
        min_balance: f32::MAX,
        max_balance: 0.0,
        indices_failed: Vec::new(),
    };

    let mut sorted_results: Vec<FailedInfo> = Vec::new();
    
    for index in 0..historical_returns.annual_returns.len() {
        let historical_scenario = run_scenario(
            index,
            &historical_returns,
            input)?;
        results.num_simulations += 1;
        let last_index = historical_scenario.simulation_results.monthly_snapshot.len() - 1;
        let last_balance = historical_scenario.simulation_results.monthly_snapshot[last_index].balance;
        if last_balance > 0.0 {
            results.num_successful += 1;
            results.min_balance = f32::min(results.min_balance, last_balance);
            results.max_balance = f32::max(results.max_balance, last_balance);
        }
        else {
            sorted_results.push(FailedInfo {
                index,
                num_months: historical_scenario.simulation_results.monthly_snapshot.len(),
                ending_balance: last_balance,
            });
        }
        results.scenario_results.push(historical_scenario);
    }

    sorted_results.sort_by(|a, b| {
        a.num_months.cmp(&b.num_months)
            .then_with(|| a.ending_balance.partial_cmp(&b.ending_balance).unwrap())
    });

    for v in sorted_results {
        results.indices_failed.push(v.index);
    }

    Ok(results)
}
