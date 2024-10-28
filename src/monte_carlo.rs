/**************************************************************************
* monte_carlo.rs
*
* Run a large number of random simulations
**************************************************************************/

use rand_distr::{Normal, Distribution};
use crate::{Input, scan, simulate};

pub struct MonteCarloScan {
}

impl MonteCarloScan {
    pub fn new() -> Self {
        MonteCarloScan {}
    }

    fn run_scenario(&mut self,
                    input: &Input) -> Result<scan::Scenario, String> {
        let mut simulation = simulate::Simulation::new(input);

        let us_distribution = Normal::new(input.portfolio.us_equity_expected_returns,
                                          input.portfolio.us_equity_standard_deviation).unwrap();
        let international_distribution = Normal::new(input.portfolio.international_equity_expected_returns,
                                          input.portfolio.international_equity_standard_deviation).unwrap();
        let bonds_distribution = Normal::new(input.portfolio.bonds_expected_returns,
                                          input.portfolio.bonds_standard_deviation).unwrap();

        'outer: loop {
            let us_returns = us_distribution.sample(&mut rand::thread_rng());
            let international_returns = international_distribution.sample(&mut rand::thread_rng());
            let bonds_returns = bonds_distribution.sample(&mut rand::thread_rng());
            for _ in 0..12 {
                let is_finished = simulation.run_simulation_one_month(
                    us_returns,
                    international_returns,
                    bonds_returns)?;
                if is_finished {
                    break 'outer;
                }
            }
        }

        Ok(scan::Scenario {
            simulation_results: simulation.simulation_results_,
            starting_year: 0,
            ending_year: 0,
        })
    }
}

impl scan::Scannable for MonteCarloScan {
    fn run_scan(&mut self, input: &Input) -> Result<scan::ScanResults, String> {
        let mut results = scan::ScanResults::new();

        for index in 0..1000 {
            let scenario = self.run_scenario(
                input)?;
            scan::add_scenario_to_results(&mut results, scenario, index);
        }

        results.sort_results();

        Ok(results)
    }
}

