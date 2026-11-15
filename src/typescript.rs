/**************************************************************************
 * typescript.rs
 *
 * TypeScript types for the JSON a TypeScript program, such as a website
 * running the engine as WebAssembly, sends to and gets back from the
 * engine: the plan, the plan defaults, validation errors and the report.
 * Built only with the "typescript" feature. The types are made from the
 * Rust structs, so a change here shows up as a type error in that program
 * instead of a silent bug.
 **************************************************************************/

use std::path::Path;

use ts_rs::{Config, ExportError, TS};

use crate::defaults::PlanDefaults;
use crate::report::Report;
use crate::{Input, ValidationError};

// writes a .ts file for each type, and every type they use, into dir
pub fn export_all(dir: &Path) -> Result<(), ExportError> {
    let config = Config::new().with_out_dir(dir);
    Input::export_all(&config)?;
    PlanDefaults::export_all(&config)?;
    ValidationError::export_all(&config)?;
    Report::export_all(&config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn read(dir: &Path, name: &str) -> String {
        fs::read_to_string(dir.join(format!("{name}.ts"))).unwrap()
    }

    #[test]
    fn test_export_all() {
        let dir = std::env::temp_dir().join(format!("ts-export-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        export_all(&dir).unwrap();

        let input = read(&dir, "Input");
        assert!(input.contains("retirees: Array<Retiree>"), "{input}");
        // built in, or set by the program, never in the plan file
        assert!(!input.contains("tax_rates"), "{input}");
        assert!(!input.contains("start_date"), "{input}");

        let retiree = read(&dir, "Retiree");
        assert!(retiree.contains("date_of_birth: string"), "{retiree}");
        assert!(retiree.contains("wage_annual_salary: number"), "{retiree}");
        // left out of a saved plan when not given
        assert!(
            retiree.contains("social_security_amount_full?: number,"),
            "{retiree}"
        );
        assert!(
            retiree.contains("pension_cola_percent?: number,"),
            "{retiree}"
        );

        let portfolio = read(&dir, "Portfolio");
        assert!(
            portfolio.contains("forecasts?: ForecastChoices"),
            "{portfolio}"
        );
        assert!(
            portfolio.contains("taxable_cost_basis?: number,"),
            "{portfolio}"
        );
        let forecasts = read(&dir, "ForecastChoices");
        assert!(
            forecasts.contains("bonds_interest_yield?: number,"),
            "{forecasts}"
        );

        let error = read(&dir, "ValidationError");
        assert!(error.contains("person: string | null"), "{error}");

        let defaults = read(&dir, "PlanDefaults");
        assert!(
            defaults.contains("pre_retirement_allocation: Allocation"),
            "{defaults}"
        );

        let monte_carlo = read(&dir, "MonteCarloReport");
        assert!(monte_carlo.contains("seed: number"), "{monte_carlo}");
        let year = read(&dir, "YearRow");
        assert!(year.contains("date: string"), "{year}");

        fs::remove_dir_all(&dir).unwrap();
    }
}
