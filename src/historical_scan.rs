/**************************************************************************
* historical_scan.rs
*
* Simulate starting a retirement at every year since 1928.
**************************************************************************/

use crate::{scan, simulate, Input};
use std::io::BufRead;

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
    gold: f32,
    international: Option<f32>,
}

#[allow(dead_code)]
pub struct HistoricalReturns {
    annual_returns: Vec<HistoricalReturnsOneYear>,
    averages: HistoricalReturnsOneYear,
}

fn str_to_u32(s: &str) -> Result<u32, String> {
    s.trim()
        .parse::<u32>()
        .map_err(|v| format!("Invalid integer: {}", v))
}

fn str_to_f32(s: &str) -> Result<f32, String> {
    s.trim()
        .parse::<f32>()
        .map_err(|v| format!("Invalid floating point: {}", v))
}

fn str_to_f32_optional(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok()
}

fn calculate_averages(returns: &[HistoricalReturnsOneYear]) -> HistoricalReturnsOneYear {
    let mut totals = HistoricalReturnsOneYear::default();
    let mut international_total = 0.0;
    let mut international_count = 0.0;

    for ret in returns.iter() {
        totals.inflation += ret.inflation;
        totals.sp500return += ret.sp500return;
        totals.tbill3month += ret.tbill3month;
        totals.tbill10year += ret.tbill10year;
        totals.corp_bonds += ret.corp_bonds;
        totals.real_estate += ret.real_estate;
        totals.gold += ret.gold;
        if let Some(international) = ret.international {
            international_total += international;
            international_count += 1.0;
        }
    }

    let mut averages = HistoricalReturnsOneYear::default();
    let count = returns.len() as f32;
    if count > 0.0 {
        averages.inflation = totals.inflation / count;
        averages.sp500return = totals.sp500return / count;
        averages.tbill3month = totals.tbill3month / count;
        averages.tbill10year = totals.tbill10year / count;
        averages.corp_bonds = totals.corp_bonds / count;
        averages.real_estate = totals.real_estate / count;
        averages.gold = totals.gold / count;
        averages.international = Some(if international_count > 0.0 {
            international_total / international_count
        } else {
            0.0
        });
    };

    averages
}

// returns.csv is built into the program, so it runs from any directory.
// Rebuild after changing the file.
const RETURNS_CSV: &str = include_str!("../returns.csv");

fn parse_returns() -> Result<HistoricalReturns, String> {
    parse_returns_from(RETURNS_CSV.as_bytes())
}

// columns: year, 7 nominal returns, inflation, then real returns for
// S&P 500, 3-month T-bill, 10-year T-bond, Baa corporate bonds, real estate,
// gold, and international
fn parse_returns_from<R: BufRead>(reader: R) -> Result<HistoricalReturns, String> {
    let mut annual_returns: Vec<HistoricalReturnsOneYear> = Vec::new();

    for (i, line) in reader.lines().enumerate() {
        if i < 2 {
            continue;
        }
        let line = line.map_err(|v| format!("Can't read line from returns.csv: {}", v))?;
        let toks: Vec<&str> = line.split(',').collect();
        if toks.len() < 16 {
            return Err(format!("Can't parse line [{}]", line));
        }

        let year = str_to_u32(toks[0])?;
        let inflation = str_to_f32(toks[8])? * 100.0;
        let sp500return = str_to_f32(toks[9])? * 100.0;
        let tbill3month = str_to_f32(toks[10])? * 100.0;
        let tbill10year = str_to_f32(toks[11])? * 100.0;
        let corp_bonds = str_to_f32(toks[12])? * 100.0;
        let real_estate = str_to_f32(toks[13])? * 100.0;
        let gold = str_to_f32(toks[14])? * 100.0;
        let international = str_to_f32_optional(toks[15]).map(|v| v * 100.0);

        let returns = HistoricalReturnsOneYear {
            year,
            inflation,
            sp500return,
            tbill3month,
            tbill10year,
            corp_bonds,
            real_estate,
            gold,
            international,
        };

        annual_returns.push(returns);
    }

    let averages = calculate_averages(&annual_returns);
    let historical_returns = HistoricalReturns {
        annual_returns,
        averages,
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
        Ok(HistoricalScan { historical_returns })
    }

    fn run_scenario(
        &mut self,
        starting_index: usize,
        input: &Input,
    ) -> Result<scan::Scenario, String> {
        let mut simulation = simulate::Simulation::new(input);
        let mut index = starting_index;

        'outer: loop {
            for _month in 0..12 {
                let international = self.historical_returns.annual_returns[index]
                    .international
                    .unwrap_or(self.historical_returns.annual_returns[index].sp500return);
                let is_finished = simulation.run_simulation_one_month(
                    self.historical_returns.annual_returns[index].sp500return,
                    international,
                    self.historical_returns.annual_returns[index].tbill10year,
                )?;
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
            let historical_scenario = self.run_scenario(index, input)?;
            scan::add_scenario_to_results(&mut results, historical_scenario, index);
        }

        results.sort_results();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "title\nheader\n";

    #[test]
    fn test_parse_returns_columns() {
        let csv = format!("{HEADER}2025,0.17,0.04,0.07,0.06,0.01,0.66,0.31,0.0274,0.1459,0.0144,0.0493,0.0412,-0.0113,0.6179,0.2838\n");
        let returns = parse_returns_from(csv.as_bytes()).unwrap();
        let year = &returns.annual_returns[0];
        assert_eq!(year.year, 2025);
        assert!((year.sp500return - 14.59).abs() < 0.001);
        assert!((year.tbill10year - 4.93).abs() < 0.001);
        assert!((year.gold - 61.79).abs() < 0.001);
        assert!((year.international.unwrap() - 28.38).abs() < 0.001);
    }

    #[test]
    fn test_parse_returns_missing_international() {
        let csv = format!("{HEADER}1928,0.43,0.03,0.008,0.03,0.01,0.001,,-0.0116,0.4549,0.0429,0.0201,0.0443,0.0268,0.0127,\n");
        let returns = parse_returns_from(csv.as_bytes()).unwrap();
        assert_eq!(returns.annual_returns[0].international, None);
    }

    #[test]
    fn test_parse_returns_short_line_is_error() {
        let csv = format!("{HEADER}1928,0.43,0.03,0.008,0.03,0.01,0.001,,-0.0116,0.4549,0.0429,0.0201,0.0443,0.0268\n");
        assert!(parse_returns_from(csv.as_bytes()).is_err());
    }

    #[test]
    fn test_parse_returns_file() {
        let returns = parse_returns().unwrap();
        assert_eq!(returns.annual_returns[0].year, 1928);
        assert!(returns
            .annual_returns
            .iter()
            .any(|r| r.international.is_none()));
    }
}
