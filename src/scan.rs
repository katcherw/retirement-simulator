/**************************************************************************
* scan.rs
*
* Common functions and traits for scanning a series of simulations 
**************************************************************************/

use crate::{Input, simulate};

// A scenario is a particular simulation (one retirement cycle) in a scan.
#[derive(Debug)]
pub struct Scenario {
    pub simulation_results: simulate::SimulationResults,
    pub starting_year: u32,
    pub ending_year: u32,
}

// Information for a vector element intended for sorting
#[derive(Debug)]
struct ScenarioSortingInfo {
    index: usize,
    num_months: usize,
    ending_balance: f32,
}
    
// The results of all the scenarios in the scan
#[derive(Debug)]
pub struct ScanResults {
    pub scenario_results: Vec<Scenario>,
    pub num_simulations: u32,
    pub num_successful: u32,
    pub min_balance: f32,
    pub max_balance: f32,
    pub sorted_indices: Vec<usize>,
    sorting_info: Vec<ScenarioSortingInfo>,
}

impl ScanResults {
    pub fn new() -> Self {
        ScanResults {
            scenario_results: Vec::new(),
            num_simulations: 0,
            num_successful: 0,
            min_balance: f32::MAX,
            max_balance: 0.0,
            sorted_indices: Vec::new(),
            sorting_info: Vec::new(),
        }
    }

    pub fn add_sorting_info(&mut self, index: usize, num_months: usize, ending_balance: f32) {
        self.sorting_info.push(ScenarioSortingInfo{index, num_months, ending_balance});
    }
                                                   
    pub fn sort_results(&mut self) {
        self.sorting_info.sort_by(|a, b| {
            a.num_months.cmp(&b.num_months)
                .then_with(|| a.ending_balance.partial_cmp(&b.ending_balance).unwrap())
        });

        for v in self.sorting_info.iter() {
            self.sorted_indices.push(v.index);
        }
    }
        
}

pub trait Scannable {
    fn run_scan(&mut self, input: &Input) -> Result<ScanResults, String>;
}

pub fn add_scenario_to_results(results: &mut ScanResults, scenario: Scenario, index: usize) {
    results.num_simulations += 1;
    let last_index = scenario.simulation_results.monthly_snapshot.len() - 1;
    let last_balance = scenario.simulation_results.monthly_snapshot[last_index].balance;
    results.min_balance = f32::min(results.min_balance, last_balance);
    results.max_balance = f32::max(results.max_balance, last_balance);
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
