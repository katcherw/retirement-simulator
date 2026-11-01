/**************************************************************************
* scan.rs
*
* Common functions and traits for scanning a series of simulations
**************************************************************************/

use crate::{simulate, Input};
use chrono::NaiveDate;

// A scenario is a particular simulation (one retirement cycle) in a scan.
#[derive(Debug)]
pub struct Scenario {
    pub simulation_results: simulate::SimulationResults,
    pub starting_year: u32,
    pub ending_year: u32,
}

impl Scenario {
    pub fn ending_balance(&self) -> f64 {
        self.last_snapshot().balance
    }

    fn last_snapshot(&self) -> &simulate::MonthlySnapshot {
        self.simulation_results.monthly_snapshot.last().unwrap()
    }
}

// Information for a vector element intended for sorting
#[derive(Debug)]
struct ScenarioSortingInfo {
    index: usize,
    num_months: usize,
    ending_balance: f64,
}

// The results of all the scenarios in the scan
#[derive(Debug, Default)]
pub struct ScanResults {
    pub scenario_results: Vec<Scenario>,
    pub num_simulations: u32,
    pub num_successful: u32,
    // scenario indices from worst to best: scenarios where the money ran out
    // (earliest first), then by ending balance
    pub sorted_indices: Vec<usize>,
    sorting_info: Vec<ScenarioSortingInfo>,
}

impl ScanResults {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_sorting_info(&mut self, index: usize, num_months: usize, ending_balance: f64) {
        self.sorting_info.push(ScenarioSortingInfo {
            index,
            num_months,
            ending_balance,
        });
    }

    pub fn sort_results(&mut self) {
        self.sorting_info.sort_by(|a, b| {
            a.num_months
                .cmp(&b.num_months)
                .then_with(|| a.ending_balance.total_cmp(&b.ending_balance))
        });

        for v in self.sorting_info.iter() {
            self.sorted_indices.push(v.index);
        }
    }

    // the scenario at the given percentile (0 = worst, 100 = best). Must be
    // called after sort_results.
    pub fn scenario_at_percentile(&self, percentile: f64) -> &Scenario {
        let position = percentile_index(self.sorted_indices.len(), percentile);
        &self.scenario_results[self.sorted_indices[position]]
    }

    // ending balance at the given percentile (0 = worst, 100 = best).
    // Scenarios where the money ran out count as $0.
    pub fn ending_balance_percentile(&self, percentile: f64) -> f64 {
        self.scenario_at_percentile(percentile).ending_balance()
    }

    // dates the money ran out in failed scenarios, earliest first. Must be
    // called after sort_results.
    pub fn failure_dates(&self) -> Vec<NaiveDate> {
        self.sorted_indices
            .iter()
            .map(|&i| &self.scenario_results[i])
            .filter(|scenario| scenario.ending_balance() <= 0.0)
            .map(|scenario| scenario.last_snapshot().date)
            .collect()
    }
}

// index of the given percentile (0 = first, 100 = last) in a sorted list of
// len values, rounded to the nearest value. len must be at least 1.
pub fn percentile_index(len: usize, percentile: f64) -> usize {
    let last = len - 1;
    let position = (percentile / 100.0 * last as f64).round() as usize;
    position.min(last)
}

pub trait Scannable {
    fn run_scan(&mut self, input: &Input) -> Result<ScanResults, String>;
}

pub fn add_scenario_to_results(results: &mut ScanResults, scenario: Scenario, index: usize) {
    results.num_simulations += 1;
    let last_balance = scenario.ending_balance();
    results.add_sorting_info(
        index,
        scenario.simulation_results.monthly_snapshot.len(),
        last_balance,
    );
    if last_balance > 0.0 {
        results.num_successful += 1;
    }
    results.scenario_results.push(scenario);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, 1).unwrap()
    }

    // a scenario starting in January 2026 that lasts the given number of
    // months, ending with the given balance
    fn scenario(num_months: u32, ending_balance: f64) -> Scenario {
        let mut simulation_results = simulate::SimulationResults::default();
        for month in 0..num_months {
            let is_last = month == num_months - 1;
            simulation_results
                .monthly_snapshot
                .push(simulate::MonthlySnapshot {
                    date: date(2026, 1)
                        .checked_add_months(chrono::Months::new(month))
                        .unwrap(),
                    balance: if is_last { ending_balance } else { 1000.0 },
                    expenses: 0.0,
                    income: 0.0,
                    tax_rate: 0.0,
                    taxes: 0.0,
                    withdrawal_rate: 0.0,
                    annualized_return: 0.0,
                });
        }
        Scenario {
            simulation_results,
            starting_year: 0,
            ending_year: 0,
        }
    }

    fn scan(scenarios: Vec<Scenario>) -> ScanResults {
        let mut results = ScanResults::new();
        for (index, scenario) in scenarios.into_iter().enumerate() {
            add_scenario_to_results(&mut results, scenario, index);
        }
        results.sort_results();
        results
    }

    #[test]
    fn test_ending_balance_percentiles() {
        let results = scan(vec![
            scenario(120, 500.0),
            scenario(120, 100.0),
            scenario(120, 300.0),
            scenario(120, 200.0),
            scenario(120, 400.0),
        ]);
        assert_eq!(results.ending_balance_percentile(0.0), 100.0);
        assert_eq!(results.ending_balance_percentile(25.0), 200.0);
        assert_eq!(results.ending_balance_percentile(50.0), 300.0);
        assert_eq!(results.ending_balance_percentile(75.0), 400.0);
        assert_eq!(results.ending_balance_percentile(100.0), 500.0);
        assert!(results.failure_dates().is_empty());
    }

    #[test]
    fn test_failed_scenarios_are_worst() {
        let results = scan(vec![
            scenario(120, 300.0),
            scenario(60, 0.0),
            scenario(120, 100.0),
            scenario(24, 0.0),
            scenario(120, 200.0),
        ]);
        assert_eq!(results.num_successful, 3);
        assert_eq!(results.ending_balance_percentile(0.0), 0.0);
        assert_eq!(results.ending_balance_percentile(25.0), 0.0);
        assert_eq!(results.ending_balance_percentile(50.0), 100.0);
        assert_eq!(results.ending_balance_percentile(100.0), 300.0);

        // the money ran out in the last month of each failed scenario
        assert_eq!(
            results.failure_dates(),
            vec![date(2027, 12), date(2030, 12)]
        );
    }

    #[test]
    fn test_sort_with_nan_balance_does_not_panic() {
        let results = scan(vec![
            scenario(120, 100.0),
            scenario(120, f64::NAN),
            scenario(120, 200.0),
        ]);
        assert_eq!(results.sorted_indices.len(), 3);
        assert_eq!(results.ending_balance_percentile(0.0), 100.0);
    }

    #[test]
    fn test_failure_in_final_month() {
        // money runs out in the very last month: a failure, even though the
        // scenario lasted as long as the successful ones
        let results = scan(vec![scenario(120, 0.0), scenario(120, 100.0)]);
        assert_eq!(results.num_successful, 1);
        assert_eq!(results.failure_dates(), vec![date(2035, 12)]);
    }
}
