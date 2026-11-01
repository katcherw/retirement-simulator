/**************************************************************************
* retirement-simulator
*
* Simulation engine: parses and validates a retirement plan (input), runs
* the uniform, historical, and Monte Carlo simulations (runner), and
* analyzes the results (report). Used by the command line program, and
* later by the web site.
**************************************************************************/

pub mod defaults;
pub mod historical_scan;
pub mod input;
pub mod monte_carlo;
pub mod portfolio;
pub mod report;
pub mod runner;
pub mod scan;
pub mod simulate;
pub mod social_security;
pub mod tax_rates;
mod utils;

pub use input::{Expenses, Input, Retiree};
pub use tax_rates::{TaxLevel, TaxRates, TaxTable};
