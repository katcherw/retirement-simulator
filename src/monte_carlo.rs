/**************************************************************************
* monte_carlo.rs
*
* Run a large number of random simulations
**************************************************************************/

use crate::portfolio::Portfolio;
use crate::{scan, simulate, Input};
use rand::Rng;
use rand_distr::StandardNormal;

// Draws annual returns for US equities, international equities, and bonds
// (in that order) from a normal distribution, with the returns correlated
// with each other as configured in the portfolio.
pub struct ReturnsSampler {
    means: [f64; 3],
    standard_deviations: [f64; 3],
    // lower triangular matrix L where L * L^T is the correlation matrix
    cholesky: [[f64; 3]; 3],
}

impl ReturnsSampler {
    pub fn new(portfolio: &Portfolio) -> Result<Self, String> {
        let standard_deviations = [
            portfolio.us_equity_standard_deviation as f64,
            portfolio.international_equity_standard_deviation as f64,
            portfolio.bonds_standard_deviation as f64,
        ];
        if standard_deviations
            .iter()
            .any(|&v| v < 0.0 || !v.is_finite())
        {
            return Err("standard deviations can't be negative".to_string());
        }

        let us_international = portfolio.us_international_correlation as f64;
        let us_bonds = portfolio.us_bonds_correlation as f64;
        let international_bonds = portfolio.international_bonds_correlation as f64;
        if [us_international, us_bonds, international_bonds]
            .iter()
            .any(|v| !(-1.0..=1.0).contains(v))
        {
            return Err("correlations must be between -1.0 and 1.0".to_string());
        }
        let correlations = [
            [1.0, us_international, us_bonds],
            [us_international, 1.0, international_bonds],
            [us_bonds, international_bonds, 1.0],
        ];

        Ok(ReturnsSampler {
            means: [
                portfolio.us_equity_expected_returns as f64,
                portfolio.international_equity_expected_returns as f64,
                portfolio.bonds_expected_returns as f64,
            ],
            standard_deviations,
            cholesky: cholesky(&correlations)?,
        })
    }

    // returns annual returns (percentages) for [us, international, bonds]
    pub fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> [f32; 3] {
        // independent standard normal values, mixed by the cholesky matrix to
        // give standard normal values with the desired correlations
        let independent: [f64; 3] = [
            rng.sample(StandardNormal),
            rng.sample(StandardNormal),
            rng.sample(StandardNormal),
        ];
        let mut returns = [0.0; 3];
        for (i, row) in self.cholesky.iter().enumerate() {
            let correlated: f64 = row.iter().zip(independent.iter()).map(|(l, z)| l * z).sum();
            returns[i] = (self.means[i] + self.standard_deviations[i] * correlated) as f32;
        }
        returns
    }
}

// Cholesky decomposition of a correlation matrix. Fails if the correlations
// are inconsistent with each other (the matrix isn't positive semi-definite),
// for example if A and B are strongly correlated, and A and C are strongly
// correlated, then B and C can't be strongly negatively correlated.
fn cholesky(matrix: &[[f64; 3]; 3]) -> Result<[[f64; 3]; 3], String> {
    const TOLERANCE: f64 = 1e-9;
    let inconsistent = || "correlations are inconsistent with each other".to_string();
    let mut lower = [[0.0; 3]; 3];

    for j in 0..3 {
        let diagonal = matrix[j][j] - (0..j).map(|k| lower[j][k] * lower[j][k]).sum::<f64>();
        if diagonal < -TOLERANCE {
            return Err(inconsistent());
        }
        lower[j][j] = diagonal.max(0.0).sqrt();

        for i in (j + 1)..3 {
            let remainder = matrix[i][j] - (0..j).map(|k| lower[i][k] * lower[j][k]).sum::<f64>();
            lower[i][j] = if lower[j][j] > TOLERANCE {
                remainder / lower[j][j]
            } else if remainder.abs() <= TOLERANCE {
                // perfectly correlated with an earlier asset; nothing left to add
                0.0
            } else {
                return Err(inconsistent());
            };
        }
    }

    Ok(lower)
}

pub struct MonteCarloScan {}

impl MonteCarloScan {
    pub fn new() -> Self {
        MonteCarloScan {}
    }

