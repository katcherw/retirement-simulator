/**************************************************************************
* report.rs
*
* Analyzes the results of the simulations (success rates, percentiles, when
* the money ran out) and collects them in a form that can be printed by the
* command line program or sent to a web page as JSON. All amounts are in
* today's dollars, and rates are percentages.
**************************************************************************/

use chrono::NaiveDate;
use serde::Serialize;

use crate::historical_scan::HistoricalScan;
use crate::runner::RunResults;
use crate::{scan, simulate, utils};

// percentiles reported for ending balances and balance bands
pub const PERCENTILES: [f64; 7] = [0.0, 10.0, 25.0, 50.0, 75.0, 90.0, 100.0];

#[derive(Debug, Serialize)]
pub struct Report {
    pub start_date: NaiveDate,
    // simulation with the expected returns every year
    pub uniform: SimulationReport,
    pub historical: HistoricalReport,
    pub monte_carlo: MonteCarloReport,
}

// one simulation, year by year
#[derive(Debug, Serialize)]
pub struct SimulationReport {
    // the money lasted until the end of the plan
    pub succeeded: bool,
    // when the first retiree retires and expenses start
    pub retirement_date: NaiveDate,
    pub average_return: f64,
    pub years: Vec<YearRow>,
}

// the first month of each year of a simulation
#[derive(Debug, Serialize)]
pub struct YearRow {
    // years since the start of the simulation
    pub year_index: usize,
    pub date: NaiveDate,
    // age of the first retiree
    pub age: u32,
    // total of all accounts
    pub balance: f64,
    pub monthly_expenses: f64,
    pub monthly_income: f64,
    pub monthly_taxes: f64,
    // marginal tax bracket
    pub tax_rate: f64,
    // annual withdrawals as a percentage of the balance
    pub withdrawal_rate: f64,
    pub annualized_return: f64,
    // the household has retired
    pub retired: bool,
}

// summary of a scan of many simulations
#[derive(Debug, Serialize)]
pub struct ScanSummary {
    pub num_simulations: u32,
    pub num_successful: u32,
    pub success_rate: f64,
    // balance at the end of the plan, at each of PERCENTILES. Simulations
    // where the money ran out count as $0.
    pub ending_balances: Vec<PercentileBalance>,
    // None if the money never ran out
    pub failures: Option<Failures>,
    // balance at the start of each year, at each of PERCENTILES, for
    // charting the range of outcomes over time
    pub balance_bands: Vec<BalanceBand>,
}

#[derive(Debug, Serialize)]
pub struct PercentileBalance {
    pub percentile: f64,
    pub balance: f64,
}

#[derive(Debug, Serialize)]
pub struct BalanceBand {
    pub year_index: usize,
    pub date: NaiveDate,
    pub balances: Vec<PercentileBalance>,
}

// when the money ran out, in the simulations where it did
#[derive(Debug, Serialize)]
pub struct Failures {
    pub count: usize,
    pub percent: f64,
    pub earliest: FailurePoint,
    pub median: FailurePoint,
    pub latest: FailurePoint,
}

#[derive(Debug, Serialize)]
pub struct FailurePoint {
    pub date: NaiveDate,
    // the retirees still alive then
    pub ages: Vec<RetireeAge>,
}

#[derive(Debug, Serialize)]
pub struct RetireeAge {
    pub name: String,
    pub age: u32,
}

#[derive(Debug, Serialize)]
pub struct HistoricalReport {
    pub summary: ScanSummary,
    // years of historical data
    pub first_year: u32,
    pub last_year: u32,
    // scenarios that ran past last_year and continued from first_year
    pub num_wrapped: usize,
    // sorted from worst to best
    pub scenarios: Vec<HistoricalScenario>,
}

#[derive(Debug, Serialize)]
pub struct HistoricalScenario {
    pub starting_year: u32,
    pub ending_year: u32,
    pub wrapped: bool,
    // e.g. "2010 to 2025, then 1928 to 1941"
    pub description: String,
    pub ending_balance: f64,
}

#[derive(Debug, Serialize)]
pub struct MonteCarloReport {
    // running again with the same seed gives the same results
    pub seed: u64,
    pub summary: ScanSummary,
    pub worst_scenario: SimulationReport,
}

