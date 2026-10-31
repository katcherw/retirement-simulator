/**************************************************************************
* runner.rs
*
* Runs all the simulations for a plan: uniform returns, every starting year
* of historical returns, and the Monte Carlo simulations.
**************************************************************************/

use chrono::NaiveDate;

use crate::historical_scan::HistoricalScan;
use crate::monte_carlo::MonteCarloScan;
use crate::scan::{ScanResults, Scannable};
use crate::{simulate, Input};

// the raw results of every simulation, for the report module to analyze
pub struct RunResults {
    pub start_date: NaiveDate,
    // simulation with the expected returns every year
    pub uniform: simulate::SimulationResults,
    pub historical: ScanResults,
    // the historical returns used, to describe the years of each scenario
    pub historical_scan: HistoricalScan,
    pub monte_carlo: ScanResults,
    // running again with the same seed gives the same Monte Carlo results
    pub seed: u64,
}

pub fn run_all(input: &Input, seed: u64) -> Result<RunResults, String> {
    let uniform =
        simulate::run_simulation(input).map_err(|e| format!("Error running simulation: {e}"))?;

    let mut historical_scan =
        HistoricalScan::new().map_err(|e| format!("Error parsing historical returns: {e}"))?;
    let historical = historical_scan
        .run_scan(input)
        .map_err(|e| format!("Error running historical simulation: {e}"))?;

    let monte_carlo = MonteCarloScan::new(seed)
        .run_scan(input)
        .map_err(|e| format!("Error running monte carlo simulation: {e}"))?;

    Ok(RunResults {
        start_date: input.start_date,
        uniform,
        historical,
        historical_scan,
        monte_carlo,
        seed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ending balance of every monte carlo scenario, in the order they ran
    fn monte_carlo_ending_balances(seed: u64) -> Vec<f64> {
        let start_date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let text = std::fs::read_to_string("input.yaml").unwrap();
        let input = Input::from_yaml(&text, start_date).unwrap();
        let results = run_all(&input, seed).unwrap();
        results
            .monte_carlo
            .scenario_results
            .iter()
            .map(|s| s.ending_balance())
            .collect()
    }

    #[test]
    fn test_monte_carlo_same_seed_same_results() {
        assert_eq!(
            monte_carlo_ending_balances(42),
            monte_carlo_ending_balances(42)
        );
        assert_ne!(
            monte_carlo_ending_balances(42),
            monte_carlo_ending_balances(43)
        );
    }
}
