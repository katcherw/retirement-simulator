/**************************************************************************
* portfolio.rs
*
* The portfolio entered by the user (balances, allocations, and forecasts),
* and the accounts tracked during a simulation: pre-tax, Roth, and taxable.
* Right now assume an asset allocation and continuous rebalancing. Every
* account holds the same allocation.
**************************************************************************/

use crate::defaults;
use crate::utils::*;
use serde::{Deserialize, Serialize};

// all values are percentages (0-100.0)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allocation {
    pub us_equities: f64,
    pub international: f64,
    pub bonds: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Portfolio {
    // 401(k), 403(b), traditional IRA, etc. Withdrawals are taxed as
    // ordinary income.
    pub pre_tax_balance: f64,
    // Roth accounts (and HSA). Withdrawals aren't taxed. Optional.
    #[serde(default)]
    pub roth_balance: f64,
    // brokerage accounts, savings, CDs, etc. Interest and dividends are
    // taxed every year; withdrawals are taxed only on the gain. Optional.
    #[serde(default)]
    pub taxable_balance: f64,
    // what was paid for the taxable investments. Optional: left out, it's
    // the same as the balance (no gains yet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taxable_cost_basis: Option<f64>,

    // the hyphenated names are still accepted so existing files keep working
    pub pre_retirement_allocation: Allocation,
    pub post_retirement_allocation: Allocation,

    // optional: left out, the built-in forecasts are used
    #[serde(default)]
    pub forecasts: Forecasts,
}

// forecasts for each asset class: real (after inflation) geometric average
// returns and standard deviations, as percentages. Each value is optional:
// left out, it comes from the defaults module.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Forecasts {
    pub us_equity_expected_returns: f64,
    pub us_equity_standard_deviation: f64,
    pub international_equity_expected_returns: f64,
    pub international_equity_standard_deviation: f64,
    pub bonds_expected_returns: f64,
    pub bonds_standard_deviation: f64,

    // correlation of annual returns between asset classes (-1.0 to 1.0),
    // used by the monte carlo simulation
    pub us_international_correlation: f64,
    pub us_bonds_correlation: f64,
    pub international_bonds_correlation: f64,

    // income paid each year as a percentage of the balance, taxed every year
    // in the taxable account: stock dividends as qualified dividends, bond
    // interest as ordinary income
    pub us_equity_dividend_yield: f64,
    pub international_equity_dividend_yield: f64,
    pub bonds_interest_yield: f64,
}

impl Default for Forecasts {
    fn default() -> Self {
        Forecasts {
            us_equity_expected_returns: defaults::US_EQUITY_EXPECTED_RETURNS,
            us_equity_standard_deviation: defaults::US_EQUITY_STANDARD_DEVIATION,
            international_equity_expected_returns: defaults::INTERNATIONAL_EQUITY_EXPECTED_RETURNS,
            international_equity_standard_deviation:
                defaults::INTERNATIONAL_EQUITY_STANDARD_DEVIATION,
            bonds_expected_returns: defaults::BONDS_EXPECTED_RETURNS,
            bonds_standard_deviation: defaults::BONDS_STANDARD_DEVIATION,
            us_international_correlation: defaults::US_INTERNATIONAL_CORRELATION,
            us_bonds_correlation: defaults::US_BONDS_CORRELATION,
            international_bonds_correlation: defaults::INTERNATIONAL_BONDS_CORRELATION,
            us_equity_dividend_yield: defaults::US_EQUITY_DIVIDEND_YIELD,
            international_equity_dividend_yield: defaults::INTERNATIONAL_EQUITY_DIVIDEND_YIELD,
            bonds_interest_yield: defaults::BONDS_INTEREST_YIELD,
        }
    }
}

impl Portfolio {
    // the accounts at the start of a simulation
    pub fn accounts(&self) -> Accounts {
        Accounts {
            pre_tax: self.pre_tax_balance,
            roth: self.roth_balance,
            taxable: self.taxable_balance,
            taxable_basis: self.taxable_cost_basis.unwrap_or(self.taxable_balance),
        }
    }

    pub fn allocation(&self, retired: bool) -> &Allocation {
        if retired {
            &self.post_retirement_allocation
        } else {
            &self.pre_retirement_allocation
        }
    }
}

// the income from withdrawing from the accounts, for taxes
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WithdrawalIncome {
    // from the pre-tax account
    pub ordinary: f64,
    // the gain on what's sold in the taxable account
    pub capital_gains: f64,
}