pub fn build(results: &RunResults) -> Report {
    let monte_carlo = &results.monte_carlo;
    Report {
        start_date: results.start_date,
        uniform: simulation_report(&results.uniform),
        historical: historical_report(&results.historical, &results.historical_scan),
        monte_carlo: MonteCarloReport {
            seed: results.seed,
            summary: scan_summary(monte_carlo),
            worst_scenario: simulation_report(
                &monte_carlo.scenario_at_percentile(0.0).simulation_results,
            ),
        },
    }
}

fn simulation_report(results: &simulate::SimulationResults) -> SimulationReport {
    let first_retiree = &results.retirees[0];
    let years = results
        .monthly_snapshot
        .iter()
        .enumerate()
        .step_by(12)
        .map(|(i, snapshot)| YearRow {
            year_index: i / 12,
            date: snapshot.date,
            age: utils::get_age(&first_retiree.date_of_birth, &snapshot.date),
            balance: snapshot.balance,
            monthly_expenses: snapshot.expenses,
            monthly_income: snapshot.income,
            monthly_taxes: snapshot.taxes,
            tax_rate: snapshot.tax_rate,
            withdrawal_rate: snapshot.withdrawal_rate * 100.0,
            annualized_return: snapshot.annualized_return,
            retired: snapshot.date >= results.retirement_date,
        })
        .collect();

    SimulationReport {
        succeeded: results
            .monthly_snapshot
            .last()
            .is_some_and(|s| s.balance > 0.0),
        retirement_date: results.retirement_date,
        average_return: results.average_return,
        years,
    }
}

fn percent(count: usize, total: u32) -> f64 {
    count as f64 / total as f64 * 100.0
}

fn scan_summary(results: &scan::ScanResults) -> ScanSummary {
    let ending_balances = PERCENTILES
        .iter()
        .map(|&percentile| PercentileBalance {
            percentile,
            balance: results.ending_balance_percentile(percentile),
        })
        .collect();

    let failure_dates = results.failure_dates();
    let retirees = &results.scenario_results[0].simulation_results.retirees;
    let failure_point = |date: &NaiveDate| FailurePoint {
        date: *date,
        ages: retirees
            .iter()
            .filter(|retiree| *date < retiree.death_date)
            .map(|retiree| RetireeAge {
                name: retiree.name.clone(),
                age: utils::get_age(&retiree.date_of_birth, date),
            })
            .collect(),
    };
    let failures = match (failure_dates.first(), failure_dates.last()) {
        (Some(earliest), Some(latest)) => Some(Failures {
            count: failure_dates.len(),
            percent: percent(failure_dates.len(), results.num_simulations),
            earliest: failure_point(earliest),
            median: failure_point(&failure_dates[failure_dates.len() / 2]),
            latest: failure_point(latest),
        }),
        _ => None,
    };

    ScanSummary {
        num_simulations: results.num_simulations,
        num_successful: results.num_successful,
        success_rate: percent(results.num_successful as usize, results.num_simulations),
        ending_balances,
        failures,
        balance_bands: balance_bands(results),
    }
}

// balances at the start of each year across all scenarios. Scenarios where
// the money already ran out count as $0.
fn balance_bands(results: &scan::ScanResults) -> Vec<BalanceBand> {
    let Some(longest) = results
        .scenario_results
        .iter()
        .map(|s| &s.simulation_results.monthly_snapshot)
        .max_by_key(|snapshots| snapshots.len())
    else {
        return Vec::new();
    };

    (0..longest.len())
        .step_by(12)
        .map(|month| {
            let mut balances: Vec<f64> = results
                .scenario_results
                .iter()
                .map(|s| {
                    s.simulation_results
                        .monthly_snapshot
                        .get(month)
                        .map_or(0.0, |snapshot| snapshot.balance)
                })
                .collect();
            balances.sort_by(f64::total_cmp);
            BalanceBand {
                year_index: month / 12,
                date: longest[month].date,
                balances: PERCENTILES
                    .iter()
                    .map(|&percentile| PercentileBalance {
                        percentile,
                        balance: balances[scan::percentile_index(balances.len(), percentile)],
                    })
                    .collect(),
            }
        })
        .collect()
}

