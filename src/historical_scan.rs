/**************************************************************************
* historical_scan.rs
*
* Simulate starting a retirement at every year since 1928.
**************************************************************************/

use crate::{Input, scan, simulate};
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;

///////////////////////////////////////////////////////////////////////////
// Parse csv file with historical returns
///////////////////////////////////////////////////////////////////////////

#[derive(Debug, Default)]
struct HistoricalReturnsOneYear {
    year: u32,
    inflation: f32,

    // all returns are real returns (after inflation)
    sp500return: f32,
    tbill3month: f32,
    tbill10year: f32,
    corp_bonds: f32,
    real_estate: f32,
    international: Option<f32>,
}

pub struct HistoricalReturns {
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

fn calculate_averages(returns: &[HistoricalReturnsOneYear]) -> HistoricalReturnsOneYear {
    let mut totals = HistoricalReturnsOneYear::default();
    totals.international = Some(0.0);
    let mut international_count = 0.0;
    
    for ret in returns.iter() {
        totals.inflation += ret.inflation;
        totals.sp500return += ret.sp500return;
        totals.tbill3month += ret.tbill3month;
        totals.tbill10year += ret.tbill10year;
        totals.corp_bonds += ret.corp_bonds;
        totals.real_estate += ret.real_estate;
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
        averages.corp_bonds = totals.corp_bonds / count;
        averages.real_estate = totals.real_estate / count;
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
        let corp_bonds = str_to_f32(toks[12])? * 100.0;
        let real_estate = str_to_f32(toks[13])? * 100.0;
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
            corp_bonds,
            real_estate,
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

///////////////////////////////////////////////////////////////////////////
// Run scan
///////////////////////////////////////////////////////////////////////////

pub struct HistoricalScan {
    pub historical_returns: HistoricalReturns,
}

impl HistoricalScan {
    pub fn new() -> Result<Self, String> {
        let historical_returns = parse_returns()?;
        // println!("Averages: {:?}", historical_returns.averages);
        Ok(HistoricalScan {historical_returns})
    }
    
    fn run_scenario(&mut self,
                    starting_index: usize, 
                    input: &Input) -> Result<scan::Scenario, String> {
        let mut simulation = simulate::Simulation::new(input);
        let mut index = starting_index;

        'outer: loop {
            for _month in 0..12 {
                let international = self.historical_returns.annual_returns[index].international.unwrap_or(
                    self.historical_returns.annual_returns[index].sp500return);
                let is_finished = simulation.run_simulation_one_month(
                    self.historical_returns.annual_returns[index].sp500return,
                    international,
                    self.historical_returns.annual_returns[index].tbill10year)?;
                if is_finished {
                    break 'outer;
                }
            }
            index += 1;
            if index >= self.historical_returns.annual_returns.len() {
                index = 0;
            }
        }

        Ok(scan::Scenario {
            simulation_results: simulation.simulation_results_,
            starting_year: self.historical_returns.annual_returns[starting_index].year,
            ending_year: self.historical_returns.annual_returns[index].year,
        })
    }
}

impl scan::Scannable for HistoricalScan {
    fn run_scan(&mut self, input: &Input) -> Result<scan::ScanResults, String> {
        let mut results = scan::ScanResults::new();

        for index in 0..self.historical_returns.annual_returns.len() {
            let historical_scenario = self.run_scenario(
                index,
                input)?;
            scan::add_scenario_to_results(&mut results, historical_scenario, index);
        }

        results.sort_results();

        Ok(results)
    }
}

    