// the account balances during a simulation
#[derive(Debug, Clone, Copy, Default)]
pub struct Accounts {
    pub pre_tax: f64,
    pub roth: f64,
    pub taxable: f64,
    // cost basis of the taxable account, in today's dollars
    pub taxable_basis: f64,
}

impl Accounts {
    pub fn balance(&self) -> f64 {
        self.pre_tax + self.roth + self.taxable
    }

    // retirement plan contributions from wages
    pub fn contribute(&mut self, amount: f64) {
        self.pre_tax += amount;
    }

    // income left over after expenses and taxes is invested in the taxable
    // account
    pub fn deposit(&mut self, amount: f64) {
        self.taxable += amount;
        self.taxable_basis += amount;
    }

    // part of the taxable account that is gain, which is taxed when sold.
    // Losses aren't deducted.
    fn gain_fraction(&self) -> f64 {
        if self.taxable > 0.0 {
            (1.0 - self.taxable_basis / self.taxable).max(0.0)
        } else {
            0.0
        }
    }

    // the taxable income from withdrawing the given amount. Withdrawals
    // come from the taxable account first, then pre-tax, then Roth.
    pub fn withdrawal_income(&self, amount: f64) -> WithdrawalIncome {
        let from_taxable = amount.min(self.taxable);
        let from_pre_tax = (amount - from_taxable).min(self.pre_tax);
        WithdrawalIncome {
            ordinary: from_pre_tax,
            capital_gains: from_taxable * self.gain_fraction(),
        }
    }

    pub fn withdraw(&mut self, amount: f64) {
        let from_taxable = amount.min(self.taxable);
        if from_taxable > 0.0 {
            // the basis of what's sold, in proportion
            self.taxable_basis *= 1.0 - from_taxable / self.taxable;
            self.taxable -= from_taxable;
        }
        let rest = amount - from_taxable;
        let from_pre_tax = rest.min(self.pre_tax);
        self.pre_tax -= from_pre_tax;
        self.roth = (self.roth - (rest - from_pre_tax)).max(0.0);
    }

    // this month's interest and qualified dividends from the taxable
    // account. They're part of the returns, and reinvested.
    pub fn investment_income(&self, allocation: &Allocation, forecasts: &Forecasts) -> (f64, f64) {
        let monthly = self.taxable / 100.0 / 100.0 / 12.0;
        let interest = allocation.bonds * forecasts.bonds_interest_yield * monthly;
        let dividends = (allocation.us_equities * forecasts.us_equity_dividend_yield
            + allocation.international * forecasts.international_equity_dividend_yield)
            * monthly;
        (interest, dividends)
    }

    // reinvested income has already been taxed, so it adds to the basis
    pub fn reinvest(&mut self, income: f64) {
        self.taxable_basis += income;
    }

    // the basis is in the dollars of the year the investments were bought.
    // Balances are in today's dollars, so inflation shrinks the basis (and
    // makes more of the account taxable gain). Inflation is annual percent.
    pub fn adjust_basis_for_inflation(&mut self, inflation: f64) {
        self.taxable_basis /= (1.0 + inflation / 100.0).powf(1.0 / 12.0);
    }

