/**************************************************************************
 * retirement-simulator
 *
 * Simulation engine: parses and validates a retirement plan (input), runs
 * the uniform, historical, and Monte Carlo simulations (runner), and
 * analyzes the results (report). Used by the command line program, and
 * by other programs such as a website.
 **************************************************************************/

pub mod defaults;
pub mod historical_scan;
pub mod input;
pub mod monte_carlo;
pub mod portfolio;
pub mod report;
pub mod rmd;
pub mod runner;
pub mod scan;
pub mod simulate;
pub mod social_security;
pub mod tax_rates;
#[cfg(feature = "typescript")]
pub mod typescript;
mod utils;

pub use input::{Expenses, Input, Retiree, ValidationError};
pub use tax_rates::{TaxLevel, TaxRates, TaxTable};