fn historical_report(
    results: &scan::ScanResults,
    historical_scan: &HistoricalScan,
) -> HistoricalReport {
    let (first_year, last_year) = historical_scan.first_and_last_years();
    let scenarios: Vec<HistoricalScenario> = results
        .sorted_indices
        .iter()
        .map(|&index| {
            let scenario = &results.scenario_results[index];
            HistoricalScenario {
                starting_year: scenario.starting_year,
                ending_year: scenario.ending_year,
                wrapped: scenario.ending_year < scenario.starting_year,
                description: historical_scan.describe_years(scenario),
                ending_balance: scenario.ending_balance(),
            }
        })
        .collect();

    HistoricalReport {
        summary: scan_summary(results),
        first_year,
        last_year,
        num_wrapped: scenarios.iter().filter(|s| s.wrapped).count(),
        scenarios,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{runner, Input};

    // report for the sample input with one piece of text replaced
    fn modified_sample_report(from: &str, to: &str) -> Report {
        let start_date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let text = std::fs::read_to_string("input.yaml").unwrap();
        let input = Input::from_yaml(&text.replacen(from, to, 1), start_date).unwrap();
        build(&runner::run_all(&input, 42).unwrap())
    }

    fn sample_report() -> Report {
        modified_sample_report("", "")
    }

    #[test]
    fn test_seed_reported() {
        assert_eq!(sample_report().monte_carlo.seed, 42);
    }

    #[test]
    fn test_year_rows() {
        let report = sample_report();
        let years = &report.uniform.years;
        assert_eq!(years[0].date, report.start_date);
        assert_eq!(years[1].date, NaiveDate::from_ymd_opt(2027, 1, 15).unwrap());
        assert_eq!(years[0].age, 65);
        assert!(years.iter().enumerate().all(|(i, row)| row.year_index == i));
        // John Doe retires at 67, on 9/30/2027
        assert!(!years[1].retired);
        assert!(years[2].retired);
    }

    #[test]
    fn test_balance_bands() {
        let report = sample_report();
        let summary = &report.monte_carlo.summary;
        let bands = &summary.balance_bands;
        assert_eq!(bands.len(), report.uniform.years.len());
        assert_eq!(bands[0].date, report.start_date);
        for band in bands {
            let balances: Vec<f64> = band.balances.iter().map(|p| p.balance).collect();
            assert!(balances.windows(2).all(|w| w[0] <= w[1]), "{balances:?}");
        }
        // the money never runs out for the sample plan
        assert!(summary.failures.is_none());
        assert!(bands.last().unwrap().balances[0].balance > 0.0);
    }

    #[test]
    fn test_failures_reported() {
        let report = modified_sample_report("monthly: 9000.0", "monthly: 15000.0");
        let summary = &report.monte_carlo.summary;
        let failures = summary.failures.as_ref().unwrap();
        assert_eq!(
            failures.count as u32,
            summary.num_simulations - summary.num_successful
        );
        assert!(failures.earliest.date <= failures.median.date);
        assert!(failures.median.date <= failures.latest.date);
        assert_eq!(failures.earliest.ages[0].name, "John Doe");
        // failed scenarios count as $0 in the bands from when they fail
        let last = report.monte_carlo.summary.balance_bands.last().unwrap();
        assert_eq!(last.balances[0].balance, 0.0);
        assert!(!report.monte_carlo.worst_scenario.succeeded);
    }

    #[test]
    fn test_historical_scenarios_sorted_worst_first() {
        let report = sample_report();
        let historical = &report.historical;
        assert_eq!(historical.first_year, 1928);
        assert_eq!(
            historical.scenarios.len(),
            historical.summary.num_simulations as usize
        );
        assert_eq!(
            historical.num_wrapped,
            historical.scenarios.iter().filter(|s| s.wrapped).count()
        );
        let worst = &historical.scenarios[0];
        assert_eq!(
            worst.ending_balance,
            historical.summary.ending_balances[0].balance
        );
    }

    #[test]
    fn test_report_serializes_to_json() {
        let json = serde_json::to_value(sample_report()).unwrap();
        assert_eq!(json["start_date"], "2026-01-15");
        assert_eq!(json["monte_carlo"]["seed"], 42);
        assert!(json["uniform"]["years"][0]["balance"].is_number());
    }
}