    // grows the balances for a month and returns the annualized average
    // return
    pub fn grow(
        &mut self,
        allocation: &Allocation,
        us_equity_expected_returns: f64,
        international_equity_expected_returns: f64,
        bonds_expected_returns: f64,
    ) -> f64 {
        let growth = (allocation.us_equities
            * (get_monthly_rate(us_equity_expected_returns / 100.0) + 1.0)
            + allocation.international
                * (get_monthly_rate(international_equity_expected_returns / 100.0) + 1.0)
            + allocation.bonds * (get_monthly_rate(bonds_expected_returns / 100.0) + 1.0))
            / 100.0;
        self.pre_tax *= growth;
        self.roth *= growth;
        self.taxable *= growth;

        // return annualized return
        us_equity_expected_returns * allocation.us_equities / 100.0
            + international_equity_expected_returns * allocation.international / 100.0
            + bonds_expected_returns * allocation.bonds / 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLOCATION: Allocation = Allocation {
        us_equities: 50.0,
        international: 20.0,
        bonds: 30.0,
    };

    fn accounts(pre_tax: f64, roth: f64, taxable: f64, taxable_basis: f64) -> Accounts {
        Accounts {
            pre_tax,
            roth,
            taxable,
            taxable_basis,
        }
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn test_withdraw_taxable_then_pre_tax_then_roth() {
        let mut a = accounts(1000.0, 500.0, 200.0, 200.0);
        a.withdraw(100.0);
        assert_eq!((a.taxable, a.pre_tax, a.roth), (100.0, 1000.0, 500.0));
        a.withdraw(900.0);
        assert_eq!((a.taxable, a.pre_tax, a.roth), (0.0, 200.0, 500.0));
        a.withdraw(300.0);
        assert_eq!((a.taxable, a.pre_tax, a.roth), (0.0, 0.0, 400.0));
        a.withdraw(1000.0);
        assert_eq!(a.balance(), 0.0);
    }

    #[test]
    fn test_withdrawal_income() {
        // taxable account is 40% gain
        let a = accounts(1000.0, 500.0, 200.0, 120.0);
        assert_eq!(
            a.withdrawal_income(100.0),
            WithdrawalIncome {
                ordinary: 0.0,
                capital_gains: 40.0
            }
        );
        // all of the taxable account, then 300 from pre-tax
        let income = a.withdrawal_income(500.0);
        assert_close(income.ordinary, 300.0);
        assert_close(income.capital_gains, 80.0);
        // Roth isn't taxed
        assert_close(a.withdrawal_income(2000.0).ordinary, 1000.0);
    }

    #[test]
    fn test_withdraw_reduces_basis_in_proportion() {
        let mut a = accounts(0.0, 0.0, 200.0, 120.0);
        a.withdraw(50.0);
        assert_close(a.taxable, 150.0);
        assert_close(a.taxable_basis, 90.0);
        // the gain fraction stays the same
        assert_close(a.withdrawal_income(150.0).capital_gains, 60.0);
    }

    #[test]
    fn test_losses_are_not_negative_gains() {
        let a = accounts(0.0, 0.0, 100.0, 150.0);
        assert_eq!(a.withdrawal_income(100.0).capital_gains, 0.0);
    }

    #[test]
    fn test_deposit_to_taxable_with_basis() {
        let mut a = accounts(1000.0, 500.0, 0.0, 0.0);
        a.deposit(100.0);
        assert_eq!((a.taxable, a.taxable_basis), (100.0, 100.0));
        a.contribute(50.0);
        assert_eq!(a.pre_tax, 1050.0);
        assert_eq!(a.balance(), 1650.0);
    }

    #[test]
    fn test_investment_income() {
        let a = accounts(1000.0, 1000.0, 120000.0, 0.0);
        let forecasts = Forecasts {
            us_equity_dividend_yield: 1.0,
            international_equity_dividend_yield: 3.0,
            bonds_interest_yield: 4.0,
            ..Forecasts::default()
        };
        // only the taxable account: bonds are 30% of $120,000 at 4%, stocks
        // 50% at 1% and 20% at 3%, a month of each
        let (interest, dividends) = a.investment_income(&ALLOCATION, &forecasts);
        assert_close(interest, 120.0);
        assert_close(dividends, 50.0 + 60.0);
    }

    #[test]
    fn test_basis_shrinks_with_inflation() {
        let mut a = accounts(0.0, 0.0, 1000.0, 1000.0);
        for _ in 0..12 {
            a.adjust_basis_for_inflation(10.0);
        }
        assert_close(a.taxable_basis, 1000.0 / 1.1);
    }

    #[test]
    fn test_all_accounts_grow_at_the_same_rate() {
        let mut a = accounts(1000.0, 500.0, 200.0, 100.0);
        let annual = a.grow(&ALLOCATION, 10.0, 10.0, 10.0);
        assert!((annual - 10.0).abs() < 1e-9);
        let monthly = 1.1f64.powf(1.0 / 12.0);
        assert_close(a.pre_tax, 1000.0 * monthly);
        assert_close(a.roth, 500.0 * monthly);
        assert_close(a.taxable, 200.0 * monthly);
        assert_eq!(a.taxable_basis, 100.0);
    }

    #[test]
    fn test_cost_basis_defaults_to_balance() {
        let mut portfolio = Portfolio {
            pre_tax_balance: 0.0,
            roth_balance: 0.0,
            taxable_balance: 500.0,
            taxable_cost_basis: None,
            pre_retirement_allocation: ALLOCATION,
            post_retirement_allocation: ALLOCATION,
            forecasts: Forecasts::default(),
        };
        assert_eq!(portfolio.accounts().taxable_basis, 500.0);
        portfolio.taxable_cost_basis = Some(200.0);
        assert_eq!(portfolio.accounts().taxable_basis, 200.0);
    }
}