    fn run_scenario(
        &mut self,
        input: &Input,
        sampler: &ReturnsSampler,
    ) -> Result<scan::Scenario, String> {
        let mut simulation = simulate::Simulation::new(input);
        let mut rng = rand::thread_rng();

        'outer: loop {
            let [us_returns, international_returns, bonds_returns] = sampler.sample(&mut rng);
            for _ in 0..12 {
                let is_finished = simulation.run_simulation_one_month(
                    us_returns,
                    international_returns,
                    bonds_returns,
                )?;
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
        let sampler = ReturnsSampler::new(&input.portfolio)?;

        for index in 0..1000 {
            let scenario = self.run_scenario(input, &sampler)?;
            scan::add_scenario_to_results(&mut results, scenario, index);
        }

        results.sort_results();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::Allocation;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn portfolio(us_international: f32, us_bonds: f32, international_bonds: f32) -> Portfolio {
        let allocation = Allocation {
            us_equities: 50.0,
            international: 25.0,
            bonds: 25.0,
        };
        Portfolio {
            balance: 0.0,
            pre_retirement_allocation: allocation,
            post_retirement_allocation: allocation,
            us_equity_expected_returns: 3.2,
            us_equity_standard_deviation: 15.0,
            international_equity_expected_returns: 4.1,
            international_equity_standard_deviation: 16.0,
            bonds_expected_returns: 2.5,
            bonds_standard_deviation: 6.0,
            us_international_correlation: us_international,
            us_bonds_correlation: us_bonds,
            international_bonds_correlation: international_bonds,
        }
    }

    fn multiply_by_transpose(lower: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
        let mut result = [[0.0; 3]; 3];
        for (i, row) in result.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                *value = (0..3).map(|k| lower[i][k] * lower[j][k]).sum();
            }
        }
        result
    }

    fn assert_matrix_close(actual: &[[f64; 3]; 3], expected: &[[f64; 3]; 3]) {
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (actual[i][j] - expected[i][j]).abs() < 1e-9,
                    "[{i}][{j}]: expected {}, got {}",
                    expected[i][j],
                    actual[i][j]
                );
            }
        }
    }

    #[test]
    fn test_cholesky_identity() {
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        assert_matrix_close(&cholesky(&identity).unwrap(), &identity);
    }

    #[test]
    fn test_cholesky_reproduces_correlations() {
        let matrix = [[1.0, 0.8, 0.3], [0.8, 1.0, 0.3], [0.3, 0.3, 1.0]];
        let lower = cholesky(&matrix).unwrap();
        assert_matrix_close(&multiply_by_transpose(&lower), &matrix);
        assert_eq!(lower[0][1], 0.0);
        assert_eq!(lower[0][2], 0.0);
        assert_eq!(lower[1][2], 0.0);
    }

    #[test]
    fn test_cholesky_perfect_correlation() {
        let matrix = [[1.0, 1.0, 0.3], [1.0, 1.0, 0.3], [0.3, 0.3, 1.0]];
        let lower = cholesky(&matrix).unwrap();
        assert_matrix_close(&multiply_by_transpose(&lower), &matrix);
    }

    #[test]
    fn test_inconsistent_correlations_rejected() {
        let err = ReturnsSampler::new(&portfolio(0.9, 0.9, -0.9))
            .err()
            .unwrap();
        assert!(err.contains("inconsistent"), "{err}");
    }

    #[test]
    fn test_correlation_out_of_range_rejected() {
        let err = ReturnsSampler::new(&portfolio(1.2, 0.3, 0.3))
            .err()
            .unwrap();
        assert!(err.contains("between -1.0 and 1.0"), "{err}");
    }

    fn mean(values: &[f64]) -> f64 {
        values.iter().sum::<f64>() / values.len() as f64
    }

    fn standard_deviation(values: &[f64]) -> f64 {
        let m = mean(values);
        (values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
    }

    fn correlation(a: &[f64], b: &[f64]) -> f64 {
        let (mean_a, mean_b) = (mean(a), mean(b));
        let covariance = a
            .iter()
            .zip(b)
            .map(|(x, y)| (x - mean_a) * (y - mean_b))
            .sum::<f64>()
            / a.len() as f64;
        covariance / (standard_deviation(a) * standard_deviation(b))
    }

    #[test]
    fn test_samples_match_inputs() {
        let sampler = ReturnsSampler::new(&portfolio(0.8, 0.3, 0.3)).unwrap();
        let mut rng = StdRng::seed_from_u64(1);
        let (mut us, mut international, mut bonds) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..200_000 {
            let [u, i, b] = sampler.sample(&mut rng);
            us.push(u as f64);
            international.push(i as f64);
            bonds.push(b as f64);
        }

        assert!((mean(&us) - 3.2).abs() < 0.1);
        assert!((mean(&international) - 4.1).abs() < 0.1);
        assert!((mean(&bonds) - 2.5).abs() < 0.05);
        assert!((standard_deviation(&us) - 15.0).abs() < 0.15);
        assert!((standard_deviation(&international) - 16.0).abs() < 0.15);
        assert!((standard_deviation(&bonds) - 6.0).abs() < 0.06);
        assert!((correlation(&us, &international) - 0.8).abs() < 0.01);
        assert!((correlation(&us, &bonds) - 0.3).abs() < 0.01);
        assert!((correlation(&international, &bonds) - 0.3).abs() < 0.01);
    }

    #[test]
    fn test_zero_standard_deviation_gives_expected_return() {
        let mut p = portfolio(0.8, 0.3, 0.3);
        p.bonds_standard_deviation = 0.0;
        let sampler = ReturnsSampler::new(&p).unwrap();
        let mut rng = StdRng::seed_from_u64(1);
        for _ in 0..100 {
            assert_eq!(sampler.sample(&mut rng)[2], 2.5);
        }
    }
}
