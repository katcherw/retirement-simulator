/**************************************************************************
* tax_rates.rs
*
* Federal income tax brackets, deductions, and social security tax
* thresholds. Update them each year.
**************************************************************************/

// year of the tax brackets and deductions below. Brackets are adjusted for
// inflation every year, so in today's dollars they stay about the same, and
// older values are still close.
pub const TAX_YEAR: i32 = 2026;

// a tax bracket: income is the lower bound of taxable income (after the
// standard deduction) that is taxed at rate (percentage)
#[derive(Debug, Copy, Clone)]
pub struct TaxLevel {
    pub income: f64,
    pub rate: f64,
}

// tax brackets, deductions, and social security thresholds for one filing
// status
#[derive(Debug, Clone)]
pub struct TaxTable {
    pub standard_deduction: f64,
    // added to the standard deduction for each retiree 65 or older
    pub additional_deduction_65_or_older: f64,
    // provisional income (other income plus half of social security) above
    // which up to 50%, then up to 85%, of social security is taxable. These
    // are fixed by law and not adjusted for inflation.
    pub social_security_base: f64,
    pub social_security_adjusted_base: f64,
    // sorted by income, starting at 0
    pub tax_levels: Vec<TaxLevel>,
}

#[derive(Debug, Clone)]
pub struct TaxRates {
    // expected annual inflation (percentage), used to shrink the social
    // security thresholds in today's dollars
    pub expected_inflation: f64,
    pub married_filing_jointly: TaxTable,
    pub single: TaxTable,
}

impl TaxRates {
    // the rates for TAX_YEAR
    pub fn current() -> Self {
        TaxRates {
            expected_inflation: 2.5,
            married_filing_jointly: TaxTable {
                standard_deduction: 32200.0,
                additional_deduction_65_or_older: 1650.0,
                social_security_base: 32000.0,
                social_security_adjusted_base: 44000.0,
                tax_levels: tax_levels(&[
                    (0.0, 10.0),
                    (24800.0, 12.0),
                    (100800.0, 22.0),
                    (211400.0, 24.0),
                    (403550.0, 32.0),
                    (512450.0, 35.0),
                    (768700.0, 37.0),
                ]),
            },
            single: TaxTable {
                standard_deduction: 16100.0,
                additional_deduction_65_or_older: 2050.0,
                social_security_base: 25000.0,
                social_security_adjusted_base: 34000.0,
                tax_levels: tax_levels(&[
                    (0.0, 10.0),
                    (12400.0, 12.0),
                    (50400.0, 22.0),
                    (105700.0, 24.0),
                    (201775.0, 32.0),
                    (256225.0, 35.0),
                    (640600.0, 37.0),
                ]),
            },
        }
    }

    // married filing jointly while two retirees are alive, otherwise single
    pub fn table_for(&self, num_alive: usize) -> &TaxTable {
        if num_alive >= 2 {
            &self.married_filing_jointly
        } else {
            &self.single
        }
    }
}

// brackets as (taxable income where the bracket starts, rate)
fn tax_levels(brackets: &[(f64, f64)]) -> Vec<TaxLevel> {
    brackets
        .iter()
        .map(|&(income, rate)| TaxLevel { income, rate })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // the tax calculations depend on these, so check them when the rates
    // are updated
    fn assert_valid(table: &TaxTable) {
        let levels = &table.tax_levels;
        assert_eq!(levels[0].income, 0.0);
        assert!(levels
            .windows(2)
            .all(|pair| pair[0].income < pair[1].income));
        // each extra dollar of income can make up to 85 cents more of social
        // security taxable, so the tax on a withdrawal can be 1.85 times the
        // rate. Rates must stay low enough that this is less than 100%.
        assert!(levels
            .iter()
            .all(|level| level.rate >= 0.0 && level.rate < 50.0));
        assert!(table.social_security_base <= table.social_security_adjusted_base);
    }

    #[test]
    fn test_current_rates_are_valid() {
        let rates = TaxRates::current();
        assert_valid(&rates.married_filing_jointly);
        assert_valid(&rates.single);
        assert!(rates.single.standard_deduction < rates.married_filing_jointly.standard_deduction);
    }

    #[test]
    fn test_social_security_tax_settings() {
        let rates = TaxRates::current();
        assert_eq!(rates.expected_inflation, 2.5);
        assert_eq!(rates.married_filing_jointly.social_security_base, 32000.0);
        assert_eq!(
            rates.married_filing_jointly.social_security_adjusted_base,
            44000.0
        );
        assert_eq!(rates.single.social_security_base, 25000.0);
        assert_eq!(rates.single.additional_deduction_65_or_older, 2050.0);
    }

    #[test]
    fn test_tax_table_for_filing_status() {
        let rates = TaxRates::current();
        assert!(std::ptr::eq(
            rates.table_for(2),
            &rates.married_filing_jointly
        ));
        assert!(std::ptr::eq(rates.table_for(1), &rates.single));
    }
}
