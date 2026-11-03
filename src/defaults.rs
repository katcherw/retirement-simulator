/**************************************************************************
* defaults.rs
*
* Built-in market forecasts, so most people don't have to look them up. A
* plan can leave any of these out to use the default, or include it to
* override it.
*
* The defaults are from Fidelity's long-term asset class assumptions
**************************************************************************/

// Long-term forecasts from Fidelity Investments: real (after inflation)
// geometric average returns and standard deviations, as percentages.
pub const US_EQUITY_EXPECTED_RETURNS: f64 = 3.2;
pub const US_EQUITY_STANDARD_DEVIATION: f64 = 15.0;
pub const INTERNATIONAL_EQUITY_EXPECTED_RETURNS: f64 = 4.1;
pub const INTERNATIONAL_EQUITY_STANDARD_DEVIATION: f64 = 16.0;
pub const BONDS_EXPECTED_RETURNS: f64 = 2.5;
pub const BONDS_STANDARD_DEVIATION: f64 = 6.0;

// correlations of annual returns (-1.0 to 1.0). US and international stocks
// have historically been strongly correlated. The stock and bond correlation
// has varied widely, so 0.3 is a cautious middle ground.
pub const US_INTERNATIONAL_CORRELATION: f64 = 0.8;
pub const US_BONDS_CORRELATION: f64 = 0.3;
pub const INTERNATIONAL_BONDS_CORRELATION: f64 = 0.3;

// income paid each year as a percentage of the balance (nominal), used to
// tax the taxable account every year: approximate yields in 2026 of a US
// total stock market fund, an international stock fund, and a US aggregate
// bond fund.
pub const US_EQUITY_DIVIDEND_YIELD: f64 = 1.3;
pub const INTERNATIONAL_EQUITY_DIVIDEND_YIELD: f64 = 3.0;
pub const BONDS_INTEREST_YIELD: f64 = 4.0;
